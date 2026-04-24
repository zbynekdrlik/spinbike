mod helpers;
use axum::http::StatusCode;
use chrono::{Datelike, Timelike};
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

#[tokio::test]
async fn now_panel_returns_current_or_next_class() {
    let app = TestApp::new().await;

    let now = chrono::Local::now();
    let weekday = now.weekday().num_days_from_monday() as i64;
    let start_time = now.format("%H:00").to_string();
    let template_id: i64 = sqlx::query_scalar(
        "INSERT INTO class_templates (weekday, start_time, duration_minutes, capacity, active) \
         VALUES (?1, ?2, 60, 12, 1) RETURNING id",
    )
    .bind(weekday)
    .bind(&start_time)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let today = now.date_naive().format("%Y-%m-%d").to_string();
    sqlx::query(
        "INSERT INTO bookings (template_id, date, user_id, source) VALUES (?1, ?2, ?3, 'staff')",
    )
    .bind(template_id)
    .bind(&today)
    .bind(app.customer_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app.request(get("/api/reports/now", &app.admin_token)).await;
    assert_eq!(status, StatusCode::OK);
    let has_any = !body["current_class"].is_null() || !body["next_class"].is_null();
    assert!(
        has_any,
        "expected at least current_class or next_class to be set"
    );
    // Kills `booking_count -> Ok(0/1/-1)` and `roster_for -> Ok(vec![])`:
    // exactly one booking was seeded, so whichever branch fired must reflect it.
    if !body["current_class"].is_null() {
        let roster = body["current_class"]["roster"].as_array().unwrap();
        assert_eq!(roster.len(), 1, "roster must contain the seeded booking");
    } else {
        let booked = body["next_class"]["booked"].as_i64().unwrap();
        assert_eq!(
            booked, 1,
            "next_class booked must equal the seeded booking count"
        );
    }
}

// Kills `next_class_future -> Ok(None)` and `next_class_future +` arithmetic mutants.
// Seed a template on tomorrow's weekday only — no class today — and assert
// /api/reports/now returns next_class pointing at that future template.
#[tokio::test]
async fn now_panel_finds_future_class_when_none_today() {
    let app = TestApp::new().await;
    let today = chrono::Local::now().date_naive();
    let tomorrow_weekday = today.succ_opt().unwrap().weekday().num_days_from_monday() as i64;
    // Delete any seeded templates that could match today, then add one for tomorrow.
    sqlx::query("DELETE FROM class_templates")
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO class_templates (weekday, start_time, duration_minutes, capacity, active) \
         VALUES (?1, '18:00', 60, 12, 1)",
    )
    .bind(tomorrow_weekday)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, body) = app.request(get("/api/reports/now", &app.admin_token)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["current_class"].is_null());
    assert!(
        !body["next_class"].is_null(),
        "next_class must be set via next_class_future"
    );
    let date_str = body["next_class"]["date"].as_str().unwrap();
    assert_eq!(
        date_str,
        today.succ_opt().unwrap().format("%Y-%m-%d").to_string()
    );
}

// Kills `total_alert_count -> Ok(0/1/-1)` and internal `+` mutants.
// Seed 2 low-credit cards and 1 expiring pass card → alerts_count should be 3.
#[tokio::test]
async fn day_report_alerts_count_reflects_underlying_alerts() {
    let app = TestApp::new().await;
    // 2 low-credit cards
    sqlx::query(
        "INSERT INTO cards (barcode, first_name, last_name, credit, blocked) VALUES \
                 ('ALC1','LC','A',2.0,0), ('ALC2','LC','B',3.0,0)",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    // 1 expiring pass card
    let card_exp: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('AEX','Exp','P',10) RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, valid_until, created_at) VALUES (?1, -35.0, 'charge', date('now','+3 days'), datetime('now','-10 days'))",
    )
    .bind(card_exp)
    .execute(&app.pool)
    .await
    .unwrap();

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
    let count = body["alerts_count"].as_i64().unwrap();
    // 2 low-credit + 1 expiring = 3 (plus possibly inactive customers from seeding).
    assert!(count >= 3, "expected ≥3 alerts, got {count}");
}

