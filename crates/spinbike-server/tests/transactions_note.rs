//! Integration tests for #26 — per-transaction note support.
//! Covers: create endpoints accepting optional note (≤200 chars), and
//! PATCH /api/transactions/{id}/note.

mod helpers;

use helpers::{TestApp, delete, patch_json, post_json};
use serde_json::json;

#[tokio::test]
async fn charge_persists_note() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-CHARGE", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 2.50, "service_id": spinning_id, "note": "Proteinová tyčinka"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some("Proteinová tyčinka"));
}

#[tokio::test]
async fn topup_persists_note() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-TOPUP", 0.0, None, None, None, None)
        .await;

    let (status, _) = app
        .request(post_json(
            "/api/users/topup",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 30.0, "note": "Platil v hotovosti"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let note: Option<String> = sqlx::query_scalar(
        "SELECT note FROM transactions WHERE user_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(note.as_deref(), Some("Platil v hotovosti"));
}

#[tokio::test]
async fn sell_pass_persists_note() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-PASS", 50.0, None, None, None, None)
        .await;
    let valid_until = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();

    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({"user_id": card_id, "price": 35.0, "valid_until": valid_until,
                    "note": "Zľava 10%"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some("Zľava 10%"));
}

#[tokio::test]
async fn empty_note_stored_as_null() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-EMPTY", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    // Case 1: whitespace-only note must store as NULL.
    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id, "note": "   "}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(note.is_none(), "whitespace-only note must store as NULL");

    // Case 2: empty string note must also store as NULL.
    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id, "note": ""}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(note.is_none(), "empty-string note must store as NULL");
}

#[tokio::test]
async fn note_over_200_chars_rejected() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-LONG", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;
    let long = "x".repeat(201);

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id, "note": long}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("200 characters"),
        "error message must mention 200 characters"
    );
}

#[tokio::test]
async fn missing_note_field_works_unchanged() {
    // Legacy clients send no note field; default deserializer must keep working.
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-MISSING", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let (status, _) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

#[tokio::test]
async fn log_visit_persists_note() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-VISIT", 50.0, None, None, None, None)
        .await;

    // Give the card an active monthly pass so log-visit will succeed.
    let valid_until = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({"user_id": card_id, "price": 35.0, "valid_until": valid_until}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "sell-pass must succeed");

    // Look up a valid Spinning service id (seeded by V8 migration).
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({"user_id": card_id, "service_id": spinning_id, "note": "priviedol kamarata"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "log-visit must succeed");
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let (note, action, amount): (Option<String>, String, f64) =
        sqlx::query_as("SELECT note, action, amount FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(note.as_deref(), Some("priviedol kamarata"));
    assert_eq!(action, "visit");
    assert_eq!(amount, 0.0);
}

#[tokio::test]
async fn note_at_200_chars_accepted() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-200", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let exactly_200 = "x".repeat(200);
    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id, "note": exactly_200.clone()}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "note of exactly 200 chars must be accepted"
    );
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some(exactly_200.as_str()));
}

// ─── PATCH /api/transactions/{id}/note ────────────────────────────────────────

#[tokio::test]
async fn patch_note_updates_existing_row() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("PATCH-1", 50.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id, "note": "first"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let (patch_status, patch_resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/note"),
            &app.staff_token,
            &json!({"note": "edited"}),
        ))
        .await;
    assert_eq!(patch_status, axum::http::StatusCode::OK);
    assert_eq!(patch_resp.get("note").unwrap().as_str(), Some("edited"));

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some("edited"));
}

#[tokio::test]
async fn patch_note_clears_with_null_or_empty() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("PATCH-2", 50.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id, "note": "to clear"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    // Clear with explicit null.
    let (patch_status, patch_resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/note"),
            &app.staff_token,
            &json!({"note": null}),
        ))
        .await;
    assert_eq!(patch_status, axum::http::StatusCode::OK);
    assert!(patch_resp.get("note").unwrap().is_null());

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(note.is_none());
}

#[tokio::test]
async fn patch_note_rejects_voided_409() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PATCH-VOID", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    // Void the transaction.
    let (void_status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(void_status, axum::http::StatusCode::NO_CONTENT);

    // Attempt to patch note on voided transaction — must get 409 Conflict.
    let (patch_status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/note"),
            &app.staff_token,
            &json!({"note": "after void"}),
        ))
        .await;
    assert_eq!(patch_status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn patch_note_rejects_over_200_chars() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PATCH-LONG", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let long = "x".repeat(201);
    let (patch_status, patch_resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/note"),
            &app.staff_token,
            &json!({"note": long}),
        ))
        .await;
    assert_eq!(patch_status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        patch_resp
            .get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("200 characters"),
        "error message must mention 200 characters"
    );
}

