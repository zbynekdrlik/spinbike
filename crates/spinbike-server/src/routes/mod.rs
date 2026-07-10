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

use axum::Router;

use crate::AppState;
use crate::error::ApiError;

/// Log the real error and return a generic 500 to the client (no
/// implementation detail leaked). Retained as a helper so the ~110
/// `.map_err(internal_error)?` call sites stay untouched by the typed-error
/// migration (#158) — the return type is now `ApiError` instead of the old
/// `(StatusCode, Json<Value>)` tuple.
pub fn internal_error(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("Internal error: {e}");
    ApiError::Internal
}

/// Build a 400 Bad Request carrying `msg` as the human `error` message. The
/// machine `error_code` is the generic `bad_request`; the specifics live in
/// the message (which tests assert on). Retained so the many
/// `super::bad_request("...")` call sites stay untouched (#158).
pub fn bad_request(msg: &str) -> ApiError {
    ApiError::BadRequest(msg.to_string())
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
