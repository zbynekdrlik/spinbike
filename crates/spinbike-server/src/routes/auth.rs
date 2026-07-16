use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::AppState;
use crate::auth::{self, AuthUser, StaffUser, parse_role};
use crate::db::{login_tokens, users};
use crate::error::ApiError;
use crate::mail::MailError;
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

#[derive(Deserialize)]
pub struct RequestLoginCodeRequest {
    pub email: String,
}

#[derive(Deserialize)]
pub struct CodeLoginRequest {
    pub email: String,
    pub code: String,
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

// ---------------------------------------------------------------------------
// Rate limiter for /api/auth/code-login (the VERIFY path) — #227
// ---------------------------------------------------------------------------

/// In-memory rate-limit for the public login-code VERIFY endpoint, keyed by the
/// submitted email (String). Distinct from `LoginLinkRateLimiter` (which throttles
/// the send/REQUEST path): this bounds how many code-verify attempts a single
/// email — and all emails together — may make per minute, a second brute-force
/// layer on top of each code's own 5-attempt cap. Caps: ≤10 verify attempts per
/// email per 60 s, and a global 60-per-60 s ceiling. No per-key min-gap (a
/// legitimate user re-typing a mistyped code must not be throttled after one
/// try; the 10/min cap + the per-code attempt cap are the real guards). Same
/// thin typed wrapper over the shared `SlidingWindowLimiter` (#166) as the door
/// and login-link limiters. The verify path records BEFORE any DB lookup, so an
/// existing and a non-existent email are throttled identically — the 429 leaks
/// no account existence.
pub struct CodeLoginRateLimiter(SlidingWindowLimiter<String>);

impl CodeLoginRateLimiter {
    pub fn new() -> Self {
        Self(SlidingWindowLimiter::new(RateLimitConfig {
            per_key_window: Duration::from_secs(60),
            per_key_min_gap: None,
            per_key_max: Some(10),
            per_key_cap_reason: "code_verify_cap",
            key_memory: Duration::from_secs(120),
            global_window: Duration::from_secs(60),
            global_max: 60,
        }))
    }

    /// Returns Ok and records the attempt if allowed; Err(reason) otherwise
    /// ("code_verify_cap" / "global_cap").
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

impl Default for CodeLoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/request-login-link", post(request_login_link))
        .route("/api/auth/token-login", post(token_login))
        .route("/api/auth/request-login-code", post(request_login_code))
        .route("/api/auth/code-login", post(code_login))
        .route("/api/auth/me", get(me))
        .route("/api/users/{id}/invite", post(invite_user))
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
        // #172: same reasoning as door.rs's door_rate_limit — panic="unwind"
        // means a future panic while holding this guard now poisons the
        // mutex instead of aborting the process. Recover rather than
        // .expect(), so one panic doesn't permanently 500 every login-link
        // request until restart.
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

/// Compose the unaccented-Slovak login-code email. Returns (subject, text, html).
/// The code is shown large and prominent; the copy states the 10-minute validity
/// and "don't share it with anyone" (#227).
fn login_code_email(code: &str) -> (String, String, String) {
    let subject = "SpinBike - prihlasovaci kod".to_string();
    let text = format!(
        "Ahoj,\n\ntvoj prihlasovaci kod do SpinBike je:\n\n{code}\n\n\
         Kod plati 10 minut. Nikomu ho neposielaj. \
         Ak si o prihlasenie nepoziadal, tento email ignoruj."
    );
    let html = format!(
        "<p>Ahoj,</p><p>tvoj prihlasovaci kod do SpinBike je:</p>\
         <p style=\"font-size:32px;font-weight:bold;letter-spacing:6px;margin:16px 0\">{code}</p>\
         <p>Kod plati 10 minut. Nikomu ho neposielaj.</p>\
         <p>Ak si o prihlasenie nepoziadal, tento email ignoruj.</p>"
    );
    (subject, text, html)
}

/// Whether a user may complete a passwordless customer login (code path, #227):
/// role MUST be `customer` AND the account must not be blocked. Extracted as a
/// pure predicate so its boundary is directly unit-testable — the handlers that
/// call it are async and DB-bound, so the gate's `&&`/`==`/`!` logic would
/// otherwise only be reachable through integration tests. `Role::from` (not
/// `parse_role`) so an unknown legacy role is never treated as a customer.
fn is_eligible_customer(role: &str, blocked: bool) -> bool {
    Role::from(role) == Role::Customer && !blocked
}

/// Public passwordless-login endpoint (#227). ALWAYS returns 200
/// `{"status":"ok"}` regardless of whether the email exists — EXACTLY the same
/// no-enumeration semantics as `request-login-link`. A 6-digit code email is
/// actually sent ONLY for an existing, non-blocked, role=customer account, and
/// only when the shared per-email rate limiter allows. The SMTP send is
/// `tokio::spawn`'d off the response path (same timing-side-channel reasoning as
/// request-login-link — an existing customer must not respond measurably slower
/// than a non-customer).
async fn request_login_code(
    State(state): State<AppState>,
    Json(body): Json<RequestLoginCodeRequest>,
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
            tracing::error!(error = %e, "request-login-code: user lookup failed");
            return ok();
        }
    };

