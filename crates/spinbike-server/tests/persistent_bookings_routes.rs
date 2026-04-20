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
            &format!("/api/cards/{card_id}/persistent-bookings"),
            &app.staff_token,
            &serde_json::json!({"template_id": tid}),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = app
        .request(get(
            &format!("/api/cards/{card_id}/persistent-bookings"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, resp) = app
        .request(get(
            &format!("/api/cards/{card_id}/persistent-bookings"),
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
        &format!("/api/cards/{card_id}/persistent-bookings"),
        &app.staff_token,
        &serde_json::json!({"template_id": tid}),
    ))
    .await;
    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE card_id=? AND source='persistent' AND cancelled_at IS NULL AND charged_at IS NULL"
    ).bind(card_id).fetch_one(&app.pool).await.unwrap();
    assert!(before >= 1);

    let (status, _) = app
        .request(delete(
            &format!("/api/cards/{card_id}/persistent-bookings/{tid}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE card_id=? AND source='persistent' AND cancelled_at IS NULL AND charged_at IS NULL"
    ).bind(card_id).fetch_one(&app.pool).await.unwrap();
    assert_eq!(after, 0);

    let ended: Option<String> = sqlx::query_scalar(
        "SELECT ended_at FROM persistent_bookings WHERE card_id=? AND template_id=?",
    )
    .bind(card_id)
    .bind(tid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(ended.is_some());
}
