use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::AppState;
use crate::auth::{self, AuthUser, parse_role};
use crate::db::{login_tokens, users};
use crate::error::ApiError;
use crate::rate_limit::{RateLimitConfig, SlidingWindowLimiter};
use crate::routes::internal_error;
use spinbike_core::auth::Role;
use spinbike_core::errors::ErrorCode;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct RequestLoginLinkRequest {
    pub email: String,
}

#[derive(Deserialize)]
pub struct TokenLoginRequest {
    pub token: String,
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
    /// Typed role. Serializes to the same lowercase string as the raw DB role
    /// (wire-compat, #98) — this is the payload the frontend stores in
    /// localStorage, so the stored shape is unchanged.
    pub role: Role,
}

// ---------------------------------------------------------------------------
// Rate limiter for /api/auth/request-login-link
// ---------------------------------------------------------------------------

/// In-memory rate-limit for the public login-link endpoint. Keyed by email
/// (String) — distinct from the door route's `i64`-keyed `RateLimiter`. Two
/// caps: at most one send per email per 60 s, and a global 10-per-60 s ceiling
/// across all emails (anti-spam / anti-enumeration-amplification). A thin typed
/// wrapper over the shared `SlidingWindowLimiter` (#166) — the same abstraction
/// backs the door limiter.
///
/// The degenerate "single last-Instant" shape is expressed as `per_key_max =
/// None` (min-gap alone throttles, no per-key cap reason is ever emitted). The
/// attacker-controllable email key would otherwise grow the map unbounded, so
/// `key_memory` (120 s) is set WIDER than the 60 s decision window: an entry
/// between 60 s and 120 s old no longer throttles but is still kept, keeping the
/// too-fast decision boundary observable, and the key is evicted past 120 s.
/// Stored as `Arc<Mutex<_>>` on `AppState` so concurrent integration tests get
/// isolated windows.
pub struct LoginLinkRateLimiter(SlidingWindowLimiter<String>);

impl LoginLinkRateLimiter {
    pub fn new() -> Self {
        Self(SlidingWindowLimiter::new(RateLimitConfig {
            per_key_window: Duration::from_secs(60),
            per_key_min_gap: Some(Duration::from_secs(60)),
            per_key_max: None,
            per_key_cap_reason: "",
            key_memory: Duration::from_secs(120),
            global_window: Duration::from_secs(60),
            global_max: 10,
        }))
    }

    /// Returns Ok and records the send if allowed; Err(reason) otherwise
    /// ("too_fast" / "global_cap").
    pub fn check_and_record(&mut self, email: &str) -> Result<(), &'static str> {
        self.0.check_and_record(email.to_string())
    }

    /// Testable variant taking the current `Instant` so tests need not sleep.
    pub fn check_and_record_at(&mut self, email: &str, now: Instant) -> Result<(), &'static str> {
        self.0.check_and_record_at(email.to_string(), now)
    }

    /// Number of tracked emails — for the map-bounding tests.
    pub fn tracked_keys(&self) -> usize {
        self.0.tracked_keys()
    }
}

impl Default for LoginLinkRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/request-login-link", post(request_login_link))
        .route("/api/auth/token-login", post(token_login))
        .route("/api/auth/me", get(me))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let user = users::get_user_by_email(&state.pool, &body.email)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::Unauthorized(ErrorCode::InvalidCredentials))?;

    let password_hash = user
        .password_hash
        .as_deref()
        .ok_or(ApiError::Unauthorized(ErrorCode::OauthAccount))?;

    if !auth::verify_password(&body.password, password_hash) {
        return Err(ApiError::Unauthorized(ErrorCode::InvalidCredentials));
    }

    let role = parse_role(&user.role);
    let email_str = user.email.as_deref().unwrap_or("");
    let token =
        auth::create_token(&state.jwt_secret, user.id, email_str, &role).map_err(internal_error)?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            email: user.email.unwrap_or_default(),
            name: user.name,
            role: Role::from(user.role.as_str()),
        },
    }))
}

/// Compose the unaccented-Slovak login-link email. Returns (subject, text, html).
fn login_link_email(link: &str) -> (String, String, String) {
    let subject = "SpinBike - prihlasovaci odkaz".to_string();
    let text = format!(
        "Ahoj,\n\nklikni na tento odkaz a prihlasis sa do SpinBike:\n{link}\n\n\
         Odkaz plati 24 hodin. Ak si o prihlasenie nepoziadal, tento email ignoruj."
    );
    let html = format!(
        "<p>Ahoj,</p><p>klikni na odkaz a prihlasis sa do SpinBike:</p>\
         <p><a href=\"{link}\">Prihlasit sa</a></p>\
         <p>Odkaz plati 24 hodin. Ak si o prihlasenie nepoziadal, tento email ignoruj.</p>"
    );
    (subject, text, html)
}

