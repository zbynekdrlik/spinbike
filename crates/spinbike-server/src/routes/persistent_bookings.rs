use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use serde::Deserialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::routes::internal_error;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/cards/{card_id}/persistent-bookings",
            get(list).post(create),
        )
        .route(
            "/api/cards/{card_id}/persistent-bookings/{template_id}",
            delete(end_persistent),
        )
}

#[derive(Deserialize)]
struct CreateReq {
    template_id: i64,
}

async fn list(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(card_id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }
    let rows = crate::db::persistent_bookings::list_for_card(&state.pool, card_id)
        .await
        .map_err(internal_error)?;
    Ok(Json(serde_json::to_value(rows).unwrap()))
}

async fn create(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(card_id): Path<i64>,
    Json(body): Json<CreateReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }
    let id = crate::db::persistent_bookings::create(&state.pool, card_id, body.template_id)
        .await
        .map_err(internal_error)?;

    // Materialise now so the card page immediately reflects AUTO rows.
    if let Err(e) = crate::jobs::materialiser::sweep(&state.pool).await {
        tracing::error!("post-create materialiser sweep failed: {e}");
    }

    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": id}))))
}

async fn end_persistent(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path((card_id, template_id)): Path<(i64, i64)>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }
    crate::db::persistent_bookings::end(&state.pool, card_id, template_id)
        .await
        .map_err(internal_error)?;

    // Remove future, uncharged persistent bookings for this (card, template).
    sqlx::query(
        "UPDATE bookings SET cancelled_at = datetime('now')
         WHERE card_id = ? AND template_id = ? AND source = 'persistent'
           AND charged_at IS NULL AND cancelled_at IS NULL
           AND datetime(date || ' ' || (SELECT start_time FROM class_templates WHERE id = ?))
               > datetime('now')",
    )
    .bind(card_id)
    .bind(template_id)
    .bind(template_id)
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(StatusCode::NO_CONTENT)
}
