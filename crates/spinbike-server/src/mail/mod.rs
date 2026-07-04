//! SMTP mail sender.
//!
//! Mirrors the `ewelink` module's conventions: env-driven config, a
//! Disabled fast-path when required vars are missing/empty, and an
//! in-process test seam (`SMTP_TEST_MODE=capture`) that never touches the
//! network — used by integration/E2E tests so the triggering endpoint can
//! echo the composed message back (e.g. as `test_link`) instead of
//! needing a real mailbox.
//!
//! Unlike `ewelink` there is no persistent background task: `send()`
//! dials a fresh SMTP connection per call through `lettre`'s
//! `AsyncSmtpTransport`, so there is no `ws.rs`/`auth.rs` equivalent here.

pub mod error;

pub use error::MailError;

use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One captured message, recorded by the `capture` test-mode stub instead
/// of being sent over the network. Fields mirror `send()`'s arguments so a
/// later ticket's invite endpoint can echo them back (e.g. as
/// `test_link`) in its JSON response for Playwright to drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedMail {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

/// Internal transport state. `None` on `MailHandle::transport` is the
/// Disabled fast-path; `Some(Capture)` and `Some(Real(_))` are both
/// "active" — mirroring how `ewelink::EwelinkHandle`'s `tx: Option<_>` is
/// `Some` for both its test-mode stub task and its real WS task.
enum Transport {
    /// `SMTP_TEST_MODE=capture` — `send()` stores the message and returns
    /// `Ok` without touching the network.
    Capture,
    /// Real, env-configured SMTP relay.
    Real(AsyncSmtpTransport<Tokio1Executor>),
}

/// Cloneable handle. `send()` is `&self` so multiple route handlers share
/// one handle through axum state.
#[derive(Clone)]
pub struct MailHandle {
    transport: Option<Arc<Transport>>,
    from: String,
    /// Only ever populated when running in capture test mode.
    captured: Arc<Mutex<Option<CapturedMail>>>,
}

