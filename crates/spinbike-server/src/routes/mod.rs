pub mod admin;
pub mod auth;
pub mod cards;
pub mod classes;
pub mod payments;
pub mod persistent_bookings;
pub mod reports;
pub mod static_files;
pub mod test_fixtures;
pub mod transactions;
pub mod upcoming_classes;

use axum::{Json, Router, http::StatusCode};

use crate::AppState;

/// Log the real error and return a generic "Internal server error" to the client.
/// Prevents leaking implementation details to users.
pub fn internal_error(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("Internal error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "Internal server error"})),
    )
}

/// All API routes merged together.
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(classes::routes())
        .merge(cards::routes())
        .merge(payments::routes())
        .merge(admin::routes())
        .merge(persistent_bookings::routes())
        .merge(reports::routes())
        .merge(transactions::routes())
        .merge(upcoming_classes::routes())
}

/// All routes: API + WebSocket + static file serving.
pub fn all_routes() -> Router<AppState> {
    Router::new()
        .merge(api_routes())
        .route("/api/ws", axum::routing::get(crate::ws::ws_handler))
        .fallback(static_files::static_handler)
}
