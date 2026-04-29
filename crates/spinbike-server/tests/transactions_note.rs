//! Integration tests for #26 — per-transaction note support.
//! Covers: create endpoints accepting optional note (≤200 chars).

mod helpers;

use helpers::{TestApp, post_json};
use serde_json::json;

#[tokio::test]
async fn charge_persists_note() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-CHARGE", 50.0, None, None, None, None)
        .await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 2.50, "note": "Proteinová tyčinka"}),
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
            "/api/cards/topup",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 30.0, "note": "Platil v hotovosti"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let note: Option<String> = sqlx::query_scalar(
        "SELECT note FROM transactions WHERE card_id = ? ORDER BY id DESC LIMIT 1",
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
            &json!({"card_id": card_id, "price": 35.0, "valid_until": valid_until,
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

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 1.0, "note": "   "}),
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
}

#[tokio::test]
async fn note_over_200_chars_rejected() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("NOTE-LONG", 50.0, None, None, None, None)
        .await;
    let long = "x".repeat(201);

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 1.0, "note": long}),
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

    let (status, _) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 1.0}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}