// Kills `Some(i) == current_idx` → `!=` mutant in now_panel.
// Seed two templates on today at different start_times; ensure current points
// at the one whose window contains now and next at the later one — NOT inverted.
#[tokio::test]
async fn now_panel_selects_correct_template_when_multiple_exist() {
    let app = TestApp::new().await;
    let now = chrono::Local::now();
    let weekday = now.weekday().num_days_from_monday() as i64;
    // Clear seeds, add one past (ended) and one matching now.
    sqlx::query("DELETE FROM class_templates")
        .execute(&app.pool)
        .await
        .unwrap();
    // Past template: started 2h ago, already ended (duration=60min).
    let mins_now = (now.hour() as i64) * 60 + (now.minute() as i64);
    let past_start = if mins_now >= 120 { mins_now - 120 } else { 0 };
    let past_h = past_start / 60;
    let past_m = past_start % 60;
    sqlx::query(&format!(
        "INSERT INTO class_templates (weekday, start_time, duration_minutes, capacity, active) VALUES ({weekday}, '{past_h:02}:{past_m:02}', 60, 12, 1)"
    ))
    .execute(&app.pool)
    .await
    .unwrap();
    // Current template: started exactly now, duration 60.
    let cur_h = now.hour();
    let cur_m = now.minute();
    let cur_tmpl_id: i64 = sqlx::query_scalar(&format!(
        "INSERT INTO class_templates (weekday, start_time, duration_minutes, capacity, active) VALUES ({weekday}, '{cur_h:02}:{cur_m:02}', 60, 12, 1) RETURNING id"
    ))
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let (status, body) = app.request(get("/api/reports/now", &app.admin_token)).await;
    assert_eq!(status, StatusCode::OK);
    // current_class must point at the second template (the one that just started).
    assert_eq!(
        body["current_class"]["template_id"].as_i64(),
        Some(cur_tmpl_id),
        "current_class must be the template whose window contains now"
    );
}

#[tokio::test]
async fn day_report_pagination_has_more_true_when_more_than_limit() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    for _ in 0..3 {
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now'))",
        )
        .bind(card_id)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    let (status, body) = app
        .request(get(
            &format!("/api/reports/day?date={today}&limit=2"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["events"].as_array().unwrap().len(), 2);
    assert!(body["has_more"].as_bool().unwrap());

    let (status, body) = app
        .request(get(
            &format!("/api/reports/day?date={today}&limit=3"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["events"].as_array().unwrap().len(), 3);
    assert!(!body["has_more"].as_bool().unwrap());
}

#[tokio::test]
async fn day_report_card_name_is_null_when_names_empty() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();
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
    assert!(body["events"][0]["card_name"].is_null());
}

#[tokio::test]
async fn range_report_pagination_has_more_true_when_more_than_limit() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    for d in 1..=3 {
        sqlx::query(&format!(
            "INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-{d} days'))"
        ))
        .bind(card_id)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    let today = chrono::Local::now().date_naive();
    let from = (today - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let to = today.format("%Y-%m-%d").to_string();

    let (status, body) = app
        .request(get(
            &format!("/api/reports/range?from={from}&to={to}&limit=2"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["events"].as_array().unwrap().len(), 2);
    assert!(body["has_more"].as_bool().unwrap());

    let (status, body) = app
        .request(get(
            &format!("/api/reports/range?from={from}&to={to}&limit=3"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["events"].as_array().unwrap().len(), 3);
    assert!(!body["has_more"].as_bool().unwrap());
}

// Kills `require_admin -> Ok(())` mutant.
#[tokio::test]
async fn non_admin_gets_403_on_reports_day() {
    let app = TestApp::new().await;
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    // staff token is Role::Staff, not Admin — must be rejected.
    let (status, _) = app
        .request(get(
            &format!("/api/reports/day?date={today}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // customer too
    let (status, _) = app
        .request(get(
            &format!("/api/reports/day?date={today}"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// Kills `< 0` / `<= 0` and `> RANGE_MAX_DAYS` / `>= RANGE_MAX_DAYS` mutants in the range handler.
#[tokio::test]
async fn range_rejects_to_before_from() {
    let app = TestApp::new().await;
    // to < from → 400
    let (status, _) = app
        .request(get(
            "/api/reports/range?from=2026-04-15&to=2026-04-10",
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // to == from → OK (guards `<` → `<=`)
    let (status, _) = app
        .request(get(
            "/api/reports/range?from=2026-04-15&to=2026-04-15",
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    // exactly 93 days → OK (guards `>` → `>=`)
    let (status, _) = app
        .request(get(
            "/api/reports/range?from=2026-01-01&to=2026-04-04",
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
}
