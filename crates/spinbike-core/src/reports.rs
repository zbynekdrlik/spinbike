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
    /// Slovak label for the service (NULL when the transaction has no service).
    pub service_name_sk: Option<String>,
    /// English label for the service (NULL when the transaction has no service).
    pub service_name_en: Option<String>,
    /// Stable kind enum: `"generic"` or `"monthly_pass"`. NULL when service is NULL.
    pub service_kind: Option<String>,
    pub created_at: String,
    pub valid_until: Option<chrono::NaiveDate>,
    pub voided: bool,
    /// Free-text staff note (≤200 chars). NULL when no note was recorded.
    #[serde(default)]
    pub note: Option<String>,
}

/// Classification for UI colour/icon logic. Derived from action + amount + valid_until.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    PassSale, // valid_until IS NOT NULL  (highest precedence)
    Visit,    // action == "visit"        (next — covers €0 pass attendance)
    Charge,   // amount < 0
    TopUp,    // amount > 0
    Other,    // residual (e.g. action='storno' with amount=0)
}

/// Free function so both ReportEvent (server-side reports) and TxnInfo
/// (UI dashboard) can derive the same EventKind from the same fields.
/// Precedence (top-down, first match wins):
///   1. valid_until.is_some() → PassSale
///   2. action == "visit"     → Visit
///   3. amount < 0.0          → Charge
///   4. amount > 0.0          → TopUp
///   5. else                  → Other
pub fn classify(action: &str, amount: f64, valid_until: Option<chrono::NaiveDate>) -> EventKind {
    if valid_until.is_some() {
        EventKind::PassSale
    } else if action == "visit" {
        EventKind::Visit
    } else if amount < 0.0 {
        EventKind::Charge
    } else if amount > 0.0 {
        EventKind::TopUp
    } else {
        EventKind::Other
    }
}

impl ReportEvent {
    pub fn kind(&self) -> EventKind {
        classify(&self.action, self.amount, self.valid_until)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(amount: f64, valid_until: Option<chrono::NaiveDate>) -> ReportEvent {
        ReportEvent {
            id: 1,
            card_id: None,
            card_name: None,
            barcode: None,
            // Deliberately "x" (non-action) — tests using ev() exercise paths
            // that don't depend on action; use ev_with_action() for action-specific cases.
            action: "x".into(),
            amount,
            service_name_sk: None,
            service_name_en: None,
            service_kind: None,
            created_at: "2026-04-24 12:00:00".into(),
            valid_until,
            voided: false,
            note: None,
        }
    }

    fn ev_with_action(
        action: &str,
        amount: f64,
        valid_until: Option<chrono::NaiveDate>,
    ) -> ReportEvent {
        ReportEvent {
            id: 1,
            card_id: None,
            card_name: None,
            barcode: None,
            action: action.to_string(),
            amount,
            service_name_sk: None,
            service_name_en: None,
            service_kind: None,
            created_at: "2026-04-29 12:00:00".into(),
            valid_until,
            voided: false,
            note: None,
        }
    }

    #[test]
    fn kind_pass_sold_regardless_of_amount_when_valid_until_set() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 5, 24).unwrap();
        assert_eq!(ev(-35.0, Some(d)).kind(), EventKind::PassSale);
        assert_eq!(ev(0.0, Some(d)).kind(), EventKind::PassSale);
        assert_eq!(ev(35.0, Some(d)).kind(), EventKind::PassSale);
    }

    #[test]
    fn kind_charge_when_amount_strictly_negative_and_no_pass() {
        assert_eq!(ev(-5.0, None).kind(), EventKind::Charge);
        assert_eq!(ev(-0.01, None).kind(), EventKind::Charge);
    }

    #[test]
    fn kind_topup_when_amount_strictly_positive_and_no_pass() {
        assert_eq!(ev(5.0, None).kind(), EventKind::TopUp);
        assert_eq!(ev(0.01, None).kind(), EventKind::TopUp);
    }

    #[test]
    fn kind_other_when_amount_is_exactly_zero_and_no_pass() {
        // Guards against `<` → `<=` and `>` → `>=` mutants.
        assert_eq!(ev(0.0, None).kind(), EventKind::Other);
    }

    #[test]
    fn kind_visit_when_action_is_visit_zero_amount_no_pass() {
        // The bug fix from issue #26: today this lands in EventKind::Other.
        assert_eq!(ev_with_action("visit", 0.0, None).kind(), EventKind::Visit,);
    }

    #[test]
    fn kind_visit_overrides_charge_when_action_is_visit_and_amount_negative() {
        // Defensive: if a future bug lets a visit have a negative amount, the
        // action='visit' should still win over the amount<0 charge classification.
        assert_eq!(ev_with_action("visit", -1.0, None).kind(), EventKind::Visit,);
    }

    #[test]
    fn kind_passsale_overrides_visit_when_valid_until_set() {
        // valid_until still wins over action='visit' (defensive — should never
        // happen in practice, but the precedence must be deterministic).
        let d = chrono::NaiveDate::from_ymd_opt(2026, 5, 24).unwrap();
        assert_eq!(
            ev_with_action("visit", 0.0, Some(d)).kind(),
            EventKind::PassSale,
        );
    }

    #[test]
    fn kind_charge_when_action_charge_amount_negative_no_pass() {
        // Preserves Charge for non-visit non-pass charges.
        assert_eq!(
            ev_with_action("charge", -5.0, None).kind(),
            EventKind::Charge,
        );
    }

    #[test]
    fn kind_other_when_amount_zero_action_neither_visit_nor_charge_no_pass() {
        // Guards Other as the residual bucket for a 'storno' or unknown action
        // with amount=0.
        assert_eq!(ev_with_action("storno", 0.0, None).kind(), EventKind::Other,);
    }
}
