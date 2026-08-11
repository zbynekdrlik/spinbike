//! Integration tests for /api/transactions/{id} (PATCH valid-until + DELETE soft-delete).
mod helpers;
use axum::http::StatusCode;
#[allow(unused_imports)]
use helpers::{TestApp, delete, get, patch_json};

async fn seed_topup(app: &TestApp, amount: f64) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (user_id, amount, action) VALUES (?, ?, 'topup') RETURNING id",
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

    // Simulate "topup already applied" state by manually putting 10.0 on the user
    // AND inserting a +10 topup row (what the topup handler would have written).
    sqlx::query("UPDATE users SET credit = 10.0 WHERE id = ?")
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

    let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
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
    sqlx::query("UPDATE users SET credit = 3.0 WHERE id = ?")
        .bind(app.customer_card_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (user_id, amount, action) VALUES (?, -7.0, 'charge') RETURNING id",
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

    let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(app.customer_card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        (credit - 10.0).abs() < 0.001,
        "voiding a charge must refund; got {credit}"
    );
}

#[tokio::test]
async fn patch_valid_until_updates_pass_end_date() {
    let app = TestApp::new().await;
    // A row with valid_until set must be a monthly_pass charge (the V20
    // trigger, #204, rejects valid_until on any other action/service).
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (user_id, amount, action, service_id, valid_until)
         SELECT ?, -35.0, 'charge', id, '2026-05-01' FROM services WHERE kind = 'monthly_pass' LIMIT 1
         RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["valid_until"].as_str(), Some("2026-06-15"));

    let stored: Option<String> =
        sqlx::query_scalar("SELECT valid_until FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some("2026-06-15"));
}

#[tokio::test]
async fn patch_valid_until_rejects_non_pass_transaction() {
    let app = TestApp::new().await;
    let tx_id = seed_topup(&app, 5.0).await; // topup has valid_until = NULL
    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn patch_valid_until_forbidden_for_customer() {
    let app = TestApp::new().await;
    // A row with valid_until set must be a monthly_pass charge (the V20
    // trigger, #204, rejects valid_until on any other action/service).
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (user_id, amount, action, service_id, valid_until)
         SELECT ?, -35.0, 'charge', id, '2026-05-01' FROM services WHERE kind = 'monthly_pass' LIMIT 1
         RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// #324: `patch_valid_until` was the only one of `transactions.rs`'s 4
/// mutation endpoints (`void`/`patch_note`/`patch_created_at`/
/// `patch_valid_until`) with no `deleted_at` guard — `patch_note` and
/// `patch_created_at` each reject an already-voided transaction with 409
/// Conflict (see `patch_note_rejects_voided_409` in `transactions_note.rs`),
/// `patch_valid_until` had none and would silently rewrite the pass expiry
/// on a row every sibling endpoint treats as frozen.
///
/// [red -> green]: failed with 200 OK (the PATCH silently succeeded on a
/// voided transaction) before the guard existed; passes now that
/// `patch_valid_until` rejects with 409 Conflict, mirroring its siblings.
#[tokio::test]
async fn patch_valid_until_rejects_voided_409() {
    let app = TestApp::new().await;
    // A row with valid_until set must be a monthly_pass charge (the V20
    // trigger, #204, rejects valid_until on any other action/service).
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (user_id, amount, action, service_id, valid_until)
         SELECT ?, -35.0, 'charge', id, '2026-05-01' FROM services WHERE kind = 'monthly_pass' LIMIT 1
         RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    // Void the transaction.
    let (void_status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(void_status, StatusCode::NO_CONTENT);

    // Attempt to patch valid_until on the now-voided transaction — must get
    // 409 Conflict, same contract as patch_note / patch_created_at.
    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (patch_status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(patch_status, StatusCode::CONFLICT);

    // The value must not have changed.
    let stored: Option<String> =
        sqlx::query_scalar("SELECT valid_until FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        stored.as_deref(),
        Some("2026-05-01"),
        "valid_until must be unchanged after a rejected PATCH on a voided transaction"
    );
}

#[tokio::test]
async fn patch_valid_until_missing_returns_404() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, _) = app
        .request(patch_json(
            "/api/transactions/999999/valid-until",
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
