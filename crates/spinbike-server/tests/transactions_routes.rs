//! Integration tests for /api/transactions/{id} (PATCH valid-until + DELETE soft-delete).
mod helpers;
use axum::http::StatusCode;
#[allow(unused_imports)]
use helpers::{TestApp, delete, get};

async fn seed_topup(app: &TestApp, amount: f64) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action) VALUES (?, ?, 'topup') RETURNING id",
    )
    .bind(app.customer_card_id)
    .bind(amount)
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn delete_transaction_is_staff_only() {
    let app = TestApp::new().await;
    let tx_id = seed_topup(&app, 5.0).await;
    let (status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delete_missing_transaction_returns_404() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(delete("/api/transactions/999999", &app.staff_token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_topup_reverses_credit_and_soft_deletes() {
    let app = TestApp::new().await;

    // Simulate "topup already applied" state by manually putting 10.0 on the card
    // AND inserting a +10 topup row (what the topup handler would have written).
    sqlx::query("UPDATE cards SET credit = 10.0 WHERE id = ?")
        .bind(app.customer_card_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let tx_id = seed_topup(&app, 10.0).await;

    let (status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let credit: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(app.customer_card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        (credit - 0.0).abs() < 0.001,
        "credit should reverse to 0.0, got {credit}"
    );

    let deleted_at: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(deleted_at.is_some());
}

#[tokio::test]
async fn delete_charge_refunds_credit() {
    // Charges are stored with NEGATIVE amount. Voiding a charge of -7
    // must add 7 back to credit.
    let app = TestApp::new().await;
    sqlx::query("UPDATE cards SET credit = 3.0 WHERE id = ?")
        .bind(app.customer_card_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action) VALUES (?, -7.0, 'charge') RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let (status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let credit: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(app.customer_card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        (credit - 10.0).abs() < 0.001,
        "voiding a charge must refund; got {credit}"
    );
}