#[tokio::test]
async fn patch_note_returns_404_when_id_missing() {
    let app = TestApp::new().await;

    let (patch_status, _) = app
        .request(patch_json(
            "/api/transactions/9999999/note",
            &app.staff_token,
            &json!({"note": "x"}),
        ))
        .await;
    assert_eq!(patch_status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_note_requires_staff_role() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PATCH-403", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    // Customer token must be rejected with 403 Forbidden.
    let (patch_status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/note"),
            &app.customer_token,
            &json!({"note": "x"}),
        ))
        .await;
    assert_eq!(patch_status, axum::http::StatusCode::FORBIDDEN);
}

// ─── Boundary tests: topup ────────────────────────────────────────────────────

#[tokio::test]
async fn topup_at_200_chars_accepted() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("TOPUP-200", 0.0, None, None, None, None)
        .await;

    let exactly_200 = "x".repeat(200);
    let (status, _) = app
        .request(post_json(
            "/api/users/topup",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 30.0, "note": exactly_200.clone()}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "topup note of exactly 200 chars must be accepted"
    );

    let note: Option<String> = sqlx::query_scalar(
        "SELECT note FROM transactions WHERE user_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(note.as_deref(), Some(exactly_200.as_str()));
}

#[tokio::test]
async fn topup_over_200_chars_rejected() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("TOPUP-201", 0.0, None, None, None, None)
        .await;

    let long = "x".repeat(201);
    let (status, resp) = app
        .request(post_json(
            "/api/users/topup",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 30.0, "note": long}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "topup note of 201 chars must be rejected"
    );
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("200 characters"),
        "error message must mention 200 characters"
    );
}

// ─── Boundary tests: sell_pass ────────────────────────────────────────────────

#[tokio::test]
async fn sell_pass_at_200_chars_accepted() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PASS-200", 100.0, None, None, None, None)
        .await;
    let valid_until = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();

    let exactly_200 = "x".repeat(200);
    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({"user_id": card_id, "price": 35.0, "valid_until": valid_until,
                    "note": exactly_200.clone()}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "sell-pass note of exactly 200 chars must be accepted"
    );
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some(exactly_200.as_str()));
}

#[tokio::test]
async fn sell_pass_over_200_chars_rejected() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PASS-201", 100.0, None, None, None, None)
        .await;
    let valid_until = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();

    let long = "x".repeat(201);
    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({"user_id": card_id, "price": 35.0, "valid_until": valid_until,
                    "note": long}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "sell-pass note of 201 chars must be rejected"
    );
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("200 characters"),
        "error message must mention 200 characters"
    );
}

// ─── Boundary tests: log_visit ────────────────────────────────────────────────

#[tokio::test]
async fn log_visit_at_200_chars_accepted() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("VISIT-200", 50.0, None, None, None, None)
        .await;

    // Give the card an active pass.
    let valid_until = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({"user_id": card_id, "price": 35.0, "valid_until": valid_until}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "sell-pass must succeed");

    let spinning_id = app.spinning_service_id().await;

    let exactly_200 = "x".repeat(200);
    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({"user_id": card_id, "service_id": spinning_id, "note": exactly_200.clone()}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "log-visit note of exactly 200 chars must be accepted"
    );
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some(exactly_200.as_str()));
}

#[tokio::test]
async fn log_visit_over_200_chars_rejected() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("VISIT-201", 50.0, None, None, None, None)
        .await;

    // Give the card an active pass.
    let valid_until = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({"user_id": card_id, "price": 35.0, "valid_until": valid_until}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "sell-pass must succeed");

    let spinning_id = app.spinning_service_id().await;

    let long = "x".repeat(201);
    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({"user_id": card_id, "service_id": spinning_id, "note": long}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "log-visit note of 201 chars must be rejected"
    );
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("200 characters"),
        "error message must mention 200 characters"
    );
}

// ─── Boundary tests: patch_note ───────────────────────────────────────────────

#[tokio::test]
async fn patch_note_at_200_chars_accepted() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PATCH-200", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    let exactly_200 = "x".repeat(200);
    let (patch_status, patch_resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/note"),
            &app.staff_token,
            &json!({"note": exactly_200.clone()}),
        ))
        .await;
    assert_eq!(
        patch_status,
        axum::http::StatusCode::OK,
        "patch note of exactly 200 chars must be accepted"
    );
    assert_eq!(
        patch_resp.get("note").unwrap().as_str(),
        Some(exactly_200.as_str())
    );

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some(exactly_200.as_str()));
}

#[tokio::test]
async fn patch_note_whitespace_only_stored_as_null() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("PATCH-WS", 50.0, None, None, None, None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id, "note": "initial note"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();

    // PATCH with whitespace-only note — must normalize to NULL, not store "   ".
    let (patch_status, patch_resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/note"),
            &app.staff_token,
            &json!({"note": "   "}),
        ))
        .await;
    assert_eq!(
        patch_status,
        axum::http::StatusCode::OK,
        "patch with whitespace-only note must succeed"
    );
    assert!(
        patch_resp.get("note").unwrap().is_null(),
        "response note must be null for whitespace-only input"
    );

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        note.is_none(),
        "whitespace-only patch note must be stored as NULL in DB"
    );
}
