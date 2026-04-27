use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::cards;

#[derive(Deserialize)]
pub struct SeedExpiredPassRequest {
    pub barcode: String,
    pub valid_until: chrono::NaiveDate,
}

#[derive(Deserialize)]
pub struct SeedTransactionsRequest {
    pub barcode: String,
    pub entries: Vec<SeedEntry>,
}

#[derive(Deserialize)]
pub struct SeedEntry {
    pub amount: f64,
    pub action: String,
    pub service_name_sk: String,
}

pub fn routes() -> Router<AppState> {
    // Only registered when SPINBIKE_TEST_MODE=1.
    Router::new()
        .route("/api/test/seed-expired-pass", post(seed_expired_pass))
        .route("/api/test/seed-transactions", post(seed_transactions))
}

async fn seed_expired_pass(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedExpiredPassRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Defence in depth: even though this route is env-gated, require staff role
    // to guard against misconfiguration.
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, "Staff required".into()));
    }
    let card_id = cards::create_card(&state.pool, &body.barcode)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    // Look up the service id and its current default_price to avoid hardcoding.
    let (pass_service_id, pass_price): (i64, f64) =
        sqlx::query_as("SELECT id, default_price FROM services WHERE kind = 'monthly_pass'")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    sqlx::query(
        "INSERT INTO transactions (card_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, ?, 'charge', ?, datetime('now'))",
    )
    .bind(card_id)
    .bind(pass_service_id)
    .bind(-pass_price)
    .bind(body.valid_until)
    .execute(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "card_id": card_id })))
}

/// Seed a card (if missing) and insert one transaction per entry, each
/// pre-linked to the service whose `name_sk` matches `service_name_sk`.
/// Used by E2E to verify backfilled history rendering — flags the rows with
/// `legacy_backfilled = 1` so they look like backfill output.
async fn seed_transactions(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedTransactionsRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, "Staff required".into()));
    }

    // Insert card if it doesn't already exist.
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM cards WHERE barcode = ?")
        .bind(&body.barcode)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let card_id = match existing {
        Some(id) => id,
        None => cards::create_card(&state.pool, &body.barcode)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    for e in body.entries {
        let svc_id: Option<i64> = sqlx::query_scalar("SELECT id FROM services WHERE name_sk = ?")
            .bind(&e.service_name_sk)
            .fetch_optional(&state.pool)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, legacy_backfilled, created_at)
             VALUES (?, ?, ?, ?, 1, datetime('now'))",
        )
        .bind(card_id)
        .bind(svc_id)
        .bind(e.amount)
        .bind(&e.action)
        .execute(&state.pool)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    }

    Ok(Json(serde_json::json!({ "card_id": card_id, "count": 1 })))
}
