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

/// An endpoint on the server's ALLOWLIST (`routes/push.rs`'s
/// `ALLOWED_PUSH_HOSTS`, #264 SSRF-hardening review finding) — every test
/// that expects `subscribe` to SUCCEED must use one of these, never an
/// arbitrary host.
fn fcm_endpoint(path: &str) -> String {
    format!("https://fcm.googleapis.com/fcm/send/{path}")
}

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

/// A blocked customer's session is invalid, not just merely-unauthorized —
/// same contract #268/#274/#277 established for `/api/my/balance`,
/// `/api/door/open`, and the booking routes. `/api/push/config` reads
/// account-specific data (`subscribed`) via `AuthUser`, so it must apply
/// the same session-invalidation check even though it's a GET.
#[tokio::test]
async fn config_blocked_user_returns_401_session_invalid() {
    let app = TestApp::new().await;
    spinbike_server::db::users::set_blocked(&app.pool, app.customer_id, true)
        .await
        .unwrap();

    let (status, resp) = app
        .request(get("/api/push/config", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert_eq!(resp["error_code"], "session_invalid");
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
    let endpoint = fcm_endpoint("customer-a");
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": endpoint,
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ? AND endpoint = ?",
    )
    .bind(app.customer_id)
    .bind(&endpoint)
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

/// Endpoint present but `keys.p256dh` empty — each of the three validated
/// fields must independently reject (kills a `||` -> `&&` mutant on the
/// combined emptiness check that a single all-fields-fine-except-endpoint
/// test alone wouldn't catch).
#[tokio::test]
async fn subscribe_rejects_missing_p256dh() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": fcm_endpoint("missing-p256dh"),
                "keys": { "p256dh": "", "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

/// Same as above for `keys.auth`.
#[tokio::test]
async fn subscribe_rejects_missing_auth() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": fcm_endpoint("missing-auth"),
                "keys": { "p256dh": TEST_P256DH, "auth": "" }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

/// SSRF hardening (#264 review finding): the server's own daily job later
/// POSTs directly to whatever `endpoint` a customer registers here, so an
/// arbitrary non-push-service host — including an internal/metadata-style
/// address — must be rejected at subscribe time, never stored.
#[tokio::test]
async fn subscribe_rejects_a_disallowed_host() {
    let app = TestApp::new().await;
    let (status, resp) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": "https://169.254.169.254/latest/meta-data/",
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "a rejected endpoint must never be persisted");
    let _ = resp;
}

/// A host-spoofing trick (a real allowed host name appearing only in the
/// PATH of an otherwise-unrelated URL) must not fool a naive substring
/// check — this test only passes against a REAL URL parse of the host.
#[tokio::test]
async fn subscribe_rejects_allowed_host_name_appearing_only_in_the_path() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": "https://evil.example/fcm.googleapis.com",
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

/// A plain `http://` endpoint to an otherwise-allowed host must still be
/// rejected — the allowlist requires `https://`.
#[tokio::test]
async fn subscribe_rejects_http_scheme_even_for_an_allowed_host() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": "http://fcm.googleapis.com/fcm/send/insecure",
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn resubscribe_same_endpoint_upserts_not_duplicates() {
    let app = TestApp::new().await;
    let endpoint = fcm_endpoint("resub");
    for _ in 0..2 {
        let (status, _) = app
            .request(post_json(
                "/api/push/subscribe",
                &app.customer_token,
                &serde_json::json!({
                    "endpoint": endpoint,
                    "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
                }),
            ))
            .await;
        assert_eq!(status, axum::http::StatusCode::OK);
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE endpoint = ?")
        .bind(&endpoint)
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
                "endpoint": fcm_endpoint("anon"),
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }))
            .unwrap(),
        ))
        .unwrap();
    let (status, _) = app.request(req).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

/// Same session-invalidation contract as `config` — a blocked customer's
/// permanent JWT must not be able to keep registering push subscriptions.
#[tokio::test]
async fn subscribe_blocked_user_returns_401_session_invalid() {
    let app = TestApp::new().await;
    spinbike_server::db::users::set_blocked(&app.pool, app.customer_id, true)
        .await
        .unwrap();

    let (status, resp) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": fcm_endpoint("blocked"),
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert_eq!(resp["error_code"], "session_invalid");

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "a rejected (dead-session) subscribe must not persist");
}

#[tokio::test]
async fn unsubscribe_removes_only_the_callers_own_subscription() {
    let app = TestApp::new().await;
    let mine = fcm_endpoint("mine");
    let staff = fcm_endpoint("staff");
    app.request(post_json(
        "/api/push/subscribe",
        &app.customer_token,
        &serde_json::json!({
            "endpoint": mine,
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
            "endpoint": staff,
            "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
        }),
    ))
    .await;

    let (status, _) = app
        .request(post_json(
            "/api/push/unsubscribe",
            &app.customer_token,
            &serde_json::json!({ "endpoint": mine }),
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

/// Same session-invalidation contract as `config`/`subscribe` — a blocked
/// customer must not be able to mutate their own subscription state
/// either.
#[tokio::test]
async fn unsubscribe_blocked_user_returns_401_session_invalid() {
    let app = TestApp::new().await;
    let endpoint = fcm_endpoint("tobeblocked");
    let (status, _) = app
        .request(post_json(
            "/api/push/subscribe",
            &app.customer_token,
            &serde_json::json!({
                "endpoint": endpoint,
                "keys": { "p256dh": TEST_P256DH, "auth": TEST_AUTH }
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    spinbike_server::db::users::set_blocked(&app.pool, app.customer_id, true)
        .await
        .unwrap();

    let (status, resp) = app
        .request(post_json(
            "/api/push/unsubscribe",
            &app.customer_token,
            &serde_json::json!({ "endpoint": endpoint }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert_eq!(resp["error_code"], "session_invalid");

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        n, 1,
        "the subscription must survive a rejected (dead-session) unsubscribe"
    );
}
