//! Integration tests for /api/classes, /api/bookings, /api/my/bookings.

mod helpers;

use helpers::{TestApp, delete, get, post_json};
use spinbike_server::db::classes as db_classes;

/// Seed a basic template on weekday=0 (Monday) at 17:00 with capacity 10.
/// Returns (template_id, date_on_that_monday).
async fn seed_monday_template(app: &TestApp) -> (i64, String) {
    let id = db_classes::create_template(&app.pool, 0, "17:00", 60, None, 10)
        .await
        .unwrap();
    // Pick a known Monday so list_classes will generate an occurrence for us.
    // 2026-04-13 was a Monday (today, per the session's current date).
    (id, "2026-04-13".to_string())
}

#[tokio::test]
async fn list_classes_returns_occurrences_in_range() {
    let app = TestApp::new().await;
    let (_tid, date) = seed_monday_template(&app).await;
    let uri = format!("/api/classes?from={date}&to={date}");
    let (status, resp) = app.request(get(&uri, "")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    // At least one occurrence on the seeded Monday.
    assert!(
        !arr.is_empty(),
        "expected non-empty occurrences, got {arr:?}"
    );
    assert_eq!(arr[0]["date"].as_str().unwrap(), date);
    assert_eq!(arr[0]["capacity"].as_i64().unwrap(), 10);
}

#[tokio::test]
async fn list_classes_rejects_bad_date() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(get("/api/classes?from=nope&to=2026-04-13", ""))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn my_bookings_returns_user_bookings() {
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;
    // Create a booking for the customer user.
    let booking_body = serde_json::json!({
        "template_id": tid,
        "date": date,
    });
    let (status, _) = app
        .request(post_json(
            "/api/bookings",
            &app.customer_token,
            &booking_body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);

    // Fetch the customer's bookings.
    let (status, resp) = app
        .request(get("/api/my/bookings", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["template_id"].as_i64().unwrap(), tid);
    assert_eq!(arr[0]["user_id"].as_i64().unwrap(), app.customer_id);
}

#[tokio::test]
async fn list_participants_forbidden_for_customer() {
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;
    let uri = format!("/api/classes/{tid}/{date}/participants");
    let (status, _) = app.request(get(&uri, &app.customer_token)).await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_participants_returns_names_for_staff() {
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;
    // Book the customer into the class.
    let booking_body = serde_json::json!({ "template_id": tid, "date": date });
    let _ = app
        .request(post_json(
            "/api/bookings",
            &app.customer_token,
            &booking_body,
        ))
        .await;

    let uri = format!("/api/classes/{tid}/{date}/participants");
    let (status, resp) = app.request(get(&uri, &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["user_name"].as_str().unwrap(), "User");
    assert_eq!(arr[0]["user_email"].as_str().unwrap(), "user@test.com");
}

#[tokio::test]
async fn cancel_own_booking_returns_204() {
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;
    let booking_body = serde_json::json!({ "template_id": tid, "date": date });
    let (_, resp) = app
        .request(post_json(
            "/api/bookings",
            &app.customer_token,
            &booking_body,
        ))
        .await;
    let booking_id = resp["id"].as_i64().unwrap();

    let uri = format!("/api/bookings/{booking_id}");
    let (status, _) = app.request(delete(&uri, &app.customer_token)).await;
    // Pin the exact status; kills `Ok(Default::default())` mutant which returns 200.
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    // Booking should now be excluded from my_bookings.
    let (_, resp) = app
        .request(get("/api/my/bookings", &app.customer_token))
        .await;
    assert!(resp.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn cancel_others_booking_forbidden_for_customer() {
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;
    // Staff books for the admin via explicit user_id.
    let booking_body =
        serde_json::json!({ "template_id": tid, "date": date, "user_id": app.admin_id });
    let (_, resp) = app
        .request(post_json("/api/bookings", &app.staff_token, &booking_body))
        .await;
    let booking_id = resp["id"].as_i64().unwrap();

    // Customer tries to cancel admin's booking — should be forbidden.
    let uri = format!("/api/bookings/{booking_id}");
    let (status, _) = app.request(delete(&uri, &app.customer_token)).await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn cancel_missing_booking_is_404() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(delete("/api/bookings/999999", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn classes_routes_are_registered() {
    let app = TestApp::new().await;
    for path in [
        "/api/classes?from=2026-04-13&to=2026-04-13",
        "/api/my/bookings",
    ] {
        let (status, _) = app.request(get(path, &app.customer_token)).await;
        assert_ne!(status, axum::http::StatusCode::NOT_FOUND, "path={path}");
    }
}
