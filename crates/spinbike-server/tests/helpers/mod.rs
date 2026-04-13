//! Shared helpers for HTTP integration tests.
//!
//! Spins up an in-memory DB + the real Axum router, and lets tests send
//! requests via `tower::ServiceExt::oneshot` without binding a TCP port.

use axum::{Router, body::Body};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use serde_json::Value;
use spinbike_core::auth::Role;
use spinbike_server::AppState;
use spinbike_server::auth::{create_token, hash_password};
use spinbike_server::db::{self, cards as db_cards, users};
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

        let (event_tx, _) = broadcast::channel(16);
        let state = AppState {
            pool: pool.clone(),
            event_tx,
            jwt_secret: JWT_SECRET.to_string(),
        };
        // Use all_routes() so tests can exercise the static-file fallback too.
        let router = routes::all_routes().with_state(state);

        Self {
            router,
            pool,
            admin_token,
            staff_token,
            customer_token,
            admin_id,
            staff_id,
            customer_id,
        }
    }

    /// Seed a card with credit and optional metadata. Returns the card id.
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
        db_cards::create_card_with_info(
            &self.pool, barcode, credit, first_name, last_name, company, phone,
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

pub fn delete(uri: &str, token: &str) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}
