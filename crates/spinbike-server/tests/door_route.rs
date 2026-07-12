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

/// #179 — MONEY-BUG boundary: a monthly pass whose `valid_until` is EXACTLY
/// today's date (a bare `YYYY-MM-DD` string — the exact format
/// routes/payments.rs::sell_pass writes) must be treated as STILL ACTIVE for
/// the whole of its last valid day, mirroring the T-4h charger's inclusive
/// `date(valid_until) >= date('now')` semantics.
///
/// Before the fix, door.rs hand-rolled `valid_until > datetime('now')`. SQLite
/// compares TEXT byte-wise, and the 10-char bare date `'2026-07-11'` is a
/// prefix of the 19-char `datetime('now')` (`'2026-07-11 08:00:00'`), so the
/// shorter string sorts as LESS → the predicate is FALSE on the expiry day →
/// the customer with a still-valid pass was CHARGED for a single entry (a real
/// overcharge). RED on the old code (charged=true), GREEN after routing the
/// check through the canonical `user_active_pass` view with inclusive date
/// semantics.
#[tokio::test]
async fn first_of_day_pass_expiring_today_grants_entry_without_charge() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;
    // Give the customer credit so that IF the (buggy) charge path fired, it
    // would visibly debit — making the "no charge" assertions unambiguous.
    sqlx::query("UPDATE users SET credit = 20.0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();

    // Seed a monthly pass whose valid_until is EXACTLY today at the gym
    // (Europe/Bratislava) — the SAME basis the door route now uses (#205), so
    // this can't flake near local midnight on a UTC CI runner (where SQLite's
    // `date('now')` would be a day behind the gym's date).
    let svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until) \
         VALUES (?, ?, -35.0, 'charge', ?)",
    )
    .bind(app.customer_id)
    .bind(svc_id)
    .bind(spinbike_server::util::today_bratislava())
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
    // The pass covers the entry ON its last valid day — NO charge.
    assert_eq!(
        body["charged"], false,
        "a pass expiring TODAY must cover the entry (last day inclusive), not charge"
    );

    // Credit must be untouched (no single-entry debit happened).
    let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        (credit - 20.0).abs() < 0.001,
        "credit must be unchanged (pass covers entry), got {credit}"
    );

    // A zero-amount 'visit' row (not a 'charge') should be written for today.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND action = 'visit' AND amount = 0 AND note = 'door: 1st' \
           AND date(created_at, 'localtime') = date('now', 'localtime')",
    )
    .bind(app.customer_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        n, 1,
        "expected a zero-amount visit row for the pass-covered entry"
    );
}

/// #179 — the OTHER side of the boundary: a pass that expired YESTERDAY
/// (`valid_until = date('now','-1 day')`) is genuinely over, so the inclusive
/// `>=` fix must STILL exclude it and charge the single entry. Guards against
/// the fix over-correcting into "any past pass keeps counting". This passes
/// both before and after the fix — it is the permissiveness guard.
#[tokio::test]
async fn first_of_day_pass_expired_yesterday_is_charged() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;
    sqlx::query("UPDATE users SET credit = 20.0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();

    let svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until) \
         VALUES (?, ?, -35.0, 'charge', date('now', '-1 day'))",
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
    assert_eq!(
        body["charged"], true,
        "a pass that expired yesterday must NOT cover today's entry — charge applies"
    );
    // Single-entry price is 5.0 (V16 retag); credit 20 → 15.
    let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        (credit - 15.0).abs() < 0.01,
        "expected credit ~= 15.0 after single-entry charge, got {credit}"
    );
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

// ─── #106 — blocked users must never open the door ─────────────────────────
//
// The precondition SELECT historically loaded role/allow_self_entry/credit
// but never `blocked`, so a blocked customer with allow_self_entry=1 (or a
// blocked admin/staff, who bypass the allow_self_entry gate entirely) could
// still actuate the relay and get billed. Reject blocked users BEFORE the
// relay is pressed and BEFORE any transaction row is written, for every role.

#[tokio::test]
async fn blocked_customer_with_allow_self_entry_is_rejected() {
    let app = TestApp::with_door_mode("success").await;
    enable_self_entry(&app).await;
    sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
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
    assert_eq!(
        status,
        axum::http::StatusCode::FORBIDDEN,
        "blocked customer must be rejected even with allow_self_entry=1"
    );
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["reason"], "blocked");

    // No door-tagged transaction row must exist for this user.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND note LIKE 'door:%'",
    )
    .bind(app.customer_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "blocked user must not get a door transaction row");

    // The relay must never have been pressed — last_ack_ms_ago stays null.
    let (_, health) = app.request(get("/api/door/health", &app.admin_token)).await;
    assert!(
        health["last_ack_ms_ago"].is_null(),
        "relay must not be pressed for a rejected blocked user, got {health:?}"
    );
}

#[tokio::test]
async fn blocked_admin_is_rejected_despite_role_bypass() {
    let app = TestApp::with_door_mode("success").await;
    sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
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
    assert_eq!(
        status,
        axum::http::StatusCode::FORBIDDEN,
        "a blocked admin must be rejected — blocked-means-blocked regardless \
         of the allow_self_entry role bypass"
    );
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["reason"], "blocked");

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND note LIKE 'door:%'",
    )
    .bind(app.admin_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "blocked admin must not get a door transaction row");

    // The relay must never have been pressed — last_ack_ms_ago stays null.
    let (_, health) = app.request(get("/api/door/health", &app.admin_token)).await;
    assert!(
        health["last_ack_ms_ago"].is_null(),
        "relay must not be pressed for a rejected blocked admin, got {health:?}"
    );
}

#[tokio::test]
async fn blocked_staff_is_rejected_despite_role_bypass() {
    let app = TestApp::with_door_mode("success").await;
    sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
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
    assert_eq!(
        status,
        axum::http::StatusCode::FORBIDDEN,
        "a blocked staff account must be rejected — blocked-means-blocked \
         regardless of the allow_self_entry role bypass"
    );
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["reason"], "blocked");

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND note LIKE 'door:%'",
    )
    .bind(app.staff_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "blocked staff must not get a door transaction row");

    // The relay must never have been pressed — last_ack_ms_ago stays null.
    let (_, health) = app.request(get("/api/door/health", &app.admin_token)).await;
    assert!(
        health["last_ack_ms_ago"].is_null(),
        "relay must not be pressed for a rejected blocked staff account, got {health:?}"
    );
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
