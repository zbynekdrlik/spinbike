//! Shared types for GET /api/cards/{id}/stats.
//!
//! Both the server (which serializes them) and the WASM client (which
//! deserializes them) depend on this module. Keep WASM-safe — no tokio,
//! no sqlx — same constraint as `reports.rs`.

use serde::{Deserialize, Serialize};

/// Aggregated visits + top-ups for one named time window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodAgg {
    pub visits: i64,
    pub topped_up_eur: f64,
}

/// Three named windows the Overview tab displays as KPI rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodTotals {
    pub this_month: PeriodAgg,
    pub this_year: PeriodAgg,
    pub all_time: PeriodAgg,
}

/// One monthly bar in the chart. The server fills zero-buckets for months
/// with no rows so the UI can render exactly 12 entries unconditionally.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonthlyBucket {
    /// Calendar month label "YYYY-MM" in the server's local timezone.
    pub year_month: String,
    pub visits: i64,
    pub topped_up_eur: f64,
}

/// Response from GET /api/cards/{id}/stats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatsResponse {
    pub totals: PeriodTotals,
    /// Exactly 12 entries, oldest → newest. Zero-buckets included.
    pub monthly: Vec<MonthlyBucket>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StatsResponse {
        StatsResponse {
            totals: PeriodTotals {
                this_month: PeriodAgg {
                    visits: 11,
                    topped_up_eur: 50.0,
                },
                this_year: PeriodAgg {
                    visits: 47,
                    topped_up_eur: 200.0,
                },
                all_time: PeriodAgg {
                    visits: 812,
                    topped_up_eur: 3000.0,
                },
            },
            monthly: vec![MonthlyBucket {
                year_month: "2026-05".to_string(),
                visits: 11,
                topped_up_eur: 30.0,
            }],
        }
    }

    #[test]
    fn round_trip_via_serde_json() {
        let original = sample();
        let json = serde_json::to_string(&original).unwrap();
        let back: StatsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn json_uses_snake_case_field_names() {
        let json = serde_json::to_string(&sample()).unwrap();
        // Pin the wire format. The WASM frontend deserializes by these exact
        // keys; renaming a field would silently break the UI.
        assert!(json.contains("\"this_month\""));
        assert!(json.contains("\"topped_up_eur\""));
        assert!(json.contains("\"year_month\""));
    }
}