/// Public passwordless-recovery endpoint. ALWAYS returns 200 `{"status":"ok"}`
/// regardless of whether the email exists (no user enumeration). A login-link
/// email is actually sent ONLY when the address belongs to an existing,
/// non-blocked, role=customer account — and only when the rate limiter allows.
async fn request_login_link(
    State(state): State<AppState>,
    Json(body): Json<RequestLoginLinkRequest>,
) -> Json<serde_json::Value> {
    let ok = || Json(serde_json::json!({"status": "ok"}));
    let email = body.email.trim().to_string();
    if email.is_empty() {
        return ok();
    }

    // Look up the account. Any miss / error → uniform 200 (never leak which).
    let user = match users::get_user_by_email(&state.pool, &email).await {
        Ok(Some(u)) => u,
        Ok(None) => return ok(),
        Err(e) => {
            tracing::error!(error = %e, "request-login-link: user lookup failed");
            return ok();
        }
    };

    // Magic link is customers-only; blocked accounts get nothing. (deleted_at
    // is already filtered by get_user_by_email.) `Role::from` (not `parse_role`)
    // so a non-customer/unknown role is NOT treated as a customer — an unknown
    // legacy role must not be able to request a login link.
    if Role::from(user.role.as_str()) != Role::Customer || user.blocked {
        return ok();
    }

    // Rate limit (per-email 60 s + global 10/min). Throttled → still 200 ok,
    // no send.
    if let Err(reason) = state
        .login_link_rate_limit
        .lock()
        .expect("login-link rate-limiter mutex poisoned")
        .check_and_record(&email)
    {
        tracing::warn!(%reason, "request-login-link: throttled");
        return ok();
    }

    let raw = match login_tokens::create_token(
        &state.pool,
        user.id,
        login_tokens::PURPOSE_LOGIN,
        login_tokens::LOGIN_TTL_SECS,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, user_id = user.id, "request-login-link: token create failed");
            return ok();
        }
    };

    let link = format!(
        "{}/welcome?t={}",
        state.public_base_url.trim_end_matches('/'),
        raw
    );
    let (subject, text, html) = login_link_email(&link);

    // Fire the SMTP send OFF the response path. The relay dial can take up to
    // 10 s (mail module timeout); awaiting it here would make an existing
    // customer's response measurably slower than a non-customer's fast return —
    // a timing side-channel that partially undermines the no-enumeration
    // property. The token row is already committed synchronously above (so the
    // capability is durable); the delivery is best-effort and only logged.
    let mail = state.mail.clone();
    let user_id = user.id;
    tokio::spawn(async move {
        if let Err(e) = mail.send(&email, &subject, &text, &html).await {
            tracing::warn!(error = %e, user_id, "request-login-link: mail send failed");
        } else {
            tracing::info!(user_id, "request-login-link: sent");
        }
    });

    ok()
}

