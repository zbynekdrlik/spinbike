//! #382 — spinning booking rules through the real router: the customer
//! cancel deadline (R2), staff cancel with refund (R3) and no past bookings
//! (R4). Boundary-exact time-of-day cases (08:59:59 vs 09:00) live in the
//! pure `booking_rules` unit tests; these HTTP tests use dates whose verdict
//! does not depend on the real time of day (yesterday / a week ahead).

mod helpers;

use axum::http::StatusCode;
use chrono::{Datelike, Duration, NaiveDate};
use helpers::{TestApp, delete, get, post_json, put_json};
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

// ── R5: customer-managed recurring slots ───────────────────────────────────

const MONDAY_TEMPLATE_SQL: &str =
    "SELECT id FROM class_templates WHERE weekday = 0 AND start_time = '18:00'";

async fn active_persistent_bookings(app: &TestApp, user_id: i64) -> Vec<(i64, String)> {
    sqlx::query_as(
        "SELECT id, date FROM bookings \
         WHERE user_id = ? AND source = 'persistent' AND cancelled_at IS NULL ORDER BY date",
    )
    .bind(user_id)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

async fn monday_template(app: &TestApp) -> i64 {
    sqlx::query_scalar(MONDAY_TEMPLATE_SQL)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// #382 R5: the customer subscribes to a weekly slot themselves; the
/// response lists every active template with the `subscribed` flag, and the
/// bookings are materialised immediately (they show up in /api/my/bookings
/// as recurring and cancellable).
#[tokio::test]
async fn customer_subscribes_to_a_weekly_slot_and_gets_bookings_at_once() {
    let app = TestApp::new().await;
    let mon = monday_template(&app).await;

    let (status, slots) = app
        .request(get("/api/my/persistent-bookings", &app.customer_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(slots.as_array().unwrap().len(), 4);
    assert!(
        slots
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["subscribed"] == false)
    );

    let (status, slots) = app
        .request(put_json(
            "/api/my/persistent-bookings",
            &app.customer_token,
            &serde_json::json!({ "template_ids": [mon] }),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "got {slots}");
    let monday = slots
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["template_id"] == mon)
        .unwrap();
    assert_eq!(monday["subscribed"], true);
    assert_eq!(monday["start_time"], "18:00");
    assert_eq!(monday["instructor_name"], "Stopa");

    let booked = active_persistent_bookings(&app, app.customer_id).await;
    assert!(
        booked.len() >= 2,
        "the next two Mondays at least: {booked:?}"
    );

    let (status, mine) = app
        .request(get("/api/my/bookings", &app.customer_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let tomorrow = (today() + Duration::days(1)).to_string();
    let future: Vec<&serde_json::Value> = mine
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["date"].as_str().unwrap() >= tomorrow.as_str())
        .collect();
    assert!(!future.is_empty());
    for b in future {
        assert_eq!(b["source"], "persistent");
        assert_eq!(b["cancel_open"], true, "a future date is cancellable: {b}");
    }
}

/// #382 R5: cancelling ONE date never ends the subscription, and the
/// materialiser never resurrects that date — not even when the customer
/// re-saves the same slots (which triggers a sweep).
#[tokio::test]
async fn cancelling_one_date_keeps_the_subscription_and_is_never_resurrected() {
    let app = TestApp::new().await;
    let mon = monday_template(&app).await;
    let body = serde_json::json!({ "template_ids": [mon] });
    app.request(put_json(
        "/api/my/persistent-bookings",
        &app.customer_token,
        &body,
    ))
    .await;
    let booked = active_persistent_bookings(&app, app.customer_id).await;
    let (skip_id, skip_date) = booked.last().cloned().unwrap();

    let (status, _) = app
        .request(delete(
            &format!("/api/bookings/{skip_id}"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, slots) = app
        .request(put_json(
            "/api/my/persistent-bookings",
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let subscribed = slots
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["subscribed"] == true)
        .count();
    assert_eq!(subscribed, 1, "the subscription survives a skipped date");
    let after = active_persistent_bookings(&app, app.customer_id).await;
    assert!(
        !after.iter().any(|(_, d)| *d == skip_date),
        "the skipped {skip_date} must not come back: {after:?}"
    );
}

/// #382 R5: dropping a slot ends it and cancels its future uncharged
/// bookings ('unsubscribe'); an unknown template is a 400 that changes
/// nothing.
#[tokio::test]
async fn dropping_a_slot_cancels_its_future_bookings_and_unknown_ids_are_rejected() {
    let app = TestApp::new().await;
    let mon = monday_template(&app).await;
    app.request(put_json(
        "/api/my/persistent-bookings",
        &app.customer_token,
        &serde_json::json!({ "template_ids": [mon] }),
    ))
    .await;

    let (status, body) = app
        .request(put_json(
            "/api/my/persistent-bookings",
            &app.customer_token,
            &serde_json::json!({ "template_ids": [mon, 999_999] }),
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got {body}");
    assert!(
        !active_persistent_bookings(&app, app.customer_id)
            .await
            .is_empty(),
        "a rejected PUT changes nothing"
    );

    let (status, _) = app
        .request(put_json(
            "/api/my/persistent-bookings",
            &app.customer_token,
            &serde_json::json!({ "template_ids": [] }),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let left: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, date FROM bookings \
         WHERE user_id = ? AND source = 'persistent' AND cancelled_at IS NULL AND date > ?",
    )
    .bind(app.customer_id)
    .bind(today().to_string())
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert!(
        left.is_empty(),
        "future bookings must be cancelled: {left:?}"
    );
    let kinds: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT cancel_kind FROM bookings WHERE user_id = ? AND cancelled_at IS NOT NULL",
    )
    .bind(app.customer_id)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert!(!kinds.is_empty());
    assert!(kinds.iter().all(|k| k.as_deref() == Some("unsubscribe")));
}

// ── R7: spinner flag on the customer's home ────────────────────────────────

/// #382 R7: `/api/my/balance` carries `is_spinner` — false for a gym-only
/// customer, true once they hold a recurring slot.
#[tokio::test]
async fn my_balance_reports_is_spinner() {
    let app = TestApp::new().await;
    let (status, bal) = app
        .request(get("/api/my/balance", &app.customer_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bal["is_spinner"], false);

    let mon = monday_template(&app).await;
    app.request(put_json(
        "/api/my/persistent-bookings",
        &app.customer_token,
        &serde_json::json!({ "template_ids": [mon] }),
    ))
    .await;
    let (_, bal) = app
        .request(get("/api/my/balance", &app.customer_token))
        .await;
    assert_eq!(bal["is_spinner"], true);
}

// ── R4/R2/R8 flags on the read endpoints ───────────────────────────────────

/// The schedule exposes `started` / `cancel_open` per occurrence, computed
/// server-side (no client time-zone math); the staff participant list
/// exposes `source` and `refundable`.
#[tokio::test]
async fn read_endpoints_expose_started_cancel_open_source_and_refundable() {
    let app = TestApp::new().await;
    let yesterday = today() - Duration::days(1);
    let week_ahead = today() + Duration::days(7);
    let tid_past = template_on(&app, yesterday, "07:30").await;
    let tid_future = template_on(&app, week_ahead, "07:45").await;

    let uri = format!("/api/classes?from={yesterday}&to={yesterday}");
    let (_, past) = app.request(get(&uri, "")).await;
    let past = past
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["template_id"] == tid_past)
        .unwrap()
        .clone();
    assert_eq!(past["started"], true);
    assert_eq!(past["cancel_open"], false);

    let uri = format!("/api/classes?from={week_ahead}&to={week_ahead}");
    let (_, fut) = app.request(get(&uri, "")).await;
    let fut = fut
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["template_id"] == tid_future)
        .unwrap()
        .clone();
    assert_eq!(fut["started"], false);
    assert_eq!(fut["cancel_open"], true);

    sqlx::query("UPDATE users SET credit = 10.0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let bid = seed_booking(&app, tid_past, yesterday, app.customer_id).await;
    let uri = format!("/api/classes/{tid_past}/{yesterday}/participants");
    let (_, before) = app.request(get(&uri, &app.staff_token)).await;
    assert_eq!(before[0]["booking_id"], bid);
    assert_eq!(before[0]["source"], "manual");
    assert_eq!(before[0]["refundable"], false, "not charged yet");
    spinbike_server::jobs::charger::tick(&app.pool)
        .await
        .unwrap();
    let (_, after) = app.request(get(&uri, &app.staff_token)).await;
    assert_eq!(after[0]["refundable"], true, "charged 5.00 -> refundable");
}
