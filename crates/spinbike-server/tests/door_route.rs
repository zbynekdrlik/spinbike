//! Integration tests for POST /api/door/open.
//!
//! Each test uses `TestApp::with_door_mode("success" | "offline")` to control
//! the in-process EWELINK stub. The stub returns Ok(())/DeviceOffline after
//! ~100 ms, so tests run without touching the real eWeLink cloud.

mod helpers;

use helpers::{TestApp, get, post_json};

/// Enable self-service door entry on the seeded customer.
async fn enable_self_entry(app: &TestApp) {
    sqlx::query("UPDATE users SET allow_self_entry = 1 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
}

/// Staff with allow_self_entry=true can open the door — verifies the door
/// route is role-agnostic per the original prompt ("each user which is
/// allowed by some user configuration could come and open door").
#[tokio::test]
async fn staff_with_allow_self_entry_can_open_door() {
    let app = TestApp::with_door_mode("success").await;
    sqlx::query("UPDATE users SET allow_self_entry = 1 WHERE id = ?")
        .bind(app.staff_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.staff_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["status"], "opened");
    // Staff is never charged, regardless of pass status.
    assert_eq!(body["charged"], false);
    // The row was a 'visit' (no charge).
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND action = 'visit' AND amount = 0 AND note = 'door: 1st'",
    )
    .bind(app.staff_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

/// Admin with allow_self_entry=true can also open the door — same path,
/// different role.
#[tokio::test]
async fn admin_with_allow_self_entry_can_open_door() {
    let app = TestApp::with_door_mode("success").await;
    sqlx::query("UPDATE users SET allow_self_entry = 1 WHERE id = ?")
        .bind(app.admin_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.admin_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["charged"], false);
}

/// Staff WITHOUT allow_self_entry STILL opens the door — admin/staff
/// bypass the per-user opt-in toggle. (Customers still need the flag.)
#[tokio::test]
async fn staff_without_allow_self_entry_still_opens() {
    let app = TestApp::with_door_mode("success").await;
    // allow_self_entry defaults to 0 — but staff/admin role bypasses the gate.
    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.staff_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["status"], "opened");
    assert_eq!(body["charged"], false);
}

/// Admin WITHOUT allow_self_entry also opens the door — same role bypass.
#[tokio::test]
async fn admin_without_allow_self_entry_still_opens() {
    let app = TestApp::with_door_mode("success").await;
    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.admin_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["status"], "opened");
    assert_eq!(body["charged"], false);
}

#[tokio::test]
async fn forbidden_when_allow_self_entry_false() {
    let app = TestApp::with_door_mode("success").await;
    // allow_self_entry defaults to 0 in V16 → still 0 here.
    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["reason"], "not_allowed");
}

