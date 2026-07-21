//! Integration tests for /api/payments/sell-pass, /api/payments/log-visit,
//! and the `pass` field on CardResponse.

mod helpers;

use helpers::{TestApp, get, post_json};
use serde_json::json;

async fn set_blocked(app: &TestApp, user_id: i64) {
    sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
        .bind(user_id)
        .execute(&app.pool)
        .await
        .unwrap();
}

async fn user_credit(app: &TestApp, user_id: i64) -> f64 {
    sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn service_id(app: &TestApp, name: &str) -> i64 {
    // After V8, services has name_sk + name_en (no `name`). Callers pass
    // English-readable names ("Spinning", "Monthly pass"), so name_en is
    // the right column to look up.
    sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?")
        .bind(name)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn sell_pass_debits_credit_and_records_valid_until() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("SELL-PASS-1", 50.0, None, None, None, None)
        .await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-05-17" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "body = {resp}");
    assert_eq!(resp["new_credit"].as_f64().unwrap(), 15.0);
    assert_eq!(resp["valid_until"], "2030-05-17");
    // Kills mutation where (valid_until - today) is flipped to (today - valid_until).
    // 2030-05-17 must be in the future at test-run time, so days_remaining is strictly positive.
    let days = resp["days_remaining"].as_i64().unwrap();
    assert!(
        days > 0,
        "days_remaining must be strictly positive for a future valid_until, got {days}"
    );

    assert_eq!(user_credit(&app, user_id).await, 15.0);

    let tx_id = resp["transaction_id"].as_i64().unwrap();
    let (amount, valid_until, service_id): (f64, Option<chrono::NaiveDate>, i64) =
        sqlx::query_as("SELECT amount, valid_until, service_id FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        amount, -35.0,
        "monthly pass amount stored as negative (ledger convention)"
    );
    assert_eq!(
        valid_until,
        Some(chrono::NaiveDate::from_ymd_opt(2030, 5, 17).unwrap())
    );
    let pass_svc_id: i64 =
        sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(service_id, pass_svc_id);
}

#[tokio::test]
async fn sell_pass_rejects_past_valid_until() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("SELL-PAST", 100.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2020-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sell_pass_rejects_negative_price() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("SELL-NEG", 100.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": -1.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sell_pass_rejects_blocked_user() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("SELL-BLOCKED", 100.0, None, None, None, None)
        .await;
    set_blocked(&app, user_id).await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn log_visit_writes_zero_amount_when_pass_active() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("VISIT-1", 50.0, None, None, None, None).await;

    // Sell a pass first.
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let spinning_id = service_id(&app, "Spinning").await;
    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let tx_id = resp["transaction_id"].as_i64().unwrap();
    let (amount, action, service_id_val): (f64, String, i64) =
        sqlx::query_as("SELECT amount, action, service_id FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(amount, 0.0);
    assert_eq!(action, "visit");
    assert_eq!(service_id_val, spinning_id);

    // Credit unchanged (50 - 35 = 15)
    assert_eq!(user_credit(&app, user_id).await, 15.0);
}

#[tokio::test]
async fn log_visit_rejects_user_without_active_pass() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("VISIT-2", 50.0, None, None, None, None).await;
    let spinning_id = service_id(&app, "Spinning").await;

    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn log_visit_rejects_user_with_expired_pass() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("VISIT-3", 50.0, None, None, None, None).await;

    // Insert an expired pass transaction directly via SQL.
    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, -35.0, 'charge', ?, datetime('now'))",
    )
    .bind(user_id)
    .bind(pass_svc)
    .bind(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap())
    .execute(&app.pool)
    .await
    .unwrap();

    let spinning_id = service_id(&app, "Spinning").await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn log_visit_rejects_unknown_service_id() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("VISIT-SVC", 50.0, None, None, None, None)
        .await;

    // Sell an active pass so the pass check passes — we want to isolate the service_id check.
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": 99999 }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn user_response_includes_pass_field_when_pass_active() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("PASS-RESP-1", 50.0, None, None, None, None)
        .await;
    let (status, sell_resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let expected_tx_id = sell_resp["transaction_id"].as_i64().unwrap();

    let (status, body) = app
        .request(get("/api/users/lookup/PASS-RESP-1", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["pass"]["valid_until"], "2030-01-01");
    let days = body["pass"]["days_remaining"].as_i64().unwrap();
    assert!(
        days > 0,
        "days_remaining must be positive for an active pass"
    );
    // Response must carry the transaction_id so the UI can PATCH the
    // correct row when staff edits the pass end date.
    assert_eq!(
        body["pass"]["transaction_id"].as_i64().unwrap(),
        expected_tx_id,
        "pass.transaction_id must match the sell-pass transaction id"
    );
}

/// When a user has multiple pass sales, the response must surface the id of
/// the LATEST (max valid_until) one — that's the one the UI pencil icon will PATCH.
#[tokio::test]
async fn user_response_pass_has_transaction_id_of_latest_pass_sale() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("PASS-LATEST", 100.0, None, None, None, None)
        .await;

    // First pass: earlier valid_until.
    let (status, first) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-02-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let first_tx_id = first["transaction_id"].as_i64().unwrap();

    // Second pass: later valid_until — this is the one the UI must see.
    let (status, second) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-06-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let second_tx_id = second["transaction_id"].as_i64().unwrap();
    assert_ne!(first_tx_id, second_tx_id);

    let (status, body) = app
        .request(get("/api/users/lookup/PASS-LATEST", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["pass"]["valid_until"], "2030-06-01");
    assert_eq!(
        body["pass"]["transaction_id"].as_i64().unwrap(),
        second_tx_id,
        "pass.transaction_id must be from the LATEST pass sale, not the first"
    );
}

#[tokio::test]
async fn user_response_pass_field_is_null_when_no_pass() {
    let app = TestApp::new().await;
    app.seed_card("NO-PASS-RESP", 10.0, None, None, None, None)
        .await;
    let (status, body) = app
        .request(get("/api/users/lookup/NO-PASS-RESP", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        body["pass"].is_null(),
        "pass must be null when user has no pass"
    );
}

#[tokio::test]
async fn user_response_includes_expired_pass_with_negative_days_remaining() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("EXPIRED-RESP", 50.0, None, None, None, None)
        .await;

    // Insert an expired pass directly — cannot go through sell-pass API (validates future date).
    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, -35.0, 'charge', ?, datetime('now'))",
    )
    .bind(user_id)
    .bind(pass_svc)
    .bind(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap())
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app
        .request(get("/api/users/lookup/EXPIRED-RESP", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        body["pass"]["valid_until"], "2020-01-01",
        "expired pass must still be returned in response (state C on dashboard)"
    );
    let days = body["pass"]["days_remaining"].as_i64().unwrap();
    assert!(
        days < 0,
        "days_remaining must be NEGATIVE for expired pass, got {days}"
    );
}

#[tokio::test]
async fn sell_pass_rejects_today_as_valid_until() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("SELL-TODAY", 100.0, None, None, None, None)
        .await;
    // Gym-local (Europe/Bratislava) "today" — the basis sell_pass now validates
    // against (#205); selling a pass expiring "today" must be rejected.
    let today = spinbike_server::util::today_bratislava();
    let today_str = today.format("%Y-%m-%d").to_string();
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": today_str }),
        ))
        .await;
    // today is NOT in the future — must reject (kills `<=` → `<` mutant).
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn log_visit_accepts_pass_with_valid_until_today() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("VISIT-TODAY", 50.0, None, None, None, None)
        .await;

    // Insert a pass expiring TODAY directly — API won't allow today via sell-pass.
    // Use the gym-local (Europe/Bratislava) "today" that log_visit now keys off
    // (#205), so this can't flake near local midnight on a UTC CI runner.
    let today = spinbike_server::util::today_bratislava();
    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, -35.0, 'charge', ?, datetime('now'))",
    )
    .bind(user_id)
    .bind(pass_svc)
    .bind(today)
    .execute(&app.pool)
    .await
    .unwrap();

    let spinning_id = service_id(&app, "Spinning").await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": spinning_id }),
        ))
        .await;
    // Pass expiring today is still active — must accept (kills `>=` → `>` mutant).
    assert_eq!(status, axum::http::StatusCode::OK);
}

