//! eWeLink WebSocket dispatch loop.
//!
//! `run_real_ws` is the production task spawned by `EwelinkHandle::spawn()`:
//! it logs into the eWeLink Open API, opens a persistent WS to
//! `wss://{region}-dispa.coolkit.cc:8080/dispatch/app`, and translates
//! `PressRequest`s coming in over an mpsc into `update`-action frames
//! with `params.switch = "on"`. Acks are routed back to the caller's
//! oneshot via a HashMap keyed by the per-press `sequence` id.
//!
//! The loop transparently reconnects with exponential backoff (1 → 2 →
//! 4 → 8 → 30 s cap) on any error or close. State + last-ack timestamp
//! atomics surface to the /api/door/health endpoint.
//!
//! `connect_loop_with_url` is the inner unit — extracted so the
//! integration test in tests/ewelink_ws.rs can target a tokio-tungstenite
//! mock server and skip the real `auth::login` round-trip.

use crate::ewelink::{EwelinkError, EwelinkState, PressRequest, auth};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const APP_ID: &str = "oeVkj2lYFGnJu5XUtWisfW4utiN4u9Mq";

/// Production task. Logs in, then drives reconnect cycles until the mpsc
/// receiver closes (server shutdown). Backoff: 1, 2, 4, 8, 30 s cap.
///
/// Excluded from mutation testing — this fn is a thin reconnect wrapper
/// over `connect_loop_with_url_inner` (whose internals are unit-tested
/// individually) plus `auth::login` (which has its own httpmock tests
/// at `crates/spinbike-server/src/ewelink/auth.rs`). Exercising
/// `run_real_ws` itself would require either a real eWeLink cloud
/// connection (breaks CI hermeticity) or a non-trivial trait-object
/// refactor for one mutant. Tradeoff isn't worth it.
#[mutants::skip]
pub async fn run_real_ws(
    mut rx: mpsc::Receiver<PressRequest>,
    email: String,
    password: String,
    device_id: String,
    state: Arc<AtomicU8>,
    last_ack_ms: Arc<AtomicI64>,
) {
    state.store(EwelinkState::Disconnected as u8, Ordering::Relaxed);

    let mut backoff = Duration::from_secs(1);

    loop {
        // If the mpsc closed while we were sleeping or logging in, exit.
        if rx.is_closed() {
            tracing::info!("ewelink: rx closed, shutting down WS task");
            return;
        }

        tracing::info!("ewelink: logging in");
        let login = match auth::login(&email, &password, None).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(err = %e, ?backoff, "ewelink: login failed, retrying");
                state.store(EwelinkState::Disconnected as u8, Ordering::Relaxed);
                tokio::time::sleep(backoff).await;
                if rx.is_closed() {
                    tracing::info!("ewelink: rx closed during backoff, shutting down");
                    return;
                }
                backoff = next_backoff(backoff);
                continue;
            }
        };
        backoff = Duration::from_secs(1); // reset after success

        let url = format!("wss://{}-dispa.coolkit.cc:8080/dispatch/app", login.region);
        tracing::info!(
            region = %login.region,
            url = %url,
            "ewelink: connecting to dispatch WS"
        );

        // connect_loop_with_url returns when the connection drops or fails.
        // It moves rx in and gives it back so the outer loop can keep going.
        let outcome = connect_loop_with_url_inner(
            &url,
            &login.access_token,
            &login.apikey,
            &device_id,
            state.clone(),
            last_ack_ms.clone(),
            &mut rx,
        )
        .await;

        state.store(EwelinkState::Disconnected as u8, Ordering::Relaxed);

        match outcome {
            ConnectOutcome::ChannelClosed => {
                tracing::info!("ewelink: rx closed, shutting down WS task");
                return;
            }
            ConnectOutcome::ConnectionLost(reason) => {
                tracing::warn!(
                    reason = %reason,
                    ?backoff,
                    "ewelink: connection lost, reconnecting after backoff"
                );
                tokio::time::sleep(backoff).await;
                if rx.is_closed() {
                    tracing::info!("ewelink: rx closed during backoff, shutting down");
                    return;
                }
                backoff = next_backoff(backoff);
            }
        }
    }
}

