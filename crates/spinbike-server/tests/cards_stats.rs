//! Integration tests for GET /api/cards/{id}/stats.

mod helpers;

use chrono::Datelike;
use helpers::{TestApp, get};
use spinbike_core::stats::StatsResponse;

/// Insert a transaction row at a chosen `created_at`. Service is identified
/// by name_en so callers can mix Spinning, Fitness, Refreshments freely.
async fn seed_txn(
    pool: &sqlx::SqlitePool,
    card_id: i64,
    service_name_en: Option<&str>,
    amount: f64,
    action: &str,
    created_at: &str,
) {
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
    sqlx::query(
        "INSERT INTO transactions (card_id, service_id, amount, action, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(card_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_stats(app: &TestApp, card_id: i64) -> (axum::http::StatusCode, StatsResponse) {
    app.request_typed::<StatsResponse>(get(
        &format!("/api/cards/{card_id}/stats"),
        &app.staff_token,
    ))
    .await
}

#[tokio::test]
async fn empty_card_returns_zero_totals_and_twelve_zero_buckets() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("EMPTY", 0.0, None, None, None, None).await;

    let (status, resp) = get_stats(&app, card_id).await;

    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp.totals.this_month.visits, 0);
    assert_eq!(resp.totals.this_month.topped_up_eur, 0.0);
    assert_eq!(resp.totals.this_year.visits, 0);
    assert_eq!(resp.totals.all_time.visits, 0);
    assert_eq!(resp.monthly.len(), 12);
    for b in &resp.monthly {
        assert_eq!(b.visits, 0);
        assert_eq!(b.topped_up_eur, 0.0);
    }
    let now = chrono::Local::now();
    let expected_last = format!("{:04}-{:02}", now.year(), now.month());
    assert_eq!(resp.monthly.last().unwrap().year_month, expected_last);

    // All 12 labels must be well-formed YYYY-MM with month in 1..=12,
    // strictly ascending, and unique. Kills mutants on the year wrap-
    // around (`while month < 1` → `<= 1` or `== 1` would produce
    // labels like "2025-13" or "2026--6").
    let mut prev: Option<(i32, u32)> = None;
    for b in &resp.monthly {
        let parts: Vec<&str> = b.year_month.split('-').collect();
        assert_eq!(parts.len(), 2, "malformed label: {}", b.year_month);
        let y: i32 = parts[0]
            .parse()
            .unwrap_or_else(|_| panic!("bad year in {}", b.year_month));
        let m: u32 = parts[1]
            .parse()
            .unwrap_or_else(|_| panic!("bad month in {}", b.year_month));
        assert!(
            (1..=12).contains(&m),
            "month out of range in {}",
            b.year_month
        );
        if let Some((py, pm)) = prev {
            // Next label must be exactly one calendar month after the previous:
            // either same year + month+1, or January of the next year.
            let valid = (y == py && m == pm + 1) || (y == py + 1 && pm == 12 && m == 1);
            assert!(
                valid,
                "non-sequential labels: {py:04}-{pm:02} → {y:04}-{m:02}"
            );
        }
        prev = Some((y, m));
    }
}

#[tokio::test]
async fn mixed_services_count_only_spinning_and_fitness_as_visits() {
    // Kills "drop the Spinning/Fitness filter" mutant.
    let app = TestApp::new().await;
    let card_id = app.seed_card("MIX1", 0.0, None, None, None, None).await;

    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d %H:%M:%S").to_string();

    seed_txn(
        &app.pool,
        card_id,
        Some("Spinning"),
        -3.30,
        "charge",
        &today,
    )
    .await;
    seed_txn(&app.pool, card_id, Some("Fitness"), -5.00, "charge", &today).await;
    seed_txn(
        &app.pool,
        card_id,
        Some("Refreshments"),
        -2.0,
        "charge",
        &today,
    )
    .await;
    seed_txn(
        &app.pool,
        card_id,
        Some("Supplements"),
        -10.0,
        "charge",
        &today,
    )
    .await;
    seed_txn(
        &app.pool,
        card_id,
        Some("Card activation fee"),
        -1.0,
        "charge",
        &today,
    )
    .await;
    seed_txn(
        &app.pool,
        card_id,
        Some("Monthly pass"),
        -35.0,
        "charge",
        &today,
    )
    .await;

    let (_, resp) = get_stats(&app, card_id).await;

    assert_eq!(resp.totals.this_month.visits, 2);
    assert_eq!(resp.totals.all_time.visits, 2);
}

