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

#[tokio::test]
async fn range_report_aggregates_across_days_and_rejects_over_93_days() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;

    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, created_at) VALUES \
                 (?1, -5.0, 'charge', datetime('now','-3 days')), \
                 (?1, -5.0, 'charge', datetime('now','-2 days')), \
                 (?1, 20.0, 'topup', datetime('now','-1 days'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let today = chrono::Local::now().date_naive();
    let from = (today - chrono::Duration::days(5))
        .format("%Y-%m-%d")
        .to_string();
    let to = today.format("%Y-%m-%d").to_string();
    let (status, body) = app
        .request(get(
            &format!("/api/reports/range?from={from}&to={to}"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kpi"]["attendance"].as_i64().unwrap(), 2);
    assert_eq!(body["kpi"]["revenue_eur"].as_f64().unwrap(), 10.0);
    assert_eq!(body["kpi"]["cash_in_eur"].as_f64().unwrap(), 20.0);

    // Over-range rejection
    let from_too_far = (today - chrono::Duration::days(120))
        .format("%Y-%m-%d")
        .to_string();
    let (status, _) = app
        .request(get(
            &format!("/api/reports/range?from={from_too_far}&to={to}"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn alerts_expiring_passes_within_7_days_excludes_blocked() {
    let app = TestApp::new().await;

    let card_a: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('EXP-A','Anna','K',10) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, valid_until, created_at) \
                 VALUES (?1, -35.0, 'charge', date('now','+3 days'), datetime('now','-10 days'))",
    )
    .bind(card_a)
    .execute(&app.pool)
    .await
    .unwrap();

    let card_b: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('EXP-B','Bela','M',10) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, valid_until, created_at) \
                 VALUES (?1, -35.0, 'charge', date('now','+30 days'), datetime('now','-10 days'))",
    )
    .bind(card_b)
    .execute(&app.pool)
    .await
    .unwrap();

    let card_c: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit, blocked) VALUES ('EXP-C','Cela','N',10,1) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, valid_until, created_at) \
                 VALUES (?1, -35.0, 'charge', date('now','+2 days'), datetime('now','-10 days'))",
    )
    .bind(card_c)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app
        .request(get("/api/reports/alerts", &app.admin_token))
        .await;
    assert_eq!(status, StatusCode::OK);

    let expiring = body["expiring_passes"].as_array().unwrap();
    let names: Vec<&str> = expiring
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().any(|n| n.contains("Anna")));
    assert!(!names.iter().any(|n| n.contains("Bela")));
    assert!(!names.iter().any(|n| n.contains("Cela")));
}

#[tokio::test]
async fn alerts_low_credit_under_5_and_not_blocked() {
    let app = TestApp::new().await;
    sqlx::query(
        "INSERT INTO cards (barcode, first_name, last_name, credit, blocked) VALUES \
                 ('LC1','Low','One',2.5,0), \
                 ('LC2','Low','Two',4.99,0), \
                 ('LC3','Low','Three',5.00,0), \
                 ('LC4','Low','Four',0.0,1)",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app
        .request(get("/api/reports/alerts", &app.admin_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let low = body["low_credit"].as_array().unwrap();
    let names: Vec<String> = low
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.iter().any(|n| n.contains("Low One")));
    assert!(names.iter().any(|n| n.contains("Low Two")));
    assert!(
        !names.iter().any(|n| n.contains("Low Three")),
        "credit = 5.00 is NOT low"
    );
    assert!(
        !names.iter().any(|n| n.contains("Low Four")),
        "blocked excluded"
    );
}

#[tokio::test]
async fn alerts_inactive_60_days_excludes_zero_credit_and_blocked() {
    let app = TestApp::new().await;
    let inactive_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('INC-IN','Inact','A',20) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-70 days'))")
        .bind(inactive_id).execute(&app.pool).await.unwrap();

    let active_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('INC-AC','Act','B',20) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-5 days'))")
        .bind(active_id).execute(&app.pool).await.unwrap();

    let zero_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('INC-ZC','Zero','C',0) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-100 days'))")
        .bind(zero_id).execute(&app.pool).await.unwrap();

    let (status, body) = app
        .request(get("/api/reports/alerts", &app.admin_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    let inactive = body["inactive"].as_array().unwrap();
    let names: Vec<String> = inactive
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.iter().any(|n| n.contains("Inact")));
    assert!(!names.iter().any(|n| n.contains("Act")));
    assert!(!names.iter().any(|n| n.contains("Zero")));
}
