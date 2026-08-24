//! Door-health fault alerting (#355) — RED skeleton.
//!
//! Public surface only, no behaviour yet: `tick` never sends. The
//! `tests/door_health.rs` episode tests therefore FAIL against this commit
//! (they expect `FaultAlerted`/`Recovered`, get `Steady`). The GREEN commit
//! fills in the real fault/recovery e-mail + anti-spam dedup.

use crate::mail::MailHandle;
use sqlx::SqlitePool;

/// The outcome of one `DoorHealthMonitor::tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorHealthTick {
    Steady,
    FaultAlerted { recipients: usize },
    FaultAlertPending,
    Recovered { recipients: usize },
    RecoveryPending,
}

/// In-memory anti-spam state for the door-fault alert.
pub struct DoorHealthMonitor {
    _alerted: bool,
}

impl Default for DoorHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl DoorHealthMonitor {
    pub fn new() -> Self {
        Self { _alerted: false }
    }

    pub async fn tick(
        &mut self,
        _now_faulty: bool,
        _mail: &MailHandle,
        _pool: &SqlitePool,
    ) -> DoorHealthTick {
        DoorHealthTick::Steady
    }
}
