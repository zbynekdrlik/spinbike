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
        "INSERT INTO transactions (user_id, amount, action, service_id, created_at) \
                 SELECT ?1, -5.0, 'charge', id, datetime('now') FROM services WHERE name_en = ?2 LIMIT 1",
    )
    .bind(card_id)
    .bind(spinbike_core::services::SPINNING_NAME_EN)
    .execute(&app.pool)
    .await
    .unwrap();

    // One top-up of 20 EUR
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, action, created_at) VALUES (?1, 20.0, 'topup', datetime('now'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // One pass sale with valid_until
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, action, valid_until, created_at) VALUES (?1, -35.0, 'charge', date('now','+30 days'), datetime('now'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // One voided charge (should be excluded)
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, action, created_at, deleted_at) VALUES (?1, -5.0, 'charge', datetime('now'), datetime('now'))",
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
        kpi["spinning_visits"].as_i64().unwrap(),
        1,
        "one paid Spinning charge counts as one spinning visit"
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

    // Per #23, only Fitness/Spinning service rows count toward attendance.
    // Look up Spinning's service id and tag both charges with it so the
    // range KPI still asserts attendance == 2 across days.
    let spinning_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?1")
        .bind(spinbike_core::services::SPINNING_NAME_EN)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, created_at) VALUES \
                 (?1, ?2, -5.0, 'charge', datetime('now','-3 days')), \
                 (?1, ?2, -5.0, 'charge', datetime('now','-2 days')), \
                 (?1, NULL, 20.0, 'topup', datetime('now','-1 days'))",
    )
    .bind(card_id)
    .bind(spinning_id)
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
    assert_eq!(body["kpi"]["spinning_visits"].as_i64().unwrap(), 2);
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
async fn day_report_pagination_has_more_true_when_more_than_limit() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    for _ in 0..3 {
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now'))",
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
    // Create a user with an empty name (post-V13: users.name is NOT NULL but
    // empty strings are allowed). The report's card_name field is filtered
    // to None when the name is empty/whitespace (db/reports.rs).
    let nameless_user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (email, name, role) VALUES ('nameless@x', '', 'customer') RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now'))",
    )
    .bind(nameless_user_id)
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
    // Find the nameless user's event in the report (others may exist).
    let events = body["events"].as_array().unwrap();
    let nameless_event = events
        .iter()
        .find(|e| e["amount"].as_f64() == Some(-5.0))
        .expect("nameless user's charge event must be present");
    assert!(
        nameless_event["card_name"].is_null(),
        "card_name should be null for empty-name user, got: {:?}",
        nameless_event["card_name"]
    );
}

// Composite cursor pagination: many transactions sharing an identical
// `created_at` (SQLite has second precision) must NOT be silently dropped
// across page boundaries.
#[tokio::test]
async fn day_report_pagination_does_not_drop_rows_with_duplicate_timestamps() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    // Seed 5 charges all at the same datetime('now').
    sqlx::query(
        "INSERT INTO transactions (user_id, amount, action, created_at) VALUES \
         (?1, -5.0, 'charge', datetime('now')), \
         (?1, -5.0, 'charge', datetime('now')), \
         (?1, -5.0, 'charge', datetime('now')), \
         (?1, -5.0, 'charge', datetime('now')), \
         (?1, -5.0, 'charge', datetime('now'))",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    // Page 1 with limit=2 → 2 rows + has_more=true.
    let (status, body) = app
        .request(get(
            &format!("/api/reports/day?date={today}&limit=2"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let evts1 = body["events"].as_array().unwrap();
    assert_eq!(evts1.len(), 2);
    let last = &evts1[evts1.len() - 1];
    let created_at = last["created_at"].as_str().unwrap();
    let id = last["id"].as_i64().unwrap();
    let cursor = format!("{created_at}|{id}");

    // Page 2 using composite cursor: must return the next 2 rows, NOT empty.
    let (status, body) = app
        .request(get(
            &format!(
                "/api/reports/day?date={today}&limit=2&before={}",
                urlencoding_min(&cursor)
            ),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let evts2 = body["events"].as_array().unwrap();
    assert_eq!(
        evts2.len(),
        2,
        "page 2 must contain remaining rows even with duplicate timestamps"
    );
    // Page 2 ids must be distinct from page 1 ids.
    let p1_ids: Vec<i64> = evts1.iter().map(|e| e["id"].as_i64().unwrap()).collect();
    for e in evts2 {
        let id2 = e["id"].as_i64().unwrap();
        assert!(!p1_ids.contains(&id2), "row {id2} appeared on both pages");
    }
}

fn urlencoding_min(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[tokio::test]
async fn range_report_pagination_has_more_true_when_more_than_limit() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    for d in 1..=3 {
        sqlx::query(&format!(
            "INSERT INTO transactions (user_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-{d} days'))"
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
