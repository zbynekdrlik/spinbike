pub mod auth;
pub mod db;
pub mod routes;
pub mod ws;

use anyhow::Result;
use spinbike_core::ws::ServerMsg;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub event_tx: broadcast::Sender<ServerMsg>,
    pub jwt_secret: String,
}

/// Build the CORS layer based on the CORS_ORIGIN environment variable.
fn build_cors_layer() -> CorsLayer {
    match std::env::var("CORS_ORIGIN") {
        Ok(origin) if !origin.is_empty() => {
            info!("CORS: restricting to origin {origin}");
            let origin: tower_http::cors::AllowOrigin = origin
                .parse::<axum::http::HeaderValue>()
                .expect("Invalid CORS_ORIGIN value")
                .into();
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any)
        }
        _ => {
            warn!("CORS_ORIGIN not set — using permissive CORS. Do NOT use in production!");
            CorsLayer::permissive()
        }
    }
}

/// Build and start the Axum server.
pub async fn start_server(pool: SqlitePool, port: u16, jwt_secret: String) -> Result<()> {
    let (event_tx, _) = broadcast::channel(256);

    let state = AppState {
        pool,
        event_tx,
        jwt_secret,
    };

    let app = routes::all_routes()
        .layer(build_cors_layer())
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    info!("Starting server on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