// ─── #234: warn on same-day duplicate visit ───────────────────────────────

#[tokio::test]
async fn log_visit_warns_on_duplicate_same_day_manual_visit() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DUP-VISIT-1", 50.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let fitness_id = service_id(&app, "Fitness").await;

    // First log-visit: succeeds, no prior entry today.
    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": fitness_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    // Second log-visit, same user, same day, no force: must 409 with the
    // structured already_visited_today body — not a second visit row.
    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": fitness_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert_eq!(resp["error_code"], "already_visited_today");
    assert_eq!(resp["source"], "manual");
    assert!(
        resp["last_entry_at"].as_str().is_some(),
        "conflict body must carry last_entry_at"
    );

    let visit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions WHERE user_id = ? AND action = 'visit'",
    )
    .bind(user_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(visit_count, 1, "the rejected duplicate must not be logged");
}

#[tokio::test]
async fn log_visit_force_true_logs_duplicate_anyway() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DUP-VISIT-2", 50.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let fitness_id = service_id(&app, "Fitness").await;
    let spinning_id = service_id(&app, "Spinning").await;

    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": fitness_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    // A legitimate second visit (e.g. morning Fitness + evening Spinning) —
    // resubmit with force: true and it must go through, not 409 again.
    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": spinning_id, "force": true }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let visit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions WHERE user_id = ? AND action = 'visit'",
    )
    .bind(user_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(visit_count, 2, "force:true must log the second visit");
}

