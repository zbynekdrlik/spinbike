use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::StaffUser;
use crate::db::transactions::NOTE_MAX_CHARS;
use crate::db::users;
use crate::error::ApiError;
use crate::routes::internal_error;
use spinbike_core::errors::ErrorCode;
use spinbike_core::services::CLASS_VISIT_NAMES_EN;

#[derive(Deserialize)]
pub struct ChargeRequest {
    pub user_id: i64,
    pub amount: f64,
    pub service_id: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct StornoRequest {
    pub user_id: i64,
    pub amount: f64,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct PaymentResponse {
    pub transaction_id: i64,
    pub new_credit: f64,
}

#[derive(Deserialize)]
pub struct LogVisitRequest {
    pub user_id: i64,
    pub service_id: i64,
    #[serde(default)]
    pub note: Option<String>,
    /// #234: when a same-day visit/entry already exists for this user, the
    /// first call 409s (`already_visited_today`) instead of logging a
    /// duplicate. Resubmit with `force: true` to log it anyway (a genuine
    /// second visit in one day is legitimate — e.g. morning Fitness +
    /// evening Spinning). Additive optional field — no API break.
    #[serde(default)]
    pub force: bool,
}

#[derive(Serialize)]
pub struct LogVisitResponse {
    pub transaction_id: i64,
}

#[derive(Deserialize)]
pub struct SellPassRequest {
    pub user_id: i64,
    pub price: f64,
    pub valid_until: chrono::NaiveDate,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct SellPassResponse {
    pub transaction_id: i64,
    pub new_credit: f64,
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/payments/charge", post(charge))
        .route("/api/payments/storno", post(storno))
        .route("/api/payments/sell-pass", post(sell_pass))
        .route("/api/payments/log-visit", post(log_visit))
}

async fn charge(
    State(state): State<AppState>,
    StaffUser(claims): StaffUser,
    Json(body): Json<ChargeRequest>,
) -> Result<Json<PaymentResponse>, ApiError> {
    // #31: charge requires an explicit service_id (data integrity — untyped
    // charges pollute the activity feed and reports). Top-up stays
    // service-independent. UI also prevents this via removed empty <option>;
    // server enforces it as defense-in-depth.
    let service_id = match body.service_id {
        Some(sid) => sid,
        None => {
            return Err(super::bad_request("service_id required for charge"));
        }
    };

    // Reject Monthly pass service_id via /charge — it requires valid_until,
    // which /charge doesn't set. Staff must use /sell-pass instead.
    let is_pass: bool =
        sqlx::query_scalar("SELECT kind = 'monthly_pass' FROM services WHERE id = ?")
            .bind(service_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_error)?
            .unwrap_or(false);
    if is_pass {
        return Err(super::bad_request(
            "Use /api/payments/sell-pass for Monthly pass sales (requires valid_until)",
        ));
    }

    // C3: Validate amount is positive.
    if body.amount <= 0.0 {
        return Err(super::bad_request("Amount must be greater than zero"));
    }
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(super::bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    let amount = users::round_cents(body.amount);

    // C2: Wrap entire operation in a transaction to prevent race conditions.
    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    // Re-read user inside the transaction.
    let user = sqlx::query_as::<_, users::UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE id = ?",
    )
    .bind(body.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal_error)?
    .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;

    if user.blocked {
        return Err(ApiError::conflict(ErrorCode::UserBlocked));
    }

    // Legacy app allowed credit to go negative — any user can go into debt.

    // Debit the user within the transaction.
    sqlx::query("UPDATE users SET credit = ROUND(credit - ?, 2) WHERE id = ?")
        .bind(amount)
        .bind(body.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, note)
         VALUES (?, ?, ?, ?, 'charge', ?)
         RETURNING id",
    )
    .bind(body.user_id)
    .bind(Some(claims.sub))
    .bind(Some(service_id))
    .bind(-amount)
    .bind(note_for_db)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    let new_credit = users::round_cents(user.credit - amount);

    Ok(Json(PaymentResponse {
        transaction_id: tx_id,
        new_credit,
    }))
}

async fn storno(
    State(state): State<AppState>,
    StaffUser(claims): StaffUser,
    Json(body): Json<StornoRequest>,
) -> Result<Json<PaymentResponse>, ApiError> {
    // C3: Validate amount is positive.
    if body.amount <= 0.0 {
        return Err(super::bad_request("Amount must be greater than zero"));
    }

    let amount = users::round_cents(body.amount);

    // Wrap in a transaction for consistency.
    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let user = sqlx::query_as::<_, users::UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE id = ?",
    )
    .bind(body.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal_error)?
    .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;

    // Credit the user (refund) within the transaction.
    sqlx::query("UPDATE users SET credit = ROUND(credit + ?, 2) WHERE id = ?")
        .bind(amount)
        .bind(body.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, note)
         VALUES (?, ?, ?, ?, 'storno', NULL)
         RETURNING id",
    )
    .bind(body.user_id)
    .bind(Some(claims.sub))
    .bind::<Option<i64>>(None)
    .bind(amount)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    let new_credit = users::round_cents(user.credit + amount);

    Ok(Json(PaymentResponse {
        transaction_id: tx_id,
        new_credit,
    }))
}