/// Exponential backoff: double the current delay, capping at 30 s.
/// Extracted as a `pub` pure fn so the arithmetic (the `*` operator
/// and the 30 s cap) is unit-testable without spinning up the WS task.
pub fn next_backoff(current: Duration) -> Duration {
    const MAX_BACKOFF: Duration = Duration::from_secs(30);
    (current * 2).min(MAX_BACKOFF)
}

#[derive(Debug)]
enum ConnectOutcome {
    /// mpsc receiver closed (server shutdown).
    ChannelClosed,
    /// Connection failed or dropped — outer loop should reconnect.
    ConnectionLost(String),
}

/// Public entry-point used by the integration test. Skips `auth::login`
/// — caller passes `access_token` and `apikey` directly. Owns the rx for
/// the duration of one connection; returns when the connection drops or
/// the channel closes.
pub async fn connect_loop_with_url(
    url: &str,
    access_token: &str,
    apikey: &str,
    device_id: &str,
    state: Arc<AtomicU8>,
    last_ack_ms: Arc<AtomicI64>,
    mut rx: mpsc::Receiver<PressRequest>,
) {
    let _ = connect_loop_with_url_inner(
        url,
        access_token,
        apikey,
        device_id,
        state,
        last_ack_ms,
        &mut rx,
    )
    .await;
}