#[tokio::test]
async fn topup_count_excludes_zero_amount_and_non_topup_actions() {
    // Seeds chosen to kill the most-likely mutants on the topup predicate
    // `action='topup' AND amount > 0`:
    //
    //   * amount=0.0 topup row     → excluded (zero contributes 0 to sum either way)
    //   * amount=7.0 charge row    → excluded by action filter
    //   * amount=-5.0 topup row    → excluded by `> 0`; presence kills several
    //                                comparison-flip mutants (>0 → <0, <=0, ==0, !=0)
    //
    // Note: the `> 0` → `>= 0` mutant is observationally equivalent on a
    // sum-only output (a zero-amount row contributes 0 either way). If
    // cargo-mutants flags it, the proper fix is to expose a `topup_count`
    // field in the response — out of scope for this PR.
    let app = TestApp::new().await;
    let card_id = app.seed_card("MIX2", 0.0, None, None, None, None).await;

    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d %H:%M:%S").to_string();

    seed_txn(&app.pool, card_id, None, 10.0, "topup", &today).await;
    seed_txn(&app.pool, card_id, None, 25.0, "topup", &today).await;
    seed_txn(&app.pool, card_id, None, 0.0, "topup", &today).await;
    seed_txn(&app.pool, card_id, None, -5.0, "topup", &today).await;
    seed_txn(&app.pool, card_id, Some("Spinning"), 7.0, "charge", &today).await;

    let (_, resp) = get_stats(&app, card_id).await;
    assert_eq!(resp.totals.this_month.topped_up_eur, 35.0);
    assert_eq!(resp.totals.all_time.topped_up_eur, 35.0);
}

#[tokio::test]
async fn multi_month_buckets_align_correctly() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("MULTI", 0.0, None, None, None, None).await;

    let now = chrono::Local::now();
    let this_month = now.format("%Y-%m-15 12:00:00").to_string();
    seed_txn(
        &app.pool,
        card_id,
        Some("Spinning"),
        -3.30,
        "charge",
        &this_month,
    )
    .await;
    seed_txn(
        &app.pool,
        card_id,
        Some("Fitness"),
        -5.0,
        "charge",
        &this_month,
    )
    .await;
    let two_months_ago = (now - chrono::Duration::days(63))
        .format("%Y-%m-15 12:00:00")
        .to_string();
    seed_txn(
        &app.pool,
        card_id,
        Some("Spinning"),
        -3.30,
        "charge",
        &two_months_ago,
    )
    .await;

    let (_, resp) = get_stats(&app, card_id).await;
    assert_eq!(resp.monthly.last().unwrap().visits, 2);
    let two_months_label = format!(
        "{:04}-{:02}",
        (now - chrono::Duration::days(63)).year(),
        (now - chrono::Duration::days(63)).month()
    );
    let two_months_bucket = resp
        .monthly
        .iter()
        .find(|b| b.year_month == two_months_label);
    assert_eq!(two_months_bucket.map(|b| b.visits), Some(1));
    assert_eq!(resp.totals.this_year.visits, 3);
    assert_eq!(resp.totals.all_time.visits, 3);
}

#[tokio::test]
async fn visits_older_than_twelve_months_excluded_from_chart_but_in_all_time() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("OLD", 0.0, None, None, None, None).await;

    let eighteen_mo_ago = (chrono::Local::now() - chrono::Duration::days(548))
        .format("%Y-%m-15 12:00:00")
        .to_string();
    seed_txn(
        &app.pool,
        card_id,
        Some("Spinning"),
        -3.30,
        "charge",
        &eighteen_mo_ago,
    )
    .await;

    let (_, resp) = get_stats(&app, card_id).await;
    let total_in_chart: i64 = resp.monthly.iter().map(|b| b.visits).sum();
    assert_eq!(total_in_chart, 0);
    assert_eq!(resp.totals.all_time.visits, 1);
}

#[tokio::test]
async fn soft_deleted_rows_excluded() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("SOFT", 0.0, None, None, None, None).await;
    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    seed_txn(
        &app.pool,
        card_id,
        Some("Spinning"),
        -3.30,
        "charge",
        &now_str,
    )
    .await;
    sqlx::query(
        "UPDATE transactions SET deleted_at = datetime('now')
         WHERE card_id = ? AND service_id = (SELECT id FROM services WHERE name_en='Spinning')",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    seed_txn(&app.pool, card_id, None, 10.0, "topup", &now_str).await;
    sqlx::query(
        "UPDATE transactions SET deleted_at = datetime('now') WHERE action='topup' AND card_id = ?",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let (_, resp) = get_stats(&app, card_id).await;
    assert_eq!(resp.totals.all_time.visits, 0);
    assert_eq!(resp.totals.all_time.topped_up_eur, 0.0);
}

#[tokio::test]
async fn customer_role_forbidden() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("FORBID", 0.0, None, None, None, None).await;

    let (status, _) = app
        .request(get(
            &format!("/api/cards/{card_id}/stats"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
