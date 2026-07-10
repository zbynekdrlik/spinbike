//! Contract test for the typed HTTP error layer (#158).
//!
//! Every API error body now carries a stable machine-readable `error_code`
//! (snake_case) ALONGSIDE the human `error` message the UI/tests already read.
//! These tests lock that additive contract end-to-end through the live router
//! for one error of each HTTP class, so the `error_code` field cannot silently
//! disappear or drift.

mod helpers;

use axum::http::StatusCode;
use helpers::{TestApp, get, post_json};

#[tokio::test]
async fn forbidden_carries_staff_required_code() {
    // A customer hitting a staff-only listing endpoint. Also pins the
    // three-way "Staff access required"/"Staff only"/"Only staff can book…"
    // unification onto the single `staff_required` code (#158).
    let app = TestApp::new().await;
    let (status, body) = app.request(get("/api/users", &app.customer_token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_code"], "staff_required");
    assert_eq!(body["error"], "Staff access required");
}

#[tokio::test]
async fn unauthorized_carries_invalid_credentials_code() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "nobody@test.com",
        "password": "wrong-password",
    });
    let (status, resp) = app.request(post_json("/api/auth/login", "", &body)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(resp["error_code"], "invalid_credentials");
    assert_eq!(resp["error"], "Invalid email or password");
}

#[tokio::test]
async fn bad_request_carries_generic_code_with_specific_message() {
    // Negative charge amount → 400. The code is the generic `bad_request`;
    // the human message still carries the specifics the UI/tests read.
    let app = TestApp::new().await;
    let user_id = app.seed_card("ERRC1", 100.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;
    let body = serde_json::json!({
        "user_id": user_id,
        "amount": -5.0,
        "service_id": spinning_id,
    });
    let (status, resp) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["error_code"], "bad_request");
    assert!(
        resp["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Amount"),
        "message must still carry the specifics, got: {resp}"
    );
}
