//! Integration tests for /api/auth/* handlers.

mod helpers;

use helpers::{TestApp, post_json};

#[tokio::test]
async fn register_rejects_email_missing_at() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "no-at-sign.example.com",
        "password": "password123",
        "name": "Nobody",
    });
    let (status, _) = app
        .request(post_json("/api/auth/register", "", &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_rejects_email_missing_dot() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "no-dot@localhost",
        "password": "password123",
        "name": "Nobody",
    });
    let (status, _) = app
        .request(post_json("/api/auth/register", "", &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_rejects_short_password() {
    let app = TestApp::new().await;
    // exactly 7 chars — boundary. The guard is `< 8`, so this must be rejected.
    let body = serde_json::json!({
        "email": "short@example.com",
        "password": "1234567",
        "name": "Short",
    });
    let (status, _) = app
        .request(post_json("/api/auth/register", "", &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_accepts_minimum_password() {
    let app = TestApp::new().await;
    // exactly 8 chars — boundary. The guard is `< 8`, so 8 must be accepted.
    let body = serde_json::json!({
        "email": "ok@example.com",
        "password": "12345678",
        "name": "Okay",
    });
    let (status, _) = app
        .request(post_json("/api/auth/register", "", &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
}

#[tokio::test]
async fn register_rejects_empty_name() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "noname@example.com",
        "password": "12345678",
        "name": "   ",
    });
    let (status, _) = app
        .request(post_json("/api/auth/register", "", &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_duplicate_email_conflict() {
    let app = TestApp::new().await;
    // staff@test.com already exists (seeded by TestApp::new).
    let body = serde_json::json!({
        "email": "staff@test.com",
        "password": "anotherpw",
        "name": "Impostor",
    });
    let (status, _) = app
        .request(post_json("/api/auth/register", "", &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn login_success_returns_token_and_user() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "admin@test.com",
        "password": "password",
    });
    let (status, resp) = app.request(post_json("/api/auth/login", "", &body)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(resp["token"].as_str().unwrap().len() > 10);
    assert_eq!(resp["user"]["role"].as_str().unwrap(), "admin");
}

#[tokio::test]
async fn login_wrong_password_unauthorized() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "admin@test.com",
        "password": "not-the-password",
    });
    let (status, _) = app.request(post_json("/api/auth/login", "", &body)).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_unknown_email_unauthorized() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "nobody@example.com",
        "password": "password",
    });
    let (status, _) = app.request(post_json("/api/auth/login", "", &body)).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}
