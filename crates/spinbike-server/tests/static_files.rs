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

/// #212: `/sw.js` was served with NO `Cache-Control` header at all, so
/// Cloudflare's default extension-based edge caching applied its own
/// `max-age=14400` (4h) to it — confirmed live on prod
/// (`cf-cache-status: HIT`, `age` growing toward 14400). That means a new
/// service-worker script (including future fixes) can take up to 4h to
/// reach users after a deploy. The SW script must always be revalidated,
/// so it needs an explicit `Cache-Control: no-cache` — this is the
/// regression guard: it FAILS against the pre-fix handler (no header at
/// all) and PASSES once `static_handler` special-cases `sw.js` the same
/// way it already special-cases `assets/`.
#[tokio::test]
async fn sw_js_gets_no_cache_control_for_revalidation() {
    let app = TestApp::new().await;
    let resp = app.router.clone().oneshot(get("/sw.js", "")).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let cache_control = resp
        .headers()
        .get(axum::http::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        cache_control,
        Some("no-cache"),
        "sw.js must always be revalidated so a new deploy's SW script reaches \
         users immediately instead of sitting behind Cloudflare's default \
         edge cache for up to 4h (#212)"
    );
}

/// Characterizes existing behavior that MUST survive the #212 fix: a
/// hashed asset under `assets/` still gets the long-cache immutable
/// header (the new `sw.js` branch must be an `else if`, never replace
/// the `assets/` branch).
#[tokio::test]
async fn hashed_asset_still_gets_long_cache_immutable_header() {
    let app = TestApp::new().await;
    let resp = app
        .router
        .clone()
        .oneshot(get("/assets/app-abc123.js", ""))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let cache_control = resp
        .headers()
        .get(axum::http::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok());
    assert_eq!(cache_control, Some("public, max-age=31536000, immutable"));
}

/// At the time of #212 (the `sw.js` edge-caching fix), `manifest.json` had
/// NO explicit `Cache-Control` — it was already `cf-cache-status: DYNAMIC`
/// on prod (not edge-cached), so #212 intentionally left it untouched.
/// #261 (round-5 belt-and-braces) DELIBERATELY changes this: the manifest
/// now optionally carries an install token (`?it=<raw>`), and a cached
/// stale/tokenless response was explicitly called out in that design as a
/// silent failure mode — so every `/manifest.json` response (token-bearing
/// or not) now sets `Cache-Control: no-store`. See
/// `routes/manifest.rs::manifest_response` and
/// `manifest_route.rs`'s `manifest_response_always_sets_cache_control_no_store`
/// unit test for the authoritative coverage of this header.
#[tokio::test]
async fn manifest_json_sets_cache_control_no_store() {
    let app = TestApp::new().await;
    let resp = app
        .router
        .clone()
        .oneshot(get("/manifest.json", ""))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "manifest.json must set Cache-Control: no-store (#261)"
    );
}
