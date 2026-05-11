//! eWeLink cloud client for pressing a Sonoff MINI-D dry-contact relay.
//!
//! The module owns a long-lived tokio task that holds a persistent
//! WebSocket to the eWeLink cloud. Callers send `PressRequest`s over an
//! `mpsc` channel; the task relays the device ack back via a `oneshot`.
//!
//! This file contains the public surface and the Disabled fast-path.
//! Real WS / auth code lives in `ws.rs` and `auth.rs`. The Disabled
//! path runs when any of EWELINK_EMAIL / EWELINK_PASSWORD /
//! EWELINK_DEVICE_ID is empty or unset — useful for dev, CI, and as a
//! kill switch in production.

use tokio::sync::{mpsc, oneshot};

pub mod auth;
pub mod error;
pub mod ws;

pub use error::EwelinkError;

/// One press command in flight. The task replies on `ack` with Ok(()) or
/// the error encountered.
pub struct PressRequest {
    pub ack: oneshot::Sender<Result<(), EwelinkError>>,
}

/// Snapshot of the WS task's state, for the health endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EwelinkState {
    /// EWELINK_* env vars unset.
    Disabled,
    /// WS connection up; last ack within configured window.
    Connected,
    /// WS dropped or last ack missing for > 60 s. Reconnecting in background.
    Disconnected,
}

/// Cloneable handle. `press()` is `&self` so multiple route handlers
/// share one handle through axum state.
#[derive(Clone)]
pub struct EwelinkHandle {
    tx: Option<mpsc::Sender<PressRequest>>,
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    last_ack_ms: std::sync::Arc<std::sync::atomic::AtomicI64>,
}

