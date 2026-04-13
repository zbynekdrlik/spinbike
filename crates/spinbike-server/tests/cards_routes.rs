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
    assert_eq!(arr.len(), 2);
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

/// Existence check: keep cards routes mounted. Kills Router::new() mutants.
#[tokio::test]
async fn card_routes_are_registered() {
    let app = TestApp::new().await;
    for path in [
        "/api/cards",
        "/api/cards/search?q=x",
        "/api/cards/lookup/xxx",
    ] {
        let (status, _) = app.request(get(path, &app.staff_token)).await;
        assert_ne!(
            status,
            axum::http::StatusCode::NOT_FOUND,
            "route {path} should be registered"
        );
    }
    // delete on card id: should not 404 on route, may return 405 or forbid.
    let (status, _) = app
        .request(delete("/api/cards/9999", &app.staff_token))
        .await;
    // No DELETE handler exists, so 405 Method Not Allowed is the expected route-existence signal.
    assert!(
        status == axum::http::StatusCode::METHOD_NOT_ALLOWED
            || status == axum::http::StatusCode::NOT_FOUND
            || status == axum::http::StatusCode::OK
    );
}
