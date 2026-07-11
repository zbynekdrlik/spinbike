pub mod auth;
pub mod db;
pub mod error;
pub mod ewelink;
pub mod jobs;
pub mod mail;
pub mod rate_limit;
pub mod routes;
pub mod util;
pub mod ws;

use anyhow::Result;
use spinbike_core::ws::ServerMsg;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub event_tx: broadcast::Sender<ServerMsg>,
    pub jwt_secret: String,
    pub ewelink: crate::ewelink::EwelinkHandle,
    pub mail: crate::mail::MailHandle,
    /// Base URL used to compose magic-link emails, e.g.
    /// `https://spinbike.newlevel.media`. Read from `PUBLIC_BASE_URL` at
    /// startup; empty when unset (invite/login-link then compose a relative
    /// `/welcome?t=...` link and a boot-time warn is logged).
    pub public_base_url: String,
    /// In-memory door-route rate-limit state. Per-AppState so concurrent
    /// integration tests don't share throttle windows across separate
    /// TestApp instances.
    pub door_rate_limit: std::sync::Arc<std::sync::Mutex<crate::routes::door::RateLimiter>>,
    /// In-memory rate-limit for the public `/api/auth/request-login-link`
    /// endpoint (email-keyed). Separate from `door_rate_limit`.
    pub login_link_rate_limit:
        std::sync::Arc<std::sync::Mutex<crate::routes::auth::LoginLinkRateLimiter>>,
}

/// Build the CORS layer by reading the CORS_ORIGIN environment variable.
/// Thin wrapper around `cors_layer_for` so the env lookup is kept out of
/// the testable body.
fn build_cors_layer() -> CorsLayer {
    cors_layer_for(std::env::var("CORS_ORIGIN").ok())
}

/// Pure, unit-testable variant of the CORS layer builder.
/// `Some(non_empty)` → restricted to that origin; `None` or `Some("")` → permissive.
pub(crate) fn cors_layer_for(origin: Option<String>) -> CorsLayer {
    match origin {
        Some(origin) if !origin.is_empty() => {
            info!("CORS: restricting to origin {origin}");
            let origin: tower_http::cors::AllowOrigin = origin
                .parse::<axum::http::HeaderValue>()
                .expect("Invalid CORS_ORIGIN value")
                .into();
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any)
        }
        _ => {
            warn!("CORS_ORIGIN not set — using permissive CORS. Do NOT use in production!");
            CorsLayer::permissive()
        }
    }
}

/// Pure predicate: returns true when the given raw env value indicates test mode.
/// Unit-tested; callers pass in the current SPINBIKE_TEST_MODE env value.
pub fn is_test_mode_from_env(raw: Option<&str>) -> bool {
    raw == Some("1")
}

/// Pure, unit-testable JWT-secret resolver (#157).
///
/// `raw` is the current `JWT_SECRET` env value (`None` = unset); `test_mode`
/// is `is_test_mode_from_env(SPINBIKE_TEST_MODE)`. Fails closed: when
/// `JWT_SECRET` is unset/empty and NOT in test mode, refuses to start rather
/// than falling back to the well-known dev default — that default signs
/// forgeable HS256 admin JWTs (`auth::create_token`), letting anyone reach
/// the door/payments/users routes if a redeploy drops the env var. The
/// insecure default is allowed ONLY when `SPINBIKE_TEST_MODE=1`.
pub fn resolve_jwt_secret(raw: Option<&str>, test_mode: bool) -> Result<String, String> {
    match raw {
        Some(s) if !s.is_empty() => {
            info!("JWT_SECRET configured from environment");
            Ok(s.to_string())
        }
        _ if test_mode => {
            warn!("JWT_SECRET not set — using insecure default. DO NOT use in production!");
            Ok("dev-secret-change-in-production".to_string())
        }
        _ => Err("JWT_SECRET must be set (or SPINBIKE_TEST_MODE=1 for local dev)".to_string()),
    }
}

/// Build the full application router, applying the `SPINBIKE_TEST_MODE`
/// fixture-route gate. This is the SINGLE place that decides whether the
/// unauthenticated, arbitrary-role `/api/test/*` fixtures
/// (`routes::test_fixtures`) are mounted — `start_server()` (production)
/// and the `production_router_does_not_expose_test_fixtures` test below
/// both call THIS function, so a future regression that inverts or removes
/// the gate is caught by the test regardless of which caller it's viewed
/// through (#161: a prior version of that test hand-copied this logic
/// instead of sharing it, so it could never have caught a real regression
/// in `start_server` itself).
pub fn build_router(test_mode: bool) -> axum::Router<AppState> {
    let mut router = routes::all_routes();
    if test_mode {
        tracing::warn!(
            "SPINBIKE_TEST_MODE=1 — test fixture endpoints are active. Do NOT use in production!"
        );
        router = router.merge(routes::test_fixtures::routes());
    }
    router
}

