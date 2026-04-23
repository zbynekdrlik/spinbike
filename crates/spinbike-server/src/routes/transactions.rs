use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, patch},
};
use serde::Deserialize;

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
    valid_until: Option<String>,
}

#[derive(Deserialize)]
struct PatchValidUntilReq {
    valid_until: chrono::NaiveDate,
}

#[derive(serde::Serialize)]
struct PatchValidUntilResp {
    id: i64,
    valid_until: chrono::NaiveDate,
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

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, card_id, deleted_at, valid_until FROM transactions WHERE id = ?",
    )
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
        // Single-formula credit reversal works because amounts are SIGNED
        // in the transactions table:
        //   - charges/visits store NEGATIVE amounts → `credit - (-X)` = `credit + X` (refund)
        //   - top-ups       store POSITIVE amounts → `credit - (+X)` = `credit - X` (claw-back)
        // ROUND keeps SQLite from drifting on float math.
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

async fn patch_valid_until(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchValidUntilReq>,
) -> Result<Json<PatchValidUntilResp>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, card_id, valid_until, deleted_at FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Transaction not found"})),
        ));
    };
    if row.valid_until.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Only pass transactions have valid_until"})),
        ));
    }

    sqlx::query("UPDATE transactions SET valid_until = ? WHERE id = ?")
        .bind(body.valid_until.to_string())
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(PatchValidUntilResp {
        id,
        valid_until: body.valid_until,
    }))
}
