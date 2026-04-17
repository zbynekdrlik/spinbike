use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;

use crate::AppState;
use crate::db::cards;

#[derive(Deserialize)]
pub struct SeedExpiredPassRequest {
    pub barcode: String,
    pub valid_until: chrono::NaiveDate,
}

pub fn routes() -> Router<AppState> {
    // Only registered when SPINBIKE_TEST_MODE=1.
    Router::new().route("/api/test/seed-expired-pass", post(seed_expired_pass))
}

async fn seed_expired_pass(
    State(state): State<AppState>,
    Json(body): Json<SeedExpiredPassRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let card_id = cards::create_card(&state.pool, &body.barcode)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let pass_service_id: i64 =
        sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    sqlx::query(
        "INSERT INTO transactions (card_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, ?, 'charge', ?, datetime('now'))",
    )
    .bind(card_id)
    .bind(pass_service_id)
    .bind(-35.0_f64)
    .bind(body.valid_until)
    .execute(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "card_id": card_id })))
}
