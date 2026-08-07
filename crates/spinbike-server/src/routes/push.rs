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
