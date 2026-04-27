//! Integration tests for /api/admin/* handlers.

mod helpers;

use helpers::{TestApp, delete, get, post_json, put_json};

// ---------- Templates ----------

#[tokio::test]
async fn list_templates_forbidden_for_customer() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(get("/api/admin/templates", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn create_and_list_templates() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "weekday": 0,
        "start_time": "17:00",
        "duration_minutes": 60,
        "instructor_id": null,
        "capacity": 10,
    });
    let (status, resp) = app
        .request(post_json("/api/admin/templates", &app.admin_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let tid = resp["id"].as_i64().unwrap();

    let (status, resp) = app
        .request(get("/api/admin/templates", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(arr.iter().any(|t| t["id"].as_i64() == Some(tid)));
}

#[tokio::test]
async fn delete_template_soft_deactivates() {
    let app = TestApp::new().await;
    // Create first.
    let body = serde_json::json!({
        "weekday": 1,
        "start_time": "18:00",
        "duration_minutes": 45,
        "capacity": 5,
    });
    let (_, resp) = app
        .request(post_json("/api/admin/templates", &app.admin_token, &body))
        .await;
    let tid = resp["id"].as_i64().unwrap();

    let (status, _) = app
        .request(delete(
            &format!("/api/admin/templates/{tid}"),
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    // Template should be inactive (excluded from default list).
    let (_, resp) = app
        .request(get("/api/admin/templates", &app.admin_token))
        .await;
    assert!(
        !resp
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"].as_i64() == Some(tid))
    );
}

#[tokio::test]
async fn delete_template_forbidden_for_staff() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(delete("/api/admin/templates/1", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_template_forbidden_for_staff() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "capacity": 50 });
    let (status, _) = app
        .request(put_json("/api/admin/templates/1", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_template_applies_changes() {
    let app = TestApp::new().await;
    let create = serde_json::json!({
        "weekday": 2,
        "start_time": "17:00",
        "duration_minutes": 60,
        "capacity": 10,
    });
    let (_, resp) = app
        .request(post_json("/api/admin/templates", &app.admin_token, &create))
        .await;
    let tid = resp["id"].as_i64().unwrap();

    let update = serde_json::json!({ "capacity": 25, "start_time": "19:30" });
    let (status, resp) = app
        .request(put_json(
            &format!("/api/admin/templates/{tid}"),
            &app.admin_token,
            &update,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp["capacity"].as_i64().unwrap(), 25);
    assert_eq!(resp["start_time"].as_str().unwrap(), "19:30");
    // Unchanged field round-trips correctly.
    assert_eq!(resp["duration_minutes"].as_i64().unwrap(), 60);
    // Active must stay true (default). Kills the `active != 0` → `active == 0`
    // mutant that would flip the boolean in the response.
    assert!(resp["active"].as_bool().unwrap());
}

#[tokio::test]
async fn list_templates_include_inactive_returns_soft_deleted() {
    // Kills the `list_all_templates -> Ok(vec![])` mutant: the include_inactive
    // path must still return soft-deleted templates.
    let app = TestApp::new().await;
    let create = serde_json::json!({
        "weekday": 3,
        "start_time": "07:00",
        "duration_minutes": 60,
        "capacity": 4,
    });
    let (_, resp) = app
        .request(post_json("/api/admin/templates", &app.admin_token, &create))
        .await;
    let tid = resp["id"].as_i64().unwrap();

    // Soft-delete (sets active=0).
    let _ = app
        .request(delete(
            &format!("/api/admin/templates/{tid}"),
            &app.admin_token,
        ))
        .await;

    // Default listing omits it.
    let (_, resp) = app
        .request(get("/api/admin/templates", &app.admin_token))
        .await;
    assert!(
        !resp
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"].as_i64() == Some(tid))
    );

    // include_inactive=true must still return it — this is the path that calls
    // `list_all_templates` (otherwise we hit `list_active_templates`).
    let (status, resp) = app
        .request(get(
            "/api/admin/templates?include_inactive=true",
            &app.admin_token,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let row = resp
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"].as_i64() == Some(tid))
        .expect("soft-deleted template must appear when include_inactive=true");
    assert!(!row["active"].as_bool().unwrap());
}

// ---------- Cancel class ----------

#[tokio::test]
async fn cancel_class_persists_cancellation() {
    let app = TestApp::new().await;
    let create = serde_json::json!({
        "weekday": 0,
        "start_time": "10:00",
        "duration_minutes": 60,
        "capacity": 10,
    });
    let (_, resp) = app
        .request(post_json("/api/admin/templates", &app.admin_token, &create))
        .await;
    let tid = resp["id"].as_i64().unwrap();

    let cancel = serde_json::json!({
        "template_id": tid,
        "date": "2026-04-13",
        "reason": "instructor sick",
    });
    let (status, _) = app
        .request(post_json(
            "/api/admin/cancel-class",
            &app.staff_token,
            &cancel,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    // Verify the cancellation row exists in the DB.
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM class_cancellations WHERE template_id = ?")
            .bind(tid)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn cancel_class_forbidden_for_customer() {
    let app = TestApp::new().await;
    let body = serde_json::json!({
        "template_id": 1,
        "date": "2026-04-13",
    });
    let (status, _) = app
        .request(post_json(
            "/api/admin/cancel-class",
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

// ---------- Instructors ----------

#[tokio::test]
async fn instructors_crud_smoke() {
    let app = TestApp::new().await;

    // Customer forbidden from listing.
    let (status, _) = app
        .request(get("/api/admin/instructors", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);

    // Admin can create.
    let body = serde_json::json!({ "name": "Judita" });
    let (status, resp) = app
        .request(post_json("/api/admin/instructors", &app.admin_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let iid = resp["id"].as_i64().unwrap();
    assert_eq!(resp["name"].as_str().unwrap(), "Judita");

    // Staff can list (sees the new one).
    let (status, resp) = app
        .request(get("/api/admin/instructors", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        resp.as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"].as_i64() == Some(iid))
    );

    // Staff cannot update (admin-only).
    let upd = serde_json::json!({ "name": "Renamed" });
    let (status, _) = app
        .request(put_json(
            &format!("/api/admin/instructors/{iid}"),
            &app.staff_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);

    // Admin can update.
    let (status, resp) = app
        .request(put_json(
            &format!("/api/admin/instructors/{iid}"),
            &app.admin_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp["name"].as_str().unwrap(), "Renamed");
}

// ---------- Services ----------

#[tokio::test]
async fn services_list_and_update() {
    let app = TestApp::new().await;

    // Customer forbidden from listing.
    let (status, _) = app
        .request(get("/api/admin/services", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);

    // Staff can list. Seed data guarantees at least the two default services.
    let (status, resp) = app
        .request(get("/api/admin/services", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(resp.as_array().unwrap().len() >= 2);
    let sid = resp.as_array().unwrap()[0]["id"].as_i64().unwrap();

    // Staff cannot update.
    let upd = serde_json::json!({ "default_price": 999.99 });
    let (status, _) = app
        .request(put_json(
            &format!("/api/admin/services/{sid}"),
            &app.staff_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);

    // Admin can update the price.
    let (status, resp) = app
        .request(put_json(
            &format!("/api/admin/services/{sid}"),
            &app.admin_token,
            &upd,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp["default_price"].as_f64().unwrap(), 999.99);
}

// ---------- Settings ----------

#[tokio::test]
async fn settings_get_returns_seeded_rows() {
    let app = TestApp::new().await;
    let (status, resp) = app
        .request(get("/api/admin/settings", &app.admin_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(
        !arr.is_empty(),
        "migrations seed at least one setting (bike_count)"
    );
}

#[tokio::test]
async fn settings_update_persists() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "key": "bike_count", "value": "42" });
    let (status, _) = app
        .request(put_json("/api/admin/settings", &app.admin_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let (_, resp) = app
        .request(get("/api/admin/settings", &app.admin_token))
        .await;
    let bike_count = resp
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"].as_str() == Some("bike_count"))
        .unwrap();
    assert_eq!(bike_count["value"].as_str().unwrap(), "42");
}

#[tokio::test]
async fn settings_update_forbidden_for_staff() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "key": "bike_count", "value": "1" });
    let (status, _) = app
        .request(put_json("/api/admin/settings", &app.staff_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

// ---------- Users ----------

#[tokio::test]
async fn list_users_forbidden_for_staff() {
    let app = TestApp::new().await;
    let (status, _) = app.request(get("/api/admin/users", &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_users_returns_all_for_admin() {
    let app = TestApp::new().await;
    let (status, resp) = app.request(get("/api/admin/users", &app.admin_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    // We seeded 3 users (admin, staff, customer).
    assert_eq!(resp.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn update_user_role_accepts_valid_and_persists() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "role": "staff" });
    let (status, _) = app
        .request(put_json(
            &format!("/api/admin/users/{}/role", app.customer_id),
            &app.admin_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let persisted: String = sqlx::query_scalar("SELECT role FROM users WHERE id = ?")
        .bind(app.customer_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(persisted, "staff");
}

#[tokio::test]
async fn update_user_role_rejects_invalid_role() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "role": "wizard" });
    let (status, _) = app
        .request(put_json(
            &format!("/api/admin/users/{}/role", app.customer_id),
            &app.admin_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

// ---------- Services (dual-language + kind) ----------

#[tokio::test]
async fn create_and_list_services_with_dual_language() {
    let app = TestApp::new().await;

    // Create a new service.
    let body = serde_json::json!({
        "name_sk": "Voda",
        "name_en": "Water",
        "default_price": 1.0,
    });
    let (status, row) = app
        .request(post_json("/api/admin/services", &app.admin_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert_eq!(row["kind"], "generic");
    assert_eq!(row["name_sk"], "Voda");
    assert_eq!(row["name_en"], "Water");
    assert_eq!(row["default_price"], 1.0);

    // List includes seeded rows from V8.
    let (status, rows) = app
        .request(get("/api/admin/services", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = rows.as_array().unwrap();
    assert!(arr.iter().any(|r| r["kind"] == "monthly_pass"));
    assert!(arr.iter().any(|r| r["name_sk"] == "Občerstvenie"));
    assert!(arr.iter().any(|r| r["name_sk"] == "Doplnky výživy"));
    assert!(arr.iter().any(|r| r["name_sk"] == "Aktivácia karty"));
}

#[tokio::test]
async fn put_service_cannot_change_kind() {
    let app = TestApp::new().await;

    let pass_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
        .fetch_one(&app.pool)
        .await
        .unwrap();

    let body = serde_json::json!({ "name_sk": "Renamed", "kind": "generic" });
    let (status, row) = app
        .request(put_json(
            &format!("/api/admin/services/{pass_id}"),
            &app.admin_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(row["kind"], "monthly_pass", "kind must remain monthly_pass");
    assert_eq!(row["name_sk"], "Renamed");
}

#[tokio::test]
async fn create_second_monthly_pass_rejected() {
    let app = TestApp::new().await;

    let body = serde_json::json!({
        "name_sk": "Druhý",
        "name_en": "Second",
        "default_price": 35.0,
        "kind": "monthly_pass",
    });
    let (status, _) = app
        .request(post_json("/api/admin/services", &app.admin_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn create_service_with_invalid_kind_rejected() {
    let app = TestApp::new().await;

    let body = serde_json::json!({
        "name_sk": "X",
        "name_en": "Y",
        "default_price": 1.0,
        "kind": "foobar",
    });
    let (status, _) = app
        .request(post_json("/api/admin/services", &app.admin_token, &body))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_user_role_forbidden_for_staff() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "role": "admin" });
    let (status, _) = app
        .request(put_json(
            &format!("/api/admin/users/{}/role", app.customer_id),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
