//! Request-logging middleware (#260).
//!
//! `build_router()` previously wired only `CatchPanicLayer` — the journal
//! contained zero lines with a request's method/path/status/User-Agent. That
//! made round 4 of the iOS install auto-login fix (#258) undiagnosable: the
//! only two facts available were two "install-token: minted" log lines, and
//! three competing hypotheses fit that evidence identically.
//!
//! A small `axum::middleware::from_fn` — deliberately NOT
//! `tower_http::trace::TraceLayer` — so exactly what gets logged is under
//! direct control: TraceLayer's default request span logs the full URI
//! VERBATIM (including a raw, un-redacted query string), which would leak a
//! full login token on every request; reshaping it to redact and to add
//! User-Agent needs the same amount of custom code as this module anyway.
//!
//! Never logs request or response bodies — only method, path, the REDACTED
//! query (`spinbike_core::redact::redact_query`), the response status, and
//! User-Agent.

use axum::extract::Request;
use axum::http::header::USER_AGENT;
use axum::middleware::Next;
use axum::response::Response;

use spinbike_core::redact::redact_query;

/// Logs one `tracing::info!` line per request: method, path, redacted
/// query (only when present), response status, and User-Agent. Applied to
/// every route mounted by `build_router` — a low-traffic, single-instance
/// server, so logging every route is acceptable (per #260's own scope note).
pub async fn middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let raw_query = req.uri().query().unwrap_or("").to_string();
    let user_agent = req
        .headers()
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let response = next.run(req).await;
    let status = response.status().as_u16();

    if raw_query.is_empty() {
        tracing::info!(%method, %path, status, %user_agent, "request");
    } else {
        let query = redact_query(&raw_query);
        tracing::info!(%method, %path, %query, status, %user_agent, "request");
    }

    response
}
