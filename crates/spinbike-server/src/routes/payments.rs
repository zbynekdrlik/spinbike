use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::{cards, transactions};

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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/payments/charge", post(charge))
        .route("/api/payments/storno", post(storno))
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

    // Get card and check if blocked.
    let card = sqlx::query_as::<_, cards::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?
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

    // Check sufficient credit (unless allow_debit is set).
    if card.credit < body.amount && card.allow_debit == 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Insufficient credit"})),
        ));
    }

    // Debit the card.
    cards::update_credit(&state.pool, body.card_id, -body.amount)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let tx_id = transactions::create_transaction(
        &state.pool,
        card.user_id,
        Some(body.card_id),
        Some(claims.sub),
        body.service_id,
        -body.amount,
        "charge",
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    let new_credit = card.credit - body.amount;

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

    let card = sqlx::query_as::<_, cards::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;

    // Credit the card (refund).
    cards::update_credit(&state.pool, body.card_id, body.amount)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let tx_id = transactions::create_transaction(
        &state.pool,
        card.user_id,
        Some(body.card_id),
        Some(claims.sub),
        None,
        body.amount,
        "storno",
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    let new_credit = card.credit + body.amount;

    Ok(Json(PaymentResponse {
        transaction_id: tx_id,
        new_credit,
    }))
}
