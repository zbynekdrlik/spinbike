mod helpers;
use axum::http::StatusCode;
use helpers::{TestApp, delete, get, post_json};

#[tokio::test]
async fn create_and_list_persistent_booking() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let tid: i64 =
        sqlx::query_scalar("SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    let (status, _) = app
        .request(post_json(
            &format!("/api/users/{card_id}/persistent-bookings"),
            &app.staff_token,
            &serde_json::json!({"template_id": tid}),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = app
        .request(get(
            &format!("/api/users/{card_id}/persistent-bookings"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, resp) = app
        .request(get(
            &format!("/api/users/{card_id}/persistent-bookings"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp.as_array().unwrap().len(), 1);
    assert_eq!(resp[0]["template_id"].as_i64().unwrap(), tid);
}

#[tokio::test]
async fn delete_persistent_ends_it_and_removes_future_uncharged() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let tid: i64 =
        sqlx::query_scalar("SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    app.request(post_json(
        &format!("/api/users/{card_id}/persistent-bookings"),
        &app.staff_token,
        &serde_json::json!({"template_id": tid}),
    ))
    .await;
    // Count only FUTURE uncharged persistent bookings — by contract, the
    // DELETE route leaves past (uncharged) bookings alone because they
    // cannot meaningfully be cancelled. Without the future-only filter,
    // this test flakes whenever CI runs after 18:00 local on a Monday
    // (today's seeded 18:00 class is already past).
    let future_count_sql = "SELECT COUNT(*) FROM bookings b \
         JOIN class_templates t ON t.id = b.template_id \
         WHERE b.user_id=? AND b.source='persistent' \
           AND b.cancelled_at IS NULL AND b.charged_at IS NULL \
           AND datetime(b.date || ' ' || t.start_time) > datetime('now')";
    let before: i64 = sqlx::query_scalar(future_count_sql)
        .bind(card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        before >= 1,
        "materialiser must create at least one future booking"
    );

    let (status, _) = app
        .request(delete(
            &format!("/api/users/{card_id}/persistent-bookings/{tid}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let after: i64 = sqlx::query_scalar(future_count_sql)
        .bind(card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(after, 0, "all future persistent bookings must be cancelled");

    let ended: Option<String> = sqlx::query_scalar(
        "SELECT ended_at FROM persistent_bookings WHERE user_id=? AND template_id=?",
    )
    .bind(card_id)
    .bind(tid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(ended.is_some());
}
