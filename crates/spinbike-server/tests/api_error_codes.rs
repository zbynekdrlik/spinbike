//! Contract test for the typed HTTP error layer (#158).
//!
//! Every API error body now carries a stable machine-readable `error_code`
//! (snake_case) ALONGSIDE the human `error` message the UI/tests already read.
//! These tests lock that additive contract end-to-end through the live router
//! for one error of each HTTP class, so the `error_code` field cannot silently
//! disappear or drift.

mod helpers;

use axum::http::StatusCode;
use helpers::{TestApp, delete, get, post_json};

#[tokio::test]
async fn forbidden_carries_staff_required_code() {
    // A customer hitting a staff-only listing endpoint. Also pins the
    // three-way "Staff access required"/"Staff only"/"Only staff can book…"
    // unification onto the single `staff_required` code (#158).
    let app = TestApp::new().await;
    let (status, body) = app.request(get("/api/users", &app.customer_token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_code"], "staff_required");
    assert_eq!(body["error"], "Staff access required");
}

#[tokio::test]
async fn unauthorized_carries_invalid_credentials_code() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "email": "nobody@test.com",
        "password": "wrong-password",
    });
    let (status, resp) = app.request(post_json("/api/auth/login", "", &body)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(resp["error_code"], "invalid_credentials");
    assert_eq!(resp["error"], "Invalid email or password");
}

#[tokio::test]
async fn bad_request_carries_generic_code_with_specific_message() {
    // Negative charge amount → 400. The code is the generic `bad_request`;
    // the human message still carries the specifics the UI/tests read.
    let app = TestApp::new().await;
    let user_id = app.seed_card("ERRC1", 100.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;
    let body = serde_json::json!({
        "user_id": user_id,
        "amount": -5.0,
        "service_id": spinning_id,
    });
    let (status, resp) = app
        .request(post_json("/api/payments/charge", &app.staff_token, &body))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp["error_code"], "bad_request");
    assert!(
        resp["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Amount"),
        "message must still carry the specifics, got: {resp}"
    );
}

#[tokio::test]
async fn create_user_with_soft_deleted_email_returns_resolvable_409_not_500() {
    // #143: the `email UNIQUE` constraint covers ALL rows (delete only sets
    // deleted_at, it keeps the email), so re-using a SOFT-DELETED user's email
    // used to slip past the (deleted-filtered) pre-check and 500 (or return a
    // generic email_or_card_conflict). It now returns a STRUCTURED, resolvable
    // 409 naming the archived account (id / name / deleted_at) so the staff UI
    // can offer restore / free-email — never a 500.
    let app = TestApp::new().await;
    let email = "dup-fallback@test.com";

    let (status, body) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({ "name": "Fallback A", "email": email }),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED, "create A must succeed: {body}");
    let id = body["id"].as_i64().expect("created user id");

    let (status, _) = app
        .request(delete(&format!("/api/users/{id}"), &app.staff_token))
        .await;
    assert_eq!(status, StatusCode::OK, "soft-delete A must succeed");

    // Same email again: the soft-deleted holder is now surfaced as a resolvable
    // conflict, not an opaque 500.
    let (status, body) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({ "name": "Fallback B", "email": email }),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "re-using a soft-deleted email must be a clean 409, got {status}: {body}"
    );
    assert_eq!(body["error_code"], "email_belongs_to_deleted_account");
    assert_eq!(body["conflict_id"].as_i64(), Some(id));
    assert_eq!(body["conflict_name"], "Fallback A");
}

// ---- #160: role-enforcing extractors (StaffUser / AdminUser) ----
//
// The inline `if !claims.role.can_*() { return Err(Forbidden(..)) }` guards were
// replaced by the `StaffUser` / `AdminUser` request extractors. These lock that
// the extraction-boundary rejection produces the SAME typed 403 body the inline
// guards did — for both role tiers.

#[tokio::test]
async fn staff_extractor_rejects_customer_with_staff_required() {
    // `/api/users/{id}/persistent-bookings` (list) is now guarded by the
    // `StaffUser` extractor, not an inline body check. A customer must still get
    // the identical 403 `staff_required` body.
    let app = TestApp::new().await;
    let (status, body) = app
        .request(get("/api/users/1/persistent-bookings", &app.customer_token))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_code"], "staff_required");
    assert_eq!(body["error"], "Staff access required");
}

#[tokio::test]
async fn admin_extractor_rejects_staff_with_admin_required() {
    // `/api/reports/day` is now guarded by the `AdminUser` extractor. A STAFF
    // token (not admin) must be rejected with the typed 403 `admin_required`
    // body — proving the admin tier is enforced at extraction, distinct from
    // the staff tier.
    let app = TestApp::new().await;
    let (status, body) = app
        .request(get("/api/reports/day?date=2026-01-01", &app.staff_token))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_code"], "admin_required");
    assert_eq!(body["error"], "Admin access required");
}