    // Customers-only; blocked accounts get nothing. (deleted_at is already
    // filtered by get_user_by_email.)
    if !is_eligible_customer(user.role.as_str(), user.blocked) {
        return ok();
    }

    // Reuse the login-link email-send budget (per-email 60 s + global 10/min):
    // requesting a code and requesting a link are the same "send an email to
    // this address" operation, so they share one throttle. Throttled → still
    // 200 ok, no send.
    if let Err(reason) = state
        .login_link_rate_limit
        // #172: recover from poisoning rather than .expect(), so one panic
        // doesn't permanently 500 this endpoint until restart.
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .check_and_record(&email)
    {
        tracing::warn!(%reason, "request-login-code: throttled");
        return ok();
    }

    let code = match login_tokens::create_code(&state.pool, user.id).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, user_id = user.id, "request-login-code: code create failed");
            return ok();
        }
    };

    let (subject, text, html) = login_code_email(&code);

    // Fire the SMTP send OFF the response path — same no-enumeration timing
    // reasoning as request-login-link. The code row is committed synchronously
    // above (durable); delivery is best-effort and only logged.
    let mail = state.mail.clone();
    let user_id = user.id;
    tokio::spawn(async move {
        if let Err(e) = mail.send(&email, &subject, &text, &html).await {
            tracing::warn!(error = %e, user_id, "request-login-code: mail send failed");
        } else {
            tracing::info!(user_id, "request-login-code: sent");
        }
    });

    ok()
}

