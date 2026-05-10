pub mod admin;
pub mod auth;
pub mod classes;
pub mod door;
pub mod payments;
pub mod persistent_bookings;
pub mod reports;
pub mod static_files;
pub mod test_fixtures;
pub mod transactions;
pub mod upcoming_classes;
pub mod users;
pub mod version;

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

/// Build a BAD_REQUEST response with an error message body.
///
/// Wraps the `(StatusCode, Json<Value>)` tuple so cargo-mutants can mutate
/// the message string reliably (#36 — `axum::Json` newtype has no `::new()`
/// constructor for cargo-mutants to synthesize). Behaviorally identical to
/// inline `(StatusCode::BAD_REQUEST, Json(json!({"error": msg})))`.
pub fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": msg})),
    )
}

/// All API routes merged together.
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(classes::routes())
        .merge(door::routes())
        .merge(users::routes())
        .merge(payments::routes())
        .merge(admin::routes())
        .merge(persistent_bookings::routes())
        .merge(reports::routes())
        .merge(transactions::routes())
        .merge(upcoming_classes::routes())
        .merge(version::routes())
}

/// All routes: API + WebSocket + static file serving.
pub fn all_routes() -> Router<AppState> {
    Router::new()
        .merge(api_routes())
        .route("/api/ws", axum::routing::get(crate::ws::ws_handler))
        .fallback(static_files::static_handler)
}
