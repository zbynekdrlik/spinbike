//! Integration tests for #76 — PATCH /api/transactions/{id}/created-at.
//! Covers happy path, time-portion preservation, 30-day window enforcement,
//! 404, 409 (voided), and 403 (non-staff).

mod helpers;

use helpers::{TestApp, delete, patch_json, post_json};
use serde_json::json;

async fn seed_charge(app: &TestApp, code: &str) -> i64 {
    let card_id = app.seed_card(code, 50.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;
    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    resp.get("transaction_id").unwrap().as_i64().unwrap()
}

#[tokio::test]
async fn patch_created_at_happy_path_preserves_time() {
    use chrono::TimeZone;
    let bratislava = chrono_tz::Europe::Bratislava;

    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-OK").await;

    // Fetch the original stored UTC value and convert to Bratislava local
    // to determine the original local time-of-day.
    let original: String = sqlx::query_scalar("SELECT created_at FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let original_utc =
        chrono::NaiveDateTime::parse_from_str(&original, "%Y-%m-%d %H:%M:%S").unwrap();
    let original_local_time = bratislava.from_utc_datetime(&original_utc).time();

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(3);
    let target_str = target.format("%Y-%m-%d").to_string();

    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target_str}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        resp.get("created_at_date").unwrap().as_str(),
        Some(target_str.as_str())
    );

    let stored: String = sqlx::query_scalar("SELECT created_at FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let stored_utc = chrono::NaiveDateTime::parse_from_str(&stored, "%Y-%m-%d %H:%M:%S").unwrap();
    let stored_local = bratislava.from_utc_datetime(&stored_utc);

    // The stored value, when converted back to Bratislava local, must show the
    // user-picked date and the same local time-of-day as the original entry.
    assert_eq!(
        stored_local.date_naive(),
        target,
        "stored UTC must round-trip to user-picked LOCAL date"
    );
    assert_eq!(
        stored_local.time(),
        original_local_time,
        "local time-of-day must be preserved across edit"
    );
}

#[tokio::test]
async fn patch_created_at_31_days_back_rejected() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-31").await;

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(31);
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("30 days"),
        "error message must mention the 30-day window"
    );
}

#[tokio::test]
async fn patch_created_at_future_date_rejected() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-FUT").await;

    let target = chrono::Local::now().date_naive() + chrono::Duration::days(1);
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("30 days"),
        "error message must mention the 30-day window"
    );
}

#[tokio::test]
async fn patch_created_at_missing_id_returns_404() {
    let app = TestApp::new().await;
    let target = chrono::Local::now().date_naive();
    let (status, _) = app
        .request(patch_json(
            "/api/transactions/9999999/created-at",
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_created_at_voided_returns_409() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-VOID").await;

    // Void the transaction first.
    let (void_status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(void_status, axum::http::StatusCode::NO_CONTENT);

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(1);
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn patch_created_at_non_staff_returns_403() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-403").await;

    let target = chrono::Local::now().date_naive();
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.customer_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn patch_created_at_exactly_30_days_back_accepted() {
    // Boundary kill: window check is `< earliest`; mutating to `<=` would
    // reject today-30d. This test asserts today-30d IS accepted.
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-30").await;

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(30);
    let target_str = target.format("%Y-%m-%d").to_string();

    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target_str}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        resp.get("created_at_date").unwrap().as_str(),
        Some(target_str.as_str())
    );
}

#[tokio::test]
async fn patch_created_at_today_accepted() {
    // Boundary kill: window check is `> today`; mutating to `>=` would
    // reject today. This test asserts today IS accepted.
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-TODAY").await;

    let target = chrono::Local::now().date_naive();
    let target_str = target.format("%Y-%m-%d").to_string();

    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target_str}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        resp.get("created_at_date").unwrap().as_str(),
        Some(target_str.as_str())
    );
}

#[tokio::test]
async fn patch_created_at_preserves_local_time_across_utc_date_boundary() {
    // Pin: a tx whose UTC date and Bratislava-local date differ by 1
    // (entry made at 23:00 UTC = 01:00 CEST / 00:00 CET next day) must be
    // backdated by LOCAL-date semantics, not by raw UTC date swap.
    //
    // Setup (relative dates keep the target inside the 30-day window at any
    // point in the future):
    //   forced_utc = (today_local - 3 days) 23:00:00
    //     → local displayed date = today_local - 2 days (either UTC+1 or UTC+2)
    //   target = today_local - 5 days  (within window; different from displayed)
    //
    // Correct stored result:
    //   local time-of-day is preserved; local date = target
    //   → stored UTC = (target - 1 day) 23:00:00  (valid for both CET and CEST)
    use chrono::TimeZone;
    let bratislava = chrono_tz::Europe::Bratislava;

    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-TZ").await;

    let today_local = chrono::Local::now().date_naive();
    let forced_utc_date = today_local - chrono::Duration::days(3);
    let forced_utc_str = format!("{} 23:00:00", forced_utc_date.format("%Y-%m-%d"));

    // Force the existing row to a UTC datetime that crosses the local-date
    // boundary: 23:00 UTC is 01:00 CEST (UTC+2) or 00:00 CET (UTC+1) the
    // NEXT calendar day in Bratislava — either way a different local date.
    sqlx::query("UPDATE transactions SET created_at = ? WHERE id = ?")
        .bind(&forced_utc_str)
        .bind(tx_id)
        .execute(&app.pool)
        .await
        .unwrap();

    // Confirm the forced UTC and local dates differ (i.e. the boundary really
    // crosses) — this is guaranteed by 23:00 UTC with any Bratislava offset
    // (UTC+1 or UTC+2), but assert it explicitly to make the test self-documenting.
    let forced_naive =
        chrono::NaiveDateTime::parse_from_str(&forced_utc_str, "%Y-%m-%d %H:%M:%S").unwrap();
    let forced_local = bratislava.from_utc_datetime(&forced_naive);
    assert_ne!(
        forced_local.date_naive(),
        forced_naive.date(),
        "precondition: forced UTC and Bratislava-local dates must differ"
    );

    // The user, looking at the dashboard, sees the row dated as the LOCAL date.
    // They want to change it to a DIFFERENT local date: today - 5 days.
    let target = today_local - chrono::Duration::days(5);
    let target_str = target.format("%Y-%m-%d").to_string();

    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target_str}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        resp.get("created_at_date").unwrap().as_str(),
        Some(target_str.as_str())
    );

    let stored: String = sqlx::query_scalar("SELECT created_at FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();

    // Round-trip: stored UTC → Bratislava-local must give the user-picked date.
    let stored_utc = chrono::NaiveDateTime::parse_from_str(&stored, "%Y-%m-%d %H:%M:%S")
        .expect("stored value must parse as UTC datetime");
    let stored_local = bratislava.from_utc_datetime(&stored_utc);
    assert_eq!(
        stored_local.date_naive(),
        target,
        "stored UTC must round-trip to user-picked LOCAL date {}",
        target_str
    );

    // Local time-of-day must be preserved across the edit.
    let original_local_time = forced_local.time();
    assert_eq!(
        stored_local.time(),
        original_local_time,
        "local time-of-day must be preserved: expected {}, got {}",
        original_local_time.format("%H:%M:%S"),
        stored_local.time().format("%H:%M:%S")
    );
}
