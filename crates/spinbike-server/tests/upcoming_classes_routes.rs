mod helpers;
use axum::http::StatusCode;
use helpers::{TestApp, get};

#[tokio::test]
async fn upcoming_classes_staff_only() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(get(
            &format!(
                "/api/cards/{}/upcoming-classes?days=14",
                app.customer_card_id
            ),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn upcoming_classes_returns_states() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let (status, resp) = app
        .request(get(
            &format!("/api/cards/{card_id}/upcoming-classes?days=14"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(!arr.is_empty());
    let first = &arr[0];
    assert!(first["state"].is_string());
    // capacity is 19 from V6 seed
    assert_eq!(first["capacity"].as_i64().unwrap(), 19);
}
