//! Integration tests for the `last_visit_at` field on /api/users/search.

mod helpers;

use helpers::{TestApp, get};

/// Local DTO mirroring just the fields these tests assert on. The server's
/// `UserResponse` derives only `Serialize`, so this test file defines its own
/// `Deserialize` shape; serde_json ignores extra fields by default, so the
/// wire response's other fields (credit, blocked, etc.) are silently skipped.
#[derive(serde::Deserialize, Debug)]
struct UserResponse {
    pub id: i64,
    pub card_code: String,
    pub last_visit_at: Option<String>,
}

/// Insert a transaction at a chosen timestamp, optionally tied to a service.
async fn seed_txn(
    pool: &sqlx::SqlitePool,
    user_id: i64,
    service_name_en: Option<&str>,
    amount: f64,
    action: &str,
    created_at: &str,
) -> i64 {
    let service_id: Option<i64> = if let Some(n) = service_name_en {
        Some(
            sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?")
                .bind(n)
                .fetch_one(pool)
                .await
                .unwrap(),
        )
    } else {
        None
    };
    let result = sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
    result.last_insert_rowid()
}

async fn search(app: &TestApp, q: &str) -> (axum::http::StatusCode, Vec<UserResponse>) {
    app.request_typed::<Vec<UserResponse>>(get(
        &format!("/api/users/search?q={q}&limit=50"),
        &app.staff_token,
    ))
    .await
}

