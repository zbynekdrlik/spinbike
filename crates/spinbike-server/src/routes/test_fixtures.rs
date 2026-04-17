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

pub fn routes() -> Router<AppState> {
    // Only registered when SPINBIKE_TEST_MODE=1.
    Router::new().route("/api/test/seed-expired-pass", post(seed_expired_pass))
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
        sqlx::query_as("SELECT id, default_price FROM services WHERE name = 'Monthly pass'")
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