async fn sell_pass(
    State(state): State<AppState>,
    StaffUser(claims): StaffUser,
    Json(body): Json<SellPassRequest>,
) -> Result<Json<SellPassResponse>, ApiError> {
    if body.price < 0.0 {
        return Err(super::bad_request("Price must be zero or greater"));
    }
    // "In the future" means after the gym's local day (Europe/Bratislava), the
    // same basis the pass-expiry checks now use (#205) — so a pass sold late in
    // the evening isn't judged against a UTC / OS-zone "today".
    let today = crate::util::today_bratislava();
    if body.valid_until <= today {
        return Err(super::bad_request("valid_until must be in the future"));
    }
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(super::bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    let price = users::round_cents(body.price);

    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let user = sqlx::query_as::<_, users::UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE id = ?",
    )
    .bind(body.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal_error)?
    .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;
    if user.blocked {
        return Err(ApiError::conflict(ErrorCode::UserBlocked));
    }

    // Resolve Monthly pass service id by name (seeded by V4 migration).
    let service_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&mut *tx)
        .await
        .map_err(internal_error)?;

    sqlx::query("UPDATE users SET credit = ROUND(credit - ?, 2) WHERE id = ?")
        .bind(price)
        .bind(body.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, valid_until, note)
         VALUES (?, ?, ?, ?, 'charge', ?, ?)
         RETURNING id",
    )
    .bind(body.user_id)
    .bind(Some(claims.sub))
    .bind(Some(service_id))
    .bind(-price)
    .bind(body.valid_until)
    .bind(note_for_db)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    let new_credit = users::round_cents(user.credit - price);
    let days_remaining = (body.valid_until - today).num_days() as i32;

    Ok(Json(SellPassResponse {
        transaction_id: tx_id,
        new_credit,
        valid_until: body.valid_until,
        days_remaining,
    }))
}

async fn log_visit(
    State(state): State<AppState>,
    StaffUser(claims): StaffUser,
    Json(body): Json<LogVisitRequest>,
) -> Result<Json<LogVisitResponse>, ApiError> {
    // Gym-local "today" (Europe/Bratislava), consistent with the door route,
    // my_balance and the T-4h charger — a monthly pass grants a free logged
    // visit through the whole of its last GYM-LOCAL day (#205). `chrono::Local`
    // / SQLite `date('now')` would key this off the server OS zone / UTC and
    // could disagree with the door near local midnight.
    let today = crate::util::today_bratislava();
    let valid_until = users::get_user_pass_valid_until(&state.pool, body.user_id)
        .await
        .map_err(internal_error)?;
    match valid_until {
        Some(d) if d >= today => {} // active — OK
        _ => {
            return Err(ApiError::conflict(ErrorCode::NoActiveMonthlyPass));
        }
    }

    // Validate service exists — prevents bogus service_id in history.
    let service_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM services WHERE id = ? AND active = 1")
            .bind(body.service_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_error)?;
    if service_exists.is_none() {
        return Err(ApiError::NotFound(ErrorCode::ServiceNotFound));
    }

    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(super::bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    // #234: warn (not hard-block) when this user already has a same-day
    // visit/entry — from EITHER source. A manual log-visit and a door
    // self-entry both land here: door rows are written against the
    // `kind='single_entry'` services row, which migration V16 re-tags onto
    // the SAME row as the seeded 'Fitness' service (name_en='Fitness'), so
    // door entries are already inside the CLASS_VISIT_NAMES_EN filter below.
    // Canonical attendance definition — the same UNION `db/reports.rs` uses
    // (db-migrations skill): `action='visit'` OR a per-class pay-as-you-go
    // charge (amount<0, valid_until IS NULL; excludes pass purchases and
    // door's own Nth-press amount=0 audit rows). `force: true` skips this
    // gate entirely — a genuine second visit in a day (e.g. morning Fitness
    // + evening Spinning) is legitimate.
    if !body.force {
        let (day_start, day_end) = crate::util::bratislava_day_range_utc(today);
        let day_start = day_start.format("%Y-%m-%d %H:%M:%S").to_string();
        let day_end = day_end.format("%Y-%m-%d %H:%M:%S").to_string();
        let placeholders: String = std::iter::repeat_n("?", CLASS_VISIT_NAMES_EN.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT created_at, note FROM transactions \
             WHERE user_id = ? AND deleted_at IS NULL \
               AND created_at >= ? AND created_at < ? \
               AND service_id IN (SELECT id FROM services WHERE name_en IN ({placeholders})) \
               AND ( action = 'visit' \
                     OR (action = 'charge' AND amount < 0 AND valid_until IS NULL) ) \
             ORDER BY created_at DESC LIMIT 1"
        );
        let mut q = sqlx::query_as::<_, (String, Option<String>)>(&sql)
            .bind(body.user_id)
            .bind(&day_start)
            .bind(&day_end);
        for n in CLASS_VISIT_NAMES_EN {
            q = q.bind(*n);
        }
        let existing: Option<(String, Option<String>)> = q
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_error)?;
        if let Some((created_at, note)) = existing {
            // Door rows carry the English `"door: 1st"`/`"door: 2nd"` note
            // prefix written by routes/door.rs — anything else (including no
            // note) is a manual log-visit.
            let source = if note.as_deref().is_some_and(|n| n.starts_with("door:")) {
                "door"
            } else {
                "manual"
            };
            return Err(ApiError::conflict_extra(
                ErrorCode::AlreadyVisitedToday,
                serde_json::json!({
                    "last_entry_at": created_at,
                    "source": source,
                }),
            ));
        }
    }

    let tx_id = crate::db::transactions::create_transaction(
        &state.pool,
        Some(body.user_id),
        Some(claims.sub),
        Some(body.service_id),
        0.0,
        "visit",
        note_for_db,
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(LogVisitResponse {
        transaction_id: tx_id,
    }))
}
