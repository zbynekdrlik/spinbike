mod helpers;
use axum::http::StatusCode;
use helpers::{TestApp, get};

#[tokio::test]
async fn upcoming_classes_staff_only() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(get(
            &format!(
                "/api/users/{}/upcoming-classes?days=14",
                app.customer_card_id
            ),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn upcoming_classes_returns_states() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let (status, resp) = app
        .request(get(
            &format!("/api/users/{card_id}/upcoming-classes?days=14"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(!arr.is_empty());
    let first = &arr[0];
    assert!(first["state"].is_string());
    // capacity is 19 from V6 seed
    assert_eq!(first["capacity"].as_i64().unwrap(), 19);
}

#[tokio::test]
async fn upcoming_classes_state_flips_free_to_booked_after_booking() {
    // Asserts the state-selection logic (not just the shape): before booking
    // the next Monday is `free`; after booking via /api/bookings it becomes
    // `booked`. Kills mutations that hard-code the state or drop the
    // my_row branch entirely.
    use chrono::{Datelike, Duration, Local};

    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let tid: i64 =
        sqlx::query_scalar("SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let today = Local::now().date_naive();
    let m = (7 - today.weekday().num_days_from_monday() as i64) % 7;
    let next_mon = today + Duration::days(if m == 0 { 7 } else { m });
    let date = next_mon.to_string();

    let uri = format!("/api/users/{card_id}/upcoming-classes?days=14");
    let (_, resp) = app.request(get(&uri, &app.staff_token)).await;
    let before = resp
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["date"].as_str() == Some(&date) && r["start_time"].as_str() == Some("18:00"))
        .expect("next Monday 18:00 should appear");
    assert_eq!(before["state"].as_str(), Some("free"));

    // Book the class via the real route (exercises create_booking).
    let body = serde_json::json!({ "template_id": tid, "date": date, "user_id": app.customer_id });
    let (status, _) = app
        .request(helpers::post_json("/api/bookings", &app.staff_token, &body))
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, resp) = app.request(get(&uri, &app.staff_token)).await;
    let after = resp
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["date"].as_str() == Some(&date) && r["start_time"].as_str() == Some("18:00"))
        .expect("next Monday 18:00 should still appear");
    assert_eq!(after["state"].as_str(), Some("booked"));
    assert!(after["booking_id"].is_i64());
}

#[tokio::test]
async fn upcoming_classes_range_extends_forward() {
    // Kills upcoming_classes.rs `today + Duration::days(days)` -> `-` mutation.
    // With the mutant, the window collapses to `[today, today - days]` which
    // the DB layer clamps to span 0, so at most one occurrence (today's) is
    // returned. A 7-day window must cover all four Mon/Tue/Wed/Thu templates.
    let app = TestApp::new().await;
    let (status, resp) = app
        .request(get(
            &format!(
                "/api/users/{}/upcoming-classes?days=7",
                app.customer_card_id
            ),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(
        arr.len() >= 4,
        "expected >=4 occurrences in a 7-day window (all four weekday templates), got {}: {arr:?}",
        arr.len()
    );
}
