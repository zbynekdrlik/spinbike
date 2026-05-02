use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::cards;
use crate::db::transactions::NOTE_MAX_CHARS;
use crate::routes::internal_error;

#[derive(Deserialize)]
pub struct ChargeRequest {
    pub card_id: i64,
    pub amount: f64,
    pub service_id: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct StornoRequest {
    pub card_id: i64,
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
    pub card_id: i64,
    pub service_id: i64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct LogVisitResponse {
    pub transaction_id: i64,
}

#[derive(Deserialize)]
pub struct SellPassRequest {
    pub card_id: i64,
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

/// Build a BAD_REQUEST response with an error message body.
///
/// Wraps the `(StatusCode, Json<Value>)` tuple so cargo-mutants can mutate
/// the message string reliably (#36 — `axum::Json` newtype has no `::new()`
/// constructor for cargo-mutants to synthesize). Behaviorally identical to
/// inline `(StatusCode::BAD_REQUEST, Json(json!({"error": msg})))`.
fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}

async fn charge(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<ChargeRequest>,
) -> Result<Json<PaymentResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // #31: charge requires an explicit service_id (data integrity — untyped
    // charges pollute the activity feed and reports). Top-up stays
    // service-independent. UI also prevents this via removed empty <option>;
    // server enforces it as defense-in-depth.
    let service_id = match body.service_id {
        Some(sid) => sid,
        None => {
            return Err(bad_request("service_id required for charge"));
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
        return Err(bad_request(
            "Use /api/payments/sell-pass for Monthly pass sales (requires valid_until)",
        ));
    }

    // C3: Validate amount is positive.
    if body.amount <= 0.0 {
        return Err(bad_request("Amount must be greater than zero"));
    }
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    let amount = cards::round_cents(body.amount);

    // C2: Wrap entire operation in a transaction to prevent race conditions.
    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    // Re-read card inside the transaction.
    let card = sqlx::query_as::<_, cards::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;

    if card.blocked != 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Card is blocked"})),
        ));
    }

    // Legacy app allowed credit to go negative — any card can go into debt.

    // Debit the card within the transaction.
    sqlx::query("UPDATE cards SET credit = ROUND(credit - ?, 2) WHERE id = ?")
        .bind(amount)
        .bind(body.card_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, note)
         VALUES (?, ?, ?, ?, ?, 'charge', ?)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind(Some(service_id))
    .bind(-amount)
    .bind(note_for_db)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    let new_credit = cards::round_cents(card.credit - amount);

    Ok(Json(PaymentResponse {
        transaction_id: tx_id,
        new_credit,
    }))
}

async fn storno(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<StornoRequest>,
) -> Result<Json<PaymentResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // C3: Validate amount is positive.
    if body.amount <= 0.0 {
        return Err(bad_request("Amount must be greater than zero"));
    }

    let amount = cards::round_cents(body.amount);

    // Wrap in a transaction for consistency.
    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let card = sqlx::query_as::<_, cards::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;

    // Credit the card (refund) within the transaction.
    sqlx::query("UPDATE cards SET credit = ROUND(credit + ?, 2) WHERE id = ?")
        .bind(amount)
        .bind(body.card_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, note)
         VALUES (?, ?, ?, ?, ?, 'storno', NULL)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind::<Option<i64>>(None)
    .bind(amount)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    let new_credit = cards::round_cents(card.credit + amount);

    Ok(Json(PaymentResponse {
        transaction_id: tx_id,
        new_credit,
    }))
}

async fn sell_pass(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SellPassRequest>,
) -> Result<Json<SellPassResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }
    if body.price < 0.0 {
        return Err(bad_request("Price must be zero or greater"));
    }
    let today = chrono::Local::now().date_naive();
    if body.valid_until <= today {
        return Err(bad_request("valid_until must be in the future"));
    }
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    let price = cards::round_cents(body.price);

    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let card = sqlx::query_as::<_, cards::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;
    if card.blocked != 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Card is blocked"})),
        ));
    }

    // Resolve Monthly pass service id by name (seeded by V4 migration).
    let service_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&mut *tx)
        .await
        .map_err(internal_error)?;

    sqlx::query("UPDATE cards SET credit = ROUND(credit - ?, 2) WHERE id = ?")
        .bind(price)
        .bind(body.card_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, valid_until, note)
         VALUES (?, ?, ?, ?, ?, 'charge', ?, ?)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind(Some(service_id))
    .bind(-price)
    .bind(body.valid_until)
    .bind(note_for_db)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    let new_credit = cards::round_cents(card.credit - price);
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
    AuthUser(claims): AuthUser,
    Json(body): Json<LogVisitRequest>,
) -> Result<Json<LogVisitResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let today = chrono::Local::now().date_naive();
    let valid_until = cards::get_card_pass_valid_until(&state.pool, body.card_id)
        .await
        .map_err(internal_error)?;
    match valid_until {
        Some(d) if d >= today => {} // active — OK
        _ => {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Card has no active monthly pass; use /api/payments/charge"
                })),
            ));
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Service not found"})),
        ));
    }

    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    let tx_id = crate::db::transactions::create_transaction(
        &state.pool,
        None,
        Some(body.card_id),
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