/// Internal version that borrows rx so `run_real_ws` can reuse it across
/// reconnects. Returns the reason the loop ended.
async fn connect_loop_with_url_inner(
    url: &str,
    access_token: &str,
    apikey: &str,
    device_id: &str,
    state: Arc<AtomicU8>,
    last_ack_ms: Arc<AtomicI64>,
    rx: &mut mpsc::Receiver<PressRequest>,
) -> ConnectOutcome {
    let (mut ws, _resp) = match connect_async(url).await {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!(err = %e, %url, "ewelink: connect_async failed");
            return ConnectOutcome::ConnectionLost(format!("connect_async: {e}"));
        }
    };
    tracing::info!(%url, "ewelink: WS handshake (TCP+TLS) ok, sending userOnline");

    // 1) userOnline handshake
    let nonce = random_nonce();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let now_s = chrono::Utc::now().timestamp();
    let user_online = json!({
        "action": "userOnline",
        "at": access_token,
        "apikey": apikey,
        "appid": APP_ID,
        "nonce": nonce,
        "ts": now_s,
        "version": 8,
        "sequence": now_ms.to_string(),
    });
    if let Err(e) = ws.send(Message::Text(user_online.to_string())).await {
        tracing::warn!(err = %e, "ewelink: failed to send userOnline");
        return ConnectOutcome::ConnectionLost(format!("send userOnline: {e}"));
    }

    // Wait for the userOnline reply. It must include {"error":0,...}.
    match ws.next().await {
        Some(Ok(Message::Text(txt))) => match serde_json::from_str::<Value>(&txt) {
            Ok(v) => {
                // Use Option<i64> directly so the "absent error field" case
                // and the "non-zero error code" case both reject without a
                // sentinel magic number.
                let err_code = v.get("error").and_then(|e| e.as_i64());
                if err_code != Some(0) {
                    let display = err_code
                        .map_or_else(|| "missing".to_string(), |c| c.to_string());
                    tracing::warn!(
                        code = %display,
                        body = %txt,
                        "ewelink: userOnline rejected"
                    );
                    return ConnectOutcome::ConnectionLost(format!("userOnline error {display}"));
                }
                tracing::info!("ewelink: WS connected + handshake ok");
            }
            Err(e) => {
                tracing::warn!(err = %e, body = %txt, "ewelink: userOnline parse failed");
                return ConnectOutcome::ConnectionLost(format!("parse userOnline: {e}"));
            }
        },
        Some(Ok(other)) => {
            tracing::warn!(?other, "ewelink: unexpected first WS frame");
            return ConnectOutcome::ConnectionLost("unexpected first frame".into());
        }
        Some(Err(e)) => {
            tracing::warn!(err = %e, "ewelink: WS error during userOnline");
            return ConnectOutcome::ConnectionLost(format!("ws err during userOnline: {e}"));
        }
        None => {
            tracing::warn!("ewelink: WS closed during userOnline");
            return ConnectOutcome::ConnectionLost("closed during userOnline".into());
        }
    }

    state.store(EwelinkState::Connected as u8, Ordering::Relaxed);

    // 2) Main dispatch loop.
    //
    // Pending presses keyed by the `sequence` id we send to the cloud. A
    // timer per press sweeps stale entries after 10 s; the caller's own
    // 5 s timeout (in EwelinkHandle::press) is what surfaces the failure
    // to the HTTP route.
    let mut pending: HashMap<String, oneshot::Sender<Result<(), EwelinkError>>> = HashMap::new();
    let (sweep_tx, mut sweep_rx) = mpsc::unbounded_channel::<String>();

    let mut ping_interval = tokio::time::interval(Duration::from_secs(60));
    // First tick fires immediately by default — skip it so we don't
    // ping right after userOnline.
    ping_interval.tick().await;

    loop {
        tokio::select! {
            // Press request from the rest of the server.
            press = rx.recv() => {
                let Some(req) = press else {
                    // mpsc closed → exit cleanly.
                    let _ = ws.send(Message::Close(None)).await;
                    return ConnectOutcome::ChannelClosed;
                };
                let sequence = chrono::Utc::now().timestamp_millis().to_string();
                let frame = json!({
                    "action": "update",
                    "deviceid": device_id,
                    "apikey": apikey,
                    "sequence": sequence,
                    "params": { "switch": "on" },
                    "selfApikey": apikey,
                });
                tracing::debug!(%sequence, %device_id, "ewelink: press sent");
                if let Err(e) = ws.send(Message::Text(frame.to_string())).await {
                    tracing::warn!(err = %e, "ewelink: failed to send update frame");
                    let _ = req.ack.send(Err(EwelinkError::Network(format!("send: {e}"))));
                    return ConnectOutcome::ConnectionLost(format!("send update: {e}"));
                }
                pending.insert(sequence.clone(), req.ack);
                // Sweep the entry after 10 s if the cloud never replies.
                let sweep = sweep_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    let _ = sweep.send(sequence);
                });
            }

            // Stale-entry sweep.
            stale = sweep_rx.recv() => {
                if let Some(sequence) = stale
                    && pending.remove(&sequence).is_some()
                {
                    tracing::debug!(%sequence, "ewelink: sweeping stale pending entry");
                }
            }

            // Outgoing ping every 60 s.
            _ = ping_interval.tick() => {
                let ping = json!({"action": "ping"}).to_string();
                if let Err(e) = ws.send(Message::Text(ping)).await {
                    tracing::warn!(err = %e, "ewelink: ping send failed");
                    return ConnectOutcome::ConnectionLost(format!("ping: {e}"));
                }
                tracing::trace!("ewelink: keepalive ping sent");
            }

            // Incoming WS frame.
            frame = ws.next() => {
                match frame {
                    Some(Ok(Message::Text(txt))) => {
                        handle_text_frame(&txt, &mut pending, &last_ack_ms);
                    }
                    Some(Ok(Message::Binary(_))) => {
                        tracing::trace!("ewelink: ignoring binary frame");
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        // tungstenite normally auto-pongs, but be defensive.
                        let _ = ws.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(frame))) => {
                        tracing::warn!(?frame, "ewelink: peer sent close frame");
                        return ConnectOutcome::ConnectionLost("peer close".into());
                    }
                    Some(Ok(Message::Frame(_))) => {}
                    Some(Err(e)) => {
                        tracing::error!(err = %e, "ewelink: ws error, will reconnect");
                        return ConnectOutcome::ConnectionLost(format!("ws err: {e}"));
                    }
                    None => {
                        tracing::error!("ewelink: ws stream ended, will reconnect");
                        return ConnectOutcome::ConnectionLost("stream ended".into());
                    }
                }
            }
        }
    }
}

