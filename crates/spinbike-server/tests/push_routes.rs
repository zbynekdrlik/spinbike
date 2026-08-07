//! Integration tests for `/api/push/config`, `/api/push/subscribe`,
//! `/api/push/unsubscribe` (#264).
//!
//! `TestApp` wires `PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64)`
//! (see `helpers::mod.rs`) — a real, valid test VAPID key, always enabled,
//! deterministic (no env var mutation).

mod helpers;

use helpers::{TestApp, get, post_json};

/// A real, valid uncompressed P-256 point + 16-byte auth secret from the
/// `web-push` crate's own test suite (see `push.rs`'s
/// `TEST_P256DH_B64`/`TEST_AUTH_B64`) — not a secret, just needs to be
/// well-formed enough for the server's encryption step to accept it
/// (these routes never actually SEND — the daily job does — but the DB
/// layer stores whatever the client posts, so any string round-trips; kept
/// realistic anyway for consistency with the job's own tests).
const TEST_P256DH: &str =
    "BH1HTeKM7-NwaLGHEqxeu2IamQaVVLkcsFHPIHmsCnqxcBHPQBprF41bEMOr3O1hUQ2jU1opNEm1F_lZV_sxMP8";
const TEST_AUTH: &str = "sBXU5_tIYz-5w7G2B25BEw";

#[tokio::test]
async fn config_reports_enabled_and_the_public_key() {
    let app = TestApp::new().await;
    let (status, body) = app
        .request(get("/api/push/config", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["enabled"], true);
    assert!(body["public_key"].as_str().is_some());
    assert_eq!(body["subscribed"], false);
}

#[tokio::test]
async fn config_requires_authentication() {
    let app = TestApp::new().await;
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/api/push/config")
        .body(axum::body::Body::empty())
        .unwrap();
    let (status, _) = app.request(req).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn subscribe_stores_the_subscription_and_config_then_reports_subscribed() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": "https://push.example/customer-a",
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ? AND endpoint = ?",
    )
    .bind(app.customer_id)
    .bind("https://push.example/customer-a")
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);

    let (_, body) = app
        .request(get("/api/push/config", &app.customer_token))
        .await;
    assert_eq!(body["subscribed"], true);
}

#[tokio::test]
async fn subscribe_rejects_missing_endpoint() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": "",
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn resubscribe_same_endpoint_upserts_not_duplicates() {
    let app = TestApp::new().await;
    for _ in 0..2 {
        let (status, _) = app
            .request(post_json(
                "/api/push/subscribe",
                &app.customer_token,
                &serde_json::json!({
                    "endpoint": "https://push.example/resub",
                    "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
                }),
            ))
            .await;
        assert_eq!(status, axum::http::StatusCode::OK);
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE endpoint = ?")
        .bind("https://push.example/resub")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "re-subscribing must upsert, not duplicate");
}

#[tokio::test]
async fn subscribe_requires_authentication() {
    let app = TestApp::new().await;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/push/subscribe")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "endpoint": "https://push.example/anon",
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }))
            .unwrap(),
        ))
        .unwrap();
    let (status, _) = app.request(req).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unsubscribe_removes_only_the_callers_own_subscription() {
    let app = TestApp::new().await;
    app.request(post_json(
        "/api/push/subscribe",
        &app.customer_token,
        &serde_json::json!({
            "endpoint": "https://push.example/mine",
            "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
        }),
    ))
    .await;

    // A different user subscribes too, to prove unsubscribe is scoped and
    // doesn't touch an unrelated row.
    app.request(post_json(
        "/api/push/subscribe",
        &app.staff_token,
        &serde_json::json!({
            "endpoint": "https://push.example/staff",
            "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
        }),
    ))
    .await;

    let (status, _) = app
        .request(post_json(
            "/api/push/unsubscribe",
            &app.customer_token,
            &serde_json::json!({ "endpoint": "https://push.example/mine" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "only the customer's own row must be removed");

    let (_, body) = app
        .request(get("/api/push/config", &app.customer_token))
        .await;
    assert_eq!(body["subscribed"], false);
}