/// Redeem a magic-link token (invite OR login) and return a JWT session. All
/// failure paths return a single uniform 401 (no enumeration). Blocked/deleted
/// users are rejected even with an otherwise-valid token.
async fn token_login(
    State(state): State<AppState>,
    Json(body): Json<TokenLoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let invalid = || ApiError::Unauthorized(ErrorCode::InvalidOrExpiredLink);

    // Both purposes authorize login: an 'invite' token logs a client in the
    // first time; a 'login' token is the recovery path.
    let user_id = login_tokens::redeem(
        &state.pool,
        &body.token,
        &[login_tokens::PURPOSE_INVITE, login_tokens::PURPOSE_LOGIN],
    )
    .await
    .map_err(internal_error)?
    .ok_or_else(invalid)?;

    let user = users::get_user_by_id(&state.pool, user_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(invalid)?;

    if user.deleted_at.is_some() || user.blocked {
        tracing::warn!(user_id, "token-login: rejected — blocked or deleted user");
        return Err(invalid());
    }

    let role = parse_role(&user.role);
    let email_str = user.email.as_deref().unwrap_or("");
    let token =
        auth::create_token(&state.jwt_secret, user.id, email_str, &role).map_err(internal_error)?;

    tracing::info!(user_id = user.id, "token-login: session issued");
    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            email: user.email.unwrap_or_default(),
            name: user.name,
            role: Role::from(user.role.as_str()),
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

#[cfg(test)]
mod tests {
    use super::LoginLinkRateLimiter;
    use std::time::{Duration, Instant};

    /// Wire-compat guard (#98): the `AuthResponse.user.role` (the payload the
    /// frontend persists in localStorage) serializes to the same lowercase
    /// string as the previous `String` field, so stored sessions are unchanged.
    #[test]
    fn user_info_serializes_role_to_lowercase_string() {
        use super::UserInfo;
        use spinbike_core::auth::Role;
        for (role, expected) in [
            (Role::Admin, "admin"),
            (Role::Staff, "staff"),
            (Role::Customer, "customer"),
        ] {
            let ui = UserInfo {
                id: 1,
                email: "a@b.com".into(),
                name: "N".into(),
                role,
            };
            assert_eq!(serde_json::to_value(&ui).unwrap()["role"], expected);
        }
    }

    #[test]
    fn login_link_first_send_allowed() {
        let mut rl = LoginLinkRateLimiter::new();
        assert!(rl.check_and_record("a@x.com").is_ok());
    }

    #[test]
    fn login_link_second_within_60s_throttled() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at("a@x.com", t0).unwrap();
        assert_eq!(
            rl.check_and_record_at("a@x.com", t0 + Duration::from_secs(30)),
            Err("too_fast"),
            "a second send within 60 s must be throttled"
        );
    }

    #[test]
    fn login_link_allowed_at_exactly_60s_boundary() {
        // Check is `< 60s`; at exactly 60 s the second send MUST be allowed.
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at("a@x.com", t0).unwrap();
        assert!(
            rl.check_and_record_at("a@x.com", t0 + Duration::from_secs(60))
                .is_ok(),
            "send exactly 60 s later must be allowed"
        );
    }

    #[test]
    fn login_link_just_under_60s_throttled() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at("a@x.com", t0).unwrap();
        assert_eq!(
            rl.check_and_record_at("a@x.com", t0 + Duration::from_millis(59_999)),
            Err("too_fast")
        );
    }

    #[test]
    fn login_link_distinct_emails_independent() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at("a@x.com", t0).unwrap();
        rl.check_and_record_at("b@x.com", t0)
            .expect("a different email must not be throttled by the first");
    }

    #[test]
    fn login_link_global_cap_at_11th() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        for i in 0..10 {
            let email = format!("u{i}@x.com");
            rl.check_and_record_at(&email, t0 + Duration::from_millis(i as u64))
                .unwrap_or_else(|e| panic!("send #{i} should succeed, got {e}"));
        }
        assert_eq!(
            rl.check_and_record_at("u10@x.com", t0 + Duration::from_millis(100)),
            Err("global_cap"),
            "11th distinct email inside the 60 s window must hit global_cap"
        );
    }

    #[test]
    fn login_link_global_window_prunes_after_60s() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        for i in 0..10 {
            let email = format!("u{i}@x.com");
            rl.check_and_record_at(&email, t0 + Duration::from_millis(i as u64))
                .unwrap();
        }
        // After the 60 s window slides, the global counter clears.
        assert!(
            rl.check_and_record_at("late@x.com", t0 + Duration::from_secs(90))
                .is_ok(),
            "global cap must clear after the 60 s window slides"
        );
    }

    /// Global prune at EXACTLY 60 s uses strict `>`, so a 60-s-old entry is
    /// KEPT (elapsed of 60 s is not > 60 s). Catches the `>` → `>=` mutation on
    /// the global-window prune.
    #[test]
    fn login_link_global_keeps_entry_at_exactly_60s() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        for i in 0..10 {
            rl.check_and_record_at(&format!("u{i}@x.com"), t0).unwrap();
        }
        // At t0 + exactly 60 s the 10 entries are exactly 60 s old → strict `>`
        // keeps them → the deque is still full → 11th hits global_cap.
        assert_eq!(
            rl.check_and_record_at("late@x.com", t0 + Duration::from_secs(60)),
            Err("global_cap"),
            "at exactly 60 s the strict `>` prune must keep global entries"
        );
    }

    /// The per-email map must not grow without bound: entries older than the
    /// 120 s memory window are evicted on each call. Locks the `retain` prune
    /// (a removed prune would leave all six entries).
    #[test]
    fn login_link_per_email_map_is_bounded_to_window() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        for i in 0..5 {
            rl.check_and_record_at(&format!("u{i}@x.com"), t0 + Duration::from_millis(i as u64))
                .unwrap();
        }
        assert_eq!(
            rl.tracked_keys(),
            5,
            "five distinct emails within the window"
        );
        // Advance well past the 120 s memory window and record one more: the five
        // stale entries must be evicted, leaving only the fresh one.
        rl.check_and_record_at("late@x.com", t0 + Duration::from_secs(200))
            .unwrap();
        assert_eq!(
            rl.tracked_keys(),
            1,
            "stale per-email entries must be evicted, leaving only the recent one"
        );
    }

    /// At EXACTLY the 120 s memory-window boundary the strict `<` evicts the
    /// old entry (120 s is not < 120 s). Catches the `<` → `<=` mutation on the
    /// `retain` prune.
    #[test]
    fn login_link_per_email_evicts_at_exactly_120s() {
        let mut rl = LoginLinkRateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at("old@x.com", t0).unwrap();
        // A different email exactly 120 s later: `old` is exactly 120 s old, so
        // strict `<` drops it → only `new` remains. `<=` would keep both.
        rl.check_and_record_at("new@x.com", t0 + Duration::from_secs(120))
            .unwrap();
        assert_eq!(
            rl.tracked_keys(),
            1,
            "the exactly-120s-old entry must be evicted (strict `<`)"
        );
    }
}
