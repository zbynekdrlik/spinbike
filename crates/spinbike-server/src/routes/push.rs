//! PWA push subscription endpoints (#264). Customer-facing self-service:
//! read the server's public VAPID key, store/remove the browser's own
//! subscription. All three handlers act on the CALLER's own account, so
//! each calls `auth::require_live_session` first (session-invalidation
//! contract, #268/#274/#277 — see `.claude/rules/session-invalidation.md`).

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::{bad_request, internal_error};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/push/config", get(config))
        .route("/api/push/subscribe", post(subscribe))
        .route("/api/push/unsubscribe", post(unsubscribe))
}

/// Known Web Push service hosts (exact match). The server's own daily
/// `jobs::notifications` job later POSTs directly to whatever `endpoint` a
/// customer registers here — an ALLOWLIST, never a denylist, is mandatory:
/// without one, any authenticated customer could register an
/// internal/metadata URL (`http://169.254.169.254/...`, an internal admin
/// endpoint, etc.) and the server's own background job would issue an
/// outbound POST to it once a day (authenticated SSRF — #264 review
/// finding). `*.notify.windows.com` (Edge/Windows) is checked separately
/// as a suffix match below since it's per-device, not one fixed host.
const ALLOWED_PUSH_HOSTS: &[&str] = &[
    "fcm.googleapis.com",                // Chrome / Chromium / Edge (Blink)
    "updates.push.services.mozilla.com", // Firefox
    "web.push.apple.com",                // Safari
];

/// True when `endpoint` parses as `https://` with a host on the known
/// push-service allowlist. Parsed via `reqwest::Url` (immune to
/// path/query tricks like `https://fcm.googleapis.com.evil.com/...` or
/// `https://evil.com/fcm.googleapis.com` — those parse to a DIFFERENT
/// host than the real one, so a substring check on the raw string would be
/// unsafe here). The `.notify.windows.com` suffix check has a leading
/// dot, so it can only match a genuine subdomain of Microsoft's own DNS
/// zone — an attacker cannot register `evilnotify.windows.com` and have
/// it pass (no dot boundary) or `notify.windows.com.evil.com` (that ENDS
/// in `.evil.com`, not `.notify.windows.com`).
fn is_allowed_push_endpoint(endpoint: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    ALLOWED_PUSH_HOSTS.contains(&host) || host.ends_with(".notify.windows.com")
}

#[derive(Serialize)]
struct PushConfigResponse {
    /// False when the server has no `VAPID_PRIVATE_KEY` configured — the
    /// client must not offer the "enable notifications" button at all.
    enabled: bool,
    /// Base64url (no padding) uncompressed EC public key, the
    /// `applicationServerKey` `PushManager.subscribe()` needs. `None` when
    /// `enabled` is false.
    public_key: Option<String>,
    /// Whether the CALLER already has at least one stored subscription —
    /// lets the client show on/off state without keeping its own
    /// server-authoritative flag in local storage.
    subscribed: bool,
}

async fn config(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<PushConfigResponse>, ApiError> {
    crate::auth::require_live_session(&state.pool, claims.sub).await?;

    let subscribed = crate::db::push::has_subscription(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;
    Ok(Json(PushConfigResponse {
        enabled: state.push.public_key().is_some(),
        public_key: state.push.public_key().map(str::to_string),
        subscribed,
    }))
}

#[derive(Deserialize)]
struct SubscriptionKeysReq {
    p256dh: String,
    auth: String,
}

/// Mirrors the shape of the browser's own `PushSubscription.toJSON()` —
/// `{ endpoint, keys: { p256dh, auth } }` — so the client can POST the
/// subscription object with no reshaping.
#[derive(Deserialize)]
struct SubscribeRequest {
    endpoint: String,
    keys: SubscriptionKeysReq,
}

async fn subscribe(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<SubscribeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::auth::require_live_session(&state.pool, claims.sub).await?;

    if req.endpoint.trim().is_empty()
        || req.keys.p256dh.trim().is_empty()
        || req.keys.auth.trim().is_empty()
    {
        return Err(bad_request(
            "endpoint and keys.p256dh/keys.auth are required",
        ));
    }

    if !is_allowed_push_endpoint(&req.endpoint) {
        tracing::warn!(
            user_id = claims.sub,
            endpoint = %req.endpoint,
            "push: rejected subscribe — endpoint is not a recognized push service"
        );
        return Err(bad_request("endpoint is not a recognized push service"));
    }

    crate::db::push::upsert_subscription(
        &state.pool,
        claims.sub,
        &req.endpoint,
        &req.keys.p256dh,
        &req.keys.auth,
    )
    .await
    .map_err(internal_error)?;

    tracing::info!(user_id = claims.sub, "push: subscription stored");
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct UnsubscribeRequest {
    endpoint: String,
}

async fn unsubscribe(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<UnsubscribeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::auth::require_live_session(&state.pool, claims.sub).await?;

    crate::db::push::delete_subscription(&state.pool, claims.sub, &req.endpoint)
        .await
        .map_err(internal_error)?;

    tracing::info!(user_id = claims.sub, "push: subscription removed");
    Ok(Json(serde_json::json!({ "ok": true })))
}