#[tokio::test]
async fn first_of_day_with_pass_writes_visit_row() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;

    // Seed an active monthly_pass for this user.
    let svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until) \
         VALUES (?, ?, -35.0, 'charge', datetime('now', '+30 days'))",
    )
    .bind(app.customer_id)
    .bind(svc_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["status"], "opened");
    assert_eq!(body["charged"], false);
    assert_eq!(body["door_count_today"], 1);

    // A 'visit' row with amount=0 and note='door: 1st' should exist for today.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND action = 'visit' AND amount = 0 AND note = 'door: 1st' \
           AND date(created_at, 'localtime') = date('now', 'localtime')",
    )
    .bind(app.customer_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn first_of_day_no_pass_writes_charge_row_and_deducts() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;
    // Set the customer's running balance to 20.
    sqlx::query("UPDATE users SET credit = 20.0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();

    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["status"], "opened");
    assert_eq!(body["charged"], true);
    assert_eq!(body["door_count_today"], 1);

    // V16 retags Fitness → kind='single_entry' with default_price=5.0; new_credit = 20 - 5 = 15.
    let final_credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        (final_credit - 15.0).abs() < 0.01,
        "expected credit ~= 15.0, got {final_credit}"
    );

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND action = 'charge' AND amount = -5.0 AND note = 'door: 1st'",
    )
    .bind(app.customer_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn second_of_day_writes_zero_amount_row() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;

    // Seed a synthetic first open earlier today.
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, action, note) \
         VALUES (?, 0, 'visit', 'door: 1st')",
    )
    .bind(app.customer_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["status"], "opened");
    assert_eq!(body["door_count_today"], 2);
    assert_eq!(body["charged"], false);

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND action = 'charge' AND amount = 0 AND note = 'door: 2nd'",
    )
    .bind(app.customer_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn rate_limited_after_quick_consecutive_presses() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;
    // Active monthly pass so each successful press is a fast no-charge visit.
    let svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until) \
         VALUES (?, ?, -35.0, 'charge', datetime('now', '+30 days'))",
    )
    .bind(app.customer_id)
    .bind(svc_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // First press succeeds.
    let (status, _) = app
        .request(post_json(
            "/api/door/open",
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    // Next 5 immediate presses all hit the 10s consecutive-press cap → 429.
    for _ in 0..5 {
        let (status, body) = app
            .request(post_json(
                "/api/door/open",
                &app.customer_token,
                &serde_json::json!({}),
            ))
            .await;
        assert_eq!(status, axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["status"], "rejected");
        assert_eq!(body["reason"], "rate_limited");
    }
}

/// Locks down the DIRECTION of the `credit -= price` operation. If the
/// `-=` were mutated to `+=`, the credit after the open would jump UP by
/// the price; `/=` would yield the price's reciprocal-ish noise. Both
/// would fail the strict equality below.
#[tokio::test]
async fn first_open_without_pass_deducts_exact_price_from_credit() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;
    // Start with a known credit and read back the price the service uses.
    sqlx::query("UPDATE users SET credit = 10.0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let price: f64 = sqlx::query_scalar(
        "SELECT default_price FROM services WHERE kind = 'single_entry' AND active = 1 LIMIT 1",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let expected_after = 10.0 - price;

    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["status"], "opened");
    assert_eq!(body["charged"], true);
    // The response field must reflect the deducted (= decreased) credit.
    let returned: f64 = body["new_credit"]
        .as_f64()
        .expect("new_credit must be a number");
    assert!(
        (returned - expected_after).abs() < 0.001,
        "credit deduction direction broke: started at 10.0, price={price}, \
         expected new_credit={expected_after}, got {returned}"
    );
    // Round-trip via DB to catch a mutation that only patched the response.
    let db_credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        (db_credit - expected_after).abs() < 0.001,
        "DB credit must match response; expected {expected_after}, got {db_credit}"
    );
}

// ─── /api/door/health role gating ───────────────────────────────────────────
//
// `require_admin_or_staff` is the inline guard. Mutation L297 replaces
// its body with `Ok(())` — which would let customers in. We assert all
// three role paths through the live router.

#[tokio::test]
async fn door_health_403_for_customer() {
    let app = TestApp::with_door_mode("success").await;
    let (status, body) = app
        .request(get("/api/door/health", &app.customer_token))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::FORBIDDEN,
        "customer must NOT be able to read /api/door/health"
    );
    assert_eq!(body["error"], "Staff access required");
}

#[tokio::test]
async fn door_health_200_for_admin() {
    let app = TestApp::with_door_mode("success").await;
    let (status, body) = app.request(get("/api/door/health", &app.admin_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        body["ewelink_ws"].is_string(),
        "response should include ewelink_ws"
    );
}

#[tokio::test]
async fn door_health_200_for_staff() {
    let app = TestApp::with_door_mode("success").await;
    let (status, body) = app.request(get("/api/door/health", &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(body["ewelink_ws"].is_string());
}

#[tokio::test]
async fn hardware_failure_rolls_back_no_tx_written() {
    let app = TestApp::with_door_mode("offline").await;
    enable_self_entry(&app).await;
    let svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until) \
         VALUES (?, ?, -35.0, 'charge', datetime('now', '+30 days'))",
    )
    .bind(app.customer_id)
    .bind(svc_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app
        .request(post_json(
            "/api/door/open",
            &app.customer_token,
            &serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["reason"], "hardware_unavailable");

    // No door-tagged transaction row should have been committed.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND note LIKE 'door:%'",
    )
    .bind(app.customer_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}
