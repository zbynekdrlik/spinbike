//! Integration tests for /api/auth/* + /api/users/{id}/invite handlers.

mod helpers;

use axum::http::StatusCode;
use helpers::{TestApp, post_json};
use spinbike_server::auth::{CUSTOMER_SESSION_SECS, validate_token};
use spinbike_server::db::login_tokens::{
    self, INVITE_TTL_SECS, LOGIN_TTL_SECS, PURPOSE_INVITE, PURPOSE_LOGIN,
};

// ── login (unchanged behavior) ───────────────────────────────────────────

#[tokio::test]
async fn login_success_returns_token_and_user() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "admin@test.com",
        "password": "password",
    });
    let (status, resp) = app.request(post_json("/api/auth/login", "", &body)).await;
    assert_eq!(status, StatusCode::OK);
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
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_unknown_email_unauthorized() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "nobody@example.com",
        "password": "password",
    });
    let (status, _) = app.request(post_json("/api/auth/login", "", &body)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ── register removed (#108) ──────────────────────────────────────────────

/// Public self-registration is gone: the register handler no longer exists, so
/// a POST creates no account and never issues a JWT. (Unmatched `/api/*` paths
/// fall through to the SPA static fallback, which answers 200 with index.html —
/// so the meaningful proof of removal is behavioral, not a router 404.)
#[tokio::test]
async fn register_endpoint_is_removed() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "someone-new@example.com",
        "password": "12345678",
        "name": "Nope",
    });
    let (status, resp) = app
        .request(post_json("/api/auth/register", "", &body))
        .await;
    // No register handler → never a 201 Created and never a JWT in the body.
    assert_ne!(
        status,
        StatusCode::CREATED,
        "register must not create (201), got {status}"
    );
    assert!(
        resp.get("token").is_none(),
        "removed register must not issue a JWT, got {resp:?}"
    );
    // And crucially: no account was created.
    let created: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = 'someone-new@example.com'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        created, 0,
        "removed register must not create a user account"
    );
}

// ── invite (POST /api/users/{id}/invite) ─────────────────────────────────

