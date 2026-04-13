//! Exercises the static-file fallback handler (rust-embed + SPA routing).
//!
//! The test binary relies on a placeholder `spinbike-ui/dist/index.html`
//! (ensured by CI's "Create placeholder dist" step). The actual HTML content
//! doesn't matter — we only assert routing decisions.

mod helpers;

use helpers::{TestApp, get};
use tower::util::ServiceExt;

#[tokio::test]
async fn root_path_serves_index_html() {
    let app = TestApp::new().await;
    let resp = app.router.clone().oneshot(get("/", "")).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    // Placeholder dist/index.html has content, so body must be non-empty.
    // Kills the `static_handler -> Default::default()` mutant (which returns
    // an empty body with default status).
    assert!(!body.is_empty(), "/ must serve non-empty index.html");
}

#[tokio::test]
async fn unknown_spa_route_also_serves_index_html() {
    // Path without a file extension and no matching asset → SPA fallback
    // must return index.html. Kills mutants on line 36 guard:
    //   !path.contains('.') || path.is_empty()
    let app = TestApp::new().await;
    let resp = app
        .router
        .clone()
        .oneshot(get("/staff/cards", ""))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert!(!body.is_empty());
}

#[tokio::test]
async fn missing_asset_with_extension_is_404() {
    // Path with a file extension that doesn't exist → must NOT fall back
    // to index.html (the `||` → `&&` mutant would still return index).
    let app = TestApp::new().await;
    let resp = app
        .router
        .clone()
        .oneshot(get("/nothing.css", ""))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
}
