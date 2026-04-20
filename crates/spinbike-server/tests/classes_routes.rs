//! Integration tests for /api/classes, /api/bookings, /api/my/bookings.

mod helpers;

use helpers::{TestApp, delete, get, post_json};
use spinbike_server::db::classes as db_classes;

/// Pick a Monday at least a week in the future so /api/my/bookings
/// (which filters `date >= date('now')`) always includes our seeded booking.
fn future_monday() -> String {
    use chrono::{Datelike, Days, Utc, Weekday};
    let mut d = Utc::now().date_naive() + Days::new(7);
    while d.weekday() != Weekday::Mon {
        d = d.succ_opt().unwrap();
    }
    d.format("%Y-%m-%d").to_string()
}

/// Seed a basic Monday template at 17:00 with capacity 10.
/// Returns (template_id, date_on_a_future_monday).
async fn seed_monday_template(app: &TestApp) -> (i64, String) {
    let id = db_classes::create_template(&app.pool, 0, "17:00", 60, None, 10)
        .await
        .unwrap();
    (id, future_monday())
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
async fn list_classes_returns_persistent_source_for_customer_auto_booking() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;

    // V6 seeds a Monday 18:00 template.
    let tid: i64 =
        sqlx::query_scalar("SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    // Create a persistent subscription and run the materialiser directly.
    spinbike_server::db::persistent_bookings::create(&app.pool, card_id, tid)
        .await
        .unwrap();
    spinbike_server::jobs::materialiser::sweep(&app.pool)
        .await
        .unwrap();

    // Find the next Monday (strictly in future to avoid same-day flake).
    use chrono::{Datelike, Duration, Local};
    let today = Local::now().date_naive();
    let m = (7 - today.weekday().num_days_from_monday() as i64) % 7;
    let mon = today + Duration::days(if m == 0 { 7 } else { m });

    let uri = format!("/api/classes?from={mon}&to={mon}");
    let (status, resp) = app.request(get(&uri, &app.customer_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    let monday = arr
        .iter()
        .find(|c| c["date"].as_str() == Some(&mon.to_string()))
        .unwrap();
    assert_eq!(monday["user_booking_source"].as_str(), Some("persistent"));
    assert!(monday["user_booking_id"].is_i64());
}

#[tokio::test]
async fn post_bookings_with_card_id_resolves_booking_user_from_card() {
    // Regression: the Upcoming-Classes panel on the staff card page sends only
    // { template_id, date, card_id } (no explicit user_id). The server must
    // derive user_id from cards.user_id so the booking attaches to the
    // card-holder, not the staff caller.
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;

    let body = serde_json::json!({
        "template_id": tid,
        "date": date,
        "card_id": app.customer_card_id, // customer_card_id is owned by customer_id
    });
    let (status, _resp) = app
        .request(post_json("/api/bookings", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);

    // Verify the booking's user_id points at the card's user, not the staff
    // caller.
    let (booking_user_id, booking_card_id): (i64, Option<i64>) = sqlx::query_as(
        "SELECT user_id, card_id FROM bookings
         WHERE template_id = ? AND date = ? AND cancelled_at IS NULL",
    )
    .bind(tid)
    .bind(&date)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        booking_user_id, app.customer_id,
        "user_id must match card owner"
    );
    assert_eq!(booking_card_id, Some(app.customer_card_id));

    // And the class participants list shows the card owner's name, not the
    // staff member's.
    let uri = format!("/api/classes/{tid}/{date}/participants");
    let (status, resp) = app.request(get(&uri, &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["user_email"].as_str(), Some("user@test.com"));
}

#[tokio::test]
async fn customer_can_book_own_card_via_card_id() {
    // Kills classes.rs create_booking mutation `uid != claims.sub` -> `==`.
    // With the mutant, a customer booking their own card would be rejected
    // with 403 because `uid == claims.sub && !can_book_for_others()` holds.
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;

    let body = serde_json::json!({
        "template_id": tid,
        "date": date,
        "card_id": app.customer_card_id,
    });
    let (status, _) = app
        .request(post_json("/api/bookings", &app.customer_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
}

#[tokio::test]
async fn post_bookings_with_card_id_for_unlinked_card_fails() {
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;

    // Seed a card with NO user_id.
    let orphan_card_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, user_id, credit) VALUES ('ORPH', NULL, 0) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let body = serde_json::json!({
        "template_id": tid,
        "date": date,
        "card_id": orphan_card_id,
    });
    let (status, resp) = app
        .request(post_json("/api/bookings", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp["error"]
            .as_str()
            .map(|s| s.to_lowercase().contains("linked user"))
            .unwrap_or(false),
        "expected 'no linked user' error, got {resp:?}"
    );
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
