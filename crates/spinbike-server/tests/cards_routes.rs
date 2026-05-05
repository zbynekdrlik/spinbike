//! Integration tests for /api/cards/* handlers.

mod helpers;

use helpers::{TestApp, delete, get, post_json, put_json};

#[tokio::test]
async fn topup_rejects_zero_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("T1", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": 0.0 });
    let (status, _) = app
        .request(post_json("/api/cards/topup", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn topup_rejects_negative_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("T2", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": -5.0 });
    let (status, _) = app
        .request(post_json("/api/cards/topup", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn topup_adds_exact_amount() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("T3", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": 25.0 });
    let (status, resp) = app
        .request(post_json("/api/cards/topup", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp["credit"].as_f64().unwrap(), 35.0);
}

#[tokio::test]
async fn topup_forbidden_for_customer() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("T4", 10.0, None, None, None, None).await;
    let body = serde_json::json!({ "card_id": card_id, "amount": 5.0 });
    let (status, _) = app
        .request(post_json("/api/cards/topup", &app.customer_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_cards_returns_all_cards_for_staff() {
    let app = TestApp::new().await;
    app.seed_card("L1", 0.0, Some("A"), None, None, None).await;
    app.seed_card("L2", 0.0, Some("B"), None, None, None).await;
    let (status, resp) = app.request(get("/api/cards", &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    // 2 seeded here + 1 auto-seeded by TestApp::new (CUST1)
    assert_eq!(arr.len(), 3);
}

#[tokio::test]
async fn list_cards_forbidden_for_customer() {
    let app = TestApp::new().await;
    app.seed_card("L3", 0.0, None, None, None, None).await;
    let (status, _) = app.request(get("/api/cards", &app.customer_token)).await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

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
        .request(get("/api/cards/search?q=Drlik", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["barcode"].as_str().unwrap(), "SRCH1");
}

#[tokio::test]
async fn search_forbidden_for_customer() {
    let app = TestApp::new().await;
    app.seed_card("SRCH2", 0.0, Some("X"), None, None, None)
        .await;
    let (status, _) = app
        .request(get("/api/cards/search?q=X", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn search_default_limit_is_ten() {
    // Seed 15 cards so we can distinguish default (10) from "all" or "1".
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
        .request(get("/api/cards/search?q=LimitTest", &app.staff_token))
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
            "/api/cards/search?q=Explicit&limit=3",
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp.as_array().unwrap().len(), 3);
}

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
        .request(get("/api/cards/negative-balance", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    // TestApp::new() auto-seeds a CUST1 card. Filter to ours by barcode
    // so this test stays robust if that fixture changes.
    let ours: Vec<_> = arr
        .iter()
        .filter(|r| {
            let b = r["barcode"].as_str().unwrap_or("");
            b == "NEG-A" || b == "NEG-B" || b == "POS-A"
        })
        .collect();
    assert_eq!(ours.len(), 2, "positive card must be excluded");
    assert_eq!(ours[0]["barcode"], "NEG-A", "most-negative first");
    assert_eq!(ours[1]["barcode"], "NEG-B");
}

#[tokio::test]
async fn negative_balance_endpoint_forbidden_for_customer() {
    let app = TestApp::new().await;
    app.seed_card("NEG-X", -1.0, None, None, None, None).await;
    let (status, _) = app
        .request(get("/api/cards/negative-balance", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn activate_duplicate_barcode_returns_conflict() {
    let app = TestApp::new().await;
    app.seed_card("DUP", 0.0, None, None, None, None).await;
    let body = serde_json::json!({ "barcode": "DUP", "initial_credit": 0.0 });
    let (status, _) = app
        .request(post_json("/api/cards/activate", &app.staff_token, &body))
        .await;
    // Kills the `||` → `&&` mutant in the UNIQUE-match branch: either
    // "UNIQUE" or "unique" must trip the conflict path, not both.
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn activate_new_card_succeeds() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "barcode": "NEW-001",
        "initial_credit": 100.0,
        "first_name": "Ivan",
        "last_name": "Novak",
    });
    let (status, resp) = app
        .request(post_json("/api/cards/activate", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert_eq!(resp["barcode"].as_str().unwrap(), "NEW-001");
    assert_eq!(resp["credit"].as_f64().unwrap(), 100.0);
    assert_eq!(resp["first_name"].as_str().unwrap(), "Ivan");
}

#[tokio::test]
async fn update_card_info_persists_and_staff_only() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("U1", 0.0, None, None, None, None).await;
    let body = serde_json::json!({
        "first_name": "Updated",
        "last_name": "Name",
        "company": "Acme",
        "phone": "+421900000000",
    });
    // Customer cannot update.
    let (status, _) = app
        .request(put_json(
            &format!("/api/cards/{card_id}"),
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);

    // Staff can update.
    let (status, resp) = app
        .request(put_json(
            &format!("/api/cards/{card_id}"),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp["first_name"].as_str().unwrap(), "Updated");
    assert_eq!(resp["company"].as_str().unwrap(), "Acme");
}

#[tokio::test]
async fn card_transactions_returns_ledger_for_staff() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("TX1", 100.0, None, None, None, None).await;
    // Make a topup to generate a transaction row.
    let body = serde_json::json!({ "card_id": card_id, "amount": 5.0 });
    let _ = app
        .request(post_json("/api/cards/topup", &app.staff_token, &body))
        .await;

    let (status, resp) = app
        .request(get(
            &format!("/api/cards/{card_id}/transactions"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one transaction");
}

#[tokio::test]
async fn card_transactions_forbidden_for_customer() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("TX2", 0.0, None, None, None, None).await;
    let (status, _) = app
        .request(get(
            &format!("/api/cards/{card_id}/transactions"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn block_and_unblock_toggles() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("B1", 0.0, None, None, None, None).await;
    let block_body = serde_json::json!({ "card_id": card_id, "blocked": true });
    let (status, resp) = app
        .request(post_json("/api/cards/block", &app.staff_token, &block_body))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(resp["blocked"].as_bool().unwrap());

    let unblock_body = serde_json::json!({ "card_id": card_id, "blocked": false });
    let (_, resp) = app
        .request(post_json(
            "/api/cards/block",
            &app.staff_token,
            &unblock_body,
        ))
        .await;
    assert!(!resp["blocked"].as_bool().unwrap());
}

#[tokio::test]
async fn transactions_endpoint_returns_deleted_at_field() {
    let app = TestApp::new().await;
    // Create a topup then soft-delete it.
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action) VALUES (?, 5.0, 'topup') RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    spinbike_server::db::transactions::soft_delete(&app.pool, tx_id)
        .await
        .unwrap();

    let uri = format!("/api/cards/{}/transactions", app.customer_card_id);
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

/// Existence check: keep cards routes mounted. Kills Router::new() mutants.
/// We only probe endpoints whose handler cannot legitimately return 404 —
/// otherwise a handler-level 404 is indistinguishable from a missing route.
#[tokio::test]
async fn card_routes_are_registered() {
    let app = TestApp::new().await;
    app.seed_card("REG1", 0.0, Some("Probe"), None, None, None)
        .await;
    for path in [
        "/api/cards",                   // list: 200 with array
        "/api/cards/search?q=Probe",    // search: 200 with array
        "/api/cards/9999/transactions", // list txns for unknown id: 200 with []
        "/api/my/balance",              // balance: 200
    ] {
        let (status, _) = app.request(get(path, &app.staff_token)).await;
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "route {path} should be registered and return 200"
        );
    }
    // Unrelated delete: DELETE isn't wired on /api/cards/{id}, so Axum returns
    // 405 Method Not Allowed. That confirms the path segment itself is known.
    let (status, _) = app
        .request(delete("/api/cards/9999", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::METHOD_NOT_ALLOWED);
}
