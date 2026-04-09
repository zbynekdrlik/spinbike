pub mod oauth;

use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{StatusCode, request::Parts},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use spinbike_core::auth::{Claims, Role};

use crate::AppState;

/// Hash a password using Argon2id.
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {e}"))?;
    Ok(hash.to_string())
}

/// Verify a password against a stored hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Create a JWT token with 7-day expiry.
pub fn create_token(secret: &str, user_id: i64, email: &str, role: &Role) -> Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        email: email.to_string(),
        role: role.clone(),
        exp: now + 7 * 24 * 60 * 60,
        iat: now,
    };
    let token = jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("Failed to create JWT")?;
    Ok(token)
}

/// Validate a JWT token and return the claims.
pub fn validate_token(secret: &str, token: &str) -> Result<Claims> {
    let data = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .context("Invalid token")?;
    Ok(data.claims)
}

/// Parse a role string from the DB into a Role enum.
pub fn parse_role(role_str: &str) -> Role {
    match role_str {
        "admin" => Role::Admin,
        "staff" => Role::Staff,
        _ => Role::Customer,
    }
}

/// Axum extractor that reads a JWT from the Authorization header.
/// Extracts authenticated user claims.
pub struct AuthUser(pub Claims);

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({"error": "Missing authorization header"})),
                )
            })?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({"error": "Invalid authorization format"})),
            )
        })?;

        let claims = validate_token(&app_state.jwt_secret, token).map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({"error": "Invalid or expired token"})),
            )
        })?;

        Ok(AuthUser(claims))
    }
}

/// Optional auth extractor — returns None if no valid token is present.
pub struct OptionalAuthUser(pub Option<Claims>);

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let claims = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .and_then(|token| validate_token(&app_state.jwt_secret, token).ok());

        Ok(OptionalAuthUser(claims))
    }
}

// AppState: Clone automatically implements FromRef<AppState> for AppState via blanket impl.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_and_verify() {
        let password = "mysecretpassword";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash));
        assert!(!verify_password("wrongpassword", &hash));
    }

    #[test]
    fn jwt_create_and_validate() {
        let secret = "test-secret-key-12345";
        let token = create_token(secret, 42, "test@example.com", &Role::Customer).unwrap();
        let claims = validate_token(secret, &token).unwrap();
        assert_eq!(claims.sub, 42);
        assert_eq!(claims.email, "test@example.com");
        assert_eq!(claims.role, Role::Customer);
    }

    #[test]
    fn jwt_invalid_secret_fails() {
        let token = create_token("secret1", 1, "a@b.com", &Role::Admin).unwrap();
        let result = validate_token("wrong-secret", &token);
        assert!(result.is_err());
    }
}
