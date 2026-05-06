use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::users;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SeedUserRequest {
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub card_code: Option<String>,
    pub credit: Option<f64>,
}

/// `barcode` is kept as the field name so existing E2E callers remain
/// source-compatible. Internally it maps to `users.card_code`.
#[derive(Deserialize)]
pub struct SeedExpiredPassRequest {
    pub barcode: String,
    pub valid_until: chrono::NaiveDate,
}

/// `barcode` is kept for E2E source-compatibility; maps to `users.card_code`.
#[derive(Deserialize)]
pub struct SeedTransactionsRequest {
    pub barcode: String,
    pub entries: Vec<SeedEntry>,
}

#[derive(Deserialize)]
pub struct SeedEntry {
    pub amount: f64,
    pub action: String,
    pub service_name_sk: String,
    /// Optional pass-sale expiry. None for normal transactions; Some(date)
    /// when the seeded row should classify as PassSale. The serde default
    /// keeps existing E2E callers source-compatible.
    #[serde(default)]
    pub valid_until: Option<chrono::NaiveDate>,
    /// Optional override of the row's created_at. Format: "YYYY-MM-DD HH:MM:SS"
    /// (the SQLite literal). When omitted, the handler uses datetime('now') as
    /// before. Used by E2E tests that need to seed historical visits at specific
    /// timestamps to exercise the relative-time bucket logic (issue #57).
    #[serde(default)]
    pub created_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn routes() -> Router<AppState> {
    // Only registered when SPINBIKE_TEST_MODE=1.
    Router::new()
        .route("/api/test/seed-expired-pass", post(seed_expired_pass))
        .route("/api/test/seed-transactions", post(seed_transactions))
        .route("/api/test/seed-user", post(seed_user))
        // Legacy alias kept so E2E tests that still call seed-credit continue
        // to work; the handler interprets the body as {barcode, credit} and
        // creates/updates the matching user's credit field.
        .route("/api/test/seed-credit", post(seed_credit_compat))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look up a user by card_code, or create a stub user if none exists.
/// Returns the user_id.
async fn find_or_create_user_by_card_code(
    pool: &sqlx::SqlitePool,
    card_code: &str,
) -> Result<i64, (StatusCode, String)> {
    // Try existing user first.
    let existing = users::get_user_by_card_code(pool, card_code)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(row) = existing {
        return Ok(row.id);
    }

    // Create a stub user whose name equals the card_code so the test DB
    // has a recognisable identity.
    users::create_user(
        pool,
        None,            // email
        None,            // password_hash
        card_code,       // name (stub)
        None,            // phone
        None,            // company
        Some(card_code), // card_code
        "customer",
        None, // initial_credit
        None, // oauth_provider
        None, // oauth_id
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Create a user with optional initial credit. Returns `{"user_id": N}`.
async fn seed_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedUserRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    // Defence in depth: even though this route is env-gated, require staff role
    // to guard against misconfiguration.
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, "Staff required".into()));
    }
    let user_id = users::create_user(
        &state.pool,
        body.email.as_deref(),
        None,
        &body.name,
        body.phone.as_deref(),
        body.company.as_deref(),
        body.card_code.as_deref(),
        "customer",
        body.credit,
        None,
        None,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "user_id": user_id })),
    ))
}

/// Seed-credit compatibility shim: accepts `{barcode, credit}` and sets the
/// matching user's credit. Creates a stub user if the card_code is unknown.
#[derive(Deserialize)]
struct SeedCreditCompatRequest {
    barcode: String,
    credit: f64,
}

async fn seed_credit_compat(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedCreditCompatRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, "Staff required".into()));
    }
    let user_id = find_or_create_user_by_card_code(&state.pool, &body.barcode).await?;
    sqlx::query("UPDATE users SET credit = ROUND(?, 2) WHERE id = ?")
        .bind(body.credit)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "user_id": user_id })))
}

/// Seed an expired monthly pass for the user identified by `barcode`
/// (= card_code). Creates a stub user if the card_code is unknown.
async fn seed_expired_pass(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedExpiredPassRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Defence in depth: even though this route is env-gated, require staff role
    // to guard against misconfiguration.
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, "Staff required".into()));
    }
    let user_id = find_or_create_user_by_card_code(&state.pool, &body.barcode).await?;

    // Look up the service id and its current default_price to avoid hardcoding.
    let (pass_service_id, pass_price): (i64, f64) =
        sqlx::query_as("SELECT id, default_price FROM services WHERE kind = 'monthly_pass'")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, ?, 'charge', ?, datetime('now'))",
    )
    .bind(user_id)
    .bind(pass_service_id)
    .bind(-pass_price)
    .bind(body.valid_until)
    .execute(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "user_id": user_id })))
}

/// Seed a user (if missing by card_code) and insert one transaction per entry,
/// each pre-linked to the service whose `name_sk` matches `service_name_sk`.
/// Used by E2E to verify backfilled history rendering — flags the rows with
/// `legacy_backfilled = 1` so they look like backfill output.
async fn seed_transactions(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedTransactionsRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, "Staff required".into()));
    }

    let user_id = find_or_create_user_by_card_code(&state.pool, &body.barcode).await?;

    let count = body.entries.len();
    for e in body.entries {
        let svc_id: Option<i64> = sqlx::query_scalar("SELECT id FROM services WHERE name_sk = ?")
            .bind(&e.service_name_sk)
            .fetch_optional(&state.pool)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        // COALESCE lets callers override created_at for historical seeds (issue #57).
        // When None is bound, COALESCE falls back to datetime('now') — same as the
        // original hard-coded default.
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, legacy_backfilled, created_at)
             VALUES (?, ?, ?, ?, ?, 1, COALESCE(?, datetime('now')))",
        )
        .bind(user_id)
        .bind(svc_id)
        .bind(e.amount)
        .bind(&e.action)
        .bind(e.valid_until)
        .bind(e.created_at.as_deref())
        .execute(&state.pool)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    }

    Ok(Json(
        serde_json::json!({ "user_id": user_id, "count": count }),
    ))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::find_or_create_user_by_card_code;
    use crate::db;

    // Mutant #2: replace find_or_create_user_by_card_code → Ok(1).
    // If the function always returns Ok(1), id1 == id2 == 1 and the assert_ne!
    // fires. The find-branch test also fails because id1 != 1 on subsequent
    // calls (two different codes can't both be user 1 in a fresh DB).
    #[tokio::test]
    async fn find_or_create_user_by_card_code_creates_distinct_users() {
        let pool = db::create_memory_pool().await.unwrap();
        db::run_migrations(&pool).await.unwrap();

        let id1 = find_or_create_user_by_card_code(&pool, "TESTCODE_A")
            .await
            .unwrap();
        let id2 = find_or_create_user_by_card_code(&pool, "TESTCODE_B")
            .await
            .unwrap();
        assert_ne!(id1, id2, "different codes must yield different user IDs");
        assert!(id1 > 0);
        assert!(id2 > 0);

        let again = find_or_create_user_by_card_code(&pool, "TESTCODE_A")
            .await
            .unwrap();
        assert_eq!(
            again, id1,
            "same code must return the same user (find path)"
        );
    }
}
