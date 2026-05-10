//! WebSocket task — full implementation in Task 7. This file exists
//! so the module compiles after Task 5 and tests in Task 5 pass.

use crate::ewelink::PressRequest;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8};
use tokio::sync::mpsc;

/// Real production WS task. Implemented in Task 7. Stub for now.
pub async fn run_real_ws(
    mut rx: mpsc::Receiver<PressRequest>,
    _email: String,
    _password: String,
    _device_id: String,
    state: Arc<AtomicU8>,
    _last_ack_ms: Arc<AtomicI64>,
) {
    state.store(
        crate::ewelink::EwelinkState::Disconnected as u8,
        std::sync::atomic::Ordering::Relaxed,
    );
    while let Some(req) = rx.recv().await {
        let _ = req.ack.send(Err(crate::ewelink::EwelinkError::Network(
            "ws task not implemented yet (Task 7)".into(),
        )));
    }
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
