//! Integration tests for #56 — DELETE /api/users/{id} (soft-delete).
//! Covers happy path, 409 (already deleted), 404, 403, transactions retained,
//! search hides, negative-balance hides.

mod helpers;

use helpers::{TestApp, delete, get, post_json, put_json};
use serde_json::json;

#[tokio::test]
async fn delete_user_happy_path_sets_deleted_at() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DEL-OK", 0.0, None, None, Some("Bye"), None)
        .await;
    let (status, body) = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["id"].as_i64(), Some(user_id));
    assert!(body["deleted_at"].is_string());

    let stored: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(stored.is_some());
}

#[tokio::test]
async fn delete_user_already_deleted_returns_409() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DEL-409", 0.0, None, None, Some("Twice"), None)
        .await;
    let _ = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    let (status, _) = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn delete_user_missing_id_returns_404() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(delete("/api/users/9999999", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_user_non_staff_returns_403() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DEL-403", 0.0, None, None, Some("NotMine"), None)
        .await;
    let (status, _) = app
        .request(delete(
            &format!("/api/users/{user_id}"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delete_user_does_not_remove_transactions() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DEL-TX", 50.0, None, None, Some("Spend"), None)
        .await;
    let spinning_id = app.spinning_service_id().await;
    let (_, _) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": user_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    let (_, _) = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(count > 0, "transactions must remain after soft-delete");
}

#[tokio::test]
async fn deleted_user_hidden_from_search() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DEL-SRCH", 0.0, None, None, Some("FindMeNot"), None)
        .await;
    let _ = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    let (_, body) = app
        .request(get("/api/users/search?q=FindMeNot", &app.staff_token))
        .await;
    let arr = body.as_array().unwrap();
    assert!(
        arr.iter().all(|r| r["id"].as_i64() != Some(user_id)),
        "soft-deleted user must not appear in search"
    );
}

#[tokio::test]
async fn deleted_user_hidden_from_negative_balance() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DEL-NEG", 0.0, None, None, Some("Owes"), None)
        .await;
    // Write negative balance directly — no allow_debit route is exposed.
    sqlx::query("UPDATE users SET credit = -5.0 WHERE id = ?")
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let _ = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;

    let (_, body) = app
        .request(get("/api/users/negative-balance", &app.staff_token))
        .await;
    let arr = body.as_array().unwrap();
    assert!(
        arr.iter().all(|r| r["id"].as_i64() != Some(user_id)),
        "soft-deleted user must not appear in negative-balance list"
    );
}

/// After soft-delete, `POST /api/users/topup` on the same id must return 404
/// — soft-deleted users are invariant-frozen (#56 review).
#[tokio::test]
async fn deleted_user_topup_returns_404() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("FROZ-T", 0.0, None, None, Some("Frozen"), None)
        .await;
    let _ = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/users/topup",
            &app.staff_token,
            &json!({"user_id": user_id, "amount": 5.0}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::NOT_FOUND,
        "topup on soft-deleted user must 404"
    );
}

/// After soft-delete, `POST /api/users/block` must return 404 (#56 review).
#[tokio::test]
async fn deleted_user_block_returns_404() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("FROZ-B", 0.0, None, None, Some("Frozen B"), None)
        .await;
    let _ = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/users/block",
            &app.staff_token,
            &json!({"user_id": user_id, "blocked": true}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::NOT_FOUND,
        "block on soft-deleted user must 404"
    );
}

/// After soft-delete, `PUT /api/users/{id}` must return 404 (#56 review).
#[tokio::test]
async fn deleted_user_update_returns_404() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("FROZ-U", 0.0, None, None, Some("Frozen U"), None)
        .await;
    let _ = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{user_id}"),
            &app.staff_token,
            &json!({"name": "RenamedAfterDelete"}),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::NOT_FOUND,
        "update on soft-deleted user must 404"
    );
}

/// Delete must succeed regardless of credit balance — modal warns but does
/// not block (#56 spec). Backend never enforces a balance gate.
#[tokio::test]
async fn delete_user_succeeds_with_negative_balance() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("WARN-NEG", 0.0, None, None, Some("InDebt"), None)
        .await;
    sqlx::query("UPDATE users SET credit = -7.5 WHERE id = ?")
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, _) = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

/// Delete must succeed even when an active permanentka exists — modal warns
/// but does not block (#56 spec).
#[tokio::test]
async fn delete_user_succeeds_with_active_permanentka() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("WARN-PASS", 0.0, None, None, Some("HasPass"), None)
        .await;
    let future = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let pass_service: i64 =
        sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass' LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    // A real active pass is a 'charge' against the monthly_pass service (the
    // V20 trigger, #204, rejects valid_until on any other action/service).
    sqlx::query(
        "INSERT INTO transactions(user_id, service_id, amount, action, valid_until)
         VALUES(?, ?, -35.0, 'charge', ?)",
    )
    .bind(user_id)
    .bind(pass_service)
    .bind(&future)
    .execute(&app.pool)
    .await
    .unwrap();
    let (status, _) = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}
