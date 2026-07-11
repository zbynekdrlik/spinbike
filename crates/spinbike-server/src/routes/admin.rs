use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::{AdminUser, StaffUser};
use crate::db::{classes, settings, users};
use crate::error::ApiError;
use crate::routes::internal_error;
use spinbike_core::auth::Role;
use spinbike_core::errors::ErrorCode;
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

#[derive(Debug, Deserialize)]
pub struct CreateServiceRequest {
    pub name_sk: String,
    pub name_en: String,
    pub default_price: f64,
    /// Optional. Defaults to "generic". Only "generic" or "monthly_pass" accepted.
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ServiceRow {
    pub id: i64,
    pub kind: String,
    pub name_sk: String,
    pub name_en: String,
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
    pub email: Option<String>,
    pub name: String,
    pub phone: Option<String>,
    /// Typed role; serializes to the same lowercase string as the raw DB role
    /// (wire-compat, #98).
    pub role: Role,
    pub created_at: String,
}

// ---------- Update requests ----------

#[derive(Deserialize)]
pub struct UpdateTemplateRequest {
    pub weekday: Option<i64>,
    pub start_time: Option<String>,
    pub duration_minutes: Option<i64>,
    pub instructor_id: Option<Option<i64>>,
    pub capacity: Option<i64>,
    pub active: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateInstructorRequest {
    pub name: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateServiceRequest {
    pub name_sk: Option<String>,
    pub name_en: Option<String>,
    pub default_price: Option<f64>,
    pub active: Option<bool>,
    // NOTE: `kind` is intentionally absent — it's read-only after create.
}

// ---------- Query params ----------

#[derive(Deserialize)]
pub struct ListTemplatesQuery {
    pub include_inactive: Option<bool>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/templates",
            get(list_templates).post(create_template),
        )
        .route(
            "/api/admin/templates/{id}",
            delete(delete_template).put(update_template),
        )
        .route("/api/admin/cancel-class", post(cancel_class))
        .route(
            "/api/admin/instructors",
            get(list_instructors).post(create_instructor),
        )
        .route("/api/admin/instructors/{id}", put(update_instructor))
        .route(
            "/api/admin/services",
            get(list_services).post(create_service),
        )
        .route("/api/admin/services/{id}", put(update_service))
        .route("/api/admin/settings", get(get_settings).put(update_setting))
        .route("/api/admin/users", get(list_users_handler))
        .route("/api/admin/users/{id}/role", put(update_user_role))
}

// ---------- Template handlers ----------

// I5: list_templates now requires staff role.
async fn list_templates(
    State(state): State<AppState>,
    _: StaffUser,
    Query(query): Query<ListTemplatesQuery>,
) -> Result<Json<Vec<TemplateResponse>>, ApiError> {
    // M3: Support ?include_inactive=true for admin use.
    let templates = if query.include_inactive.unwrap_or(false) {
        classes::list_all_templates(&state.pool)
            .await
            .map_err(internal_error)?
    } else {
        classes::list_active_templates(&state.pool)
            .await
            .map_err(internal_error)?
    };

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
    _: AdminUser,
    Json(body): Json<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<TemplateResponse>), ApiError> {
    let id = classes::create_template(
        &state.pool,
        body.weekday,
        &body.start_time,
        body.duration_minutes,
        body.instructor_id,
        body.capacity,
    )
    .await
    .map_err(internal_error)?;

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
    _: AdminUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    sqlx::query("UPDATE class_templates SET active = 0 WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn update_template(
    State(state): State<AppState>,
    _: AdminUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateTemplateRequest>,
) -> Result<Json<TemplateResponse>, ApiError> {
    // Fetch existing row, merge fields, then do a full UPDATE.
    let existing = sqlx::query_as::<_, classes::ClassTemplateRow>(
        "SELECT * FROM class_templates WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(internal_error)?;

    let weekday = body.weekday.unwrap_or(existing.weekday);
    let start_time = body.start_time.unwrap_or(existing.start_time);
    let duration_minutes = body.duration_minutes.unwrap_or(existing.duration_minutes);
    let instructor_id = body.instructor_id.unwrap_or(existing.instructor_id);
    let capacity = body.capacity.unwrap_or(existing.capacity);
    let active: i64 = body
        .active
        .map(|a| if a { 1 } else { 0 })
        .unwrap_or(existing.active);

    sqlx::query(
        "UPDATE class_templates SET weekday=?, start_time=?, duration_minutes=?, instructor_id=?, capacity=?, active=? WHERE id=?",
    )
    .bind(weekday)
    .bind(&start_time)
    .bind(duration_minutes)
    .bind(instructor_id)
    .bind(capacity)
    .bind(active)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(TemplateResponse {
        id,
        weekday,
        start_time,
        duration_minutes,
        instructor_id,
        capacity,
        active: active != 0,
    }))
}

// ---------- Cancel class ----------

async fn cancel_class(
    State(state): State<AppState>,
    StaffUser(claims): StaffUser,
    Json(body): Json<CancelClassRequest>,
) -> Result<StatusCode, ApiError> {
    classes::cancel_occurrence(
        &state.pool,
        body.template_id,
        &body.date,
        body.reason.as_deref(),
        Some(claims.sub),
    )
    .await
    .map_err(internal_error)?;

    let _ = state.event_tx.send(ServerMsg::ClassCancelled {
        template_id: body.template_id,
        date: body.date,
    });

    Ok(StatusCode::NO_CONTENT)
}

// ---------- Instructor handlers ----------

// I5: list_instructors now requires staff role.
async fn list_instructors(
    State(state): State<AppState>,
    _: StaffUser,
) -> Result<Json<Vec<InstructorRow>>, ApiError> {
    let rows =
        sqlx::query_as::<_, InstructorRow>("SELECT id, name, active FROM instructors ORDER BY id")
            .fetch_all(&state.pool)
            .await
            .map_err(internal_error)?;

    Ok(Json(rows))
}

async fn create_instructor(
    State(state): State<AppState>,
    _: AdminUser,
    Json(body): Json<CreateInstructorRequest>,
) -> Result<(StatusCode, Json<InstructorRow>), ApiError> {
    let id: i64 = sqlx::query_scalar("INSERT INTO instructors (name) VALUES (?) RETURNING id")
        .bind(&body.name)
        .fetch_one(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(InstructorRow {
            id,
            name: body.name,
            active: 1,
        }),
    ))
}

async fn update_instructor(
    State(state): State<AppState>,
    _: AdminUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateInstructorRequest>,
) -> Result<Json<InstructorRow>, ApiError> {
    let existing =
        sqlx::query_as::<_, InstructorRow>("SELECT id, name, active FROM instructors WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(internal_error)?;

    let name = body.name.unwrap_or(existing.name);
    let active: i64 = body
        .active
        .map(|a| if a { 1 } else { 0 })
        .unwrap_or(existing.active);

    sqlx::query("UPDATE instructors SET name=?, active=? WHERE id=?")
        .bind(&name)
        .bind(active)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(InstructorRow { id, name, active }))
}

// ---------- Service handlers ----------

// I5: list_services requires staff role.
async fn list_services(
    State(state): State<AppState>,
    _: StaffUser,
) -> Result<Json<Vec<ServiceRow>>, ApiError> {
    let rows = sqlx::query_as::<_, ServiceRow>(
        "SELECT id, kind, name_sk, name_en, default_price, active FROM services ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(rows))
}

async fn create_service(
    State(state): State<AppState>,
    _: AdminUser,
    Json(body): Json<CreateServiceRequest>,
) -> Result<(StatusCode, Json<ServiceRow>), ApiError> {
    if body.name_sk.trim().is_empty() || body.name_en.trim().is_empty() {
        return Err(super::bad_request("name_sk and name_en are required"));
    }
    let kind = body.kind.as_deref().unwrap_or("generic");
    if !matches!(kind, "generic" | "monthly_pass") {
        return Err(super::bad_request(
            "kind must be 'generic' or 'monthly_pass'",
        ));
    }
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO services (kind, name_sk, name_en, default_price)
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(kind)
    .bind(&body.name_sk)
    .bind(&body.name_en)
    .bind(body.default_price)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        // Partial unique index on kind='monthly_pass' surfaces here. Use the
        // sqlx-native unique-violation detector rather than string-matching
        // the error message so we're robust to SQLite locale / version drift.
        if let sqlx::Error::Database(db_err) = &e
            && db_err.is_unique_violation()
        {
            return ApiError::conflict(ErrorCode::MonthlyPassExists);
        }
        internal_error(e)
    })?;
    Ok((
        StatusCode::CREATED,
        Json(ServiceRow {
            id,
            kind: kind.to_string(),
            name_sk: body.name_sk,
            name_en: body.name_en,
            default_price: body.default_price,
            active: 1,
        }),
    ))
}

async fn update_service(
    State(state): State<AppState>,
    _: AdminUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateServiceRequest>,
) -> Result<Json<ServiceRow>, ApiError> {
    let existing = sqlx::query_as::<_, ServiceRow>(
        "SELECT id, kind, name_sk, name_en, default_price, active FROM services WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?
    .ok_or(ApiError::NotFound(ErrorCode::ServiceNotFound))?;

    let name_sk = body.name_sk.unwrap_or(existing.name_sk);
    let name_en = body.name_en.unwrap_or(existing.name_en);
    let default_price = body.default_price.unwrap_or(existing.default_price);
    let active: i64 = body
        .active
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(existing.active);

    sqlx::query("UPDATE services SET name_sk=?, name_en=?, default_price=?, active=? WHERE id=?")
        .bind(&name_sk)
        .bind(&name_en)
        .bind(default_price)
        .bind(active)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(ServiceRow {
        id,
        kind: existing.kind, // unchanged: kind is read-only after create
        name_sk,
        name_en,
        default_price,
        active,
    }))
}

// ---------- Settings handlers ----------

// #175: staff-only, matching the sibling admin GET handlers
// (list_templates/list_instructors/list_services). The write path
// (update_setting) stays AdminUser -- the read/write asymmetry is
// intentional.
async fn get_settings(
    State(state): State<AppState>,
    _: StaffUser,
) -> Result<Json<Vec<SettingRow>>, ApiError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&state.pool)
            .await
            .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter()
            .map(|(key, value)| SettingRow { key, value })
            .collect(),
    ))
}

