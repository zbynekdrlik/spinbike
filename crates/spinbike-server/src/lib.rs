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

/// Build the CORS layer by reading the CORS_ORIGIN environment variable.
/// Thin wrapper around `cors_layer_for` so the env lookup is kept out of
/// the testable body.
fn build_cors_layer() -> CorsLayer {
    cors_layer_for(std::env::var("CORS_ORIGIN").ok())
}

/// Pure, unit-testable variant of the CORS layer builder.
/// `Some(non_empty)` → restricted to that origin; `None` or `Some("")` → permissive.
pub(crate) fn cors_layer_for(origin: Option<String>) -> CorsLayer {
    match origin {
        Some(origin) if !origin.is_empty() => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use axum::routing::get;
    use db::{create_memory_pool, run_migrations};
    use tower::ServiceExt;

    /// Build a tiny router with the CORS layer and a single GET / route, then
    /// send a preflight OPTIONS and inspect the Access-Control-Allow-Origin
    /// header. Permissive layer → `*`; restricted → the specific origin.
    async fn preflight_origin(cors: CorsLayer, request_origin: &str) -> Option<String> {
        let app: Router = Router::new().route("/", get(|| async { "hi" })).layer(cors);
        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/")
            .header("origin", request_origin)
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        resp.headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    #[tokio::test]
    async fn cors_layer_for_none_is_permissive() {
        let layer = cors_layer_for(None);
        let echoed = preflight_origin(layer, "https://anywhere.example").await;
        // Permissive allows any origin — tower-http echoes `*`.
        assert_eq!(echoed.as_deref(), Some("*"));
    }

    #[tokio::test]
    async fn cors_layer_for_empty_is_permissive() {
        let layer = cors_layer_for(Some(String::new()));
        let echoed = preflight_origin(layer, "https://anywhere.example").await;
        assert_eq!(echoed.as_deref(), Some("*"));
    }

    #[tokio::test]
    async fn cors_layer_for_value_restricts_to_that_origin() {
        let allowed = "https://spinbike.example.com";
        let layer = cors_layer_for(Some(allowed.to_string()));
        // Matching origin is echoed back.
        let echoed = preflight_origin(layer, allowed).await;
        assert_eq!(echoed.as_deref(), Some(allowed));

        // Non-matching origin is rejected: no Access-Control-Allow-Origin header.
        let layer2 = cors_layer_for(Some(allowed.to_string()));
        let echoed2 = preflight_origin(layer2, "https://evil.example").await;
        assert_eq!(echoed2, None);
    }

    /// Kills `start_server -> Ok(())`: if the function is replaced by a no-op,
    /// the TCP port never binds and this test times out.
    #[tokio::test]
    async fn start_server_binds_and_accepts_connections() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Reserve a free port, drop the temp listener, hand the port to start_server.
        let tmp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tmp.local_addr().unwrap();
        drop(tmp);

        let handle =
            tokio::spawn(
                async move { start_server(pool, addr.port(), "test-jwt".to_string()).await },
            );

        // Try to connect for up to ~2s.
        let mut connected = false;
        for _ in 0..40 {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                connected = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        handle.abort();
        let _ = handle.await;
        assert!(connected, "start_server did not bind within 2s");
    }
}
