//! Integration tests for #76 — PATCH /api/transactions/{id}/created-at.
//! Covers happy path, time-portion preservation, 30-day window enforcement,
//! 404, 409 (voided), and 403 (non-staff).

mod helpers;

use helpers::{TestApp, delete, patch_json, post_json};
use serde_json::json;

async fn seed_charge(app: &TestApp, code: &str) -> i64 {
    let card_id = app.seed_card(code, 50.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;
    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    resp.get("transaction_id").unwrap().as_i64().unwrap()
}

#[tokio::test]
async fn patch_created_at_happy_path_preserves_time() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-OK").await;

    // Fetch the original time portion so we can assert it survived.
    let original: String = sqlx::query_scalar("SELECT created_at FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let original_time = original.split_once(' ').unwrap().1.to_string();

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(3);
    let target_str = target.format("%Y-%m-%d").to_string();

    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target_str}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        resp.get("created_at_date").unwrap().as_str(),
        Some(target_str.as_str())
    );

    let stored: String = sqlx::query_scalar("SELECT created_at FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let (date_part, time_part) = stored.split_once(' ').unwrap();
    assert_eq!(date_part, target_str);
    assert_eq!(
        time_part, original_time,
        "time portion of created_at must be preserved across edit"
    );
}

#[tokio::test]
async fn patch_created_at_31_days_back_rejected() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-31").await;

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(31);
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("30 days"),
        "error message must mention the 30-day window"
    );
}

#[tokio::test]
async fn patch_created_at_future_date_rejected() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-FUT").await;

    let target = chrono::Local::now().date_naive() + chrono::Duration::days(1);
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("30 days"),
        "error message must mention the 30-day window"
    );
}

#[tokio::test]
async fn patch_created_at_missing_id_returns_404() {
    let app = TestApp::new().await;
    let target = chrono::Local::now().date_naive();
    let (status, _) = app
        .request(patch_json(
            "/api/transactions/9999999/created-at",
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_created_at_voided_returns_409() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-VOID").await;

    // Void the transaction first.
    let (void_status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(void_status, axum::http::StatusCode::NO_CONTENT);

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(1);
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn patch_created_at_non_staff_returns_403() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-403").await;

    let target = chrono::Local::now().date_naive();
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.customer_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
