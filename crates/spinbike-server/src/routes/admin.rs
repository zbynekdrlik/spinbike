use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthUser, parse_role};
use crate::db::{classes, settings, users};
use crate::AppState;
use spinbike_core::ws::ServerMsg;

// ---------- Templates ----------

#[derive(Deserialize)]
pub struct CreateTemplateRequest {
    pub weekday: i64,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub capacity: i64,
}

#[derive(Serialize)]
pub struct TemplateResponse {
    pub id: i64,
    pub weekday: i64,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub capacity: i64,
    pub active: bool,
}

// ---------- Cancel class ----------

#[derive(Deserialize)]
pub struct CancelClassRequest {
    pub template_id: i64,
    pub date: String,
    pub reason: Option<String>,
}

// ---------- Instructors ----------

#[derive(Deserialize)]
pub struct CreateInstructorRequest {
    pub name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct InstructorRow {
    pub id: i64,
    pub name: String,
    pub active: i64,
}

// ---------- Services ----------

#[derive(Deserialize)]
pub struct CreateServiceRequest {
    pub name: String,
    pub default_price: f64,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ServiceRow {
    pub id: i64,
    pub name: String,
    pub default_price: f64,
    pub active: i64,
}

// ---------- Settings ----------

#[derive(Deserialize)]
pub struct UpdateSettingRequest {
    pub key: String,
    pub value: String,
}

#[derive(Serialize)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
}

// ---------- Users ----------

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub phone: Option<String>,
    pub role: String,
    pub created_at: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/templates", get(list_templates).post(create_template))
        .route("/api/admin/templates/{id}", delete(delete_template))
        .route("/api/admin/cancel-class", post(cancel_class))
        .route("/api/admin/instructors", get(list_instructors).post(create_instructor))
        .route("/api/admin/services", get(list_services).post(create_service))
        .route("/api/admin/settings", get(get_settings).put(update_setting))
        .route("/api/admin/users", get(list_users_handler))
        .route("/api/admin/users/{id}/role", put(update_user_role))
}

// ---------- Template handlers ----------

async fn list_templates(
    State(state): State<AppState>,
    AuthUser(_claims): AuthUser,
) -> Result<Json<Vec<TemplateResponse>>, (StatusCode, Json<serde_json::Value>)> {
    let templates = classes::list_active_templates(&state.pool).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(
        templates
            .into_iter()
            .map(|t| TemplateResponse {
                id: t.id,
                weekday: t.weekday,
                start_time: t.start_time,
                duration_minutes: t.duration_minutes,
                instructor_id: t.instructor_id,
                capacity: t.capacity,
                active: t.active != 0,
            })
            .collect(),
    ))
}

async fn create_template(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<TemplateResponse>), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_users() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ));
    }

    let id = classes::create_template(
        &state.pool,
        body.weekday,
        &body.start_time,
        body.duration_minutes,
        body.instructor_id,
        body.capacity,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(TemplateResponse {
            id,
            weekday: body.weekday,
            start_time: body.start_time,
            duration_minutes: body.duration_minutes,
            instructor_id: body.instructor_id,
            capacity: body.capacity,
            active: true,
        }),
    ))
}

async fn delete_template(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_users() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ));
    }

    sqlx::query("UPDATE class_templates SET active = 0 WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- Cancel class ----------

async fn cancel_class(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<CancelClassRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_cancel_class() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    classes::cancel_occurrence(
        &state.pool,
        body.template_id,
        &body.date,
        body.reason.as_deref(),
        Some(claims.sub),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    let _ = state.event_tx.send(ServerMsg::ClassCancelled {
        template_id: body.template_id,
        date: body.date,
    });

    Ok(StatusCode::NO_CONTENT)
}

// ---------- Instructor handlers ----------

async fn list_instructors(
    State(state): State<AppState>,
    AuthUser(_claims): AuthUser,
) -> Result<Json<Vec<InstructorRow>>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, InstructorRow>(
        "SELECT id, name, active FROM instructors ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(rows))
}

async fn create_instructor(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<CreateInstructorRequest>,
) -> Result<(StatusCode, Json<InstructorRow>), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_users() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ));
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO instructors (name) VALUES (?) RETURNING id",
    )
    .bind(&body.name)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(InstructorRow {
            id,
            name: body.name,
            active: 1,
        }),
    ))
}

// ---------- Service handlers ----------

async fn list_services(
    State(state): State<AppState>,
    AuthUser(_claims): AuthUser,
) -> Result<Json<Vec<ServiceRow>>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, ServiceRow>(
        "SELECT id, name, default_price, active FROM services ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(rows))
}

async fn create_service(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<CreateServiceRequest>,
) -> Result<(StatusCode, Json<ServiceRow>), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_users() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ));
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO services (name, default_price) VALUES (?, ?) RETURNING id",
    )
    .bind(&body.name)
    .bind(body.default_price)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(ServiceRow {
            id,
            name: body.name,
            default_price: body.default_price,
            active: 1,
        }),
    ))
}

// ---------- Settings handlers ----------

async fn get_settings(
    State(state): State<AppState>,
    AuthUser(_claims): AuthUser,
) -> Result<Json<Vec<SettingRow>>, (StatusCode, Json<serde_json::Value>)> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
            })?;

    Ok(Json(
        rows.into_iter()
            .map(|(key, value)| SettingRow { key, value })
            .collect(),
    ))
}

async fn update_setting(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<UpdateSettingRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_users() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ));
    }

    settings::set_setting(&state.pool, &body.key, &body.value)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- User management handlers ----------

async fn list_users_handler(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<UserResponse>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_users() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ));
    }

    let rows = users::list_users(&state.pool).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(
        rows.into_iter()
            .map(|u| UserResponse {
                id: u.id,
                email: u.email,
                name: u.name,
                phone: u.phone,
                role: u.role,
                created_at: u.created_at,
            })
            .collect(),
    ))
}

async fn update_user_role(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(user_id): Path<i64>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_users() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        ));
    }

    // Validate role string.
    let _role = parse_role(&body.role);

    users::update_user_role(&state.pool, user_id, &body.role)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}
