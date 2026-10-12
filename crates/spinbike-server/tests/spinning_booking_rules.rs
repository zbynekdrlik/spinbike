//! #382 — spinning booking rules through the real router: the customer
//! cancel deadline (R2), staff cancel with refund (R3) and no past bookings
//! (R4). Boundary-exact time-of-day cases (08:59:59 vs 09:00) live in the
//! pure `booking_rules` unit tests; these HTTP tests use dates whose verdict
//! does not depend on the real time of day (yesterday / a week ahead).

mod helpers;

use axum::http::StatusCode;
use chrono::{Datelike, Duration, NaiveDate};
use helpers::{TestApp, delete, post_json};
use spinbike_server::db::classes as db_classes;

fn today() -> NaiveDate {
    spinbike_server::util::today_bratislava()
}

/// A fresh template on the weekday of `date` (so `date` is a real
/// occurrence of it), capacity 10.
async fn template_on(app: &TestApp, date: NaiveDate, start: &str) -> i64 {
    let weekday = date.weekday().num_days_from_monday() as i64;
    db_classes::create_template(&app.pool, weekday, start, 60, None, 10)
        .await
        .unwrap()
}

/// Insert a booking straight into the DB (bypassing the route's own R4
/// guard) — the only way to get a booking for a date that is already past,
/// exactly like one booked last week that is still on the books today.
async fn seed_booking(app: &TestApp, tid: i64, date: NaiveDate, user_id: i64) -> i64 {
    db_classes::create_booking(&app.pool, tid, &date.to_string(), user_id, None, "manual")
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

async fn is_active(app: &TestApp, booking_id: i64) -> bool {
    let cancelled: Option<String> =
        sqlx::query_scalar("SELECT cancelled_at FROM bookings WHERE id = ?")
            .bind(booking_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    cancelled.is_none()
}

// ── R2: customer cancel deadline ───────────────────────────────────────────

/// RED (#382 R2): the customer's own booking for a class day whose 09:00
/// deadline has passed (yesterday) can no longer be cancelled by the
/// customer — 409 `cancel_deadline_passed`, booking still active. Before the
/// fix `DELETE /api/bookings/{id}` had no deadline at all.
#[tokio::test]
async fn customer_cannot_cancel_after_the_0900_deadline() {
    let app = TestApp::new().await;
    let yesterday = today() - Duration::days(1);
    let tid = template_on(&app, yesterday, "18:00").await;
    let bid = seed_booking(&app, tid, yesterday, app.customer_id).await;

    let (status, body) = app
        .request(delete(&format!("/api/bookings/{bid}"), &app.customer_token))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "got {body}");
    assert_eq!(body["error_code"], "cancel_deadline_passed");
    assert!(is_active(&app, bid).await, "the booking must stay active");
}

/// R2: a booking a week ahead is cancellable by the customer.
#[tokio::test]
async fn customer_can_cancel_a_booking_a_week_ahead() {
    let app = TestApp::new().await;
    let week_ahead = today() + Duration::days(7);
    let tid = template_on(&app, week_ahead, "18:00").await;
    let bid = seed_booking(&app, tid, week_ahead, app.customer_id).await;

    let (status, body) = app
        .request(delete(&format!("/api/bookings/{bid}"), &app.customer_token))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "got {body}");
    assert!(!is_active(&app, bid).await);
}

// ── R3: staff cancel with refund ───────────────────────────────────────────

