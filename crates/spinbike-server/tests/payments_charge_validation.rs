//! Integration tests for #31 — charge endpoint must reject null service_id
//! as defense-in-depth (UI already prevents this, but curl / future endpoints
//! must not slip past). Top-up is unaffected (service-independent).

mod helpers;

use helpers::{TestApp, post_json};
use serde_json::json;

#[tokio::test]
async fn charge_rejects_null_service_id_with_400() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("CHARGE-NULL-SVC", 50.0, None, None, None, None)
        .await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": user_id, "amount": 1.50}),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let err = resp.get("error").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        err.contains("service_id"),
        "error message must mention service_id, got: {err}"
    );
}

#[tokio::test]
async fn charge_with_valid_service_id_still_succeeds() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("CHARGE-VALID-SVC", 50.0, None, None, None, None)
        .await;

    // Find the Fitness service id via /api/services or directly from DB.
    let fitness_id: i64 =
        sqlx::query_scalar("SELECT id FROM services WHERE name_en = 'Fitness' AND active = 1")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    let (status, _) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": user_id, "amount": 5.00, "service_id": fitness_id}),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::OK);
}

#[tokio::test]
async fn topup_still_accepts_null_service_id() {
    // Top-up is service-independent — the new charge rule must NOT leak into top-up.
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("TOPUP-NULL-SVC", 0.0, None, None, None, None)
        .await;

    let (status, _) = app
        .request(post_json(
            "/api/users/topup",
            &app.staff_token,
            &json!({"user_id": user_id, "amount": 30.0}),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::OK);
}
