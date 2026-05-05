//! Shared helpers for HTTP integration tests.
//!
//! Spins up an in-memory DB + the real Axum router, and lets tests send
//! requests via `tower::ServiceExt::oneshot` without binding a TCP port.

// Each tests/*.rs file is a separate binary; clippy flags helpers not used
// by that particular file as dead code. Suppressing — unused per-binary is
// expected when a helper is shared across the suite.
#![allow(dead_code)]

use axum::{Router, body::Body};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use serde_json::Value;
use spinbike_core::auth::Role;
use spinbike_server::AppState;
use spinbike_server::auth::{create_token, hash_password};
use spinbike_server::db::{self, users};
use spinbike_server::routes;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tower::util::ServiceExt;

pub const JWT_SECRET: &str = "test-secret-for-integration";

pub struct TestApp {
    pub router: Router,
    pub pool: SqlitePool,
    pub admin_token: String,
    pub staff_token: String,
    pub customer_token: String,
    pub admin_id: i64,
    pub staff_id: i64,
    pub customer_id: i64,
    /// The user_id of the pre-seeded customer (formerly customer_card_id).
    /// This field retains its old name so existing tests continue to compile
    /// without mechanical renaming — semantically it is the customer's user_id.
    pub customer_card_id: i64,
}

impl TestApp {
    pub async fn new() -> Self {
        let pool = db::create_memory_pool().await.unwrap();
        db::run_migrations(&pool).await.unwrap();

        let hash = hash_password("password").unwrap();
        let admin_id = users::create_user(
            &pool,
            "admin@test.com",
            Some(&hash),
            "Admin",
            None,
            "admin",
            None,
            None,
        )
        .await
        .unwrap();
        let staff_id = users::create_user(
            &pool,
            "staff@test.com",
            Some(&hash),
            "Staff",
            None,
            "staff",
            None,
            None,
        )
        .await
        .unwrap();
        let customer_id = users::create_user(
            &pool,
            "user@test.com",
            Some(&hash),
            "User",
            None,
            "customer",
            None,
            None,
        )
        .await
        .unwrap();

        let admin_token =
            create_token(JWT_SECRET, admin_id, "admin@test.com", &Role::Admin).unwrap();
        let staff_token =
            create_token(JWT_SECRET, staff_id, "staff@test.com", &Role::Staff).unwrap();
        let customer_token =
            create_token(JWT_SECRET, customer_id, "user@test.com", &Role::Customer).unwrap();

        // customer_card_id now stores the customer's user_id (cards table was
        // dropped in V13). The field name is kept for source-compat with tests
        // that use it as a user identifier for transaction/booking seeds.
        let customer_card_id = customer_id;

        let (event_tx, _) = broadcast::channel(16);
        let state = AppState {
            pool: pool.clone(),
            event_tx,
            jwt_secret: JWT_SECRET.to_string(),
        };
        // TestApp always merges test_fixtures regardless of SPINBIKE_TEST_MODE —
        // the harness knows it's a test context. start_server() in production uses
        // the env-var gate instead.
        let router = routes::all_routes()
            .merge(spinbike_server::routes::test_fixtures::routes())
            .with_state(state);

        Self {
            router,
            pool,
            admin_token,
            staff_token,
            customer_token,
            admin_id,
            staff_id,
            customer_id,
            customer_card_id,
        }
    }

    /// Returns the id of the Spinning service (always active in the test DB).
    pub async fn spinning_service_id(&self) -> i64 {
        sqlx::query_scalar("SELECT id FROM services WHERE name_en = 'Spinning' AND active = 1")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// Seed a user with credit and optional metadata. Returns the user_id.
    ///
    /// Named `seed_card` for backwards-compat with all existing test callers.
    /// The `barcode` parameter maps to `card_code`; `first_name`/`last_name`
    /// are concatenated into `name`.
    #[allow(clippy::too_many_arguments)]
    pub async fn seed_card(
        &self,
        barcode: &str,
        credit: f64,
        first_name: Option<&str>,
        last_name: Option<&str>,
        company: Option<&str>,
        phone: Option<&str>,
    ) -> i64 {
        let name = match (first_name, last_name) {
            (Some(f), Some(l)) => format!("{f} {l}"),
            (Some(f), None) => f.to_string(),
            (None, Some(l)) => l.to_string(),
            (None, None) => barcode.to_string(),
        };
        let initial_credit = if credit != 0.0 { Some(credit) } else { None };
        users::create_user(
            &self.pool,
            None, // email
            None, // password_hash
            &name,
            phone,
            company,
            Some(barcode), // card_code
            "customer",
            initial_credit,
            None, // oauth_provider
            None, // oauth_id
        )
        .await
        .unwrap()
    }

    /// Seed a user with the new user-keyed API. Returns the user_id.
    pub async fn seed_user(
        &self,
        name: &str,
        email: Option<&str>,
        credit: Option<f64>,
        card_code: Option<&str>,
    ) -> i64 {
        users::create_user(
            &self.pool, email, None, // password_hash
            name, None, // phone
            None, // company
            card_code, "customer", credit, None, None,
        )
        .await
        .unwrap()
    }

    pub async fn request(&self, req: axum::http::Request<Body>) -> (axum::http::StatusCode, Value) {
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        // Empty body is common (204 / Ok<StatusCode>) — treat as Null.
        let json: Value = if body_bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body_bytes).unwrap_or(Value::Null)
        };
        (status, json)
    }

    pub async fn request_typed<T: DeserializeOwned>(
        &self,
        req: axum::http::Request<Body>,
    ) -> (axum::http::StatusCode, T) {
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let typed: T = serde_json::from_slice(&body_bytes).unwrap();
        (status, typed)
    }
}

pub fn get(uri: &str, token: &str) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

pub fn post_json<B: serde::Serialize>(
    uri: &str,
    token: &str,
    body: &B,
) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

pub fn put_json<B: serde::Serialize>(
    uri: &str,
    token: &str,
    body: &B,
) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("PUT")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

pub fn patch_json<B: serde::Serialize>(
    uri: &str,
    token: &str,
    body: &B,
) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("PATCH")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

pub fn delete(uri: &str, token: &str) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}