impl MailHandle {
    /// Reads `SMTP_TEST_MODE` / `SMTP_HOST` / `SMTP_PORT` / `SMTP_USERNAME`
    /// / `SMTP_PASSWORD` / `SMTP_FROM` from env. Never panics; safe to
    /// call once at server startup.
    ///
    /// `SMTP_TEST_MODE=capture` is checked FIRST (mirrors
    /// `ewelink::EwelinkHandle::spawn`'s `EWELINK_TEST_MODE` branch) and
    /// short-circuits before any real transport is built — production
    /// must never set this var. Otherwise, if any required var is
    /// empty/missing (or `SMTP_PORT` doesn't parse as a port number),
    /// returns the Disabled handle: `send()` always errors with
    /// `MailError::Disabled`.
    pub fn spawn() -> Self {
        let test_mode = std::env::var("SMTP_TEST_MODE").ok();
        let from = std::env::var("SMTP_FROM").ok().unwrap_or_default();
        let captured = Arc::new(Mutex::new(None));

        if test_mode.as_deref() == Some("capture") {
            tracing::info!("mail: SMTP_TEST_MODE=capture active — messages captured, never sent");
            return Self {
                transport: Some(Arc::new(Transport::Capture)),
                from,
                captured,
            };
        }

        let host = std::env::var("SMTP_HOST").ok().unwrap_or_default();
        let port_raw = std::env::var("SMTP_PORT").ok().unwrap_or_default();
        let username = std::env::var("SMTP_USERNAME").ok().unwrap_or_default();
        let password = std::env::var("SMTP_PASSWORD").ok().unwrap_or_default();

        if host.is_empty()
            || port_raw.is_empty()
            || username.is_empty()
            || password.is_empty()
            || from.is_empty()
        {
            tracing::warn!(
                host_set = !host.is_empty(),
                port_set = !port_raw.is_empty(),
                username_set = !username.is_empty(),
                password_set = !password.is_empty(),
                from_set = !from.is_empty(),
                "mail: disabled — required SMTP_* env vars unset"
            );
            return Self {
                transport: None,
                from,
                captured,
            };
        }

        let port: u16 = match port_raw.parse() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    port = %port_raw,
                    error = %e,
                    "mail: disabled — SMTP_PORT is not a valid port number"
                );
                return Self {
                    transport: None,
                    from,
                    captured,
                };
            }
        };

        let builder = match AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(%host, error = %e, "mail: disabled — failed to build SMTP relay");
                return Self {
                    transport: None,
                    from,
                    captured,
                };
            }
        };

        let transport = builder
            .port(port)
            .credentials(Credentials::new(username, password))
            .build();

        tracing::info!(%host, %port, "mail: SMTP transport configured");
        Self {
            transport: Some(Arc::new(Transport::Real(transport))),
            from,
            captured,
        }
    }

    /// Compose and send an email. Disabled fast-path returns
    /// `MailError::Disabled` without composing a message. In capture test
    /// mode, stores the composed fields (readable via `last_captured`)
    /// and returns `Ok` without touching the network. Otherwise composes
    /// a real message and dials the configured relay under a 10 s
    /// timeout.
    pub async fn send(
        &self,
        to: &str,
        subject: &str,
        text: &str,
        html: &str,
    ) -> Result<(), MailError> {
        let Some(transport) = &self.transport else {
            return Err(MailError::Disabled);
        };

        match transport.as_ref() {
            Transport::Capture => {
                *self.captured.lock().expect("captured mutex poisoned") = Some(CapturedMail {
                    to: to.to_string(),
                    subject: subject.to_string(),
                    text: text.to_string(),
                    html: html.to_string(),
                });
                tracing::info!(%to, %subject, "mail: captured (test mode, not sent)");
                Ok(())
            }
            Transport::Real(transport) => {
                let message = build_message(&self.from, to, subject, text, html)?;
                send_via_transport(transport, message, to, subject).await
            }
        }
    }

    /// The last message captured in `SMTP_TEST_MODE=capture`. `None`
    /// outside capture mode, or before any `send()` call.
    pub fn last_captured(&self) -> Option<CapturedMail> {
        self.captured
            .lock()
            .expect("captured mutex poisoned")
            .clone()
    }
}

/// Build a text+HTML alternative message. Pure/network-free — errors only
/// when `from` or `to` don't parse as an RFC 5322 mailbox.
fn build_message(
    from: &str,
    to: &str,
    subject: &str,
    text: &str,
    html: &str,
) -> Result<Message, MailError> {
    let from_mailbox: Mailbox = from
        .parse()
        .map_err(|e| MailError::Send(format!("invalid SMTP_FROM address {from:?}: {e}")))?;
    let to_mailbox: Mailbox = to
        .parse()
        .map_err(|e| MailError::Send(format!("invalid recipient address {to:?}: {e}")))?;

    Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject(subject)
        .multipart(MultiPart::alternative_plain_html(
            text.to_string(),
            html.to_string(),
        ))
        .map_err(|e| MailError::Send(format!("failed to build message: {e}")))
}