/// Parse a text frame and route any ack to the matching pending oneshot.
pub(crate) fn handle_text_frame(
    txt: &str,
    pending: &mut HashMap<String, oneshot::Sender<Result<(), EwelinkError>>>,
    last_ack_ms: &Arc<AtomicI64>,
) {
    let v: Value = match serde_json::from_str(txt) {
        Ok(v) => v,
        Err(e) => {
            tracing::trace!(err = %e, body = %txt, "ewelink: non-JSON frame");
            return;
        }
    };

    let sequence = match v.get("sequence").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => {
            tracing::trace!(body = %txt, "ewelink: frame without sequence (broadcast?)");
            return;
        }
    };
    let error_code = v.get("error").and_then(|e| e.as_i64());
    let Some(ack_tx) = pending.remove(&sequence) else {
        tracing::trace!(%sequence, "ewelink: ack for unknown/swept sequence");
        return;
    };

    tracing::debug!(%sequence, ?error_code, "ewelink: ack received");

    let result = match error_code {
        Some(0) => {
            last_ack_ms.store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
            Ok(())
        }
        Some(code) if is_offline_code(code) => Err(EwelinkError::DeviceOffline),
        Some(code) => Err(EwelinkError::BadResponse(format!("error {code}"))),
        None => Err(EwelinkError::BadResponse(format!(
            "ack without error field: {txt}"
        ))),
    };
    let _ = ack_tx.send(result);
}

/// Map known eWeLink error codes to `DeviceOffline`. The cloud uses 503
/// for offline devices; the rest are treated as bad-response so the
/// caller surfaces the exact code in tracing.
///
/// `pub` so unit tests can lock down the 503-vs-everything-else boundary
/// without simulating a full WS round-trip.
pub fn is_offline_code(code: i64) -> bool {
    matches!(code, 503)
}

/// 8-char base36 nonce. `pub` so unit tests can assert uniqueness +
/// character set without driving the WS task.
pub fn random_nonce() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

