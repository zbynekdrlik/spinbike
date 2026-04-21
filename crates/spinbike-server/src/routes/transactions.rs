use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, patch},
};

use crate::AppState;
use crate::auth::AuthUser;
use crate::routes::internal_error;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/transactions/{id}", delete(void_transaction))
        .route(
            "/api/transactions/{id}/valid-until",
            patch(patch_valid_until),
        )
}

#[derive(sqlx::FromRow)]
struct TxMini {
    amount: f64,
    card_id: Option<i64>,
    deleted_at: Option<String>,
}

async fn void_transaction(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let row: Option<TxMini> =
        sqlx::query_as("SELECT amount, card_id, deleted_at FROM transactions WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal_error)?;

    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Transaction not found"})),
        ));
    };
    if row.deleted_at.is_some() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Transaction already voided"})),
        ));
    }

    sqlx::query("UPDATE transactions SET deleted_at = datetime('now') WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    if let Some(card_id) = row.card_id {
        sqlx::query("UPDATE cards SET credit = ROUND(credit - ?, 2) WHERE id = ?")
            .bind(row.amount)
            .bind(card_id)
            .execute(&mut *tx)
            .await
            .map_err(internal_error)?;
    }

    tx.commit().await.map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// Stub that Task 6 replaces.
async fn patch_valid_until(
    State(_state): State<AppState>,
    AuthUser(_claims): AuthUser,
    Path(_id): Path<i64>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({"error": "Not yet implemented"})),
    ))
}
