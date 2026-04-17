//! Integration tests for /api/payments/sell-pass, /api/payments/log-visit,
//! and the `pass` field on CardResponse.

mod helpers;

use helpers::{TestApp, get, post_json};
use serde_json::json;

async fn set_blocked(app: &TestApp, card_id: i64) {
    sqlx::query("UPDATE cards SET blocked = 1 WHERE id = ?")
        .bind(card_id)
        .execute(&app.pool)
        .await
        .unwrap();
}

async fn card_credit(app: &TestApp, card_id: i64) -> f64 {
    sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn service_id(app: &TestApp, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM services WHERE name = ?")
        .bind(name)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn sell_pass_debits_credit_and_records_valid_until() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("SELL-PASS-1", 50.0, None, None, None, None)
        .await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-05-17" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "body = {resp}");
    assert_eq!(resp["new_credit"].as_f64().unwrap(), 15.0);
    assert_eq!(resp["valid_until"], "2030-05-17");
    // Kills mutation where (valid_until - today) is flipped to (today - valid_until).
    // 2030-05-17 must be in the future at test-run time, so days_remaining is strictly positive.
    let days = resp["days_remaining"].as_i64().unwrap();
    assert!(
        days > 0,
        "days_remaining must be strictly positive for a future valid_until, got {days}"
    );

    assert_eq!(card_credit(&app, card_id).await, 15.0);

    let tx_id = resp["transaction_id"].as_i64().unwrap();
    let (amount, valid_until, service_id): (f64, Option<chrono::NaiveDate>, i64) =
        sqlx::query_as("SELECT amount, valid_until, service_id FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        amount, -35.0,
        "monthly pass amount stored as negative (ledger convention)"
    );
    assert_eq!(
        valid_until,
        Some(chrono::NaiveDate::from_ymd_opt(2030, 5, 17).unwrap())
    );
    let pass_svc_id: i64 =
        sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(service_id, pass_svc_id);
}

#[tokio::test]
async fn sell_pass_rejects_past_valid_until() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("SELL-PAST", 100.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2020-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sell_pass_rejects_negative_price() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("SELL-NEG", 100.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": -1.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sell_pass_rejects_blocked_card() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("SELL-BLOCKED", 100.0, None, None, None, None)
        .await;
    set_blocked(&app, card_id).await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn log_visit_writes_zero_amount_when_pass_active() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("VISIT-1", 50.0, None, None, None, None).await;

    // Sell a pass first (relies on Task 5's handler)
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let spinning_id = service_id(&app, "Spinning").await;
    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "card_id": card_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let tx_id = resp["transaction_id"].as_i64().unwrap();
    let (amount, action, service_id_val): (f64, String, i64) =
        sqlx::query_as("SELECT amount, action, service_id FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(amount, 0.0);
    assert_eq!(action, "visit");
    assert_eq!(service_id_val, spinning_id);

    // Credit unchanged (50 - 35 = 15)
    assert_eq!(card_credit(&app, card_id).await, 15.0);
}

#[tokio::test]
async fn log_visit_rejects_card_without_active_pass() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("VISIT-2", 50.0, None, None, None, None).await;
    let spinning_id = service_id(&app, "Spinning").await;

    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "card_id": card_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn log_visit_rejects_card_with_expired_pass() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("VISIT-3", 50.0, None, None, None, None).await;

    // Insert an expired pass transaction directly via SQL
    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (card_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, -35.0, 'charge', ?, datetime('now'))",
    )
    .bind(card_id)
    .bind(pass_svc)
    .bind(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap())
    .execute(&app.pool)
    .await
    .unwrap();

    let spinning_id = service_id(&app, "Spinning").await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "card_id": card_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn log_visit_rejects_unknown_service_id() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("VISIT-SVC", 50.0, None, None, None, None)
        .await;

    // Sell an active pass so the pass check passes — we want to isolate the service_id check
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "card_id": card_id, "service_id": 99999 }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn card_response_includes_pass_field_when_pass_active() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PASS-RESP-1", 50.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (status, body) = app
        .request(get("/api/cards/lookup/PASS-RESP-1", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["pass"]["valid_until"], "2030-01-01");
    let days = body["pass"]["days_remaining"].as_i64().unwrap();
    assert!(
        days > 0,
        "days_remaining must be positive for an active pass"
    );
}

#[tokio::test]
async fn card_response_pass_field_is_null_when_no_pass() {
    let app = TestApp::new().await;
    app.seed_card("NO-PASS-RESP", 10.0, None, None, None, None)
        .await;
    let (status, body) = app
        .request(get("/api/cards/lookup/NO-PASS-RESP", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        body["pass"].is_null(),
        "pass must be null when card has no pass"
    );
}
