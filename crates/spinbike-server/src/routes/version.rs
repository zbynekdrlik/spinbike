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
    use axum::routing::get;
    use tower::ServiceExt;

    #[tokio::test]
    async fn version_endpoint_returns_cargo_pkg_version() {
        // The /api/version handler doesn't read AppState, so we mount the
        // bare handler against a stateless Router for the test. That keeps
        // the test independent of the rest of the app's wiring.
        let app: Router = Router::new().route("/api/version", get(version));

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