#[tokio::test]
async fn log_visit_duplicate_source_is_door_when_note_prefixed() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DUP-VISIT-3", 50.0, None, None, None, None)
        .await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    // Fitness IS the kind='single_entry' row after migration V16 — a door
    // press today lands on this exact service_id with a "door: 1st" note.
    let fitness_id = service_id(&app, "Fitness").await;
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, note, created_at)
         VALUES (?, ?, 0.0, 'visit', 'door: 1st', datetime('now'))",
    )
    .bind(user_id)
    .bind(fitness_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "user_id": user_id, "service_id": fitness_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert_eq!(resp["error_code"], "already_visited_today");
    assert_eq!(resp["source"], "door");
}

#[tokio::test]
async fn user_response_pass_field_when_valid_until_equals_today() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("BOUNDARY-TODAY", 50.0, None, None, None, None)
        .await;

    // Gym-local (Europe/Bratislava) "today" — the basis the staff-list
    // days_remaining now uses (#205); avoids a UTC-vs-local midnight flake.
    let today = spinbike_server::util::today_bratislava();
    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, -35.0, 'charge', ?, datetime('now'))",
    )
    .bind(user_id)
    .bind(pass_svc)
    .bind(today)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app
        .request(get("/api/users/lookup/BOUNDARY-TODAY", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        body["pass"]["days_remaining"].as_i64().unwrap(),
        0,
        "valid_until == today must have 0 days_remaining; pass still present"
    );
    assert_eq!(body["pass"]["valid_until"], today.to_string());
}

// ── Mutant-killing tests added below ───────────────────────────────────────

/// Kills `u.allow_debit != 0` → `u.allow_debit == 0` mutant in user_response_from_row.
/// allow_debit defaults to 0 on new users, so this test verifies the default false path.
#[tokio::test]
async fn user_response_allow_debit_reflects_db_value() {
    let app = TestApp::new().await;
    app.seed_card("ALLOW-DEBIT", 10.0, None, None, None, None)
        .await;
    let (status, body) = app
        .request(get("/api/users/lookup/ALLOW-DEBIT", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "body={body}");
    // allow_debit defaults to 0 (false) for new users created via seed_card.
    assert_eq!(
        body["allow_debit"], false,
        "allow_debit must be false when DB row has allow_debit=0 (default)"
    );
}

/// Kills `body.price < 0.0` → `body.price <= 0.0` mutant in sell_pass.
/// price=0 is a valid promotional pass — mutation would wrongly reject it with 400.
#[tokio::test]
async fn sell_pass_accepts_zero_price() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("SELL-ZERO", 10.0, None, None, None, None)
        .await;
    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 0.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "price=0 must be accepted; body={resp}"
    );
    // Credit unchanged when pass is free (price=0 → no debit).
    assert_eq!(
        resp["new_credit"].as_f64().unwrap(),
        10.0,
        "credit unchanged when price=0"
    );
}

/// list_users must return correct pass info via the single GROUP BY query,
/// not N+1 individual queries. Regression guard for the refactored path.
#[tokio::test]
async fn list_users_returns_correct_pass_info_after_n_plus_one_fix() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("LIST-PASS-1", 50.0, None, None, None, None)
        .await;

    // Sell a pass so the user has a non-null pass_valid_until.
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-06-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (status, body) = app.request(get("/api/users", &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK, "body={body}");

    let users = body.as_array().expect("response must be an array");
    let user = users
        .iter()
        .find(|u| u["id"].as_i64() == Some(user_id))
        .expect("seeded user must appear in list");
    assert_eq!(
        user["pass"]["valid_until"], "2030-06-01",
        "list_users must include pass.valid_until from the GROUP BY query"
    );
    let days = user["pass"]["days_remaining"].as_i64().unwrap();
    assert!(days > 0, "days_remaining must be positive; got {days}");
}

