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
    // Exactly two templates match a Monday: the test-seeded 17:00 (cap 10)
    // and the V6 seeded 18:00 (cap 19). Pinning the length + both entries
    // kills mutations that drop the weekday guard or the capacity binding.
    let app = TestApp::new().await;
    let (_tid, date) = seed_monday_template(&app).await;
    let uri = format!("/api/classes?from={date}&to={date}");
    let (status, resp) = app.request(get(&uri, "")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 2, "expected 2 Monday occurrences, got {arr:?}");
    for item in arr {
        assert_eq!(item["date"].as_str().unwrap(), date);
        assert_eq!(item["weekday"].as_i64().unwrap(), 0);
    }
    let cap_at = |time: &str| -> Option<i64> {
        arr.iter()
            .find(|c| c["start_time"].as_str() == Some(time))
            .and_then(|c| c["capacity"].as_i64())
    };
    assert_eq!(cap_at("17:00"), Some(10));
    assert_eq!(cap_at("18:00"), Some(19));
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
    // #146: the row is enriched with the class start time (joined from
    // class_templates) — seed_monday_template seeds "17:00" with no instructor.
    assert_eq!(arr[0]["start_time"].as_str().unwrap(), "17:00");
    assert!(arr[0]["instructor_name"].is_null());
}

/// #146: when the booked class HAS an instructor, `/api/my/bookings` returns
/// its name (joined via class_templates.instructor_id) — the V6 migration
/// seeds a Monday 18:00 template taught by "Stevo".
#[tokio::test]
async fn my_bookings_includes_instructor_name() {
    let app = TestApp::new().await;
    let tid: i64 =
        sqlx::query_scalar("SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let date = future_monday();

    let booking_body = serde_json::json!({ "template_id": tid, "date": date });
    let (status, _) = app
        .request(post_json(
            "/api/bookings",
            &app.customer_token,
            &booking_body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);

    let (status, resp) = app
        .request(get("/api/my/bookings", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["start_time"].as_str().unwrap(), "18:00");
    assert_eq!(arr[0]["instructor_name"].as_str().unwrap(), "Stevo");
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
    let user_id = app.customer_id;

    // V6 seeds a Monday 18:00 template.
    let tid: i64 =
        sqlx::query_scalar("SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    // Create a persistent subscription and run the materialiser directly.
    spinbike_server::db::persistent_bookings::create(&app.pool, user_id, tid)
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
async fn post_bookings_with_user_id_books_for_target_user() {
    // Staff books for a specific user via explicit user_id.
    // The booking must attach to the target user, not the staff caller.
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;

    let body = serde_json::json!({
        "template_id": tid,
        "date": date,
        "user_id": app.customer_id,
    });
    let (status, _resp) = app
        .request(post_json("/api/bookings", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);

    // Verify the booking's user_id points at the target user, not the staff caller.
    let booking_user_id: i64 = sqlx::query_scalar(
        "SELECT user_id FROM bookings
         WHERE template_id = ? AND date = ? AND cancelled_at IS NULL",
    )
    .bind(tid)
    .bind(&date)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        booking_user_id, app.customer_id,
        "user_id must match the target user"
    );

    // And the class participants list shows the target user's name.
    let uri = format!("/api/classes/{tid}/{date}/participants");
    let (status, resp) = app.request(get(&uri, &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["user_email"].as_str(), Some("user@test.com"));
}

#[tokio::test]
async fn staff_booking_for_other_user_records_created_by() {
    // Audit regression: when staff books for another user via user_id,
    // the booking row must record `created_by = staff_id`
    // so the audit trail distinguishes staff bookings from self-book.
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;

    let body = serde_json::json!({
        "template_id": tid,
        "date": date,
        "user_id": app.customer_id,
    });
    let (status, _resp) = app
        .request(post_json("/api/bookings", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);

    let created_by: Option<i64> = sqlx::query_scalar(
        "SELECT created_by FROM bookings
         WHERE template_id = ? AND date = ? AND cancelled_at IS NULL",
    )
    .bind(tid)
    .bind(&date)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        created_by,
        Some(app.staff_id),
        "staff booking for another user must stamp created_by with the staff id"
    );
}

#[tokio::test]
async fn customer_can_book_for_self_via_user_id() {
    // Kills classes.rs create_booking mutation `uid != claims.sub` -> `==`.
    // With the mutant, a customer booking via their own user_id would be rejected
    // with 403 because `uid == claims.sub && !can_book_for_others()` holds.
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;

    let body = serde_json::json!({
        "template_id": tid,
        "date": date,
        "user_id": app.customer_id,
    });
    let (status, _) = app
        .request(post_json("/api/bookings", &app.customer_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
}

#[tokio::test]
async fn post_bookings_with_nonexistent_user_id_fails() {
    // After V13 there is no cards table; booking for a nonexistent user_id
    // must fail (FK violation or 404-style error from the route).
    let app = TestApp::new().await;
    let (tid, date) = seed_monday_template(&app).await;

    let body = serde_json::json!({
        "template_id": tid,
        "date": date,
        "user_id": 999_999,
    });
    let (status, _resp) = app
        .request(post_json("/api/bookings", &app.staff_token, &body))
        .await;
    // FK failure on users(id) → 500 internal error or 422; anything but 201 is correct.
    assert_ne!(
        status,
        axum::http::StatusCode::CREATED,
        "booking for nonexistent user must not succeed"
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