fn fmt(d: chrono::DateTime<chrono::Local>) -> String {
    d.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[tokio::test]
async fn last_visit_at_populated_correctly_for_each_seed_shape() {
    let app = TestApp::new().await;
    let prefix = "LVTEST";
    let card_a = app
        .seed_card(
            &format!("{prefix}A"),
            0.0,
            Some("Alpha"),
            Some("A"),
            None,
            None,
        )
        .await;
    let card_b = app
        .seed_card(
            &format!("{prefix}B"),
            0.0,
            Some("Bravo"),
            Some("B"),
            None,
            None,
        )
        .await;
    let card_c = app
        .seed_card(
            &format!("{prefix}C"),
            0.0,
            Some("Charlie"),
            Some("C"),
            None,
            None,
        )
        .await;
    let card_d = app
        .seed_card(
            &format!("{prefix}D"),
            0.0,
            Some("Delta"),
            Some("D"),
            None,
            None,
        )
        .await;
    let card_e = app
        .seed_card(
            &format!("{prefix}E"),
            0.0,
            Some("Echo"),
            Some("E"),
            None,
            None,
        )
        .await;
    let card_f = app
        .seed_card(
            &format!("{prefix}F"),
            0.0,
            Some("Foxtrot"),
            Some("F"),
            None,
            None,
        )
        .await;
    let card_g = app
        .seed_card(
            &format!("{prefix}G"),
            0.0,
            Some("Golf"),
            Some("G"),
            None,
            None,
        )
        .await;

    let now = chrono::Local::now();
    let yesterday = (now - chrono::Duration::days(1))
        .format("%Y-%m-%d 12:00:00")
        .to_string();
    let five_days = (now - chrono::Duration::days(5))
        .format("%Y-%m-%d 12:00:00")
        .to_string();
    let ten_days = (now - chrono::Duration::days(10))
        .format("%Y-%m-%d 12:00:00")
        .to_string();
    let thirty_days = (now - chrono::Duration::days(30))
        .format("%Y-%m-%d 12:00:00")
        .to_string();
    let today_str = fmt(now);

    seed_txn(
        &app.pool,
        card_a,
        Some("Spinning"),
        -3.30,
        "charge",
        &yesterday,
    )
    .await;
    seed_txn(
        &app.pool,
        card_b,
        Some("Spinning"),
        -3.30,
        "charge",
        &five_days,
    )
    .await;
    seed_txn(
        &app.pool,
        card_c,
        Some("Refreshments"),
        -2.0,
        "charge",
        &today_str,
    )
    .await;
    let e_txn = seed_txn(
        &app.pool,
        card_e,
        Some("Spinning"),
        -3.30,
        "charge",
        &today_str,
    )
    .await;
    sqlx::query("UPDATE transactions SET deleted_at = datetime('now') WHERE id = ?")
        .bind(e_txn)
        .execute(&app.pool)
        .await
        .unwrap();
    seed_txn(
        &app.pool,
        card_f,
        Some("Spinning"),
        0.0,
        "visit_pass",
        &today_str,
    )
    .await;
    seed_txn(
        &app.pool,
        card_g,
        Some("Fitness"),
        -5.0,
        "charge",
        &thirty_days,
    )
    .await;
    seed_txn(
        &app.pool,
        card_g,
        Some("Spinning"),
        -3.30,
        "charge",
        &ten_days,
    )
    .await;

    let (status, results) = search(&app, prefix).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(results.len(), 7, "expected 7 LVTEST cards in results");

    let by_id: std::collections::HashMap<i64, &UserResponse> =
        results.iter().map(|r| (r.id, r)).collect();

    let a = by_id[&card_a];
    let b = by_id[&card_b];
    let c = by_id[&card_c];
    let d = by_id[&card_d];
    let e = by_id[&card_e];
    let f = by_id[&card_f];
    let g = by_id[&card_g];

    assert!(a.last_visit_at.is_some(), "A should have last_visit_at");
    assert!(
        a.last_visit_at
            .as_deref()
            .unwrap()
            .starts_with(&yesterday[..10]),
        "A.last_visit_at = {:?}",
        a.last_visit_at
    );
    assert!(b.last_visit_at.is_some(), "B should have last_visit_at");
    assert!(
        b.last_visit_at
            .as_deref()
            .unwrap()
            .starts_with(&five_days[..10]),
        "B.last_visit_at = {:?}",
        b.last_visit_at
    );

    assert_eq!(c.last_visit_at, None, "C (Refreshments only) must be None");
    assert_eq!(d.last_visit_at, None, "D (no txns) must be None");
    assert_eq!(
        e.last_visit_at, None,
        "E (soft-deleted Spinning) must be None"
    );

    assert!(
        f.last_visit_at.is_some(),
        "F (visit_pass amount=0) should count"
    );
    assert!(
        f.last_visit_at
            .as_deref()
            .unwrap()
            .starts_with(&today_str[..10]),
        "F.last_visit_at = {:?}",
        f.last_visit_at
    );

    assert!(g.last_visit_at.is_some(), "G should have last_visit_at");
    let g_str = g.last_visit_at.as_deref().unwrap();
    assert!(
        g_str.starts_with(&ten_days[..10]),
        "G.last_visit_at must be the 10-day-ago Spinning row, got {g_str:?}"
    );
}

#[tokio::test]
async fn search_results_sort_by_last_visit_desc() {
    let app = TestApp::new().await;
    let prefix = "LVSORT";

    let card_today = app
        .seed_card(
            &format!("{prefix}1"),
            0.0,
            Some("OneToday"),
            Some("Z"),
            None,
            None,
        )
        .await;
    let card_yesterday = app
        .seed_card(
            &format!("{prefix}2"),
            0.0,
            Some("TwoYesterday"),
            Some("Z"),
            None,
            None,
        )
        .await;
    let card_ten = app
        .seed_card(
            &format!("{prefix}3"),
            0.0,
            Some("ThreeTen"),
            Some("Z"),
            None,
            None,
        )
        .await;
    let card_never = app
        .seed_card(
            &format!("{prefix}4"),
            0.0,
            Some("FourNever"),
            Some("Z"),
            None,
            None,
        )
        .await;

    let now = chrono::Local::now();
    let today_str = fmt(now);
    let yesterday = (now - chrono::Duration::days(1))
        .format("%Y-%m-%d 12:00:00")
        .to_string();
    let ten_days = (now - chrono::Duration::days(10))
        .format("%Y-%m-%d 12:00:00")
        .to_string();

    seed_txn(
        &app.pool,
        card_today,
        Some("Spinning"),
        -3.30,
        "charge",
        &today_str,
    )
    .await;
    seed_txn(
        &app.pool,
        card_yesterday,
        Some("Spinning"),
        -3.30,
        "charge",
        &yesterday,
    )
    .await;
    seed_txn(
        &app.pool,
        card_ten,
        Some("Spinning"),
        -3.30,
        "charge",
        &ten_days,
    )
    .await;

    let (status, results) = search(&app, prefix).await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let ids: Vec<i64> = results
        .iter()
        .filter(|r| r.card_code.starts_with(prefix))
        .map(|r| r.id)
        .collect();

    assert_eq!(
        ids,
        vec![card_today, card_yesterday, card_ten, card_never],
        "expected sort order: today → yesterday → 10d → never"
    );
}

#[tokio::test]
async fn barcode_prefix_match_overrides_last_visit_sort() {
    let app = TestApp::new().await;

    let card_old = app
        .seed_card("LVPFX99X", 0.0, Some("OldVisit"), Some("Z"), None, None)
        .await;
    let card_new = app
        .seed_card("XY_LVPFX99Z", 0.0, Some("NewVisit"), Some("Z"), None, None)
        .await;

    let now = chrono::Local::now();
    let hundred_days = (now - chrono::Duration::days(100))
        .format("%Y-%m-%d 12:00:00")
        .to_string();
    let today_str = fmt(now);

    seed_txn(
        &app.pool,
        card_old,
        Some("Spinning"),
        -3.30,
        "charge",
        &hundred_days,
    )
    .await;
    seed_txn(
        &app.pool,
        card_new,
        Some("Spinning"),
        -3.30,
        "charge",
        &today_str,
    )
    .await;

    let (status, results) = search(&app, "LVPFX99").await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let lv_results: Vec<i64> = results
        .iter()
        .filter(|r| r.card_code.contains("LVPFX"))
        .map(|r| r.id)
        .collect();

    assert_eq!(
        lv_results.first().copied(),
        Some(card_old),
        "barcode-prefix match (card_old, 100 days ago) must come BEFORE \
         the newer-visit non-prefix-match (card_new). Got {lv_results:?}"
    );
}

#[tokio::test]
async fn customer_role_forbidden() {
    let app = TestApp::new().await;
    let _ = app.seed_card("LVAUTH1", 0.0, None, None, None, None).await;

    let (status, _) = app
        .request(get("/api/users/search?q=LVAUTH", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