/// Dials the real SMTP relay with a 10 s timeout.
///
/// Excluded from mutation testing: exercising the success arm needs a
/// live SMTP server (breaks CI hermeticity), and exercising the timeout
/// arm needs a 10 s+ stall per mutant run, which would blow the
/// diff-scoped mutation gate's time budget many times over across a
/// mutant sweep. The connection-refused error arm IS covered directly by
/// a real (network-free, loopback) integration test — see
/// `real_transport_send_error_surfaces_as_mail_error_send` below — mirrors
/// the `#[mutants::skip]` precedent already used on `ewelink::ws::run_real_ws`
/// for the same class of problem.
#[mutants::skip]
async fn send_via_transport(
    transport: &AsyncSmtpTransport<Tokio1Executor>,
    message: Message,
    to: &str,
    subject: &str,
) -> Result<(), MailError> {
    match tokio::time::timeout(Duration::from_secs(10), transport.send(message)).await {
        Ok(Ok(_response)) => {
            tracing::info!(%to, %subject, "mail: sent");
            Ok(())
        }
        Ok(Err(e)) => {
            tracing::error!(%to, %subject, error = %e, "mail: send failed");
            Err(MailError::Send(e.to_string()))
        }
        Err(_) => {
            tracing::error!(%to, %subject, "mail: send timed out after 10s");
            Err(MailError::Timeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Process-wide lock guarding mutations to SMTP_* env vars in these
    /// in-crate tests. Without it, two #[tokio::test]s running concurrently
    /// race on the global env and pick up the wrong values when
    /// MailHandle::spawn() reads them.
    static MAIL_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    const ALL_VARS: &[&str] = &[
        "SMTP_HOST",
        "SMTP_PORT",
        "SMTP_USERNAME",
        "SMTP_PASSWORD",
        "SMTP_FROM",
        "SMTP_TEST_MODE",
    ];

    /// Snapshot + clear all SMTP_* env vars, run `f`, then restore the
    /// previous values. Returns whatever `f` returns.
    async fn with_clean_env<Fut, T>(f: impl FnOnce() -> Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let _guard = MAIL_TEST_LOCK.lock().await;
        let prior: Vec<Option<String>> = ALL_VARS.iter().map(|v| std::env::var(v).ok()).collect();
        // SAFETY: process-wide lock above guarantees no concurrent mutation.
        unsafe {
            for v in ALL_VARS {
                std::env::remove_var(v);
            }
        }
        let result = f().await;
        unsafe {
            for (v, prior) in ALL_VARS.iter().zip(prior) {
                match prior {
                    Some(val) => std::env::set_var(v, val),
                    None => std::env::remove_var(v),
                }
            }
        }
        result
    }

    /// Binds an ephemeral loopback port, then immediately drops the
    /// listener so a subsequent connect to it fails fast with "connection
    /// refused" — no external network dependency, no sleeping.
    async fn refused_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        listener.local_addr().expect("local_addr").port()
    }

    #[tokio::test]
    async fn disabled_when_env_unset() {
        with_clean_env(|| async {
            let h = MailHandle::spawn();
            let res = h.send("to@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Disabled)), "got {res:?}");
        })
        .await;
    }

    /// Catches `||` → `&&` mutations in the emptiness check: if only HOST
    /// is empty, the handle must still be Disabled.
    #[tokio::test]
    async fn disabled_when_only_host_unset() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_PORT", "587");
                std::env::set_var("SMTP_USERNAME", "u");
                std::env::set_var("SMTP_PASSWORD", "p");
                std::env::set_var("SMTP_FROM", "SpinBike <bike@example.com>");
            }
            let h = MailHandle::spawn();
            let res = h.send("to@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Disabled)), "got {res:?}");
        })
        .await;
    }

    #[tokio::test]
    async fn disabled_when_only_port_unset() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_HOST", "smtp.example.com");
                std::env::set_var("SMTP_USERNAME", "u");
                std::env::set_var("SMTP_PASSWORD", "p");
                std::env::set_var("SMTP_FROM", "SpinBike <bike@example.com>");
            }
            let h = MailHandle::spawn();
            let res = h.send("to@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Disabled)), "got {res:?}");
        })
        .await;
    }

    #[tokio::test]
    async fn disabled_when_only_username_unset() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_HOST", "smtp.example.com");
                std::env::set_var("SMTP_PORT", "587");
                std::env::set_var("SMTP_PASSWORD", "p");
                std::env::set_var("SMTP_FROM", "SpinBike <bike@example.com>");
            }
            let h = MailHandle::spawn();
            let res = h.send("to@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Disabled)), "got {res:?}");
        })
        .await;
    }

    #[tokio::test]
    async fn disabled_when_only_password_unset() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_HOST", "smtp.example.com");
                std::env::set_var("SMTP_PORT", "587");
                std::env::set_var("SMTP_USERNAME", "u");
                std::env::set_var("SMTP_FROM", "SpinBike <bike@example.com>");
            }
            let h = MailHandle::spawn();
            let res = h.send("to@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Disabled)), "got {res:?}");
        })
        .await;
    }

    #[tokio::test]
    async fn disabled_when_only_from_unset() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_HOST", "smtp.example.com");
                std::env::set_var("SMTP_PORT", "587");
                std::env::set_var("SMTP_USERNAME", "u");
                std::env::set_var("SMTP_PASSWORD", "p");
            }
            let h = MailHandle::spawn();
            let res = h.send("to@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Disabled)), "got {res:?}");
        })
        .await;
    }

    /// Catches a deleted/inverted `SMTP_PORT` parse check: a non-numeric
    /// port must still land in Disabled, not panic and not silently use a
    /// garbage port value.
    #[tokio::test]
    async fn disabled_when_port_invalid() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_HOST", "smtp.example.com");
                std::env::set_var("SMTP_PORT", "not-a-port");
                std::env::set_var("SMTP_USERNAME", "u");
                std::env::set_var("SMTP_PASSWORD", "p");
                std::env::set_var("SMTP_FROM", "SpinBike <bike@example.com>");
            }
            let h = MailHandle::spawn();
            let res = h.send("to@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Disabled)), "got {res:?}");
        })
        .await;
    }

    /// SMTP_TEST_MODE=capture must win even when every SMTP_* var is ALSO
    /// unset — the capture branch is checked first and never falls
    /// through to the Disabled path.
    #[tokio::test]
    async fn capture_mode_active_even_with_no_smtp_env() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_TEST_MODE", "capture");
            }
            let h = MailHandle::spawn();
            let res = h
                .send(
                    "client@example.com",
                    "Vitajte",
                    "text body",
                    "<b>html body</b>",
                )
                .await;
            assert!(res.is_ok(), "got {res:?}");
        })
        .await;
    }

    #[tokio::test]
    async fn last_captured_is_none_before_any_send() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_TEST_MODE", "capture");
            }
            let h = MailHandle::spawn();
            assert_eq!(h.last_captured(), None);
        })
        .await;
    }

    /// Exercises the full capture round-trip and checks every field lands
    /// in the right slot (catches a to/subject/text/html field-swap
    /// mutation in the `CapturedMail` construction).
    #[tokio::test]
    async fn capture_mode_stores_message_with_correct_fields() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_TEST_MODE", "capture");
            }
            let h = MailHandle::spawn();
            h.send(
                "client@example.com",
                "Vitajte v SpinBike",
                "text body",
                "<b>html body</b>",
            )
            .await
            .expect("capture mode send must succeed");

            let captured = h.last_captured().expect("must have captured a message");
            assert_eq!(captured.to, "client@example.com");
            assert_eq!(captured.subject, "Vitajte v SpinBike");
            assert_eq!(captured.text, "text body");
            assert_eq!(captured.html, "<b>html body</b>");
        })
        .await;
    }

    /// A second send() overwrites the first captured message rather than
    /// accumulating — `last_captured` is the LAST one only.
    #[tokio::test]
    async fn capture_mode_second_send_overwrites_first() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_TEST_MODE", "capture");
            }
            let h = MailHandle::spawn();
            h.send("first@example.com", "first", "t1", "h1")
                .await
                .unwrap();
            h.send("second@example.com", "second", "t2", "h2")
                .await
                .unwrap();

            let captured = h.last_captured().expect("must have captured a message");
            assert_eq!(captured.to, "second@example.com");
            assert_eq!(captured.subject, "second");
        })
        .await;
    }

    #[test]
    fn build_message_composes_headers_and_body() {
        let msg = build_message(
            "SpinBike <bike@example.com>",
            "client@example.com",
            "Vitajte v SpinBike",
            "Vitajte, otvorte tento link.",
            "<p>Vitajte, otvorte tento link.</p>",
        )
        .expect("valid inputs must build a message");

        let raw = String::from_utf8(msg.formatted()).expect("formatted email must be valid UTF-8");

        assert!(
            raw.contains("From: SpinBike <bike@example.com>"),
            "missing From header, got:\n{raw}"
        );
        assert!(
            raw.contains("To: client@example.com"),
            "missing To header, got:\n{raw}"
        );
        assert!(
            raw.contains("Subject: Vitajte v SpinBike"),
            "missing Subject header, got:\n{raw}"
        );
        assert!(
            raw.contains("Vitajte, otvorte tento link."),
            "missing plain-text body, got:\n{raw}"
        );
        assert!(
            raw.contains("<p>Vitajte, otvorte tento link.</p>"),
            "missing html body, got:\n{raw}"
        );
    }

    #[test]
    fn build_message_invalid_to_address_returns_send_error() {
        let res = build_message("SpinBike <bike@example.com>", "not-an-email", "s", "t", "h");
        assert!(matches!(res, Err(MailError::Send(_))), "got {res:?}");
    }

    #[test]
    fn build_message_invalid_from_address_returns_send_error() {
        let res = build_message("not-an-email", "client@example.com", "s", "t", "h");
        assert!(matches!(res, Err(MailError::Send(_))), "got {res:?}");
    }

    /// `SMTP_FROM` empty-check only guards against an EMPTY string;
    /// `spawn()` doesn't validate mailbox syntax, so a malformed
    /// non-empty `SMTP_FROM` builds a Real transport successfully and the
    /// parse failure surfaces from `build_message` on the first `send()`.
    /// This test drives that path end-to-end without any real network
    /// I/O — the malformed From fails before the (loopback,
    /// connection-refused) transport is ever dialed.
    #[tokio::test]
    async fn real_transport_send_invalid_from_surfaces_as_mail_error_send() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("SMTP_HOST", "127.0.0.1");
                std::env::set_var("SMTP_PORT", "1");
                std::env::set_var("SMTP_USERNAME", "u");
                std::env::set_var("SMTP_PASSWORD", "p");
                std::env::set_var("SMTP_FROM", "not-an-email");
            }
            let h = MailHandle::spawn();
            let res = h.send("client@example.com", "s", "t", "h").await;
            assert!(matches!(res, Err(MailError::Send(_))), "got {res:?}");
        })
        .await;
    }

    /// Full real-transport round trip against a loopback port that
    /// refuses the connection immediately — proves `spawn()` built a
    /// `Transport::Real` (not Disabled) for valid env, and that a network
    /// failure surfaces as `MailError::Send`, not a panic or a false
    /// `Ok`. No external network dependency; connection-refused on
    /// loopback is near-instant, so this stays well clear of the 10 s
    /// timeout path (that arm is `#[mutants::skip]`-excluded on
    /// `send_via_transport` — see its doc comment).
    #[tokio::test]
    async fn real_transport_send_error_surfaces_as_mail_error_send() {
        with_clean_env(|| async {
            let port = refused_port().await;
            unsafe {
                std::env::set_var("SMTP_HOST", "127.0.0.1");
                std::env::set_var("SMTP_PORT", port.to_string());
                std::env::set_var("SMTP_USERNAME", "u");
                std::env::set_var("SMTP_PASSWORD", "p");
                std::env::set_var("SMTP_FROM", "SpinBike <bike@example.com>");
            }
            let h = MailHandle::spawn();
            let res = h
                .send("client@example.com", "Vitajte", "text", "<b>html</b>")
                .await;
            assert!(matches!(res, Err(MailError::Send(_))), "got {res:?}");
        })
        .await;
    }
}