/// /charge must reject the Monthly pass service_id and direct staff to /sell-pass.
#[tokio::test]
async fn charge_rejects_monthly_pass_service_id() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("CHG-PASS", 100.0, None, None, None, None)
        .await;
    let pass_service_id = service_id(&app, "Monthly pass").await;
    let (status, body) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({ "user_id": user_id, "amount": 35.0, "service_id": pass_service_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("sell-pass"),
        "error must point user to sell-pass endpoint, got: {body}"
    );
}

/// Kills mutants 4, 5 (empty-router replacements) and mutant 6 (`delete -` on -35.0).
/// Empty-router mutants would return 404; real router returns 200.
/// Sign mutant (+35.0) would store a positive amount; real code stores -35.0.
#[tokio::test]
async fn seed_expired_pass_endpoint_is_reachable_and_stores_negative_amount() {
    let app = TestApp::new().await;
    let (status, body) = app
        .request(post_json(
            "/api/test/seed-expired-pass",
            &app.staff_token,
            &json!({ "barcode": "SEED-EXPIRED-1", "valid_until": "2020-01-01" }),
        ))
        .await;
    // Empty-router mutants would 404 — real router returns 200.
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "seed endpoint must be reachable; body={body}"
    );
    let user_id = body["user_id"].as_i64().expect("user_id in response");

    // Verify the stored amount is NEGATIVE — kills the `delete -` mutant on -35.0.
    let amount: f64 = sqlx::query_scalar(
        "SELECT amount FROM transactions WHERE user_id = ? AND valid_until IS NOT NULL",
    )
    .bind(user_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        amount, -35.0,
        "seed_expired_pass must store a NEGATIVE amount (ledger convention)"
    );
}

#[tokio::test]
async fn seed_transactions_endpoint_seeds_user_and_legacy_backfilled_rows() {
    // Hits POST /api/test/seed-transactions with staff token. Verifies that
    // the legacy_backfilled marker is set on inserted rows (kills the mutant
    // that drops the `, legacy_backfilled, 1` portion of the INSERT).
    let app = TestApp::new().await;
    let (status, body) = app
        .request(post_json(
            "/api/test/seed-transactions",
            &app.staff_token,
            &json!({
                "barcode": "SEED-TXN-1",
                "entries": [
                    { "amount": 1.66, "action": "debit", "service_name_sk": "Občerstvenie" },
                ],
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "body={body}");
    let user_id = body["user_id"].as_i64().expect("user_id in response");

    let (svc_id, legacy_backfilled): (Option<i64>, i64) =
        sqlx::query_as("SELECT service_id, legacy_backfilled FROM transactions WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(svc_id.is_some(), "service_id resolved from name_sk");
    assert_eq!(legacy_backfilled, 1, "marker must be set on seeded rows");
}

#[tokio::test]
async fn seed_transactions_forbidden_for_customer() {
    // Kills the `delete !` mutant on `if !claims.role.can_process_payments()`
    // in routes/test_fixtures.rs::seed_transactions: with the ! removed, a
    // customer would be allowed through.
    let app = TestApp::new().await;
    let (status, _) = app
        .request(post_json(
            "/api/test/seed-transactions",
            &app.customer_token,
            &json!({
                "barcode": "FORBIDDEN-1",
                "entries": [
                    { "amount": 1.0, "action": "debit", "service_name_sk": "Spinning" },
                ],
            }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sell_pass_works_after_admin_renames_pass() {
    // Regression test for kind-based lookup. The Monthly pass row is now
    // identified by kind='monthly_pass', not by name='Monthly pass'. Admin
    // can rename name_sk and name_en freely without breaking sell-pass.
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("PASS-RENAME-1", 50.0, None, None, None, None)
        .await;

    let pass_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();

    // Admin renames the Monthly pass — both languages.
    let (status, row) = app
        .request(helpers::put_json(
            &format!("/api/admin/services/{pass_id}"),
            &app.admin_token,
            &json!({ "name_sk": "Permanentka", "name_en": "Membership" }),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "rename must succeed: {row}"
    );

    // Sell pass must still succeed (lookup is by kind, not name).
    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "user_id": user_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "sell-pass must work after rename: {resp}"
    );
    assert_eq!(resp["new_credit"].as_f64().unwrap(), 15.0);
}
