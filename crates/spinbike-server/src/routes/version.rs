use axum::{Json, Router, routing::get};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize, serde::Deserialize, Debug)]
pub struct VersionResponse {
    pub version: &'static str,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/version", get(version))
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn version_endpoint_returns_cargo_pkg_version() {
        // Mount through `routes()` so a mutant that replaces the function
        // body with `Router::new()` (no routes wired) shows up as a 404 here.
        // The handler itself is stateless, but the `routes()` return type
        // is `Router<AppState>`, so we wrap a minimal in-memory state to
        // satisfy the type bound.
        let pool = crate::db::create_memory_pool().await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let state = crate::AppState {
            pool,
            event_tx,
            jwt_secret: "test-jwt".to_string(),
        };
        let app = Router::new().merge(routes()).with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let v = parsed
            .get("version")
            .and_then(|v| v.as_str())
            .expect("response must contain a version field");

        assert_eq!(v, env!("CARGO_PKG_VERSION"));
        // Kills the mutant that returns an empty string or a bare label.
        assert!(
            v.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false),
            "version should start with a digit, got {v:?}",
        );
    }
}