async fn invite_token_count(app: &TestApp, user_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM login_tokens WHERE user_id = ? AND purpose = 'invite'")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn login_token_count(app: &TestApp, user_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM login_tokens WHERE user_id = ? AND purpose = 'login'")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn invite_happy_path_captures_mail_and_echoes_test_link() {
    let app = TestApp::with_mail_mode("capture").await;
    // The seeded customer (user@test.com) has an email.
    let (status, resp) = app
        .request(post_json(
            &format!("/api/users/{}/invite", app.customer_id),
            &app.admin_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "invite must succeed in capture mode"
    );
    assert_eq!(resp["sent_to"].as_str().unwrap(), "user@test.com");

    let link = resp["test_link"]
        .as_str()
        .expect("capture mode must echo test_link");
    assert!(
        link.starts_with("https://test.spinbike.local/welcome?t="),
        "test_link must be the welcome magic link, got {link}"
    );

    // Exactly one invite token row was created for the customer.
    assert_eq!(invite_token_count(&app, app.customer_id).await, 1);

    // The mail was captured (never sent) with the right recipient.
    let captured = app.mail.last_captured().expect("mail must be captured");
    assert_eq!(captured.to, "user@test.com");
    // The captured body carries the same magic link.
    assert!(
        captured.text.contains("/welcome?t=") && captured.html.contains("/welcome?t="),
        "captured mail must contain the magic link"
    );
}

#[tokio::test]
async fn invite_user_without_email_is_bad_request() {
    let app = TestApp::with_mail_mode("capture").await;
    // Seed a user with NO email.
    let no_email_id = app.seed_user("No Email", None, None, Some("CODE-NE")).await;
    let (status, _) = app
        .request(post_json(
            &format!("/api/users/{no_email_id}/invite"),
            &app.admin_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // No token was created.
    assert_eq!(invite_token_count(&app, no_email_id).await, 0);
}

#[tokio::test]
async fn invite_returns_503_when_mail_disabled() {
    // Default TestApp → mail is Disabled (no SMTP_TEST_MODE).
    let app = TestApp::new().await;
    let (status, resp) = app
        .request(post_json(
            &format!("/api/users/{}/invite", app.customer_id),
            &app.admin_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(resp["error"].as_str().unwrap(), "mail_not_configured");
}

#[tokio::test]
async fn invite_rejects_non_staff() {
    let app = TestApp::with_mail_mode("capture").await;
    let (status, _) = app
        .request(post_json(
            &format!("/api/users/{}/invite", app.customer_id),
            &app.customer_token, // a customer must not be able to invite
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(invite_token_count(&app, app.customer_id).await, 0);
}

// ── request-login-link (public, no enumeration) ──────────────────────────

#[tokio::test]
async fn request_login_link_unknown_email_is_ok_but_sends_nothing() {
    let app = TestApp::with_mail_mode("capture").await;
    let (status, resp) = app
        .request(post_json(
            "/api/auth/request-login-link",
            "",
            &serde_json::json!({"email": "nobody@nowhere.com"}),
        ))
        .await;
    // Uniform 200 — never reveals whether the email exists.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["status"].as_str().unwrap(), "ok");
    // No token, no mail.
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_tokens")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(total, 0, "unknown email must create no token");
    assert!(
        app.mail.last_captured().is_none(),
        "no mail must be captured"
    );
}

#[tokio::test]
async fn request_login_link_existing_customer_creates_token_and_captures_mail() {
    let app = TestApp::with_mail_mode("capture").await;
    let (status, resp) = app
        .request(post_json(
            "/api/auth/request-login-link",
            "",
            &serde_json::json!({"email": "user@test.com"}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["status"].as_str().unwrap(), "ok");
    assert_eq!(login_token_count(&app, app.customer_id).await, 1);
    let captured = app.mail.last_captured().expect("mail must be captured");
    assert_eq!(captured.to, "user@test.com");
}

#[tokio::test]
async fn request_login_link_blocked_customer_sends_nothing() {
    let app = TestApp::with_mail_mode("capture").await;
    let blocked_id = app
        .seed_user("Blocked One", Some("blocked@test.com"), None, None)
        .await;
    sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
        .bind(blocked_id)
        .execute(&app.pool)
        .await
        .unwrap();

    let (status, resp) = app
        .request(post_json(
            "/api/auth/request-login-link",
            "",
            &serde_json::json!({"email": "blocked@test.com"}),
        ))
        .await;
    // Still 200 (no enumeration), but nothing sent.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["status"].as_str().unwrap(), "ok");
    assert_eq!(login_token_count(&app, blocked_id).await, 0);
    assert!(app.mail.last_captured().is_none());
}

#[tokio::test]
async fn request_login_link_second_within_60s_is_throttled() {
    let app = TestApp::with_mail_mode("capture").await;
    let body = serde_json::json!({"email": "user@test.com"});

    let (s1, _) = app
        .request(post_json("/api/auth/request-login-link", "", &body))
        .await;
    assert_eq!(s1, StatusCode::OK);
    let (s2, r2) = app
        .request(post_json("/api/auth/request-login-link", "", &body))
        .await;
    // 2nd call still returns 200 (no leak) but is rate-limited, so no 2nd token.
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(r2["status"].as_str().unwrap(), "ok");
    assert_eq!(
        login_token_count(&app, app.customer_id).await,
        1,
        "second request within 60 s must be throttled — no second token"
    );
}

// ── token-login ──────────────────────────────────────────────────────────

#[tokio::test]
async fn token_login_valid_invite_token_returns_permanent_customer_session() {
    let app = TestApp::new().await;
    let raw =
        login_tokens::create_token(&app.pool, app.customer_id, PURPOSE_INVITE, INVITE_TTL_SECS)
            .await
            .unwrap();
    let (status, resp) = app
        .request(post_json(
            "/api/auth/token-login",
            "",
            &serde_json::json!({"token": raw}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let token = resp["token"].as_str().expect("JWT in response");
    assert_eq!(resp["user"]["id"].as_i64().unwrap(), app.customer_id);
    assert_eq!(resp["user"]["role"].as_str().unwrap(), "customer");

    // Customer session is permanent (~100 years).
    let claims = validate_token(helpers::JWT_SECRET, token).unwrap();
    assert_eq!(
        claims.exp - claims.iat,
        CUSTOMER_SESSION_SECS,
        "customer token-login JWT must carry the permanent (100y) expiry"
    );
}

#[tokio::test]
async fn token_login_valid_login_token_works_for_recovery() {
    let app = TestApp::new().await;
    let raw = login_tokens::create_token(&app.pool, app.customer_id, PURPOSE_LOGIN, LOGIN_TTL_SECS)
        .await
        .unwrap();
    let (status, _) = app
        .request(post_json(
            "/api/auth/token-login",
            "",
            &serde_json::json!({"token": raw}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "login-purpose token must log in too"
    );
}

#[tokio::test]
async fn token_login_expired_token_rejected() {
    let app = TestApp::new().await;
    // Negative TTL → already expired.
    let raw = login_tokens::create_token(&app.pool, app.customer_id, PURPOSE_INVITE, -10)
        .await
        .unwrap();
    let (status, _) = app
        .request(post_json(
            "/api/auth/token-login",
            "",
            &serde_json::json!({"token": raw}),
        ))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_login_reused_token_rejected() {
    let app = TestApp::new().await;
    let raw = login_tokens::create_token(&app.pool, app.customer_id, PURPOSE_LOGIN, LOGIN_TTL_SECS)
        .await
        .unwrap();
    let (s1, _) = app
        .request(post_json(
            "/api/auth/token-login",
            "",
            &serde_json::json!({"token": raw}),
        ))
        .await;
    assert_eq!(s1, StatusCode::OK);
    // Second use of the same token must fail (single-use).
    let (s2, _) = app
        .request(post_json(
            "/api/auth/token-login",
            "",
            &serde_json::json!({"token": raw}),
        ))
        .await;
    assert_eq!(s2, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_login_blocked_user_rejected() {
    let app = TestApp::new().await;
    let raw =
        login_tokens::create_token(&app.pool, app.customer_id, PURPOSE_INVITE, INVITE_TTL_SECS)
            .await
            .unwrap();
    sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, _) = app
        .request(post_json(
            "/api/auth/token-login",
            "",
            &serde_json::json!({"token": raw}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "blocked user must be rejected"
    );
}

#[tokio::test]
async fn token_login_deleted_user_rejected() {
    let app = TestApp::new().await;
    let raw = login_tokens::create_token(&app.pool, app.customer_id, PURPOSE_LOGIN, LOGIN_TTL_SECS)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET deleted_at = datetime('now') WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, _) = app
        .request(post_json(
            "/api/auth/token-login",
            "",
            &serde_json::json!({"token": raw}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "deleted user must be rejected"
    );
}