impl EwelinkHandle {
    /// Construct and spawn the background WS task. Reads EWELINK_EMAIL /
    /// PASSWORD / DEVICE_ID / REGION / TEST_MODE from env. If any required
    /// var is empty, returns a handle in Disabled state — press() always
    /// errors with EwelinkError::Disabled. Never panics; safe to call
    /// once at server startup.
    pub fn spawn() -> Self {
        let test_mode = std::env::var("EWELINK_TEST_MODE").ok();
        let email = std::env::var("EWELINK_EMAIL").ok().unwrap_or_default();
        let password = std::env::var("EWELINK_PASSWORD").ok().unwrap_or_default();
        let device_id = std::env::var("EWELINK_DEVICE_ID").ok().unwrap_or_default();

        let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(
            EwelinkState::Disabled as u8,
        ));
        let last_ack_ms = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(i64::MIN));

        // Test seam: when EWELINK_TEST_MODE is set, hand off to an in-process
        // stub that returns the configured outcome after 100 ms. Used by E2E.
        if let Some(mode) = test_mode {
            let (tx, rx) = mpsc::channel::<PressRequest>(16);
            let state_for_task = state.clone();
            let last_ack_for_task = last_ack_ms.clone();
            let mode_clone = mode.clone();
            tokio::spawn(async move {
                ws::run_test_stub(rx, mode_clone, state_for_task, last_ack_for_task).await;
            });
            tracing::info!(test_mode = %mode, "ewelink: test-mode stub active");
            return Self {
                tx: Some(tx),
                state,
                last_ack_ms,
            };
        }

        // Production: all three required vars must be non-empty.
        if email.is_empty() || password.is_empty() || device_id.is_empty() {
            tracing::warn!(
                email_set = !email.is_empty(),
                password_set = !password.is_empty(),
                device_id_set = !device_id.is_empty(),
                "ewelink: disabled — required env vars unset"
            );
            return Self {
                tx: None,
                state,
                last_ack_ms,
            };
        }

        // Real WS task is wired up in Task 7.
        let (tx, rx) = mpsc::channel::<PressRequest>(16);
        let state_for_task = state.clone();
        let last_ack_for_task = last_ack_ms.clone();
        tokio::spawn(async move {
            ws::run_real_ws(
                rx,
                email,
                password,
                device_id,
                state_for_task,
                last_ack_for_task,
            )
            .await;
        });
        tracing::info!("ewelink: real WS task spawned");
        Self {
            tx: Some(tx),
            state,
            last_ack_ms,
        }
    }

    /// Send a press command; resolve when the device acks or errors.
    ///
    /// 5-second timeout from the caller's perspective. If the task is in
    /// Disabled state or the mpsc channel is closed (task crashed),
    /// returns `EwelinkError::Disabled` / `Network` respectively without
    /// awaiting.
    pub async fn press(&self) -> Result<(), EwelinkError> {
        let Some(tx) = &self.tx else {
            return Err(EwelinkError::Disabled);
        };
        let (ack_tx, ack_rx) = oneshot::channel();
        if tx.send(PressRequest { ack: ack_tx }).await.is_err() {
            return Err(EwelinkError::Network("ewelink task channel closed".into()));
        }
        match tokio::time::timeout(std::time::Duration::from_secs(5), ack_rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_recv)) => Err(EwelinkError::Network("ack oneshot dropped".into())),
            Err(_) => Err(EwelinkError::DeviceTimeout),
        }
    }

    /// Snapshot for /api/door/health.
    pub fn state(&self) -> EwelinkState {
        let raw = self.state.load(std::sync::atomic::Ordering::Relaxed);
        match raw {
            x if x == EwelinkState::Connected as u8 => EwelinkState::Connected,
            x if x == EwelinkState::Disconnected as u8 => EwelinkState::Disconnected,
            _ => EwelinkState::Disabled,
        }
    }

    /// Milliseconds since the last successful ack. `None` if never acked.
    pub fn last_ack_ms_ago(&self) -> Option<i64> {
        let ts = self.last_ack_ms.load(std::sync::atomic::Ordering::Relaxed);
        if ts == i64::MIN {
            None
        } else {
            let now = chrono::Utc::now().timestamp_millis();
            Some(now - ts)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Process-wide lock guarding mutations to EWELINK_* env vars in these
    /// in-crate tests. Without it, two #[tokio::test]s running concurrently
    /// race on the global env and pick up the wrong values when
    /// EwelinkHandle::spawn() reads them.
    static EWELINK_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Snapshot + clear EWELINK_* env vars, run `f`, then restore the
    /// previous values. Returns whatever `f` returns.
    async fn with_clean_env<Fut, T>(f: impl FnOnce() -> Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let _guard = EWELINK_TEST_LOCK.lock().await;
        let prior_email = std::env::var("EWELINK_EMAIL").ok();
        let prior_password = std::env::var("EWELINK_PASSWORD").ok();
        let prior_device = std::env::var("EWELINK_DEVICE_ID").ok();
        let prior_mode = std::env::var("EWELINK_TEST_MODE").ok();
        // SAFETY: process-wide lock above guarantees no concurrent mutation.
        unsafe {
            std::env::remove_var("EWELINK_EMAIL");
            std::env::remove_var("EWELINK_PASSWORD");
            std::env::remove_var("EWELINK_DEVICE_ID");
            std::env::remove_var("EWELINK_TEST_MODE");
        }
        let result = f().await;
        unsafe {
            match prior_email {
                Some(v) => std::env::set_var("EWELINK_EMAIL", v),
                None => std::env::remove_var("EWELINK_EMAIL"),
            }
            match prior_password {
                Some(v) => std::env::set_var("EWELINK_PASSWORD", v),
                None => std::env::remove_var("EWELINK_PASSWORD"),
            }
            match prior_device {
                Some(v) => std::env::set_var("EWELINK_DEVICE_ID", v),
                None => std::env::remove_var("EWELINK_DEVICE_ID"),
            }
            match prior_mode {
                Some(v) => std::env::set_var("EWELINK_TEST_MODE", v),
                None => std::env::remove_var("EWELINK_TEST_MODE"),
            }
        }
        result
    }

    #[tokio::test]
    async fn disabled_when_env_unset() {
        with_clean_env(|| async {
            let h = EwelinkHandle::spawn();
            assert_eq!(h.state(), EwelinkState::Disabled);
            let res = h.press().await;
            assert!(matches!(res, Err(EwelinkError::Disabled)), "got {res:?}");
        })
        .await;
    }

    /// Catches the `||` → `&&` mutation in `EwelinkHandle::spawn` at the
    /// env-var emptiness check: if only email is empty, the handle must
    /// still be Disabled.
    #[tokio::test]
    async fn disabled_when_only_email_unset() {
        with_clean_env(|| async {
            // SAFETY: under EWELINK_TEST_LOCK held inside with_clean_env.
            unsafe {
                std::env::set_var("EWELINK_PASSWORD", "pw");
                std::env::set_var("EWELINK_DEVICE_ID", "dev");
            }
            let h = EwelinkHandle::spawn();
            let res = h.press().await;
            assert!(matches!(res, Err(EwelinkError::Disabled)), "got {res:?}");
        })
        .await;
    }

    /// Catches `||` → `&&` at the password slot.
    #[tokio::test]
    async fn disabled_when_only_password_unset() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("EWELINK_EMAIL", "x@x");
                std::env::set_var("EWELINK_DEVICE_ID", "dev");
            }
            let h = EwelinkHandle::spawn();
            let res = h.press().await;
            assert!(matches!(res, Err(EwelinkError::Disabled)), "got {res:?}");
        })
        .await;
    }

    /// Catches `||` → `&&` at the device_id slot.
    #[tokio::test]
    async fn disabled_when_only_device_id_unset() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("EWELINK_EMAIL", "x@x");
                std::env::set_var("EWELINK_PASSWORD", "pw");
            }
            let h = EwelinkHandle::spawn();
            let res = h.press().await;
            assert!(matches!(res, Err(EwelinkError::Disabled)), "got {res:?}");
        })
        .await;
    }

    /// Exercises last_ack_ms_ago: None before any ack, then a value that
    /// reflects ACTUAL elapsed time after a successful test-mode press.
    /// This catches:
    ///   * L154 constant-return mutations (None / Some(0) / Some(1) / Some(-1))
    ///     — Some(0) / Some(1) are killed by the 200 ms sleep below; Some(-1)
    ///     by the lower bound; None by `expect(...)` upper bound.
    ///   * L155 == → != on the i64::MIN sentinel (would invert the
    ///     branch — would return Some before press and None after).
    ///   * L159 `now - ts` operator:
    ///     - → +   yields ~2 × Utc::now_ms ~ 3e12, fails upper bound.
    ///     - → /   yields ~1 (now and ts both ~current ms), fails lower bound.
    #[tokio::test]
    async fn last_ack_ms_ago_round_trip() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("EWELINK_TEST_MODE", "success");
            }
            let h = EwelinkHandle::spawn();
            // Before any press: None.
            assert_eq!(
                h.last_ack_ms_ago(),
                None,
                "no presses yet → last_ack_ms_ago should be None"
            );
            // Connected state once the stub starts.
            assert_eq!(h.state(), EwelinkState::Connected);
            // Successful press → last_ack_ms is now ≈ Utc::now_ms.
            h.press().await.expect("press should succeed in test stub");
            // Sleep so the elapsed window is detectably > 0 and < 10s.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let ms = h
                .last_ack_ms_ago()
                .expect("after successful ack last_ack_ms_ago must be Some");
            assert!(
                (100..10_000).contains(&ms),
                "elapsed should be ≥100 ms (we slept 200 ms) and <10 s, got {ms}"
            );
        })
        .await;
    }

    /// Test-stub "timeout" mode: caller's 5 s timeout should fire.
    /// Catches the L400 "timeout" match-arm deletion in run_test_stub.
    #[tokio::test]
    async fn test_stub_timeout_mode_returns_device_timeout() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("EWELINK_TEST_MODE", "timeout");
            }
            let h = EwelinkHandle::spawn();
            let res = h.press().await;
            assert!(
                matches!(res, Err(EwelinkError::DeviceTimeout)),
                "got {res:?}"
            );
        })
        .await;
    }

    /// Test-stub "offline" mode: should surface DeviceOffline immediately.
    /// Catches the L405 "offline" match-arm deletion in run_test_stub.
    #[tokio::test]
    async fn test_stub_offline_mode_returns_device_offline() {
        with_clean_env(|| async {
            unsafe {
                std::env::set_var("EWELINK_TEST_MODE", "offline");
            }
            let h = EwelinkHandle::spawn();
            let res = h.press().await;
            assert!(
                matches!(res, Err(EwelinkError::DeviceOffline)),
                "got {res:?}"
            );
        })
        .await;
    }
}