/// Build and start the Axum server.
pub async fn start_server(pool: SqlitePool, port: u16, jwt_secret: String) -> Result<()> {
    let (event_tx, _) = broadcast::channel(256);

    let public_base_url = std::env::var("PUBLIC_BASE_URL").unwrap_or_default();
    if public_base_url.is_empty() {
        warn!(
            "PUBLIC_BASE_URL not set — magic-link emails will use relative /welcome links. \
             Set it (e.g. https://spinbike.newlevel.media) when configuring mail."
        );
    } else {
        info!(%public_base_url, "PUBLIC_BASE_URL configured for magic-link emails");
    }

    let state = AppState {
        pool,
        event_tx,
        jwt_secret,
        ewelink: crate::ewelink::EwelinkHandle::spawn(),
        mail: crate::mail::MailHandle::spawn(),
        public_base_url,
        door_rate_limit: std::sync::Arc::new(std::sync::Mutex::new(
            crate::routes::door::RateLimiter::new(),
        )),
        login_link_rate_limit: std::sync::Arc::new(std::sync::Mutex::new(
            crate::routes::auth::LoginLinkRateLimiter::new(),
        )),
    };

    let test_mode = is_test_mode_from_env(std::env::var("SPINBIKE_TEST_MODE").ok().as_deref());
    let app = build_router(test_mode)
        .layer(build_cors_layer())
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    info!("Starting server on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::get;
    use db::{create_memory_pool, run_migrations};
    use tower::ServiceExt;

    #[test]
    fn test_mode_only_when_env_is_exactly_one() {
        assert!(is_test_mode_from_env(Some("1")));
        assert!(!is_test_mode_from_env(Some("0")));
        assert!(!is_test_mode_from_env(Some("true")));
        assert!(!is_test_mode_from_env(Some("")));
        assert!(!is_test_mode_from_env(None));
    }

    /// #157 RED: booting with no JWT_SECRET and not in test mode must refuse
    /// to start, never fall back to the source-visible dev default — that
    /// default signs forgeable admin JWTs (auth::create_token, HS256).
    #[test]
    fn resolve_jwt_secret_fails_closed_when_unset_and_not_test_mode() {
        assert!(resolve_jwt_secret(None, false).is_err());
    }

    /// #157 RED: an empty JWT_SECRET (e.g. a systemd env file with a blank
    /// value) must be treated the same as unset — fail closed.
    #[test]
    fn resolve_jwt_secret_fails_closed_when_empty_and_not_test_mode() {
        assert!(resolve_jwt_secret(Some(""), false).is_err());
    }

    #[test]
    fn resolve_jwt_secret_returns_configured_value() {
        assert_eq!(
            resolve_jwt_secret(Some("real-secret"), false).unwrap(),
            "real-secret"
        );
    }

    /// A real secret set alongside SPINBIKE_TEST_MODE=1 (CI's env, see
    /// ci.yml:241-242) must win — never silently swap in the dev default when
    /// a real secret is present.
    #[test]
    fn resolve_jwt_secret_returns_configured_value_even_in_test_mode() {
        assert_eq!(
            resolve_jwt_secret(Some("ci-test"), true).unwrap(),
            "ci-test"
        );
    }

    /// #157: the insecure dev default is allowed ONLY when SPINBIKE_TEST_MODE=1
    /// (the existing convention already used at test_fixtures.rs and ci.yml).
    #[test]
    fn resolve_jwt_secret_falls_back_to_dev_default_only_in_test_mode() {
        assert_eq!(
            resolve_jwt_secret(None, true).unwrap(),
            "dev-secret-change-in-production"
        );
    }

    /// Build a tiny router with the CORS layer and a single GET / route, then
    /// send a preflight OPTIONS and inspect the Access-Control-Allow-Origin
    /// header. Permissive layer → `*`; restricted → the specific origin.
    async fn preflight_origin(cors: CorsLayer, request_origin: &str) -> Option<String> {
        let app: Router = Router::new().route("/", get(|| async { "hi" })).layer(cors);
        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/")
            .header("origin", request_origin)
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        resp.headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    /// Kills `build_cors_layer -> Default::default()`. When CORS_ORIGIN is
    /// unset (as in CI's test env) the wrapper must return the permissive
    /// layer, not the empty default.
    #[tokio::test]
    async fn build_cors_layer_wrapper_is_permissive_when_env_unset() {
        // Defensive: ensure env doesn't leak in from another harness.
        // SAFETY: no other test sets CORS_ORIGIN in this crate.
        unsafe {
            std::env::remove_var("CORS_ORIGIN");
        }
        let layer = build_cors_layer();
        let echoed = preflight_origin(layer, "https://anywhere.example").await;
        assert_eq!(echoed.as_deref(), Some("*"));
    }

    #[tokio::test]
    async fn cors_layer_for_none_is_permissive() {
        let layer = cors_layer_for(None);
        let echoed = preflight_origin(layer, "https://anywhere.example").await;
        // Permissive allows any origin — tower-http echoes `*`.
        assert_eq!(echoed.as_deref(), Some("*"));
    }

    #[tokio::test]
    async fn cors_layer_for_empty_is_permissive() {
        let layer = cors_layer_for(Some(String::new()));
        let echoed = preflight_origin(layer, "https://anywhere.example").await;
        assert_eq!(echoed.as_deref(), Some("*"));
    }

    #[tokio::test]
    async fn cors_layer_for_value_restricts_to_that_origin() {
        // The restricted layer reports the exact allowed origin in
        // Access-Control-Allow-Origin (not `*`). That distinguishes it
        // from the permissive / default branches and kills the guard mutants
        // that send the wrong origin into the layer builder.
        let allowed = "https://spinbike.example.com";
        let layer = cors_layer_for(Some(allowed.to_string()));
        let echoed = preflight_origin(layer, allowed).await;
        assert_eq!(echoed.as_deref(), Some(allowed));
    }

    /// Kills `start_server -> Ok(())`: if the function is replaced by a no-op,
    /// the TCP port never binds and this test times out.
    #[tokio::test]
    async fn start_server_binds_and_accepts_connections() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Reserve a free port, drop the temp listener, hand the port to start_server.
        let tmp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tmp.local_addr().unwrap();
        drop(tmp);

        let handle =
            tokio::spawn(
                async move { start_server(pool, addr.port(), "test-jwt".to_string()).await },
            );

        // Try to connect for up to ~2s.
        let mut connected = false;
        for _ in 0..40 {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                connected = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        handle.abort();
        let _ = handle.await;
        assert!(connected, "start_server did not bind within 2s");
    }

    /// #161: the ONLY thing keeping the unauthenticated, arbitrary-role
    /// `/api/test/*` fixture routes (`routes::test_fixtures`) out of
    /// production is the `is_test_mode_from_env` merge gate inside
    /// `build_router()` above — `seed_account` (test_fixtures.rs) has no
    /// auth guard at all and accepts a caller-supplied `role`. No existing
    /// test exercised the negative wiring path: `TestApp`
    /// (tests/helpers/mod.rs) always merges the fixtures regardless of the
    /// env var, so the production router build was never actually driven by
    /// any test.
    ///
    /// This test calls `build_router(false)` — the SAME function
    /// `start_server()` calls with the SAME `test_mode` value it computes
    /// when `SPINBIKE_TEST_MODE` is unset/not `"1"`. A prior version of
    /// this test hand-copied the router-building logic instead of sharing
    /// it with `start_server`, which meant it could never have caught a
    /// real regression in `start_server` itself (a future edit that
    /// inverts or deletes the gate) — it would have kept passing
    /// unconditionally either way. Routing both callers through one shared
    /// function closes that gap: the gate now lives in exactly one place,
    /// and this test exercises that exact place.
    ///
    /// Note on the assertion shape: an unmatched path here does NOT 404 —
    /// `all_routes()` ends with `.fallback(static_files::static_handler)`,
    /// which serves the SPA's `index.html` (200) for any dotless unmatched
    /// path (the same behavior already locked in by
    /// `tests/static_files.rs::unknown_spa_route_also_serves_index_html`,
    /// and documented for the removed `/api/auth/register` route in the
    /// ci-deploy skill). So the meaningful assertion is the removed
    /// CAPABILITY, not a raw status code: no account is ever created, and
    /// none of the fixture paths return their handler's own JSON response.
    #[tokio::test]
    async fn production_router_does_not_expose_test_fixtures() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (event_tx, _) = broadcast::channel(16);
        let state = AppState {
            pool: pool.clone(),
            event_tx,
            jwt_secret: "placeholder".to_string(),
            ewelink: crate::ewelink::EwelinkHandle::spawn(),
            mail: crate::mail::MailHandle::spawn(),
            public_base_url: String::new(),
            door_rate_limit: std::sync::Arc::new(std::sync::Mutex::new(
                crate::routes::door::RateLimiter::new(),
            )),
            login_link_rate_limit: std::sync::Arc::new(std::sync::Mutex::new(
                crate::routes::auth::LoginLinkRateLimiter::new(),
            )),
        };

        // The SAME function start_server() calls, with the SAME test_mode
        // value it computes when SPINBIKE_TEST_MODE is unset/not "1" — NOT
        // a hand-copied reconstruction of start_server's router-building.
        let app: Router = build_router(false).with_state(state);

        // The security-critical case: an anonymous caller must not be able
        // to mint an account (let alone role="admin") via seed_account's
        // caller-supplied role field.
        let exploit_email = "exploit-161@example.invalid";
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/test/seed-account")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "email": exploit_email,
                    "password": "placeholder",
                    "name": "Attacker",
                    "role": "admin"
                })
                .to_string(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::CREATED,
            "production router routed seed_account and returned its 201 Created — \
             the SPINBIKE_TEST_MODE gate did not hold"
        );
        let created: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ?")
            .bind(exploit_email)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            created, 0,
            "an anonymous request minted a user account on the production router — \
             the SPINBIKE_TEST_MODE gate did not hold"
        );

        // The remaining fixture routes: none are reachable either — each
        // falls through to the SPA fallback (200 index.html, text/html),
        // never the fixture handler's own JSON success response.
        for path in [
            "/api/test/seed-expired-pass",
            "/api/test/seed-transactions",
            "/api/test/seed-user",
            "/api/test/seed-credit",
        ] {
            let req = Request::builder()
                .method(Method::POST)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            let content_type = resp
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            assert!(
                !content_type.starts_with("application/json"),
                "{path} returned a JSON response on the production router (content-type \
                 was {content_type:?}) — it must not be reachable there"
            );
        }
    }

    /// #172 RED: proves a panicking handler must be caught (500) rather than
    /// taking down the whole router. As written at this commit, `build_router`
    /// wires no `CatchPanicLayer` anywhere, so nothing intercepts the panic
    /// before it propagates out of `Router::oneshot` — this test would panic
    /// (and thus fail) rather than observe a 500. Deterministically RED by
    /// static proof, not an actual failing CI run: local `cargo test` is
    /// banned in this repo (CI-only, see CLAUDE.md), and the absence of any
    /// `catch_panic`/`CatchPanicLayer` reference anywhere in the crate
    /// (grep-confirmed) is exactly the condition this test targets.
    #[tokio::test]
    async fn panicking_handler_returns_500_and_server_survives() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (event_tx, _) = broadcast::channel(16);
        let state = AppState {
            pool: pool.clone(),
            event_tx,
            jwt_secret: "placeholder".to_string(),
            ewelink: crate::ewelink::EwelinkHandle::spawn(),
            mail: crate::mail::MailHandle::spawn(),
            public_base_url: String::new(),
            door_rate_limit: std::sync::Arc::new(std::sync::Mutex::new(
                crate::routes::door::RateLimiter::new(),
            )),
            login_link_rate_limit: std::sync::Arc::new(std::sync::Mutex::new(
                crate::routes::auth::LoginLinkRateLimiter::new(),
            )),
        };

        // test_mode=true mounts routes::test_fixtures::routes(), which now
        // includes the panic-only-under-SPINBIKE_TEST_MODE /api/test/panic
        // route (#172). Whatever layer stack build_router applies (or
        // doesn't) is exactly what production gets via start_server, too.
        let app: Router = build_router(true).with_state(state);

        let panic_req = Request::builder()
            .method(Method::GET)
            .uri("/api/test/panic")
            .body(Body::empty())
            .unwrap();
        let panic_resp = app.clone().oneshot(panic_req).await.unwrap();
        assert_eq!(
            panic_resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "a panicking handler must be caught and turned into a 500 response, \
             not left to crash the process"
        );

        // The server must still be alive and able to serve OTHER requests —
        // the entire point of catching the panic instead of aborting.
        let health_req = Request::builder()
            .method(Method::GET)
            .uri("/api/version")
            .body(Body::empty())
            .unwrap();
        let health_resp = app.clone().oneshot(health_req).await.unwrap();
        assert_eq!(
            health_resp.status(),
            StatusCode::OK,
            "server must still serve requests after a handler panic was caught"
        );
    }
}
