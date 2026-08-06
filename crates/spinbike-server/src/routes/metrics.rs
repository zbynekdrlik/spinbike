//! Client-side observability beacon (#260).
//!
//! `POST /api/metrics/launch` — the frontend fires this, fire-and-forget, on
//! app boot whenever it detects it is running as an installed standalone PWA
//! (`display-mode: standalone` / iOS `navigator.standalone`). This is the
//! measurement #260 exists for: a beacon carrying the launch path with no
//! login token discriminates between the three hypotheses that made round 4
//! of the iOS install auto-login fix (#258) undiagnosable — see the module
//! doc on `crate::request_log` and #260 itself for the full context.
//!
//! Deliberately UNAUTHENTICATED: the whole point is that a fresh standalone
//! launch may have no session yet. Rate-limited per client IP so the open
//! endpoint isn't a free-for-all — same `SlidingWindowLimiter` abstraction
//! (#166) as `door::RateLimiter` / `auth::LoginLinkRateLimiter`.

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::Deserialize;
use std::time::Duration;

use crate::AppState;
use crate::rate_limit::{RateLimitConfig, SlidingWindowLimiter};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/metrics/launch", post(launch))
}

/// Body the frontend sends. `query_redacted` arrives ALREADY redacted by the
/// client (`spinbike_core::redact::redact_query`, the same function this
/// server uses in `request_log`) — the server never receives a raw token via
/// this endpoint either way.
#[derive(Deserialize)]
struct LaunchBeaconRequest {
    path: String,
    query_redacted: String,
    had_token: bool,
    had_session: bool,
    src: Option<String>,
}

// ---------------------------------------------------------------------------
// Rate limiter
// ---------------------------------------------------------------------------

/// In-memory rate limit for the unauthenticated launch beacon, keyed by a
/// best-effort client-IP string (see `client_ip_key` below). Low-stakes
/// diagnostic endpoint on a low-traffic single-instance server (per #260's
/// own scope note) — the caps here are generous, just enough to bound a
/// misbehaving/looping client rather than model real abuse.
pub struct LaunchRateLimiter(SlidingWindowLimiter<String>);

impl LaunchRateLimiter {
    pub fn new() -> Self {
        Self(SlidingWindowLimiter::new(RateLimitConfig {
            per_key_window: Duration::from_secs(60),
            per_key_min_gap: Some(Duration::from_secs(2)),
            per_key_max: Some(20),
            per_key_cap_reason: "per_ip_cap",
            key_memory: Duration::from_secs(60),
            global_window: Duration::from_secs(60),
            global_max: 200,
        }))
    }

    pub fn check_and_record(&mut self, key: String) -> Result<(), &'static str> {
        self.0.check_and_record(key)
    }
}

impl Default for LaunchRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Best-effort client-IP key for rate limiting. The app sits directly behind
/// a Cloudflare tunnel (`cloudflared` -> `127.0.0.1:8080`, no local
/// nginx/caddy — see `ci-deploy` skill) with no `ConnectInfo` wiring, so the
/// TCP peer address is never the real client. Prefers `Cf-Connecting-Ip`
/// (set by Cloudflare on every proxied request), then the first hop of
/// `X-Forwarded-For`, then a fixed `"unknown"` bucket (still rate-limited —
/// just shared by every caller with neither header, e.g. a direct
/// `127.0.0.1:8080` request in local dev/tests).
fn client_ip_key(headers: &HeaderMap) -> String {
    if let Some(v) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
    {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = v.split(',').next()
    {
        let first = first.trim();
        if !first.is_empty() {
            return first.to_string();
        }
    }
    "unknown".to_string()
}

async fn launch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LaunchBeaconRequest>,
) -> StatusCode {
    let key = client_ip_key(&headers);

    // #172: panic="unwind" means a future panic while this guard is held
    // would poison the mutex instead of aborting the process — recover the
    // guard rather than propagate the poison via .expect(), same pattern as
    // door::RateLimiter / auth::LoginLinkRateLimiter.
    let allowed = state
        .launch_rate_limit
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .check_and_record(key.clone());

    if let Err(reason) = allowed {
        tracing::warn!(ip = %key, %reason, "launch beacon: rate limited");
        return StatusCode::TOO_MANY_REQUESTS;
    }

    tracing::info!(
        path = %payload.path,
        query = %payload.query_redacted,
        had_token = payload.had_token,
        had_session = payload.had_session,
        src = ?payload.src,
        "standalone launch beacon"
    );

    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn prefers_cf_connecting_ip_over_x_forwarded_for() {
        let headers = headers_with(&[
            ("cf-connecting-ip", "1.2.3.4"),
            ("x-forwarded-for", "9.9.9.9, 5.5.5.5"),
        ]);
        assert_eq!(client_ip_key(&headers), "1.2.3.4");
    }

    #[test]
    fn falls_back_to_first_x_forwarded_for_hop() {
        let headers = headers_with(&[("x-forwarded-for", "9.9.9.9, 5.5.5.5")]);
        assert_eq!(client_ip_key(&headers), "9.9.9.9");
    }

    #[test]
    fn falls_back_to_unknown_when_no_headers_present() {
        let headers = HeaderMap::new();
        assert_eq!(client_ip_key(&headers), "unknown");
    }

    #[test]
    fn blank_cf_header_falls_through_to_x_forwarded_for() {
        let headers = headers_with(&[("cf-connecting-ip", "  "), ("x-forwarded-for", "7.7.7.7")]);
        assert_eq!(client_ip_key(&headers), "7.7.7.7");
    }
}
