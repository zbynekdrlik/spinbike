//! #389 — a host brings guests to spinning and pays for them, through the
//! real router: `PUT /api/bookings/{id}/guests`, the seat counts in
//! `/api/classes`, `/api/my/bookings` and the staff participants payload.
//! Dates are chosen so the verdict never depends on the real time of day
//! (a week ahead = before the cutoff, yesterday = after it).

mod helpers;

use axum::http::StatusCode;
use chrono::{Datelike, Duration, NaiveDate};
use helpers::{TestApp, get, put_json};
use serde_json::json;
use spinbike_server::db::classes as db_classes;

fn today() -> NaiveDate {
    spinbike_server::util::today_bratislava()
}

async fn template_on(app: &TestApp, date: NaiveDate, capacity: i64) -> i64 {
    let weekday = date.weekday().num_days_from_monday() as i64;
    db_classes::create_template(&app.pool, weekday, "18:00", 60, None, capacity)
        .await
        .unwrap()
}

async fn seed_booking(app: &TestApp, tid: i64, date: NaiveDate, user_id: i64) -> i64 {
    db_classes::create_booking(&app.pool, tid, &date.to_string(), user_id, None, "manual")
        .await
        .unwrap()
}

async fn guests_of(app: &TestApp, bid: i64) -> i64 {
    sqlx::query_scalar("SELECT guests FROM bookings WHERE id = ?")
        .bind(bid)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn credit_of(app: &TestApp, user_id: i64) -> f64 {
    sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn participants(app: &TestApp, tid: i64, date: NaiveDate) -> serde_json::Value {
    let uri = format!("/api/classes/{tid}/{date}/participants");
    app.request(get(&uri, &app.staff_token)).await.1
}

fn guests_uri(bid: i64) -> String {
    format!("/api/bookings/{bid}/guests")
}

/// A customer adds guests a week ahead: the seats show up everywhere the
/// occupancy is read (schedule, own bookings).
#[tokio::test]
async fn customer_adds_guests_before_the_deadline_and_they_take_seats() {
    let app = TestApp::new().await;
    let day = today() + Duration::days(7);
    let tid = template_on(&app, day, 10).await;
    let bid = seed_booking(&app, tid, day, app.customer_id).await;

    let (status, body) = app
        .request(put_json(
            &guests_uri(bid),
            &app.customer_token,
            &json!({"guests": 2}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["guests"], 2);
    assert_eq!(body["charged"], 0.0);
    assert_eq!(guests_of(&app, bid).await, 2);

    let (status, classes) = app
        .request(get(
            &format!("/api/classes?from={day}&to={day}"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let occ = classes
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["template_id"] == tid)
        .expect("the occurrence is listed");
    assert_eq!(occ["booked"], 3, "host + 2 guests = 3 seats");
    assert_eq!(occ["user_booking_guests"], 2);

    let (_, mine) = app
        .request(get("/api/my/bookings", &app.customer_token))
        .await;
    let row = mine
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == bid)
        .unwrap();
    assert_eq!(row["guests"], 2);

    // Back to zero is allowed too.
    let (status, _) = app
        .request(put_json(
            &guests_uri(bid),
            &app.customer_token,
            &json!({"guests": 0}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(guests_of(&app, bid).await, 0);
}

/// After the 09:00 cutoff (yesterday's class) the customer can no longer
/// change the guests — same deadline as a cancel.
#[tokio::test]
async fn customer_cannot_change_guests_after_the_deadline() {
    let app = TestApp::new().await;
    let yesterday = today() - Duration::days(1);
    let tid = template_on(&app, yesterday, 10).await;
    let bid = seed_booking(&app, tid, yesterday, app.customer_id).await;

    let (status, body) = app
        .request(put_json(
            &guests_uri(bid),
            &app.customer_token,
            &json!({"guests": 1}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "got {body}");
    assert_eq!(body["error_code"], "guests_deadline_passed");
    assert_eq!(guests_of(&app, bid).await, 0);
}

#[tokio::test]
async fn more_than_four_guests_or_negative_is_rejected() {
    let app = TestApp::new().await;
    let day = today() + Duration::days(7);
    let tid = template_on(&app, day, 19).await;
    let bid = seed_booking(&app, tid, day, app.customer_id).await;
    for bad in [5, -1] {
        let (status, body) = app
            .request(put_json(
                &guests_uri(bid),
                &app.customer_token,
                &json!({"guests": bad}),
            ))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
        assert_eq!(body["error_code"], "too_many_guests");
    }
    assert_eq!(guests_of(&app, bid).await, 0);
}

#[tokio::test]
async fn a_customer_cannot_change_someone_elses_booking() {
    let app = TestApp::new().await;
    let day = today() + Duration::days(7);
    let tid = template_on(&app, day, 10).await;
    let bid = seed_booking(&app, tid, day, app.staff_id).await;
    let (status, body) = app
        .request(put_json(
            &guests_uri(bid),
            &app.customer_token,
            &json!({"guests": 1}),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "got {body}");
    assert_eq!(guests_of(&app, bid).await, 0);
}

/// Guests that do not fit into the free seats -> 409 `class_full`; a new
/// booking into a class full of guests is refused too.
#[tokio::test]
async fn guests_beyond_capacity_are_refused() {
    let app = TestApp::new().await;
    let day = today() + Duration::days(7);
    let tid = template_on(&app, day, 3).await;
    let bid = seed_booking(&app, tid, day, app.customer_id).await;

    let (status, body) = app
        .request(put_json(
            &guests_uri(bid),
            &app.customer_token,
            &json!({"guests": 3}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "got {body}");
    assert_eq!(body["error_code"], "class_full");

    let (status, _) = app
        .request(put_json(
            &guests_uri(bid),
            &app.customer_token,
            &json!({"guests": 2}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = app
        .request(helpers::post_json(
            "/api/bookings",
            &app.staff_token,
            &json!({"template_id": tid, "date": day.to_string(), "user_id": app.staff_id}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "got {body}");
    assert_eq!(body["error_code"], "class_full");
}

/// Staff may change the guests of an already CHARGED booking: removing a
/// guest refunds that seat, adding one charges it at once — both visible in
/// the participants payload.
#[tokio::test]
async fn staff_changes_guests_after_the_charge_with_refund_and_charge() {
    let app = TestApp::new().await;
    sqlx::query("UPDATE users SET credit = 20.0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let yesterday = today() - Duration::days(1);
    let tid = template_on(&app, yesterday, 10).await;
    let bid = seed_booking(&app, tid, yesterday, app.customer_id).await;
    sqlx::query("UPDATE bookings SET guests = 1 WHERE id = ?")
        .bind(bid)
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        spinbike_server::jobs::charger::tick(&app.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        credit_of(&app, app.customer_id).await,
        10.0,
        "host 5 + guest 5"
    );

    let p = participants(&app, tid, yesterday).await;
    assert_eq!(p[0]["guests"], 1);
    assert_eq!(p[0]["guest_charged_amount"], 5.0);

    let (status, body) = app
        .request(put_json(
            &guests_uri(bid),
            &app.staff_token,
            &json!({"guests": 0}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["refunded"], 5.0);
    assert_eq!(credit_of(&app, app.customer_id).await, 15.0);
    let p = participants(&app, tid, yesterday).await;
    assert_eq!(p[0]["guests"], 0);
    assert_eq!(p[0]["guest_charged_amount"], 0.0);

    let (status, body) = app
        .request(put_json(
            &guests_uri(bid),
            &app.staff_token,
            &json!({"guests": 2}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["charged"], 10.0);
    assert_eq!(credit_of(&app, app.customer_id).await, 5.0);
    let p = participants(&app, tid, yesterday).await;
    assert_eq!(p[0]["guest_charged_amount"], 10.0);

    // A staff cancel now refunds the host AND both guests.
    let (status, _) = app
        .request(helpers::delete(
            &format!("/api/bookings/{bid}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(credit_of(&app, app.customer_id).await, 20.0);
}
