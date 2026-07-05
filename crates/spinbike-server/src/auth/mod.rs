pub mod oauth;

use anyhow::{Context, Result};
use argon2::{
    Argon2, PasswordHash, PasswordVerifier,
    password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
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

/// Customer sessions are permanent (#108): the magic email link IS the auth,
/// so re-authing every 90 days ("vela neštastnych ludi") is unacceptable. exp =
/// iat + ~100 years. 36500 days ignores leap days — irrelevant for a session
/// meant to never expire in practice.
///
/// Revocation: a permanent JWT is NOT invalidated when a customer is later
/// blocked/deleted (token-leak risk accepted for MVP, per the spec). This is
/// bounded because every security-critical action re-checks `blocked` from the
/// DB at action time (door.rs, payments.rs) and `token-login` re-checks
/// blocked/deleted before issuing a session — so a stale JWT cannot bypass a
/// block for hardware or money.
pub const CUSTOMER_SESSION_SECS: i64 = 100 * 365 * 24 * 60 * 60;
/// Admin/staff keep the original 90-day expiry (they authenticate with a
/// password; a shorter session is the safer default for privileged accounts).
pub const STAFF_SESSION_SECS: i64 = 90 * 24 * 60 * 60;

/// Create a JWT token with a role-based expiry: `Role::Customer` → permanent
/// (~100 years), every other role → 90 days. NB `parse_role` maps any DB role
/// string that isn't `admin`/`staff` to `Role::Customer`, so in practice only
/// admin/staff receive the 90-day tier; the `Role::Unknown` serde fallback only
/// arises from decoding a JWT, never from `parse_role`.
pub fn create_token(secret: &str, user_id: i64, email: &str, role: &Role) -> Result<String> {
    let now = chrono::Utc::now().timestamp();
    let ttl_secs = match role {
        Role::Customer => CUSTOMER_SESSION_SECS,
        _ => STAFF_SESSION_SECS,
    };
    let claims = Claims {
        sub: user_id,
        email: email.to_string(),
        role: role.clone(),
        exp: now + ttl_secs,
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

/// Parse a DB role string into a `Role` for the AUTH/session-TTL path.
///
/// This deliberately differs from the general `Role::from(&str)` boundary
/// conversion in ONE way: an unrecognised DB role collapses to
/// `Role::Customer` (not `Role::Unknown`). That is intentional here — the
/// only consumer is `create_token`, where the role selects the session
/// lifetime, and treating an unknown legacy DB role as a customer gives it the
/// permanent-session tier without any privilege (all `can_*` checks still
/// require an explicit `Admin`/`Staff`). For faithful String↔Role round-trips
/// at wire boundaries (`UserResponse`, `UserInfo`) use `Role::from` instead,
/// which mirrors serde's `#[serde(other)]` and maps unknowns to
/// `Role::Unknown`. The known-role mapping is shared via `Role::from` so the
/// two conversions can never drift on `admin`/`staff`/`customer`.
pub fn parse_role(role_str: &str) -> Role {
    match Role::from(role_str) {
        Role::Unknown => Role::Customer,
        known => known,
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

    /// `parse_role` (auth/session-TTL path) maps the three known roles like
    /// `Role::from`, but collapses any unknown DB role to `Customer` (NOT
    /// `Unknown`) so a legacy/unexpected role still gets a valid, unprivileged
    /// session tier. This intentional divergence from `Role::from` is pinned
    /// here.
    #[test]
    fn parse_role_maps_known_and_collapses_unknown_to_customer() {
        assert_eq!(parse_role("admin"), Role::Admin);
        assert_eq!(parse_role("staff"), Role::Staff);
        assert_eq!(parse_role("customer"), Role::Customer);
        // Divergence from Role::from: unknown → Customer, not Unknown.
        assert_eq!(parse_role("trainer"), Role::Customer);
        assert_eq!(parse_role(""), Role::Customer);
        assert_eq!(Role::from("trainer"), Role::Unknown);
    }

    #[test]
    fn jwt_invalid_secret_fails() {
        let token = create_token("secret1", 1, "a@b.com", &Role::Admin).unwrap();
        let result = validate_token("wrong-secret", &token);
        assert!(result.is_err());
    }

    /// Customer sessions are permanent (#108): exp - iat must be ~100 years,
    /// NOT the old flat 90 days. Pins CUSTOMER_SESSION_SECS so any drift
    /// (e.g. a stray 90-day fallback for customers) fails here.
    #[test]
    fn jwt_expiry_customer_is_100_years() {
        let secret = "expiry-check";
        let token = create_token(secret, 1, "a@b.com", &Role::Customer).unwrap();
        let claims = validate_token(secret, &token).unwrap();
        assert_eq!(claims.exp - claims.iat, CUSTOMER_SESSION_SECS);
        // Sanity band: > 99 years, < 101 years — rules out an accidental
        // 90-day (or arithmetic-swapped) value for customers.
        assert!(claims.exp - claims.iat > 99 * 365 * 24 * 60 * 60);
        assert!(claims.exp - claims.iat < 101 * 365 * 24 * 60 * 60);
    }

    /// Admin/staff keep the 90-day expiry (they still use passwords). Pins
    /// STAFF_SESSION_SECS and guards the customer→100y branch from leaking
    /// into privileged accounts.
    #[test]
    fn jwt_expiry_admin_and_staff_are_90_days() {
        let secret = "expiry-check";
        let ninety: i64 = 90 * 24 * 60 * 60;
        for role in [Role::Admin, Role::Staff] {
            let token = create_token(secret, 1, "a@b.com", &role).unwrap();
            let claims = validate_token(secret, &token).unwrap();
            assert_eq!(
                claims.exp - claims.iat,
                STAFF_SESSION_SECS,
                "role {role:?} must get the 90-day expiry"
            );
            assert_eq!(claims.exp - claims.iat, ninety);
        }
    }
}