async fn update_setting(
    State(state): State<AppState>,
    _: AdminUser,
    Json(body): Json<UpdateSettingRequest>,
) -> Result<StatusCode, ApiError> {
    settings::set_setting(&state.pool, &body.key, &body.value)
        .await
        .map_err(internal_error)?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------- User management handlers ----------

async fn list_users_handler(
    State(state): State<AppState>,
    _: AdminUser,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
    let rows = users::list_users(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter()
            .map(|u| UserResponse {
                id: u.id,
                email: u.email,
                name: u.name,
                phone: u.phone,
                role: Role::from(u.role.as_str()),
                created_at: u.created_at,
            })
            .collect(),
    ))
}

async fn update_user_role(
    State(state): State<AppState>,
    _: AdminUser,
    Path(user_id): Path<i64>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<StatusCode, ApiError> {
    // I6: Validate the role before writing to DB. `Role::from` maps any
    // string that isn't a known lowercase role to `Role::Unknown`, so
    // rejecting `Unknown` is exactly the old `["admin","staff","customer"]`
    // allowlist — now driven by the single typed conversion.
    if Role::from(body.role.as_str()) == Role::Unknown {
        return Err(super::bad_request(
            "Invalid role. Must be admin, staff, or customer",
        ));
    }

    users::update_user_role(&state.pool, user_id, &body.role)
        .await
        .map_err(internal_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire-compat guard (#98): the admin `UserResponse.role` serializes to
    /// the same lowercase string as the previous `String` field.
    #[test]
    fn admin_user_response_serializes_role_to_lowercase() {
        for (role, expected) in [
            (Role::Admin, "admin"),
            (Role::Staff, "staff"),
            (Role::Customer, "customer"),
        ] {
            let resp = UserResponse {
                id: 1,
                email: None,
                name: "N".into(),
                phone: None,
                role,
                created_at: "2026-01-01".into(),
            };
            assert_eq!(serde_json::to_value(&resp).unwrap()["role"], expected);
        }
    }

    /// The role-update validation accepts exactly the three assignable roles
    /// and rejects everything else — matching the old string allowlist.
    #[test]
    fn update_role_validation_accepts_known_rejects_unknown() {
        for good in ["admin", "staff", "customer"] {
            assert_ne!(Role::from(good), Role::Unknown, "{good} must be accepted");
        }
        for bad in ["trainer", "superadmin", "", "Admin", "unknown"] {
            assert_eq!(Role::from(bad), Role::Unknown, "{bad} must be rejected");
        }
    }
}
