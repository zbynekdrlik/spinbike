//! Integration tests for #143 — reusing an email held by a SOFT-DELETED
//! account must return a clear, RESOLVABLE 409 (not an opaque 500), plus the
//! two staff-only resolution endpoints (restore / free-email).

mod helpers;

use axum::http::StatusCode;
use helpers::{TestApp, delete, post_json, put_json};

/// Soft-delete a seeded user via the real DELETE endpoint (sets deleted_at).
async fn soft_delete(app: &TestApp, id: i64) {
    let (status, _) = app
        .request(delete(&format!("/api/users/{id}"), &app.staff_token))
        .await;
    assert_eq!(status, StatusCode::OK, "soft-delete must succeed");
}

// ─── the bug: PUT used to 500 ──────────────────────────────────────────────

#[tokio::test]
async fn update_with_soft_deleted_email_returns_structured_409_not_500() {
    let app = TestApp::new().await;
    let old = app
        .seed_user("Old Owner", Some("shared@example.com"), None, None)
        .await;
    let target = app.seed_user("Target", None, None, Some("T-CARD")).await;
    soft_delete(&app, old).await;

    // The #143 repro: PUT another user with the soft-deleted email. Before the
    // fix this hit the raw email UNIQUE constraint → opaque 500.
    let (status, body) = app
        .request(put_json(
            &format!("/api/users/{target}"),
            &app.staff_token,
            &serde_json::json!({ "email": "shared@example.com" }),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "must be a clean 409, not 500: {body}"
    );
    assert_eq!(body["error_code"], "email_belongs_to_deleted_account");
    assert_eq!(
        body["conflict_id"].as_i64(),
        Some(old),
        "must name the archived account id"
    );
    assert_eq!(body["conflict_name"], "Old Owner");
    assert!(
        body["conflict_deleted_at"].as_str().is_some(),
        "must carry the deletion timestamp: {body}"
    );
}

#[tokio::test]
async fn create_with_soft_deleted_email_returns_structured_409() {
    let app = TestApp::new().await;
    let old = app
        .seed_user("Archived", Some("reuse@example.com"), None, None)
        .await;
    soft_delete(&app, old).await;

    let (status, body) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({ "name": "New Person", "email": "reuse@example.com" }),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error_code"], "email_belongs_to_deleted_account");
    assert_eq!(body["conflict_id"].as_i64(), Some(old));
    assert_eq!(body["conflict_name"], "Archived");
}

// ─── restore endpoint ──────────────────────────────────────────────────────

#[tokio::test]
async fn restore_endpoint_undeletes_and_keeps_data() {
    let app = TestApp::new().await;
    let old = app
        .seed_user("Restore Me", Some("r@example.com"), Some(9.0), Some("R-1"))
        .await;
    soft_delete(&app, old).await;

    let (status, _) = app
        .request(post_json(
            &format!("/api/users/{old}/restore"),
            &app.staff_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    // deleted_at cleared, data intact.
    let deleted_at: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM users WHERE id = ?")
            .bind(old)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(deleted_at, None, "account must be active again");
    let (email, credit): (Option<String>, f64) =
        sqlx::query_as("SELECT email, credit FROM users WHERE id = ?")
            .bind(old)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(email.as_deref(), Some("r@example.com"));
    assert!((credit - 9.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn restore_endpoint_forbidden_for_customer() {
    let app = TestApp::new().await;
    let old = app.seed_user("X", Some("x@example.com"), None, None).await;
    soft_delete(&app, old).await;

    let (status, body) = app
        .request(post_json(
            &format!("/api/users/{old}/restore"),
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_code"], "staff_required");
}

#[tokio::test]
async fn restore_missing_user_returns_404() {
    let app = TestApp::new().await;
    let (status, body) = app
        .request(post_json(
            "/api/users/999999/restore",
            &app.staff_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error_code"], "user_not_found");
}

// ─── free-email endpoint ───────────────────────────────────────────────────

#[tokio::test]
async fn free_email_clears_address_and_allows_reuse() {
    let app = TestApp::new().await;
    let old = app
        .seed_user("Owner", Some("free@example.com"), None, None)
        .await;
    soft_delete(&app, old).await;

    let (status, _) = app
        .request(post_json(
            &format!("/api/users/{old}/free-email"),
            &app.staff_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    // Email cleared, row stays archived.
    let (email, deleted_at): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT email, deleted_at FROM users WHERE id = ?")
            .bind(old)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(email, None, "email must be cleared");
    assert!(deleted_at.is_some(), "row must stay archived");

    // The address is now free for a brand-new user.
    let (status, body) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({ "name": "Fresh", "email": "free@example.com" }),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn free_email_forbidden_for_customer() {
    let app = TestApp::new().await;
    let old = app.seed_user("Y", Some("y@example.com"), None, None).await;
    soft_delete(&app, old).await;

    let (status, body) = app
        .request(post_json(
            &format!("/api/users/{old}/free-email"),
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_code"], "staff_required");
}

#[tokio::test]
async fn free_email_refuses_active_account() {
    let app = TestApp::new().await;
    // ACTIVE user — free-email must refuse to strip a live account's email.
    let active = app
        .seed_user("Active", Some("keep@example.com"), None, None)
        .await;

    let (status, _) = app
        .request(post_json(
            &format!("/api/users/{active}/free-email"),
            &app.staff_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "must refuse to free a live account's email"
    );
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(active)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        email.as_deref(),
        Some("keep@example.com"),
        "live email must be untouched"
    );
}

// ─── end-to-end resolution: free-email then retry succeeds ─────────────────

#[tokio::test]
async fn update_retry_after_free_email_succeeds() {
    let app = TestApp::new().await;
    let old = app
        .seed_user("Old", Some("moveme@example.com"), None, None)
        .await;
    let target = app.seed_user("Target", None, None, Some("TG")).await;
    soft_delete(&app, old).await;

    // 1) PUT collides → structured 409.
    let (status, body) = app
        .request(put_json(
            &format!("/api/users/{target}"),
            &app.staff_token,
            &serde_json::json!({ "email": "moveme@example.com" }),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error_code"], "email_belongs_to_deleted_account");

    // 2) Free the archived account's email.
    let (status, _) = app
        .request(post_json(
            &format!("/api/users/{old}/free-email"),
            &app.staff_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    // 3) Retry the SAME update — now succeeds.
    let (status, body) = app
        .request(put_json(
            &format!("/api/users/{target}"),
            &app.staff_token,
            &serde_json::json!({ "email": "moveme@example.com" }),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "retry must succeed: {body}");
    assert_eq!(body["email"], "moveme@example.com");
}
