pub mod admin;
pub mod auth;
pub mod cards;
pub mod classes;
pub mod payments;
pub mod static_files;

use axum::Router;

use crate::AppState;

/// All API routes merged together.
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(classes::routes())
        .merge(cards::routes())
        .merge(payments::routes())
        .merge(admin::routes())
}

/// All routes: API + WebSocket + static file serving.
pub fn all_routes() -> Router<AppState> {
    Router::new()
        .merge(api_routes())
        .route("/api/ws", axum::routing::get(crate::ws::ws_handler))
        .fallback(static_files::static_handler)
}