/// Verify a 6-digit login code (#227) and return a JWT session. Rate-limited by
/// the submitted email BEFORE any DB lookup (a throttled attempt returns 429 and
/// leaks no account existence, since existing and non-existent emails are
/// throttled identically). Every other failure — wrong/expired/used/exhausted
/// code, non-customer, blocked/deleted user, unknown email — collapses to a
/// single uniform 401 `invalid_or_expired_code` (no enumeration). On success the
/// code is atomically single-use redeemed and a permanent customer session is
/// issued.
async fn code_login(
    State(state): State<AppState>,
    Json(body): Json<CodeLoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let email = body.email.trim().to_string();
    let code = body.code.trim().to_string();
    let invalid = || ApiError::Unauthorized(ErrorCode::InvalidOrExpiredCode);

    // Rate limit FIRST, keyed by the submitted email — recorded before any DB
    // lookup so existing and non-existent emails behave identically. Throttled
    // → 429 (does not leak account existence).
    if let Err(reason) = state
        .code_login_rate_limit
        // #172: recover from poisoning rather than .expect().
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .check_and_record(&email)
    {
        tracing::warn!(%reason, "code-login: throttled");
        return Err(ApiError::TooManyRequests(ErrorCode::TooManyRequests));
    }

    if email.is_empty() || code.is_empty() {
        return Err(invalid());
    }

    // Resolve the account — a miss is a uniform invalid (no enumeration).
    let user = match users::get_user_by_email(&state.pool, &email).await {
        Ok(Some(u)) => u,
        Ok(None) => return Err(invalid()),
        Err(e) => return Err(internal_error(e)),
    };

    // Customers-only + not blocked (get_user_by_email already filters deleted).
    if !is_eligible_customer(user.role.as_str(), user.blocked) {
        return Err(invalid());
    }

    // Atomic verify + single-use redeem + attempt-count.
    let redeemed = login_tokens::verify_code(&state.pool, user.id, &code)
        .await
        .map_err(internal_error)?;
    if redeemed.is_none() {
        return Err(invalid());
    }

    let role = parse_role(&user.role);
    let email_str = user.email.as_deref().unwrap_or("");
    let token =
        auth::create_token(&state.jwt_secret, user.id, email_str, &role).map_err(internal_error)?;

    tracing::info!(user_id = user.id, "code-login: session issued");
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

#[derive(Serialize)]
struct InviteResponse {
    sent_to: String,
    /// Present only in `SMTP_TEST_MODE=capture` so E2E can drive the welcome
    /// flow without a real mailbox. Never populated with a real SMTP relay.
    #[serde(skip_serializing_if = "Option::is_none")]
    test_link: Option<String>,
}

/// Compose the unaccented-Slovak invite email. Returns (subject, text, html).
fn invite_email(link: &str) -> (String, String, String) {
    let subject = "Vitaj v SpinBike".to_string();
    let text = format!(
        "Ahoj,\n\nSpinBike je nasa aplikacia na rezervacie spinningu a spravu kreditu. \
         Klikni na odkaz a aktivuj si pristup:\n{link}\n\nOdkaz plati 14 dni."
    );
    let html = format!(
        "<p>Ahoj,</p>\
         <p>SpinBike je nasa aplikacia na rezervacie spinningu a spravu kreditu.</p>\
         <p>Klikni na odkaz a aktivuj si pristup:</p>\
         <p><a href=\"{link}\">Aktivovat pristup</a></p>\
         <p>Odkaz plati 14 dni.</p>"
    );
    (subject, text, html)
}

/// Admin/staff-only: email a magic invite link to the given user. Requires the
/// user to have an email. Returns 503 `mail_not_configured` when the mail
/// module is Disabled (missing SMTP env).
async fn invite_user(
    State(state): State<AppState>,
    _: StaffUser,
    Path(id): Path<i64>,
) -> Result<Json<InviteResponse>, ApiError> {
    let user = users::get_user_by_id(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;
    if user.deleted_at.is_some() {
        return Err(ApiError::NotFound(ErrorCode::UserNotFound));
    }

    let email = match user
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(e) => e.to_string(),
        None => {
            return Err(super::bad_request(
                "User has no email address to send an invite to",
            ));
        }
    };

    let raw = login_tokens::create_token(
        &state.pool,
        id,
        login_tokens::PURPOSE_INVITE,
        login_tokens::INVITE_TTL_SECS,
    )
    .await
    .map_err(internal_error)?;

    let link = format!(
        "{}/welcome?t={}",
        state.public_base_url.trim_end_matches('/'),
        raw
    );
    let (subject, text, html) = invite_email(&link);

    match state.mail.send(&email, &subject, &text, &html).await {
        Ok(()) => {}
        Err(MailError::Disabled) => {
            tracing::warn!(user_id = id, "invite: mail is disabled — returning 503");
            return Err(ApiError::ServiceUnavailable(ErrorCode::MailNotConfigured));
        }
        Err(e) => return Err(internal_error(e)),
    }

    // last_captured() is Some only in capture test mode — use it as the
    // capture-mode detector, then echo the freshly-composed link.
    let test_link = state.mail.last_captured().map(|_| link);
    tracing::info!(user_id = id, %email, "invite: sent");
    Ok(Json(InviteResponse {
        sent_to: email,
        test_link,
    }))
}

#[cfg(test)]
mod tests {
    use super::{CodeLoginRateLimiter, LoginLinkRateLimiter};
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

    // ── code-login verify limiter (#227) ──────────────────────────────────

    #[test]
    fn code_login_first_attempt_allowed() {
        let mut rl = CodeLoginRateLimiter::new();
        assert!(rl.check_and_record("a@x.com").is_ok());
    }

    #[test]
    fn code_login_per_email_cap_at_eleventh_attempt() {
        // ≤10 verify attempts per email per 60 s; the 11th (no min-gap) hits the
        // per-email cap, not global_cap.
        let mut rl = CodeLoginRateLimiter::new();
        let t0 = Instant::now();
        for i in 0..10 {
            rl.check_and_record_at("a@x.com", t0 + Duration::from_millis(i as u64))
                .unwrap_or_else(|e| panic!("attempt #{i} should succeed, got {e}"));
        }
        assert_eq!(
            rl.check_and_record_at("a@x.com", t0 + Duration::from_millis(50)),
            Err("code_verify_cap"),
            "the 11th verify for one email inside 60 s must hit the per-email cap"
        );
    }

    #[test]
    fn code_login_distinct_emails_are_independent() {
        let mut rl = CodeLoginRateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..10 {
            rl.check_and_record_at("a@x.com", t0).unwrap();
        }
        rl.check_and_record_at("b@x.com", t0)
            .expect("a different email must not be throttled by the first's cap");
    }

    #[test]
    fn code_login_global_cap_at_61st_attempt() {
        let mut rl = CodeLoginRateLimiter::new();
        let t0 = Instant::now();
        // 60 attempts spread across 60 distinct emails (each under its own cap).
        for i in 0..60 {
            rl.check_and_record_at(&format!("u{i}@x.com"), t0 + Duration::from_millis(i as u64))
                .unwrap_or_else(|e| panic!("attempt #{i} should succeed, got {e}"));
        }
        assert_eq!(
            rl.check_and_record_at("u60@x.com", t0 + Duration::from_millis(100)),
            Err("global_cap"),
            "the 61st verify inside the 60 s window must hit the global cap"
        );
    }

    /// The verify limiter's per-email map is bounded — records N distinct emails,
    /// evicts stale ones past the memory window. Also exercises `tracked_keys`
    /// with a value that is neither 0 nor 1 (locks it against a constant mutant).
    #[test]
    fn code_login_per_email_map_is_bounded() {
        let mut rl = CodeLoginRateLimiter::new();
        let t0 = Instant::now();
        for i in 0..3 {
            rl.check_and_record_at(&format!("u{i}@x.com"), t0 + Duration::from_millis(i as u64))
                .unwrap();
        }
        assert_eq!(
            rl.tracked_keys(),
            3,
            "three distinct emails tracked while fresh"
        );
        // Past the 120 s memory window, the three stale entries are evicted.
        rl.check_and_record_at("late@x.com", t0 + Duration::from_secs(200))
            .unwrap();
        assert_eq!(
            rl.tracked_keys(),
            1,
            "stale per-email entries must be evicted, leaving only the recent one"
        );
    }

    // ── customers-only gate (#227) ─────────────────────────────────────────

    /// The passwordless-login eligibility gate: customer AND not blocked. Pins
    /// every boundary (locks the `==` / `&&` / `!` operators against mutation).
    #[test]
    fn is_eligible_customer_gate() {
        assert!(
            super::is_eligible_customer("customer", false),
            "a non-blocked customer is the ONLY eligible case"
        );
        assert!(
            !super::is_eligible_customer("customer", true),
            "a blocked customer must be rejected"
        );
        assert!(
            !super::is_eligible_customer("admin", false),
            "an admin (non-customer) must be rejected"
        );
        assert!(
            !super::is_eligible_customer("staff", false),
            "staff (non-customer) must be rejected"
        );
        assert!(
            !super::is_eligible_customer("admin", true),
            "blocked non-customer must be rejected"
        );
    }

    // ── login-code email content (#227) ────────────────────────────────────

    /// The composed email must carry the actual code plus the 10-minute validity
    /// and the "don't share it" warning (locks `login_code_email`'s return
    /// against a junk-tuple mutant).
    #[test]
    fn login_code_email_embeds_the_code_and_guidance() {
        let (subject, text, html) = super::login_code_email("482913");
        assert!(
            subject.contains("prihlasovaci kod"),
            "subject must name the login code, got: {subject}"
        );
        assert!(
            text.contains("482913"),
            "plain-text body must carry the code, got: {text}"
        );
        assert!(
            text.contains("10 minut"),
            "body must state the 10-minute validity"
        );
        assert!(
            text.contains("Nikomu ho neposielaj"),
            "body must warn not to share the code"
        );
        assert!(
            html.contains("482913"),
            "html body must carry the code, got: {html}"
        );
    }
}
