//! Integration tests for the dynamic `/manifest.json` handler (#258).
//!
//! Proves the explicit route wins over the static-file fallback (distinct
//! `application/manifest+json` content type + the install-token `start_url`
//! rewrite), and that the token substitution changes ONLY `start_url` while
//! every other field (name, icons, ...) is preserved.

mod helpers;

use axum::http::{Request, StatusCode, header};
use helpers::TestApp;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::util::ServiceExt;

fn manifest_request(uri: &str) -> Request<axum::body::Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn fetch_manifest(app: &TestApp, uri: &str) -> (StatusCode, Option<String>, Value) {
    let resp = app
        .router
        .clone()
        .oneshot(manifest_request(uri))
        .await
        .unwrap();
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).expect("manifest body must be valid JSON");
    (status, content_type, json)
}

/// No `it` query → the manifest is served verbatim by the dynamic handler (NOT
/// the static fallback): 200, the `application/manifest+json` content type the
/// handler sets, the real icons, and the plain `start_url = "/"`.
#[tokio::test]
async fn manifest_without_token_is_served_verbatim_by_the_handler() {
    let app = TestApp::new().await;
    let (status, content_type, manifest) = fetch_manifest(&app, "/manifest.json").await;

    assert_eq!(status, StatusCode::OK);
    // The dynamic handler's own content type — proves it took precedence over
    // the static fallback (which would serve mime_guess's `application/json`).
    assert_eq!(content_type.as_deref(), Some("application/manifest+json"));

    assert_eq!(manifest["name"], "SpinBike");
    assert_eq!(
        manifest["start_url"], "/",
        "a token-less manifest must keep the plain root start_url"
    );
    // The icons the A2HS-eligibility heuristics read must be present.
    let icons = manifest["icons"].as_array().expect("icons array");
    assert!(
        icons.len() >= 5,
        "manifest must carry the svg + 4 png icons, got {}",
        icons.len()
    );
    let png_count = icons.iter().filter(|i| i["type"] == "image/png").count();
    assert!(png_count >= 4, "manifest must carry >= 4 png icons");
}

/// `it=<token>` → the SAME manifest with only `start_url` replaced by the
/// install-token welcome URL. Every other field (icons included) is byte-for-
/// byte identical to the token-less response.
#[tokio::test]
async fn manifest_with_install_token_swaps_only_start_url() {
    let app = TestApp::new().await;
    let install_tok = "raw-abc_123XYZ";
    let (status, content_type, swapped) =
        fetch_manifest(&app, &format!("/manifest.json?it={install_tok}")).await;
    let (_, _, verbatim) = fetch_manifest(&app, "/manifest.json").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/manifest+json"));
    assert_eq!(
        swapped["start_url"],
        format!("/welcome?t={install_tok}&src=install"),
        "start_url must carry the exact install token"
    );

    // Everything except start_url is preserved — compare the two objects with
    // start_url stripped from both (the "JSON equality incl. icons" the design
    // asks for).
    let mut a = swapped.as_object().unwrap().clone();
    let mut b = verbatim.as_object().unwrap().clone();
    a.remove("start_url");
    b.remove("start_url");
    assert_eq!(a, b, "no field other than start_url may change");
    assert_eq!(
        swapped["icons"], verbatim["icons"],
        "icons must be identical to the verbatim manifest"
    );
}

/// An empty `it=` (present but blank) is treated as absent → verbatim manifest.
#[tokio::test]
async fn manifest_with_empty_token_is_served_verbatim() {
    let app = TestApp::new().await;
    let (status, _, manifest) = fetch_manifest(&app, "/manifest.json?it=").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        manifest["start_url"], "/",
        "an empty it= must be treated as absent (verbatim start_url)"
    );
}
