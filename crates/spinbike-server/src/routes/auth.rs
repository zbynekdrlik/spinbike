use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::{self, AuthUser, parse_role};
use crate::db::users;
use crate::routes::internal_error;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: String,
    pub phone: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", get(me))
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), (StatusCode, Json<serde_json::Value>)> {
    // I4: Input validation.
    let name = body.name.trim();
    if name.is_empty() {
        return Err(super::bad_request("Name must not be empty"));
    }

    if !body.email.contains('@') || !body.email.contains('.') {
        return Err(super::bad_request("Invalid email address"));
    }

    if body.password.len() < 8 {
        return Err(super::bad_request("Password must be at least 8 characters"));
    }

    // Check for duplicate email.
    let existing = users::get_user_by_email(&state.pool, &body.email)
        .await
        .map_err(internal_error)?;

    if existing.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Email already registered"})),
        ));
    }

    let password_hash = auth::hash_password(&body.password).map_err(internal_error)?;

    let user_id = users::create_user(
        &state.pool,
        &body.email,
        Some(&password_hash),
        name,
        body.phone.as_deref(),
        "customer",
        None,
        None,
    )
    .await
    .map_err(internal_error)?;

    let role = spinbike_core::auth::Role::Customer;
    let token = auth::create_token(&state.jwt_secret, user_id, &body.email, &role)
        .map_err(internal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            token,
            user: UserInfo {
                id: user_id,
                email: body.email,
                name: name.to_string(),
                role: "customer".to_string(),
            },
        }),
    ))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user = users::get_user_by_email(&state.pool, &body.email)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Invalid email or password"})),
            )
        })?;

    let password_hash = user.password_hash.as_deref().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Account uses OAuth login"})),
        )
    })?;

    if !auth::verify_password(&body.password, password_hash) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid email or password"})),
        ));
    }

    let role = parse_role(&user.role);
    let token = auth::create_token(&state.jwt_secret, user.id, &user.email, &role)
        .map_err(internal_error)?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            email: user.email,
            name: user.name,
            role: user.role,
        },
    }))
}

async fn me(AuthUser(claims): AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "id": claims.sub,
        "email": claims.email,
        "role": claims.role,
    }))
}
