use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::cards;
use crate::routes::internal_error;

#[derive(Deserialize)]
pub struct ChargeRequest {
    pub card_id: i64,
    pub amount: f64,
    pub service_id: Option<i64>,
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
pub struct SellPassRequest {
    pub card_id: i64,
    pub price: f64,
    pub valid_until: chrono::NaiveDate,
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

    // C3: Validate amount is positive.
    if body.amount <= 0.0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Amount must be greater than zero"})),
        ));
    }

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
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action)
         VALUES (?, ?, ?, ?, ?, 'charge')
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind(body.service_id)
    .bind(-amount)
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Amount must be greater than zero"})),
        ));
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
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action)
         VALUES (?, ?, ?, ?, ?, 'storno')
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Price must be zero or greater"})),
        ));
    }
    let today = chrono::Local::now().date_naive();
    if body.valid_until <= today {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "valid_until must be in the future"})),
        ));
    }

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
    let service_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
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
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, valid_until)
         VALUES (?, ?, ?, ?, ?, 'charge', ?)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind(Some(service_id))
    .bind(-price)
    .bind(body.valid_until)
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
