use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::routes::internal_error;

use spinbike_core::reports::ReportResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/reports/day", get(day))
        .route("/api/reports/range", get(range))
}

/// Require admin role. Reports contain business-level data and are admin-only.
fn require_admin(
    claims: &spinbike_core::auth::Claims,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(claims.role, spinbike_core::auth::Role::Admin) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct DayQuery {
    date: chrono::NaiveDate,
    limit: Option<i64>,
    before: Option<String>,
}

async fn day(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<DayQuery>,
) -> Result<Json<ReportResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let (kpi, events, has_more) = db::reports::day_report(&state.pool, q.date, limit, q.before)
        .await
        .map_err(internal_error)?;
    Ok(Json(ReportResponse {
        kpi,
        events,
        has_more,
    }))
}

#[derive(Debug, Deserialize)]
struct RangeQuery {
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    limit: Option<i64>,
    before: Option<String>,
}

async fn range(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<RangeQuery>,
) -> Result<Json<ReportResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    if q.to < q.from {
        return Err(super::bad_request("to < from"));
    }
    let days = (q.to - q.from).num_days();
    if days > db::reports::RANGE_MAX_DAYS {
        return Err(super::bad_request("range too large (max 93 days)"));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let (kpi, events, has_more) =
        db::reports::range_report(&state.pool, q.from, q.to, limit, q.before)
            .await
            .map_err(internal_error)?;
    Ok(Json(ReportResponse {
        kpi,
        events,
        has_more,
    }))
}
