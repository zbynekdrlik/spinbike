//! Integration tests for /api/payments/* handlers.
//!
//! Runs against the real Axum router with an in-memory DB. Kills mutation-test
//! survivors around amount validation, credit arithmetic, and transaction-log sign.

mod helpers;

use helpers::{TestApp, get, post_json};

#[tokio::test]
async fn charge_rejects_zero_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("C1", 100.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;

    let body = serde_json::json!({ "card_id": card_id, "amount": 0.0, "service_id": spinning_id });
    let (status, resp) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let err = resp.get("error").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        err.contains("Amount"),
        "expected amount-validation error, got: {err}"
    );
}

#[tokio::test]
async fn charge_rejects_negative_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("C2", 100.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;

    let body = serde_json::json!({ "card_id": card_id, "amount": -5.0, "service_id": spinning_id });
    let (status, resp) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let err = resp.get("error").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        err.contains("Amount"),
        "expected amount-validation error, got: {err}"
    );
}

#[tokio::test]
async fn charge_reduces_credit_by_exact_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("C3", 100.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;

    let body = serde_json::json!({ "card_id": card_id, "amount": 10.0, "service_id": spinning_id });
    let (status, resp) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    // Pinning the arithmetic — kills mutants that swap - for +, *, /.
    assert_eq!(resp["new_credit"].as_f64().unwrap(), 90.0);
    let tx_id = resp["transaction_id"].as_i64().unwrap();

    // Verify the ledger row is stored as NEGATIVE (ledger convention for debits).
    // This kills the "delete -" mutant on `-amount` at the transaction insert.
    let stored_amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored_amount, -10.0);

    // Verify the card credit persisted correctly (kills mutants on the balance math).
    let persisted: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(persisted, 90.0);
}

#[tokio::test]
async fn charge_forbidden_for_customer() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("C4", 100.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": 5.0 });
    let (status, _) = app
        .request(post_json(
            "/api/payments/charge",
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn storno_rejects_zero_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("S1", 100.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": 0.0 });
    let (status, _) = app
        .request(post_json("/api/payments/storno", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn storno_rejects_negative_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("S2", 100.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": -3.0 });
    let (status, _) = app
        .request(post_json("/api/payments/storno", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn storno_increases_credit_by_exact_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("S3", 50.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": 20.0 });
    let (status, resp) = app
        .request(post_json("/api/payments/storno", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    // Kills mutants that swap + for -, *, / in storno's credit math.
    assert_eq!(resp["new_credit"].as_f64().unwrap(), 70.0);

    // Ledger row must be POSITIVE (credit/refund convention).
    let tx_id = resp["transaction_id"].as_i64().unwrap();
    let stored_amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored_amount, 20.0);

    // Verify persistence.
    let persisted: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(persisted, 70.0);
}

#[tokio::test]
async fn storno_forbidden_for_customer() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("S4", 50.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": 5.0 });
    let (status, _) = app
        .request(post_json(
            "/api/payments/storno",
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn charge_blocked_card_is_conflict() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("BLK", 100.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;
    // Block the card directly in the DB.
    spinbike_server::db::cards::set_blocked(&app.pool, card_id, true)
        .await
        .unwrap();

    let body = serde_json::json!({ "card_id": card_id, "amount": 10.0, "service_id": spinning_id });
    let (status, _) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn charge_missing_card_is_404() {
    let app = TestApp::new().await;
    let spinning_id = app.spinning_service_id().await;
    let body = serde_json::json!({ "card_id": 999_999, "amount": 10.0, "service_id": spinning_id });
    let (status, _) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

/// Existence check: the payment routes must be registered on the router.
/// Kills mutants that replace `routes()` with `Router::new()` / default.
#[tokio::test]
async fn payment_routes_are_registered() {
    let app = TestApp::new().await;
    // Seed a real card so handler's "Card not found" 404 can't mask a
    // route-registration 404.
    let card_id = app
        .seed_card("REG-PAY", 100.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;
    let body = serde_json::json!({
        "card_id": card_id,
        "amount": 5.0,
        "service_id": spinning_id
    });
    let (status, _) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    // Storno doesn't require service_id (legacy reversal path); bare body OK.
    let storno_body = serde_json::json!({ "card_id": card_id, "amount": 5.0 });
    let (status, _) = app
        .request(post_json(
            "/api/payments/storno",
            &app.staff_token,
            &storno_body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    // Touch /api/cards so the `get` helper is exercised in this binary too.
    let _ = app.request(get("/api/cards", &app.staff_token)).await;
}
