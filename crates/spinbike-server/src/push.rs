//! Web Push send path (#264): VAPID-signed, RFC-8291-encrypted notifications
//! to subscribed customers.
//!
//! Mirrors `mail::MailHandle`'s convention: env-driven config, a Disabled
//! fast-path when the private key is unset (`send()` becomes a no-op that
//! reports `SendOutcome::Failed`), and a `Clone`-able handle shared through
//! `AppState`.
//!
//! Uses the `web-push` crate (`default-features = false` in Cargo.toml)
//! ONLY for VAPID JWT signing (`PartialVapidSignatureBuilder`, built once at
//! startup from `VAPID_PRIVATE_KEY`) and RFC-8291 `aes128gcm` payload
//! encryption — its bundled HTTP client (isahc/hyper) is disabled to avoid
//! a second, non-rustls HTTP/TLS stack. The actual POST to the push service
//! is sent with the project's existing `reqwest` client, copying the
//! crate-produced `Authorization: vapid t=…,k=…` header (attached
//! automatically by the crate whenever a payload is set — see `send()`)
//! plus our own `TTL`/`Content-Encoding` headers.

use base64::Engine as _;
use web_push::{
    ContentEncoding, PartialVapidSignatureBuilder, SubscriptionInfo, VapidSignatureBuilder,
    WebPushMessageBuilder,
};

/// A well-known, PUBLIC test fixture from the `web-push` crate's own test
/// suite (`PRIVATE_BASE64` in its `vapid::builder` tests) — NOT a secret,
/// safe to compile into every binary. Used by integration/unit tests (and
/// `tests/helpers/mod.rs`'s `TestApp`) so a real, valid VAPID key is
/// available without touching the `VAPID_PRIVATE_KEY` env var.
pub const TEST_VAPID_PRIVATE_KEY_B64: &str = "IQ9Ur0ykXoHS9gzfYX0aBjy9lvdrjx_PFUXmie9YRcY";

/// A well-known PUBLIC test subscriber keypair (also from the `web-push`
/// crate's own test suite) — a REAL, valid uncompressed P-256 point + a
/// real 16-byte auth secret, both base64url. Encryption genuinely validates
/// key shape (length, curve point format), so an arbitrary placeholder
/// string like `"test-key"` fails at `WebPushMessageBuilder::build()`
/// before any HTTP call is even made. Test-only; not a secret (it's a
/// throwaway browser-side subscription example, not ours).
#[cfg(test)]
pub(crate) const TEST_P256DH_B64: &str =
    "BH1HTeKM7-NwaLGHEqxeu2IamQaVVLkcsFHPIHmsCnqxcBHPQBprF41bEMOr3O1hUQ2jU1opNEm1F_lZV_sxMP8";
#[cfg(test)]
pub(crate) const TEST_AUTH_B64: &str = "sBXU5_tIYz-5w7G2B25BEw";

/// Cloneable handle. `send()` is `&self` so multiple call sites (the daily
/// job, in this case) share one handle through `AppState`.
#[derive(Clone)]
pub struct PushHandle {
    inner: Option<std::sync::Arc<Inner>>,
}

struct Inner {
    vapid: PartialVapidSignatureBuilder,
    /// Uncompressed EC public key, base64url-no-pad — served to the client
    /// via `GET /api/push/config` as `applicationServerKey`.
    public_key_b64: String,
    http: reqwest::Client,
}

/// Result of one send attempt against the push service, mapped from its
/// HTTP response — the exact four outcomes the issue calls out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// 2xx — delivered to the push service (not proof the device showed it).
    Sent,
    /// 404/410 — the endpoint is gone; the caller must prune the subscription.
    Gone,
    /// 429 — back off; try again next tick, not now.
    RateLimited,
    /// 5xx or a transport error — retry next tick.
    Retryable,
    /// Anything else (bad request, disabled handle, encryption/signing
    /// failure) — logged, not retried aggressively.
    Failed,
}

