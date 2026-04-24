mod helpers;
use axum::http::StatusCode;
use helpers::{TestApp, get};

#[tokio::test]
async fn day_report_aggregates_charges_topups_passes_and_excludes_voided() {
    let app = TestApp::new().await;

    // Seed: card for the existing customer
    let card_id = app.customer_card_id;

    // One charge of 5 EUR (amount = -5)
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, service_id, created_at) \
                 SELECT ?1, -5.0, 'charge', id, datetime('now') FROM services WHERE name = 'Spinning' LIMIT 1",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // One top-up of 20 EUR
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, 20.0, 'topup', datetime('now'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // One pass sale with valid_until
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, valid_until, created_at) VALUES (?1, -35.0, 'charge', date('now','+30 days'), datetime('now'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // One voided charge (should be excluded)
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, created_at, deleted_at) VALUES (?1, -5.0, 'charge', datetime('now'), datetime('now'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // Call /api/reports/day for today
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let (status, body) = app
        .request(get(
            &format!("/api/reports/day?date={today}"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    let kpi = &body["kpi"];
    assert_eq!(
        kpi["revenue_eur"].as_f64().unwrap(),
        40.0,
        "5 charge + 35 pass = 40 revenue"
    );
    assert_eq!(
        kpi["attendance"].as_i64().unwrap(),
        1,
        "only one regular charge counts as a visit"
    );
    assert_eq!(kpi["passes_sold"].as_i64().unwrap(), 1);
    assert_eq!(kpi["cash_in_eur"].as_f64().unwrap(), 20.0);

    assert_eq!(
        body["events"].as_array().unwrap().len(),
        3,
        "voided excluded"
    );
}
