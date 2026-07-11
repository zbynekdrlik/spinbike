use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::internal_error;
use spinbike_core::errors::ErrorCode;

#[derive(Serialize)]
pub struct BalanceResponse {
    pub user_id: i64,
    pub name: String,
    pub credit: f64,
    pub card_code: Option<String>,
    pub allow_self_entry: bool,
    /// SQLite UTC timestamp; `None` = no active monthly pass.
    pub monthly_pass_active_until: Option<String>,
    /// Last 20 transactions for this user, newest first.
    pub recent: Vec<RecentTx>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct RecentTx {
    pub id: i64,
    pub created_at: String,
    pub action: String,
    pub amount: f64,
    pub valid_until: Option<String>,
    pub note: Option<String>,
    /// Joined from services (#147) — None when the transaction wasn't tied
    /// to a service (e.g. a plain top-up). Same join as
    /// `db::transactions::list_transactions_for_user_paginated`, used by the
    /// admin transactions list.
    pub service_name_sk: Option<String>,
    pub service_name_en: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/my/balance", get(my_balance))
}

async fn my_balance(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<BalanceResponse>, ApiError> {
    let user_id = claims.sub;
    tracing::debug!(user_id, "my_balance: loading user row");

    // 1. User row — includes the new allow_self_entry column.
    // SQLite stores allow_self_entry as INTEGER (0/1); fetch as i64 here and
    // map to bool below — sqlx tuple destructuring is stricter about types
    // than `#[derive(FromRow)]`, so we avoid the bool type entirely at the
    // query boundary.
    let user_row: Option<(i64, String, f64, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, name, credit, card_code, allow_self_entry \
         FROM users WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let (id, name, credit, card_code, allow_self_entry) = match user_row {
        Some((id, name, credit, card_code, ase)) => {
            // Admin/staff always see the door button enabled — they bypass
            // the per-user opt-in toggle (they manage the place). Stored
            // flag stays as-is; this is just the effective UI value.
            let role_is_staff_or_admin = matches!(
                claims.role,
                spinbike_core::auth::Role::Admin | spinbike_core::auth::Role::Staff
            );
            let effective_ase = ase != 0 || role_is_staff_or_admin;
            (id, name, credit, card_code, effective_ase)
        }
        None => {
            tracing::warn!(user_id, "my_balance: user not found or soft-deleted");
            return Err(ApiError::NotFound(ErrorCode::UserNotFound));
        }
    };

    // 2. Active monthly-pass valid_until, via the canonical `user_active_pass`
    //    view (migration V18) — the SAME definition the charger and the staff
    //    user lists use. The view already exposes the user's latest non-voided
    //    monthly-pass purchase; here we surface it only while it is still valid
    //    today or later (an expired pass shows as "no active pass"). The
    //    comparison is INCLUSIVE of the last paid day and coerces both sides to
    //    a calendar date (`date(valid_until) >= date('now')`), matching the
    //    charger and the door route (#179). The previous
    //    `valid_until > datetime('now')` compared a bare date against a
    //    datetime and, via SQLite's byte-wise TEXT ordering, wrongly reported
    //    "no active pass" from midnight of the pass's own last valid day.
    tracing::debug!(user_id, "my_balance: querying monthly_pass_active_until");
    let monthly_pass_active_until: Option<String> = sqlx::query_scalar(
        "SELECT valid_until FROM user_active_pass \
          WHERE user_id = ? AND date(valid_until) >= date('now')",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    // 3. Last 20 transactions (newest first). LEFT JOIN services (#147) so
    //    the customer sees WHICH service a movement was for, same as the
    //    admin transactions list. RecentTx derives FromRow (column-name
    //    matched, not positional) so the aliases below just need to match
    //    its field names — no manual tuple destructuring to keep in sync.
    tracing::debug!(user_id, "my_balance: querying recent transactions");
    let recent: Vec<RecentTx> = sqlx::query_as::<_, RecentTx>(
        "SELECT t.id, t.created_at, t.action, t.amount, t.valid_until, t.note, \
                s.name_sk AS service_name_sk, s.name_en AS service_name_en \
           FROM transactions t \
           LEFT JOIN services s ON s.id = t.service_id \
          WHERE t.user_id = ? \
            AND t.deleted_at IS NULL \
          ORDER BY t.created_at DESC \
          LIMIT 20",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    tracing::info!(
        user_id = id,
        credit,
        allow_self_entry,
        pass_active = monthly_pass_active_until.is_some(),
        recent_count = recent.len(),
        "my_balance: ok"
    );

    Ok(Json(BalanceResponse {
        user_id: id,
        name,
        credit,
        card_code,
        allow_self_entry,
        monthly_pass_active_until,
        recent,
    }))
}
