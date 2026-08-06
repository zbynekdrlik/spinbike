//! Integration tests for `POST /api/metrics/launch` (#260) — the standalone-
//! launch beacon the frontend fires on boot. See `crate::routes::metrics`
//! for why this endpoint exists (discriminating the three hypotheses that
//! made round 4 of the iOS install auto-login fix, #258, undiagnosable).

mod helpers;

use helpers::{TestApp, post_json};
use serde_json::json;

fn beacon_body() -> serde_json::Value {
    json!({
        "path": "/my/balance",
        "query_redacted": "",
        "had_token": false,
        "had_session": true,
        "src": null,
    })
}

/// The whole point of the beacon: a fresh standalone launch may have no
/// session at all, so the endpoint must accept the request with NO
/// Authorization header (`post_json`'s `token = ""` sends `Bearer `, which
/// the handler never even inspects — this route takes no `AuthUser`
/// extractor at all).
#[tokio::test]
async fn launch_beacon_accepted_without_auth() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json("/api/metrics/launch", "", &beacon_body()))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
}

/// Two rapid-fire beacons from the SAME client (no IP header -> the
/// "unknown" bucket) hit the 2s per-key min-gap; the second is rejected.
#[tokio::test]
async fn launch_beacon_is_rate_limited_per_key() {
    let app = TestApp::new().await;

    let (first, _) = app
        .request(post_json("/api/metrics/launch", "", &beacon_body()))
        .await;
    assert_eq!(first, axum::http::StatusCode::NO_CONTENT);

    let (second, _) = app
        .request(post_json("/api/metrics/launch", "", &beacon_body()))
        .await;
    assert_eq!(
        second,
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        "back-to-back beacons from the same client must hit the min-gap"
    );
}

/// A second, DIFFERENT client (distinct `Cf-Connecting-Ip`) is unaffected by
/// the first client's rate limit — proves the rate-limit key is actually
/// derived per-IP, not a single shared bucket regardless of headers.
#[tokio::test]
async fn launch_beacon_rate_limit_is_scoped_per_ip() {
    let app = TestApp::new().await;

    let req_a = axum::http::Request::builder()
        .method("POST")
        .uri("/api/metrics/launch")
        .header("content-type", "application/json")
        .header("cf-connecting-ip", "1.1.1.1")
        .body(axum::body::Body::from(
            serde_json::to_vec(&beacon_body()).unwrap(),
        ))
        .unwrap();
    let (status_a, _) = app.request(req_a).await;
    assert_eq!(status_a, axum::http::StatusCode::NO_CONTENT);

    let req_b = axum::http::Request::builder()
        .method("POST")
        .uri("/api/metrics/launch")
        .header("content-type", "application/json")
        .header("cf-connecting-ip", "2.2.2.2")
        .body(axum::body::Body::from(
            serde_json::to_vec(&beacon_body()).unwrap(),
        ))
        .unwrap();
    let (status_b, _) = app.request(req_b).await;
    assert_eq!(
        status_b,
        axum::http::StatusCode::NO_CONTENT,
        "a different client IP must not be throttled by the first client's beacon"
    );
}