/// RED (#382 R3): staff may cancel a booking that was already charged (past
/// the deadline); the charge is refunded as a SEPARATE, linked ledger row
/// (positive amount, staff id recorded) and credited back to the customer.
/// Before the fix the cancel never refunded anything.
#[tokio::test]
async fn staff_cancel_of_a_charged_booking_refunds_it_as_a_linked_ledger_row() {
    let app = TestApp::new().await;
    sqlx::query("UPDATE users SET credit = 10.0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let yesterday = today() - Duration::days(1);
    let tid = template_on(&app, yesterday, "18:00").await;
    let bid = seed_booking(&app, tid, yesterday, app.customer_id).await;

    // The binding charge (yesterday's 09:00 cutoff has long passed).
    let charged = spinbike_server::jobs::charger::tick(&app.pool)
        .await
        .unwrap();
    assert_eq!(charged, 1);
    assert_eq!(credit_of(&app, app.customer_id).await, 5.0);

    let (status, body) = app
        .request(delete(&format!("/api/bookings/{bid}"), &app.staff_token))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "got {body}");
    assert!(!is_active(&app, bid).await);
    assert_eq!(
        credit_of(&app, app.customer_id).await,
        10.0,
        "the 5.00 charge must be credited back"
    );

    let (refund_id, charge_id): (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT refund_transaction_id, charge_transaction_id FROM bookings WHERE id = ?",
    )
    .bind(bid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let refund_id = refund_id.expect("the booking must link its refund row");
    assert_ne!(Some(refund_id), charge_id, "a separate ledger row");
    let (amount, staff_id, action, user_id): (f64, Option<i64>, String, i64) =
        sqlx::query_as("SELECT amount, staff_id, action, user_id FROM transactions WHERE id = ?")
            .bind(refund_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(amount, 5.0, "positive refund of exactly the charge");
    assert_eq!(
        staff_id,
        Some(app.staff_id),
        "the cancelling staff is recorded"
    );
    assert_eq!(action, "storno");
    assert_eq!(user_id, app.customer_id);
}

/// R3: a pass holder's 0 € visit needs no refund row.
#[tokio::test]
async fn staff_cancel_of_a_pass_visit_writes_no_refund_row() {
    let app = TestApp::new().await;
    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until)
         VALUES (?, ?, -35.0, 'charge', ?)",
    )
    .bind(app.customer_id)
    .bind(pass_svc)
    .bind(today() + Duration::days(20))
    .execute(&app.pool)
    .await
    .unwrap();
    let yesterday = today() - Duration::days(1);
    let tid = template_on(&app, yesterday, "18:00").await;
    let bid = seed_booking(&app, tid, yesterday, app.customer_id).await;
    spinbike_server::jobs::charger::tick(&app.pool)
        .await
        .unwrap();
    let rows_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE user_id = ?")
            .bind(app.customer_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();

    let (status, _) = app
        .request(delete(&format!("/api/bookings/{bid}"), &app.staff_token))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let rows_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE user_id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(rows_after, rows_before, "0 EUR visit -> no refund row");
}

// ── R4: no past bookings ───────────────────────────────────────────────────

/// RED (#382 R4): booking a class dated yesterday is refused with 409
/// `class_already_started` — for staff too. Before the fix any past date
/// was accepted (and then immediately charged).
#[tokio::test]
async fn booking_a_past_date_is_refused() {
    let app = TestApp::new().await;
    let yesterday = today() - Duration::days(1);
    let tid = template_on(&app, yesterday, "18:00").await;

    for token in [&app.customer_token, &app.staff_token] {
        let body = serde_json::json!({
            "template_id": tid,
            "date": yesterday.to_string(),
            "user_id": app.customer_id,
        });
        let (status, resp) = app.request(post_json("/api/bookings", token, &body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "got {resp}");
        assert_eq!(resp["error_code"], "class_already_started");
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bookings WHERE template_id = ?")
        .bind(tid)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

/// RED (#382 R4): today's class that already started (a 00:00 start is
/// always in the past by the time the test runs) is refused too.
#[tokio::test]
async fn booking_todays_already_started_class_is_refused() {
    let app = TestApp::new().await;
    let tid = template_on(&app, today(), "00:00").await;
    let body = serde_json::json!({ "template_id": tid, "date": today().to_string() });
    let (status, resp) = app
        .request(post_json("/api/bookings", &app.customer_token, &body))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "got {resp}");
    assert_eq!(resp["error_code"], "class_already_started");
}