impl PushHandle {
    /// Reads `VAPID_PRIVATE_KEY` from env. Never panics; safe to call once
    /// at server startup. Empty/unset/invalid → Disabled: `send()` always
    /// returns `SendOutcome::Failed` and `public_key()` returns `None`.
    pub fn spawn() -> Self {
        let key_b64 = std::env::var("VAPID_PRIVATE_KEY").ok().unwrap_or_default();
        if key_b64.is_empty() {
            tracing::warn!("push: disabled — VAPID_PRIVATE_KEY unset");
            return Self { inner: None };
        }
        Self::from_base64_private_key(&key_b64)
    }

    /// Build directly from a raw base64url (no padding) VAPID private key —
    /// the same format `spawn()` reads from env, extracted so tests can
    /// construct a handle deterministically without mutating process-wide
    /// env state (see `TEST_VAPID_PRIVATE_KEY_B64`).
    pub fn from_base64_private_key(key_b64: &str) -> Self {
        let vapid = match VapidSignatureBuilder::from_base64_no_sub(key_b64) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("push: disabled — VAPID_PRIVATE_KEY invalid: {e}");
                return Self { inner: None };
            }
        };
        let public_key_b64 =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(vapid.get_public_key());
        Self {
            inner: Some(std::sync::Arc::new(Inner {
                vapid,
                public_key_b64,
                http: reqwest::Client::new(),
            })),
        }
    }

    /// `Some(base64url public key)` when configured, `None` when push is
    /// disabled (no `VAPID_PRIVATE_KEY`) — `/api/push/config` surfaces this
    /// as `enabled`.
    pub fn public_key(&self) -> Option<&str> {
        self.inner.as_ref().map(|i| i.public_key_b64.as_str())
    }

    /// Send one notification to one subscription. `title`/`body` are the
    /// (already-localized, plain Slovak) strings the client's `sw.js`
    /// `push` handler shows via `Notification`.
    pub async fn send(
        &self,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        title: &str,
        body: &str,
    ) -> SendOutcome {
        let Some(inner) = &self.inner else {
            return SendOutcome::Failed;
        };

        let info = SubscriptionInfo::new(endpoint, p256dh, auth);

        let sig_builder = inner.vapid.clone().add_sub_info(&info);
        let signature = match sig_builder.build() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(endpoint, "push: VAPID signature build failed: {e}");
                return SendOutcome::Failed;
            }
        };

        let payload = serde_json::json!({ "title": title, "body": body }).to_string();

        let mut builder = WebPushMessageBuilder::new(&info);
        builder.set_payload(ContentEncoding::Aes128Gcm, payload.as_bytes());
        builder.set_vapid_signature(signature);
        // 24h: these are daily reminders, not time-critical alerts — no
        // point holding them longer than the next day's re-evaluation.
        builder.set_ttl(86_400);

        let message = match builder.build() {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(endpoint, "push: message build/encrypt failed: {e}");
                return SendOutcome::Failed;
            }
        };

        let mut req = inner
            .http
            .post(message.endpoint.to_string())
            .header("TTL", message.ttl.to_string());

        if let Some(payload) = message.payload {
            req = req
                .header("Content-Encoding", payload.content_encoding.to_str())
                .header("Content-Type", "application/octet-stream");
            for (k, v) in payload.crypto_headers {
                req = req.header(k, v);
            }
            req = req.body(payload.content);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    SendOutcome::Sent
                } else if status.as_u16() == 404 || status.as_u16() == 410 {
                    SendOutcome::Gone
                } else if status.as_u16() == 429 {
                    SendOutcome::RateLimited
                } else if status.is_server_error() {
                    SendOutcome::Retryable
                } else {
                    tracing::warn!(endpoint, %status, "push: send failed");
                    SendOutcome::Failed
                }
            }
            Err(e) => {
                tracing::error!(endpoint, "push: http error: {e}");
                SendOutcome::Retryable
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn spawn_with_no_env_var_is_disabled() {
        // SAFETY: no other test in this module mutates VAPID_PRIVATE_KEY, and
        // this crate's test binaries don't run this file's tests in parallel
        // with another that does (no cross-file env sharing at the process
        // level for cargo test's per-binary isolation... but to be safe we
        // never set it, only ensure-removed).
        unsafe {
            std::env::remove_var("VAPID_PRIVATE_KEY");
        }
        let handle = PushHandle::spawn();
        assert!(handle.public_key().is_none());
    }

    #[test]
    fn from_base64_private_key_with_the_test_fixture_is_enabled() {
        let handle = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let key = handle.public_key().expect("must be enabled");
        // Uncompressed P-256 point: 65 bytes -> 87 base64url-no-pad chars.
        assert_eq!(key.len(), 87);
    }

    #[test]
    fn from_base64_private_key_with_garbage_is_disabled() {
        let handle = PushHandle::from_base64_private_key("not-a-valid-vapid-key");
        assert!(handle.public_key().is_none());
    }

    #[tokio::test]
    async fn disabled_handle_send_returns_failed_without_a_network_call() {
        let handle = PushHandle::from_base64_private_key("");
        let outcome = handle
            .send("https://push.example/x", "p", "a", "t", "b")
            .await;
        assert_eq!(outcome, SendOutcome::Failed);
    }

    #[tokio::test]
    async fn send_maps_201_to_sent() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/wpush/ok");
                then.status(201);
            })
            .await;
        let handle = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let outcome = handle
            .send(
                &server.url("/wpush/ok"),
                TEST_P256DH_B64,
                TEST_AUTH_B64,
                "t",
                "b",
            )
            .await;
        assert_eq!(outcome, SendOutcome::Sent);
        assert_eq!(mock.hits_async().await, 1);
    }

    #[tokio::test]
    async fn send_maps_410_to_gone() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/wpush/gone");
                then.status(410);
            })
            .await;
        let handle = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let outcome = handle
            .send(
                &server.url("/wpush/gone"),
                TEST_P256DH_B64,
                TEST_AUTH_B64,
                "t",
                "b",
            )
            .await;
        assert_eq!(outcome, SendOutcome::Gone);
    }

    #[tokio::test]
    async fn send_maps_404_to_gone() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/wpush/missing");
                then.status(404);
            })
            .await;
        let handle = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let outcome = handle
            .send(
                &server.url("/wpush/missing"),
                TEST_P256DH_B64,
                TEST_AUTH_B64,
                "t",
                "b",
            )
            .await;
        assert_eq!(outcome, SendOutcome::Gone);
    }

    #[tokio::test]
    async fn send_maps_429_to_rate_limited() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/wpush/busy");
                then.status(429);
            })
            .await;
        let handle = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let outcome = handle
            .send(
                &server.url("/wpush/busy"),
                TEST_P256DH_B64,
                TEST_AUTH_B64,
                "t",
                "b",
            )
            .await;
        assert_eq!(outcome, SendOutcome::RateLimited);
    }

    #[tokio::test]
    async fn send_maps_500_to_retryable() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/wpush/err");
                then.status(500);
            })
            .await;
        let handle = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let outcome = handle
            .send(
                &server.url("/wpush/err"),
                TEST_P256DH_B64,
                TEST_AUTH_B64,
                "t",
                "b",
            )
            .await;
        assert_eq!(outcome, SendOutcome::Retryable);
    }

    #[tokio::test]
    async fn send_includes_the_vapid_authorization_header() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/wpush/auth")
                    .header_exists("authorization");
                then.status(201);
            })
            .await;
        let handle = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let outcome = handle
            .send(
                &server.url("/wpush/auth"),
                TEST_P256DH_B64,
                TEST_AUTH_B64,
                "t",
                "b",
            )
            .await;
        assert_eq!(outcome, SendOutcome::Sent);
        assert_eq!(mock.hits_async().await, 1);
    }
}
