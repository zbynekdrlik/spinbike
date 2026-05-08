//! Integration tests for #56 — GET /api/users/by-last-movement.
//! Covers ordering, pagination, NULL-first, voided-tx exclusion,
//! soft-deleted-user exclusion, and 403.

mod helpers;

use helpers::{TestApp, delete, get, post_json};
use serde_json::json;

#[tokio::test]
async fn list_orders_by_oldest_movement_first() {
    let app = TestApp::new().await;
    // Seed 3 users; B has an OLDER charge than C; A has no transactions.
    let a_id = app
        .seed_card("MOV-A", 0.0, None, None, Some("Alice A"), None)
        .await;
    let b_id = app
        .seed_card("MOV-B", 0.0, None, None, Some("Bob B"), None)
        .await;
    let c_id = app
        .seed_card("MOV-C", 0.0, None, None, Some("Charlie C"), None)
        .await;
    let spinning_id = app.spinning_service_id().await;

    // Charge B 2 days ago, C today
    let (_, resp_b) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": b_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    let tx_b = resp_b.get("transaction_id").unwrap().as_i64().unwrap();
    let (_, _resp_c) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": c_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;

    // Backdate B's transaction by 2 days.
    let two_days_ago = (chrono::Local::now().date_naive() - chrono::Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();
    let _ = app
        .request(helpers::patch_json(
            &format!("/api/transactions/{tx_b}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": two_days_ago}),
        ))
        .await;

    let (status, body) = app
        .request(get(
            "/api/users/by-last-movement?limit=50",
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = body.as_array().unwrap();
    let pos = |id: i64| {
        arr.iter()
            .position(|r| r["id"].as_i64() == Some(id))
            .unwrap_or_else(|| panic!("id {id} missing"))
    };
    // A first (no movement, NULLS FIRST), then B (older), then C (newer).
    assert!(pos(a_id) < pos(b_id), "A (no movement) before B");
    assert!(pos(b_id) < pos(c_id), "B (2d ago) before C (today)");
}

#[tokio::test]
async fn list_paginates_with_show_more() {
    let app = TestApp::new().await;
    for i in 0..5 {
        app.seed_card(
            &format!("PAG-{i}"),
            0.0,
            None,
            None,
            Some(&format!("U{i}")),
            None,
        )
        .await;
    }
    let (_, page1) = app
        .request(get(
            "/api/users/by-last-movement?limit=2&offset=0",
            &app.staff_token,
        ))
        .await;
    let (_, page2) = app
        .request(get(
            "/api/users/by-last-movement?limit=2&offset=2",
            &app.staff_token,
        ))
        .await;
    let p1 = page1.as_array().unwrap();
    let p2 = page2.as_array().unwrap();
    assert_eq!(p1.len(), 2);
    assert_eq!(p2.len(), 2);
    let p1_ids: Vec<i64> = p1.iter().map(|r| r["id"].as_i64().unwrap()).collect();
    let p2_ids: Vec<i64> = p2.iter().map(|r| r["id"].as_i64().unwrap()).collect();
    for id in &p2_ids {
        assert!(!p1_ids.contains(id), "page2 must not overlap page1");
    }
}

#[tokio::test]
async fn list_excludes_voided_transactions() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("VOID-A", 0.0, None, None, Some("Voider"), None)
        .await;
    let spinning_id = app.spinning_service_id().await;
    let (_, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": user_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    let tx_id = resp.get("transaction_id").unwrap().as_i64().unwrap();
    // Void it
    let (status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let (_, body) = app
        .request(get(
            "/api/users/by-last-movement?limit=50",
            &app.staff_token,
        ))
        .await;
    let arr = body.as_array().unwrap();
    let row = arr
        .iter()
        .find(|r| r["id"].as_i64() == Some(user_id))
        .unwrap();
    assert!(
        row["last_movement_at"].is_null(),
        "voided txn must not count as movement; got {row:?}"
    );
}

#[tokio::test]
async fn list_excludes_soft_deleted_users() {
    let app = TestApp::new().await;
    let user_id = app
        .seed_card("DEL-A", 0.0, None, None, Some("Doomed"), None)
        .await;
    let (status, _) = app
        .request(delete(&format!("/api/users/{user_id}"), &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (_, body) = app
        .request(get(
            "/api/users/by-last-movement?limit=200",
            &app.staff_token,
        ))
        .await;
    let arr = body.as_array().unwrap();
    assert!(
        arr.iter().all(|r| r["id"].as_i64() != Some(user_id)),
        "soft-deleted user must not appear"
    );
}

#[tokio::test]
async fn list_requires_staff_role() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(get(
            "/api/users/by-last-movement?limit=50",
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_rejects_bad_limit() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(get(
            "/api/users/by-last-movement?limit=999",
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

/// When the client omits `?limit=`, `default_limit()` returns 50 — the request
/// must succeed AND return more than 1 row (kills `default_limit -> 0` and
/// `default_limit -> 1` mutants).
#[tokio::test]
async fn list_default_limit_returns_multiple_rows() {
    let app = TestApp::new().await;
    // Seed 5 fresh users so the default-limit response contains plenty of rows
    // even on a clean test DB (TestApp seeds admin/staff/customer at minimum).
    for i in 0..5 {
        app.seed_card(
            &format!("DEF-{i}"),
            0.0,
            None,
            None,
            Some(&format!("D{i}")),
            None,
        )
        .await;
    }
    let (status, body) = app
        .request(get("/api/users/by-last-movement", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = body.as_array().unwrap();
    assert!(
        arr.len() > 1,
        "default_limit must allow more than 1 row; got {}",
        arr.len()
    );
}
