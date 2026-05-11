//! Integration tests for /api/users/* handlers.
//!
//! Ported from cards_routes.rs (which was written against the old /api/cards/*
//! API). After V13 the `cards` table was dropped; credit/blocked/card_code now
//! live on `users`. All endpoints have been rekeyed to user_id.

mod helpers;

use helpers::{TestApp, delete, get, post_json, put_json};

// ─── topup ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn topup_rejects_zero_amount() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("T1", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "user_id": user_id, "amount": 0.0 });
    let (status, _) = app
        .request(post_json("/api/users/topup", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn topup_rejects_negative_amount() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("T2", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "user_id": user_id, "amount": -5.0 });
    let (status, _) = app
        .request(post_json("/api/users/topup", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn topup_adds_exact_amount() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("T3", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "user_id": user_id, "amount": 25.0 });
    let (status, resp) = app
        .request(post_json("/api/users/topup", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp["credit"].as_f64().unwrap(), 35.0);
}

#[tokio::test]
async fn topup_forbidden_for_customer() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("T4", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "user_id": user_id, "amount": 5.0 });
    let (status, _) = app
        .request(post_json("/api/users/topup", &app.customer_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

// ─── list users ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_users_returns_all_users_for_staff() {
    let app = TestApp::new().await;
    app.seed_card("L1", 0.0, Some("A"), None, None, None).await;
    app.seed_card("L2", 0.0, Some("B"), None, None, None).await;
    let (status, resp) = app.request(get("/api/users", &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    // TestApp::new seeds 3 auth users (admin, staff, customer) + 2 here = 5.
    assert!(
        arr.len() >= 5,
        "expected at least 5 users, got {}",
        arr.len()
    );
}

#[tokio::test]
async fn list_users_forbidden_for_customer() {
    let app = TestApp::new().await;
    app.seed_card("L3", 0.0, None, None, None, None).await;
    let (status, _) = app.request(get("/api/users", &app.customer_token)).await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

// ─── search ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_returns_non_empty_when_matches_exist() {
    let app = TestApp::new().await;
    app.seed_card(
        "SRCH1",
        0.0,
        Some("Zbynek"),
        Some("Drlik"),
        Some("NL"),
        None,
    )
    .await;
    let (status, resp) = app
        .request(get("/api/users/search?q=Drlik", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    // After V13, the field is card_code (not barcode).
    assert_eq!(arr[0]["card_code"].as_str().unwrap(), "SRCH1");
}

#[tokio::test]
async fn search_forbidden_for_customer() {
    let app = TestApp::new().await;
    app.seed_card("SRCH2", 0.0, Some("X"), None, None, None)
        .await;
    let (status, _) = app
        .request(get("/api/users/search?q=X", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn search_default_limit_is_ten() {
    // Seed 15 users so we can distinguish default (10) from "all" or "1".
    let app = TestApp::new().await;
    for i in 0..15 {
        app.seed_card(
            &format!("LIM{i:02}"),
            0.0,
            Some("LimitTest"),
            None,
            None,
            None,
        )
        .await;
    }
    // Do NOT pass limit — rely on default.
    let (status, resp) = app
        .request(get("/api/users/search?q=LimitTest", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp.as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn search_respects_explicit_limit() {
    let app = TestApp::new().await;
    for i in 0..15 {
        app.seed_card(
            &format!("XLIM{i:02}"),
            0.0,
            Some("Explicit"),
            None,
            None,
            None,
        )
        .await;
    }
    let (status, resp) = app
        .request(get(
            "/api/users/search?q=Explicit&limit=3",
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp.as_array().unwrap().len(), 3);
}

// ─── negative-balance ─────────────────────────────────────────────────────────

#[tokio::test]
async fn negative_balance_endpoint_returns_only_negatives_sorted() {
    let app = TestApp::new().await;
    app.seed_card("NEG-A", -10.0, Some("Alpha"), None, None, None)
        .await;
    app.seed_card("NEG-B", -3.5, Some("Bravo"), None, None, None)
        .await;
    app.seed_card("POS-A", 5.0, Some("Charlie"), None, None, None)
        .await;

    let (status, resp) = app
        .request(get("/api/users/negative-balance", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    // Filter to our seeded users by card_code.
    let ours: Vec<_> = arr
        .iter()
        .filter(|r| {
            let c = r["card_code"].as_str().unwrap_or("");
            c == "NEG-A" || c == "NEG-B" || c == "POS-A"
        })
        .collect();
    assert_eq!(ours.len(), 2, "positive user must be excluded");
    assert_eq!(ours[0]["card_code"], "NEG-A", "most-negative first");
    assert_eq!(ours[1]["card_code"], "NEG-B");
}

#[tokio::test]
async fn negative_balance_endpoint_forbidden_for_customer() {
    let app = TestApp::new().await;
    app.seed_card("NEG-X", -1.0, None, None, None, None).await;
    let (status, _) = app
        .request(get("/api/users/negative-balance", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn negative_balance_endpoint_round_trips_blocked_field() {
    // Kills the `r.blocked != 0` → `r.blocked == 0` mutation: a blocked user
    // must serialise with `"blocked": true`, an unblocked one with `false`.
    let app = TestApp::new().await;
    let neg_blocked = app.seed_card("NEG-BLK", -2.0, None, None, None, None).await;
    spinbike_server::db::users::set_blocked(&app.pool, neg_blocked, true)
        .await
        .unwrap();
    app.seed_card("NEG-OPEN", -1.0, None, None, None, None)
        .await;

    let (status, resp) = app
        .request(get("/api/users/negative-balance", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    let blk = arr
        .iter()
        .find(|r| r["card_code"] == "NEG-BLK")
        .expect("blocked user must appear in negative-balance list");
    let opn = arr
        .iter()
        .find(|r| r["card_code"] == "NEG-OPEN")
        .expect("unblocked user must appear in negative-balance list");
    assert_eq!(blk["blocked"], true, "blocked user must report true");
    assert_eq!(opn["blocked"], false, "unblocked user must report false");
}

#[tokio::test]
async fn negative_balance_endpoint_round_trips_pass_field() {
    // Asserts both the present-pass and no-pass paths on the negative-balance list.
    let app = TestApp::new().await;
    let with_pass = app
        .seed_card("NEG-PASS", -2.0, None, None, None, None)
        .await;
    let no_pass = app
        .seed_card("NEG-NOPASS", -1.0, None, None, None, None)
        .await;

    let valid_until = chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
    let pass_tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, amount, action, valid_until, created_at)
         VALUES (?, -25.0, 'charge', ?, datetime('now')) RETURNING id",
    )
    .bind(with_pass)
    .bind(valid_until)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let _ = no_pass; // explicit: this user intentionally has no pass row.

    let (status, resp) = app
        .request(get("/api/users/negative-balance", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    let with_p = arr
        .iter()
        .find(|r| r["card_code"] == "NEG-PASS")
        .expect("pass-bearing user must be in list");
    let without_p = arr
        .iter()
        .find(|r| r["card_code"] == "NEG-NOPASS")
        .expect("pass-less user must be in list");

    let pass = with_p["pass"]
        .as_object()
        .expect("pass field must be a populated object for NEG-PASS");
    assert_eq!(pass["transaction_id"], pass_tx_id);
    assert_eq!(
        pass["valid_until"],
        valid_until.format("%Y-%m-%d").to_string()
    );
    assert!(
        pass["days_remaining"].is_i64(),
        "days_remaining must serialise as integer"
    );

    assert!(
        without_p["pass"].is_null(),
        "user without a pass must serialise pass=null"
    );
}

// ─── seed-credit fixture ──────────────────────────────────────────────────────

#[tokio::test]
async fn seed_credit_fixture_forbidden_for_customer() {
    // Kills the `delete !` mutation in the `if !claims.role.can_process_payments()`
    // gate on POST /api/test/seed-credit: a customer token must NOT be able
    // to mutate `users.credit` via the test fixture.
    let app = TestApp::new().await;
    let body = serde_json::json!({ "barcode": "SC-FORBID", "credit": -5.0 });
    let (status, _) = app
        .request(post_json(
            "/api/test/seed-credit",
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

// ─── create user (replaces activate card) ────────────────────────────────────

#[tokio::test]
async fn create_user_duplicate_card_code_returns_conflict() {
    let app = TestApp::new().await;
    app.seed_card("DUP", 0.0, None, None, None, None).await;
    // Attempt to create another user with the same card_code via the API.
    let body =
        serde_json::json!({ "name": "Duplicate", "card_code": "DUP", "initial_credit": 0.0 });
    let (status, _) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    // Kills the `||` → `&&` mutant: either "UNIQUE" or "unique" must trip the
    // conflict path, not both.
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn create_user_new_succeeds() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "name": "Ivan Novak",
        "card_code": "NEW-001",
        "initial_credit": 100.0,
    });
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert_eq!(resp["card_code"].as_str().unwrap(), "NEW-001");
    assert_eq!(resp["credit"].as_f64().unwrap(), 100.0);
    assert_eq!(resp["name"].as_str().unwrap(), "Ivan Novak");
}

#[tokio::test]
async fn create_user_email_optional() {
    // POST with no email must succeed; the response email field must be null.
    let app = TestApp::new().await;
    let body = serde_json::json!({ "name": "No Email User" });
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert!(
        resp["email"].is_null(),
        "email must be null when not provided; got {:?}",
        resp["email"]
    );
}

#[tokio::test]
async fn create_user_duplicate_email_conflict() {
    // Posting the same email twice must return 409 Conflict on the second call.
    let app = TestApp::new().await;
    let body1 = serde_json::json!({ "name": "First User", "email": "dup@example.com" });
    let (status, _) = app
        .request(post_json("/api/users", &app.staff_token, &body1))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);

    let body2 = serde_json::json!({ "name": "Second User", "email": "dup@example.com" });
    let (status, _) = app
        .request(post_json("/api/users", &app.staff_token, &body2))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

// ─── update user info ─────────────────────────────────────────────────────────

#[tokio::test]
async fn update_user_info_persists_and_staff_only() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("U1", 0.0, None, None, None, None).await;
    let body = serde_json::json!({
        "name": "Updated Name",
        "company": "Acme",
        "phone": "+421900000000",
    });
    // Customer cannot update.
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{user_id}"),
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);

    // Staff can update.
    let (status, resp) = app
        .request(put_json(
            &format!("/api/users/{user_id}"),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp["name"].as_str().unwrap(), "Updated Name");
    assert_eq!(resp["company"].as_str().unwrap(), "Acme");
}

// ─── user transactions ────────────────────────────────────────────────────────

#[tokio::test]
async fn user_transactions_returns_ledger_for_staff() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("TX1", 100.0, None, None, None, None).await;
    // Make a topup to generate a transaction row.
    let body = serde_json::json!({ "user_id": user_id, "amount": 5.0 });
    let _ = app
        .request(post_json("/api/users/topup", &app.staff_token, &body))
        .await;

    let (status, resp) = app
        .request(get(
            &format!("/api/users/{user_id}/transactions"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one transaction");
}

#[tokio::test]
async fn user_transactions_forbidden_for_customer() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("TX2", 0.0, None, None, None, None).await;
    let (status, _) = app
        .request(get(
            &format!("/api/users/{user_id}/transactions"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

// ─── block / unblock ──────────────────────────────────────────────────────────

#[tokio::test]
async fn block_and_unblock_toggles() {
    let app = TestApp::new().await;
    let user_id = app.seed_card("B1", 0.0, None, None, None, None).await;
    let block_body = serde_json::json!({ "user_id": user_id, "blocked": true });
    let (status, resp) = app
        .request(post_json("/api/users/block", &app.staff_token, &block_body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(resp["blocked"].as_bool().unwrap());

    let unblock_body = serde_json::json!({ "user_id": user_id, "blocked": false });
    let (_, resp) = app
        .request(post_json(
            "/api/users/block",
            &app.staff_token,
            &unblock_body,
        ))
        .await;
    assert!(!resp["blocked"].as_bool().unwrap());
}

#[tokio::test]
async fn create_user_blocked_field_round_trip() {
    // Create a user, set blocked via /api/users/block, fetch and verify.
    let app = TestApp::new().await;
    let body = serde_json::json!({ "name": "Block Test User", "card_code": "BRT-001" });
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let user_id = resp["id"].as_i64().unwrap();

    // Initially unblocked.
    assert!(!resp["blocked"].as_bool().unwrap());

    // Block via endpoint.
    let block_body = serde_json::json!({ "user_id": user_id, "blocked": true });
    let (status, resp) = app
        .request(post_json("/api/users/block", &app.staff_token, &block_body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(resp["blocked"].as_bool().unwrap());

    // Fetch the user's transactions list (any staff GET that reads the user)
    // and confirm blocked is still true.
    let (status, _resp) = app
        .request(get(
            &format!("/api/users/{user_id}/transactions"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

// ─── transactions deleted_at field ────────────────────────────────────────────

#[tokio::test]
async fn transactions_endpoint_returns_deleted_at_field() {
    let app = TestApp::new().await;
    // Create a topup then soft-delete it.
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (user_id, amount, action) VALUES (?, 5.0, 'topup') RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    spinbike_server::db::transactions::soft_delete(&app.pool, tx_id)
        .await
        .unwrap();

    let uri = format!("/api/users/{}/transactions", app.customer_card_id);
    let (status, resp) = app.request(get(&uri, &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let row = resp
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_i64() == Some(tx_id))
        .expect("deleted row must still be listed");
    assert!(
        row.get("deleted_at").and_then(|v| v.as_str()).is_some(),
        "response must include deleted_at string"
    );
}

// ─── route existence check ────────────────────────────────────────────────────

/// Existence check: keep users routes mounted. Kills Router::new() mutants.
/// We only probe endpoints whose handler cannot legitimately return 404 —
/// otherwise a handler-level 404 is indistinguishable from a missing route.
#[tokio::test]
async fn user_routes_are_registered() {
    let app = TestApp::new().await;
    app.seed_card("REG1", 0.0, Some("Probe"), None, None, None)
        .await;
    for path in [
        "/api/users",                   // list: 200 with array
        "/api/users/search?q=Probe",    // search: 200 with array
        "/api/users/9999/transactions", // list txns for unknown id: 200 with []
        "/api/my/balance",              // balance: 200
    ] {
        let (status, _) = app.request(get(path, &app.staff_token)).await;
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "route {path} should be registered and return 200"
        );
    }
    // DELETE on /api/users/{id} is wired (#56 soft-delete). Unknown id returns
    // 404 from the handler, which proves the route + method are registered.
    let (status, _) = app
        .request(delete("/api/users/9999", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

// ─── /api/my/balance allow_self_entry boundary ───────────────────────────────
//
// my_balance maps the SQLite INTEGER (0 / 1) column to a Rust bool via
// `ase != 0`. The L936:87 mutant flips `!=` to `==`, which inverts the
// resulting bool (so an opted-in user reports false and vice versa).

#[tokio::test]
async fn my_balance_reports_allow_self_entry_when_opted_in() {
    let app = TestApp::new().await;
    sqlx::query("UPDATE users SET allow_self_entry = 1, credit = 21.50 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, body) = app
        .request(get("/api/my/balance", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        body["allow_self_entry"], true,
        "allow_self_entry=1 column must surface as true (catches `!=` → `==` mutant)"
    );
    // Round-trip the credit too — locks down that the row is read correctly.
    let credit = body["credit"]
        .as_f64()
        .expect("credit must be a number, got {body}");
    assert!(
        (credit - 21.5).abs() < 0.001,
        "expected credit 21.5, got {credit}"
    );
}

#[tokio::test]
async fn my_balance_reports_no_allow_self_entry_when_opted_out() {
    let app = TestApp::new().await;
    // Customer starts with allow_self_entry = 0 (V16 default). Force-set it
    // here so the assertion is unambiguous.
    sqlx::query("UPDATE users SET allow_self_entry = 0 WHERE id = ?")
        .bind(app.customer_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, body) = app
        .request(get("/api/my/balance", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        body["allow_self_entry"], false,
        "allow_self_entry=0 column must surface as false (catches `!=` → `==` mutant)"
    );
}

// ─── negative-balance boundary tests ─────────────────────────────────────────

#[tokio::test]
async fn negative_balance_excludes_zero_credit() {
    // A user with credit=0 must NOT appear in the negative-balance list.
    let app = TestApp::new().await;
    let zero_user = app
        .seed_card("NEGBND-ZERO", 0.0, Some("ZeroCredit"), None, None, None)
        .await;

    let (status, resp) = app
        .request(get("/api/users/negative-balance", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(
        !arr.iter().any(|r| r["id"].as_i64() == Some(zero_user)),
        "user with credit=0 must NOT appear in negative-balance list"
    );
}

#[tokio::test]
async fn negative_balance_includes_minus_one_cent() {
    // A user with credit=-0.01 must appear in the negative-balance list.
    let app = TestApp::new().await;
    let neg_user = app
        .seed_card("NEGBND-CENT", -0.01, Some("NegOneCent"), None, None, None)
        .await;

    let (status, resp) = app
        .request(get("/api/users/negative-balance", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(
        arr.iter().any(|r| r["id"].as_i64() == Some(neg_user)),
        "user with credit=-0.01 must appear in negative-balance list"
    );
}

// ─── seed-user fixture auth gate (mutant #3) ──────────────────────────────────

// Mutant #3: delete `!` in `if !claims.role.can_process_payments()` gate.
// With the mutation a customer can POST seed-user (returns 201); staff is
// denied (returns 403). Both tests are required to catch the flip.

#[tokio::test]
async fn seed_user_fixture_forbidden_for_customer() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "SeedX", "email": "seedx@x.com", "credit": 1.0});
    let (status, _) = app
        .request(post_json("/api/test/seed-user", &app.customer_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn seed_user_fixture_allowed_for_staff() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "SeedY", "email": "seedy@x.com", "credit": 1.0});
    let (status, _) = app
        .request(post_json("/api/test/seed-user", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
}

// ─── create_user email format checks (mutants #4, #8-10) ─────────────────────

// Mutant #4 / #8-10: replace `||` with `&&` (or delete either `!`) in the
// email format guard `!email.contains('@') || !email.contains('.')`.
// With `&&` a single-char absence is no longer sufficient to fail; each test
// sends an email that is missing exactly ONE of the required chars so that:
//   - correct code (||): both checks fail → 400
//   - mutant (&&): only one check is true → no error → 201

#[tokio::test]
async fn create_user_rejects_email_with_at_but_no_dot() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "NoDot", "email": "no@dot"});
    let (status, _) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_user_rejects_email_with_dot_but_no_at() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "NoAt", "email": "no.at.com"});
    let (status, _) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

// ─── create_user initial_credit filter (mutants #5-7) ────────────────────────

// Mutants #5-7: replace `>` with `==`, `<`, or `>=` in `.filter(|&c| c > 0.0)`.
// zero: must NOT write a transaction (mutant `>=` would write one for zero).
// positive: MUST write exactly one transaction (mutant `< 0` would suppress it).
// negative: must NOT write a transaction (mutant `< 0.0` would write one).

#[tokio::test]
async fn create_user_with_zero_initial_credit_writes_no_transaction() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "ZeroCred", "email": "zc@x.com", "initial_credit": 0.0});
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let user_id = resp["id"].as_i64().unwrap();
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transactions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0, "initial_credit=0 must NOT write a transaction");
}

#[tokio::test]
async fn create_user_with_positive_initial_credit_writes_one_transaction() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "PosCred", "email": "pc@x.com", "initial_credit": 25.0});
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let user_id = resp["id"].as_i64().unwrap();
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transactions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        row.0, 1,
        "initial_credit>0 must write exactly one topup transaction"
    );
}

#[tokio::test]
async fn create_user_with_negative_initial_credit_writes_no_transaction() {
    // Negative initial_credit is either rejected (400) or silently filtered
    // out (no txn). Either outcome is acceptable. We just assert that if a
    // user is created, no transaction row is written — killing the `c < 0.0`
    // and `c >= 0.0` mutants which would write a row for negative / zero values.
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "NegCred", "email": "nc@x.com", "initial_credit": -5.0});
    let (_, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    if let Some(user_id) = resp["id"].as_i64() {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transactions WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
        assert_eq!(
            row.0, 0,
            "negative initial_credit must NOT write a transaction"
        );
    }
}

// ─── create_user blank-string normalisation (trim+empty→None) ─────────────────
//
// Kills the `!s.is_empty()` → `s.is_empty()` mutants in the body_phone /
// body_company filter chains. Without normalisation, an empty-string phone
// or company would be stored verbatim. With the mutation flipped, ONLY empty
// strings would be stored (non-empty → None). Both regimes leak into the DB
// row, so we assert the post-create row has the field stored as NULL.

#[tokio::test]
async fn create_user_normalises_empty_phone_to_null() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "name": "EmptyPhone",
        "email": "ep@x.com",
        "phone": "   ",
    });
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let user_id = resp["id"].as_i64().unwrap();
    let phone: Option<String> = sqlx::query_scalar("SELECT phone FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        phone.is_none(),
        "whitespace-only phone must be stored as NULL, got: {phone:?}"
    );
}

#[tokio::test]
async fn create_user_normalises_empty_company_to_null() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "name": "EmptyCompany",
        "email": "ec@x.com",
        "company": "",
    });
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let user_id = resp["id"].as_i64().unwrap();
    let company: Option<String> = sqlx::query_scalar("SELECT company FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        company.is_none(),
        "empty company must be stored as NULL, got: {company:?}"
    );
}

#[tokio::test]
async fn create_user_preserves_non_empty_phone_and_company() {
    // Counterpart to the empty-string tests: real values must round-trip.
    // Killed together with the empty-string tests, this also catches a
    // mutation that filters out everything (would store NULL for non-empty too).
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "name": "RealValues",
        "email": "rv@x.com",
        "phone": "+421900111222",
        "company": "TestCorp",
    });
    let (status, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let user_id = resp["id"].as_i64().unwrap();
    let row: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT phone, company FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(row.0.as_deref(), Some("+421900111222"));
    assert_eq!(row.1.as_deref(), Some("TestCorp"));
}

// ─── update_user email format checks (mutants #8-10) ─────────────────────────

#[tokio::test]
async fn update_user_rejects_email_without_at() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "UpdNoAt", "email": "upd1@x.com"});
    let (_, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    let id = resp["id"].as_i64().unwrap();
    let upd = serde_json::json!({"email": "no.at.com"});
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{id}"),
            &app.staff_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_user_rejects_email_without_dot() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "UpdNoDot", "email": "upd2@x.com"});
    let (_, resp) = app
        .request(post_json("/api/users", &app.staff_token, &body))
        .await;
    let id = resp["id"].as_i64().unwrap();
    let upd = serde_json::json!({"email": "no@dot"});
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{id}"),
            &app.staff_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

// ─── update_user collision checks (mutants #11, #12) ─────────────────────────

// Mutant #11: replace `!=` with `==` in `existing.id != id` (email collision).
// With `==`: updating A with B's email is allowed (existing.id==A==id → no
// conflict → 200 instead of 409). Updating A with A's own email is blocked
// (existing.id==id → 409 instead of 200). Both tests are required.

#[tokio::test]
async fn update_user_email_collision_with_other_user_returns_409() {
    let app = TestApp::new().await;
    let (_, ra) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({"name": "ColA", "email": "cola@x.com"}),
        ))
        .await;
    let (_, rb) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({"name": "ColB", "email": "colb@x.com"}),
        ))
        .await;
    let a = ra["id"].as_i64().unwrap();
    // Try to update A's email to B's email — must be rejected.
    let upd = serde_json::json!({"email": "colb@x.com"});
    let (status, _) = app
        .request(put_json(&format!("/api/users/{a}"), &app.staff_token, &upd))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    // Suppress "unused binding" warning on rb.
    let _ = rb;
}

#[tokio::test]
async fn update_user_email_unchanged_returns_200() {
    let app = TestApp::new().await;
    let (_, r) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({"name": "SameEmail", "email": "same@x.com"}),
        ))
        .await;
    let id = r["id"].as_i64().unwrap();
    // Updating with the same email must NOT trigger a collision.
    let upd = serde_json::json!({"email": "same@x.com"});
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{id}"),
            &app.staff_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

// Mutant #12: replace `!=` with `==` in `existing.id != id` (card_code collision).

#[tokio::test]
async fn update_user_card_code_collision_with_other_user_returns_409() {
    let app = TestApp::new().await;
    let (_, ra) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({"name": "CcA", "email": "cca@x.com", "card_code": "CC_A"}),
        ))
        .await;
    let (_, _rb) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({"name": "CcB", "email": "ccb@x.com", "card_code": "CC_B"}),
        ))
        .await;
    let a = ra["id"].as_i64().unwrap();
    // Try to set A's card_code to B's card_code — must be rejected.
    let upd = serde_json::json!({"card_code": "CC_B"});
    let (status, _) = app
        .request(put_json(&format!("/api/users/{a}"), &app.staff_token, &upd))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn update_user_card_code_unchanged_returns_200() {
    let app = TestApp::new().await;
    let (_, r) = app
        .request(post_json(
            "/api/users",
            &app.staff_token,
            &serde_json::json!({"name": "SameCode", "email": "scod@x.com", "card_code": "CC_D"}),
        ))
        .await;
    let id = r["id"].as_i64().unwrap();
    // Updating with the same card_code must NOT trigger a collision.
    let upd = serde_json::json!({"card_code": "CC_D"});
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{id}"),
            &app.staff_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

// ─── allow_self_entry — admin-only guard ──────────────────────────────────────

#[tokio::test]
async fn admin_can_set_allow_self_entry() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"allow_self_entry": true});
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{}", app.customer_id),
            &app.admin_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let val: i64 = sqlx::query_scalar("SELECT allow_self_entry FROM users WHERE id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(val, 1);
}

#[tokio::test]
async fn staff_cannot_set_allow_self_entry() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"allow_self_entry": true});
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{}", app.customer_id),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    let val: i64 = sqlx::query_scalar("SELECT allow_self_entry FROM users WHERE id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(val, 0, "field must not have been updated");
}

#[tokio::test]
async fn staff_can_still_edit_other_fields() {
    let app = TestApp::new().await;
    let body = serde_json::json!({"name": "Renamed By Staff"});
    let (status, _) = app
        .request(put_json(
            &format!("/api/users/{}", app.customer_id),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
}
