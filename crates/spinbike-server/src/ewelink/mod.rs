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

/// Consecutive failed presses that mean the door is genuinely broken
/// rather than momentarily unlucky. Two, not one: a single press can fail
/// on a transient cloud hiccup and the customer just presses again, while
/// #353 failed EVERY press for two days.
pub const FAULT_THRESHOLD: u32 = 2;

/// Cloneable handle. `press()` is `&self` so multiple route handlers
/// share one handle through axum state.
#[derive(Clone)]
pub struct EwelinkHandle {
    tx: Option<mpsc::Sender<PressRequest>>,
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    last_ack_ms: std::sync::Arc<std::sync::atomic::AtomicI64>,
    /// Wall-clock ms of the last press ATTEMPT that actually reached the
    /// task (the Disabled fast-path does not count). `i64::MIN` = never.
    last_press_ms: std::sync::Arc<std::sync::atomic::AtomicI64>,
    /// Presses that failed since the last successful one. Reset to 0 by
    /// every success. This is the signal #353 lacked: on its own,
    /// `last_ack_ms_ago: null` cannot tell "nobody has pressed since the
    /// last restart" apart from "every press since the restart failed".
    failed_presses: std::sync::Arc<std::sync::atomic::AtomicU32>,
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
        let press_ms = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(i64::MIN));
        let failed = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));

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
                last_press_ms: press_ms,
                failed_presses: failed,
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
                last_press_ms: press_ms,
                failed_presses: failed,
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
            last_press_ms: press_ms,
            failed_presses: failed,
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
            // Disabled is a configuration state, not a fault: a dev box or a
            // deliberate kill switch must never look like broken hardware.
            return Err(EwelinkError::Disabled);
        };
        self.last_press_ms.store(
            chrono::Utc::now().timestamp_millis(),
            std::sync::atomic::Ordering::Relaxed,
        );
        let (ack_tx, ack_rx) = oneshot::channel();
        if tx.send(PressRequest { ack: ack_tx }).await.is_err() {
            return self.record(Err(EwelinkError::Network(
                "ewelink task channel closed".into(),
            )));
        }
        let outcome = match tokio::time::timeout(std::time::Duration::from_secs(5), ack_rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_recv)) => Err(EwelinkError::Network("ack oneshot dropped".into())),
            Err(_) => Err(EwelinkError::DeviceTimeout),
        };
        self.record(outcome)
    }

    /// Fold a press outcome into the failure counter and hand it back
    /// untouched. Success clears the counter; anything else advances it.
    fn record(&self, outcome: Result<(), EwelinkError>) -> Result<(), EwelinkError> {
        use std::sync::atomic::Ordering::Relaxed;
        if outcome.is_ok() {
            self.failed_presses.store(0, Relaxed);
        } else {
            // Saturating: a door left broken for weeks must not wrap around
            // to 0 and read as healthy.
            let _ = self
                .failed_presses
                .fetch_update(Relaxed, Relaxed, |n| Some(n.saturating_add(1)));
        }
        outcome
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

    /// Test-only setter so unit tests can drive `state()` through every
    /// variant including `Disconnected` without spinning up a real WS.
    /// Production has only two writers: `run_real_ws` and `run_test_stub`.
    #[cfg(test)]
    pub(crate) fn set_state_for_test(&self, s: EwelinkState) {
        self.state
            .store(s as u8, std::sync::atomic::Ordering::Relaxed);
    }

    /// Milliseconds since the last successful ack. `None` if never acked.
    pub fn last_ack_ms_ago(&self) -> Option<i64> {
        Self::ms_ago(&self.last_ack_ms)
    }

    /// Milliseconds since the last press attempt that reached the task.
    /// `None` if nobody has pressed since this process started.
    pub fn last_press_ms_ago(&self) -> Option<i64> {
        Self::ms_ago(&self.last_press_ms)
    }

    /// Presses that have failed since the last successful one.
    pub fn failed_presses(&self) -> u32 {
        self.failed_presses
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The published door-fault verdict: `failed_presses` has reached
    /// `FAULT_THRESHOLD`. Shared by `GET /api/door/health` (which surfaces it
    /// as the `faulty` field) and the `jobs::door_health` alert job (#355), so
    /// both agree on exactly what "faulty" means. A `Disabled` handle never
    /// records a failure (`press()` short-circuits before `record()`), so this
    /// stays `false` on a dev box or a deliberate kill switch — a
    /// configuration state must never read as broken hardware.
    pub fn is_faulty(&self) -> bool {
        self.failed_presses() >= FAULT_THRESHOLD
    }

    fn ms_ago(cell: &std::sync::atomic::AtomicI64) -> Option<i64> {
        let ts = cell.load(std::sync::atomic::Ordering::Relaxed);
        if ts == i64::MIN {
            None
        } else {
            Some(chrono::Utc::now().timestamp_millis() - ts)
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

    /// Drives every variant of `state()`. Catches the Disconnected match-arm
    /// mutation (the integration-only tests don't normally exercise the
    /// Disconnected branch because the test stub jumps straight to Connected).
    #[tokio::test]
    async fn state_returns_each_variant() {
        with_clean_env(|| async {
            let h = EwelinkHandle::spawn();
            // initial — Disabled
            h.set_state_for_test(EwelinkState::Disabled);
            assert_eq!(h.state(), EwelinkState::Disabled);
            h.set_state_for_test(EwelinkState::Connected);
            assert_eq!(h.state(), EwelinkState::Connected);
            h.set_state_for_test(EwelinkState::Disconnected);
            assert_eq!(h.state(), EwelinkState::Disconnected);
            // Back to Disabled to confirm round-trip.
            h.set_state_for_test(EwelinkState::Disabled);
            assert_eq!(h.state(), EwelinkState::Disabled);
        })
        .await;
    }

    /// A Disabled handle is a CONFIGURATION state, not a fault. A dev box
    /// or a deliberate kill switch must never accumulate failures and read
    /// as broken hardware (#355).
    #[tokio::test]
    async fn a_disabled_handle_never_counts_a_failure() {
        with_clean_env(|| async {
            let h = EwelinkHandle::spawn();
            for _ in 0..5 {
                assert!(matches!(h.press().await, Err(EwelinkError::Disabled)));
            }
            assert_eq!(h.failed_presses(), 0);
            assert_eq!(h.last_press_ms_ago(), None);
        })
        .await;
    }

    /// The signal #353 lacked: failures accumulate until a success clears
    /// them, so "every press since the restart failed" is distinguishable
    /// from "nobody has pressed".
    #[tokio::test]
    async fn failures_accumulate_and_a_success_clears_them() {
        with_clean_env(|| async {
            // SAFETY: under EWELINK_TEST_LOCK held inside with_clean_env.
            unsafe { std::env::set_var("EWELINK_TEST_MODE", "offline") }
            let h = EwelinkHandle::spawn();
            assert_eq!(h.failed_presses(), 0, "fresh handle starts clean");

            assert!(h.press().await.is_err());
            assert_eq!(h.failed_presses(), 1);
            assert!(
                h.failed_presses() < FAULT_THRESHOLD,
                "one failure is not yet a fault"
            );

            assert!(h.press().await.is_err());
            assert_eq!(h.failed_presses(), 2);
            assert!(
                h.failed_presses() >= FAULT_THRESHOLD,
                "two consecutive failures are a fault"
            );

            assert!(h.press().await.is_err());
            assert_eq!(h.failed_presses(), 3, "counts past the threshold");
        })
        .await;
    }

    /// `is_faulty()` is the verdict shared with `/api/door/health` and the
    /// door-health alert job (#355): false below `FAULT_THRESHOLD`, true at or
    /// above it. Pins the `>=` boundary so a `>=`→`>` mutation (which would
    /// need THREE failures before alerting) is caught.
    #[tokio::test]
    async fn is_faulty_flips_exactly_at_the_fault_threshold() {
        with_clean_env(|| async {
            // SAFETY: under EWELINK_TEST_LOCK held inside with_clean_env.
            unsafe { std::env::set_var("EWELINK_TEST_MODE", "offline") }
            let h = EwelinkHandle::spawn();
            assert!(!h.is_faulty(), "fresh handle is not faulty");

            assert!(h.press().await.is_err());
            assert_eq!(h.failed_presses(), 1);
            assert!(
                !h.is_faulty(),
                "one failure (< FAULT_THRESHOLD) must NOT read as faulty"
            );

            assert!(h.press().await.is_err());
            assert_eq!(h.failed_presses(), FAULT_THRESHOLD);
            assert!(
                h.is_faulty(),
                "reaching FAULT_THRESHOLD failures must read as faulty"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn a_successful_press_clears_the_failure_count_and_stamps_both_clocks() {
        with_clean_env(|| async {
            // SAFETY: under EWELINK_TEST_LOCK held inside with_clean_env.
            unsafe { std::env::set_var("EWELINK_TEST_MODE", "success") }
            let h = EwelinkHandle::spawn();
            assert_eq!(h.last_press_ms_ago(), None, "nothing pressed yet");

            assert!(h.press().await.is_ok());
            assert_eq!(h.failed_presses(), 0);
            assert!(
                h.last_press_ms_ago()
                    .is_some_and(|ms| (0..5_000).contains(&ms)),
                "press clock stamped: {:?}",
                h.last_press_ms_ago()
            );
            assert!(
                h.last_ack_ms_ago()
                    .is_some_and(|ms| (0..5_000).contains(&ms)),
                "ack clock stamped: {:?}",
                h.last_ack_ms_ago()
            );
        })
        .await;
    }

    /// A press that reaches the task and fails must still stamp the press
    /// clock — otherwise the health endpoint keeps reporting "nobody has
    /// pressed" while every press is in fact failing, which is the exact
    /// blind spot that hid #353.
    #[tokio::test]
    async fn a_failed_press_still_stamps_the_press_clock() {
        with_clean_env(|| async {
            // SAFETY: under EWELINK_TEST_LOCK held inside with_clean_env.
            unsafe { std::env::set_var("EWELINK_TEST_MODE", "offline") }
            let h = EwelinkHandle::spawn();
            assert!(h.press().await.is_err());
            assert!(
                h.last_press_ms_ago()
                    .is_some_and(|ms| (0..5_000).contains(&ms)),
                "press clock stamped even on failure: {:?}",
                h.last_press_ms_ago()
            );
            assert_eq!(h.last_ack_ms_ago(), None, "no ack ever arrived");
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
            // Before any press: None. (spawn is sync; the stub task hasn't
            // necessarily run yet, but last_ack_ms is initialised to the
            // sentinel before spawn returns, so this is safe to assert now.)
            assert_eq!(
                h.last_ack_ms_ago(),
                None,
                "no presses yet → last_ack_ms_ago should be None"
            );
            // Successful press → drives the stub task to completion of its
            // first iteration (sets state=Connected before reading rx) AND
            // updates last_ack_ms_ago to ≈ Utc::now_ms.
            h.press().await.expect("press should succeed in test stub");
            // Now the stub has definitely run — state must be Connected.
            assert_eq!(h.state(), EwelinkState::Connected);
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