/// Test-seam stub. Real impl here — used by Playwright E2E.
pub async fn run_test_stub(
    mut rx: mpsc::Receiver<PressRequest>,
    mode: String,
    state: Arc<AtomicU8>,
    last_ack_ms: Arc<AtomicI64>,
) {
    state.store(
        crate::ewelink::EwelinkState::Connected as u8,
        std::sync::atomic::Ordering::Relaxed,
    );
    while let Some(req) = rx.recv().await {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let result = match mode.as_str() {
            "success" => {
                last_ack_ms.store(
                    chrono::Utc::now().timestamp_millis(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                Ok(())
            }
            "timeout" => {
                // Caller's 5 s timeout fires before we reply.
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                Ok(())
            }
            "offline" => Err(crate::ewelink::EwelinkError::DeviceOffline),
            _ => Err(crate::ewelink::EwelinkError::BadResponse(format!(
                "unknown EWELINK_TEST_MODE={mode}"
            ))),
        };
        let _ = req.ack.send(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_offline_code(503)` MUST be true; every other code MUST be false.
    /// Catches:
    ///   * the L368 constant-return mutations (true / false)
    ///   * the L355 match-guard mutations (true / false) — because
    ///     handle_text_frame routes to DeviceOffline only via this fn.
    #[test]
    fn is_offline_code_503_only() {
        assert!(is_offline_code(503), "503 must be classified offline");
        assert!(!is_offline_code(0), "0 (success) is not offline");
        assert!(!is_offline_code(401), "401 (auth) is not offline");
        assert!(!is_offline_code(404), "404 is not offline");
        assert!(!is_offline_code(500), "500 is not offline");
        assert!(!is_offline_code(502), "502 is not offline");
        assert!(!is_offline_code(504), "504 is not offline");
        assert!(!is_offline_code(-1), "-1 is not offline");
    }

    /// Drive `handle_text_frame` end-to-end via the offline / bad-response
    /// branches. Catches the L355 match-guard mutations (`true` would
    /// route 500 to DeviceOffline; `false` would route 503 to BadResponse).
    #[tokio::test]
    async fn handle_text_frame_routes_503_to_device_offline() {
        let mut pending: HashMap<String, oneshot::Sender<Result<(), EwelinkError>>> =
            HashMap::new();
        let last_ack_ms = Arc::new(AtomicI64::new(i64::MIN));

        // 503 — offline.
        let (tx, rx) = oneshot::channel();
        pending.insert("seq-503".into(), tx);
        handle_text_frame(
            r#"{"sequence":"seq-503","error":503}"#,
            &mut pending,
            &last_ack_ms,
        );
        let res = rx.await.expect("oneshot");
        assert!(
            matches!(res, Err(EwelinkError::DeviceOffline)),
            "got {res:?}"
        );

        // 500 — bad response, not offline.
        let (tx, rx) = oneshot::channel();
        pending.insert("seq-500".into(), tx);
        handle_text_frame(
            r#"{"sequence":"seq-500","error":500}"#,
            &mut pending,
            &last_ack_ms,
        );
        let res = rx.await.expect("oneshot");
        match res {
            Err(EwelinkError::BadResponse(msg)) => {
                assert!(
                    msg.contains("error 500"),
                    "msg should mention error 500, got {msg:?}"
                );
            }
            other => panic!("expected BadResponse, got {other:?}"),
        }

        // 0 — success, updates last_ack_ms.
        let (tx, rx) = oneshot::channel();
        pending.insert("seq-0".into(), tx);
        let before = last_ack_ms.load(Ordering::Relaxed);
        handle_text_frame(
            r#"{"sequence":"seq-0","error":0}"#,
            &mut pending,
            &last_ack_ms,
        );
        let res = rx.await.expect("oneshot");
        assert!(res.is_ok(), "got {res:?}");
        let after = last_ack_ms.load(Ordering::Relaxed);
        assert!(
            after > before,
            "last_ack_ms must advance on success: before={before} after={after}"
        );
    }

    /// random_nonce is non-empty, 8 chars, base36, and varies across calls.
    /// Catches the L372 constant-return mutations ("" / "xyzzy").
    #[test]
    fn random_nonce_varies_and_is_base36_8() {
        let a = random_nonce();
        let b = random_nonce();
        let c = random_nonce();
        assert_eq!(a.len(), 8);
        assert!(!(a == b && b == c), "three nonces identical: {a} {b} {c}");
        for ch in a.chars().chain(b.chars()).chain(c.chars()) {
            assert!(ch.is_ascii_lowercase() || ch.is_ascii_digit(), "{ch:?}");
        }
    }

    /// Backoff doubles each call and caps at 30 s.
    /// Catches the `*` → `/` and `*` → `+` mutations on the doubling step
    /// AND a hypothetical removal of the `.min(MAX_BACKOFF)` cap.
    #[test]
    fn next_backoff_doubles_then_caps_at_30s() {
        assert_eq!(next_backoff(Duration::from_secs(1)), Duration::from_secs(2));
        assert_eq!(next_backoff(Duration::from_secs(2)), Duration::from_secs(4));
        assert_eq!(next_backoff(Duration::from_secs(4)), Duration::from_secs(8));
        assert_eq!(
            next_backoff(Duration::from_secs(8)),
            Duration::from_secs(16)
        );
        assert_eq!(
            next_backoff(Duration::from_secs(15)),
            Duration::from_secs(30),
            "30 s is the cap"
        );
        assert_eq!(
            next_backoff(Duration::from_secs(16)),
            Duration::from_secs(30),
            "32 s capped at 30 s"
        );
        assert_eq!(
            next_backoff(Duration::from_secs(30)),
            Duration::from_secs(30),
            "60 s capped at 30 s"
        );
    }
}
