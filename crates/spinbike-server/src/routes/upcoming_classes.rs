use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::routes::internal_error;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/users/{user_id}/upcoming-classes", get(upcoming))
}

#[derive(Deserialize)]
struct Qs {
    days: Option<i64>,
}

async fn upcoming(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(user_id): Path<i64>,
    Query(qs): Query<Qs>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }
    let days = qs.days.unwrap_or(14).clamp(1, 60);
    let today = chrono::Local::now().date_naive();
    let to = today + chrono::Duration::days(days);
    let rows = crate::db::classes::list_upcoming_for_user(
        &state.pool,
        user_id,
        &today.to_string(),
        &to.to_string(),
    )
    .await
    .map_err(internal_error)?;
    Ok(Json(serde_json::to_value(rows).unwrap()))
}
