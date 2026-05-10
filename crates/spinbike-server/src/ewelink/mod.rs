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
pub mod crypto;
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

    #[tokio::test]
    async fn disabled_when_env_unset() {
        // SAFETY: set_var / remove_var are unsafe in 2024 edition.
        unsafe {
            std::env::remove_var("EWELINK_EMAIL");
            std::env::remove_var("EWELINK_PASSWORD");
            std::env::remove_var("EWELINK_DEVICE_ID");
            std::env::remove_var("EWELINK_TEST_MODE");
        }
        let h = EwelinkHandle::spawn();
        assert_eq!(h.state(), EwelinkState::Disabled);
        let res = h.press().await;
        assert!(matches!(res, Err(EwelinkError::Disabled)), "got {res:?}");
    }
}
