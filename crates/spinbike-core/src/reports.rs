//! Shared types for the /api/reports/* endpoints. Serialized to JSON on the
//! server and deserialized on the WASM client.

use serde::{Deserialize, Serialize};

/// Totals for a day or a date range.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KpiSummary {
    pub revenue_eur: f64,
    pub attendance: i64,
    pub passes_sold: i64,
    pub cash_in_eur: f64,
}

/// One row in the activity feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEvent {
    pub id: i64,
    pub card_id: Option<i64>,
    pub card_name: Option<String>,
    pub barcode: Option<String>,
    pub action: String,
    pub amount: f64,
    pub service_name: Option<String>,
    pub created_at: String,
    pub valid_until: Option<chrono::NaiveDate>,
    pub voided: bool,
}

/// Classification for UI colour/icon logic. Derived server-side from the event.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Charge,   // amount < 0 AND valid_until IS NULL
    TopUp,    // amount > 0
    PassSold, // valid_until IS NOT NULL
    Other,
}

impl ReportEvent {
    pub fn kind(&self) -> EventKind {
        if self.valid_until.is_some() {
            EventKind::PassSold
        } else if self.amount < 0.0 {
            EventKind::Charge
        } else if self.amount > 0.0 {
            EventKind::TopUp
        } else {
            EventKind::Other
        }
    }
}

/// Response from GET /api/reports/day and /api/reports/range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportResponse {
    pub kpi: KpiSummary,
    pub events: Vec<ReportEvent>,
    pub alerts_count: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpiringPass {
    pub card_id: i64,
    pub name: String,
    pub barcode: String,
    pub valid_until: chrono::NaiveDate,
    pub days_left: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LowCreditCard {
    pub card_id: i64,
    pub name: String,
    pub barcode: String,
    pub credit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InactiveCustomer {
    pub card_id: i64,
    pub name: String,
    pub barcode: String,
    pub last_visit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertsResponse {
    pub expiring_passes: Vec<ExpiringPass>,
    pub low_credit: Vec<LowCreditCard>,
    pub inactive: Vec<InactiveCustomer>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RosterStatus {
    Booked,
    CheckedIn,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterEntry {
    pub card_id: Option<i64>,
    pub name: String,
    pub barcode: Option<String>,
    pub booking_id: i64,
    pub status: RosterStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentClass {
    pub template_id: i64,
    pub date: chrono::NaiveDate,
    pub start_time: String, // "HH:MM"
    pub service_name: String,
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub roster: Vec<RosterEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextClass {
    pub template_id: i64,
    pub date: chrono::NaiveDate,
    pub start_time: String,
    pub service_name: String,
    pub instructor_name: Option<String>,
    pub booked: i64,
    pub capacity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowResponse {
    pub current_class: Option<CurrentClass>,
    pub next_class: Option<NextClass>,
}
