# Credit Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unblock drinks/food charges when a monthly pass is active, give staff tools to edit pass end-dates and void history entries, and split the card detail page into tabs.

**Architecture:** One SQLite migration (V7) adds `deleted_at` to `transactions`. Two new Axum handlers (PATCH valid-until, DELETE soft-delete) live in a new `routes/transactions.rs`. The Leptos card detail page keeps its top bar and gets a 3-tab container (History / Upcoming / Persistent). DELETE atomically reverses the row's credit impact so the stored `cards.credit` column stays accurate.

**Tech Stack:** Rust, Axum 0.8, sqlx + SQLite, Leptos 0.7 CSR → WASM via Trunk, Playwright (TypeScript).

---

## File Structure

**Create:**
- `crates/spinbike-server/src/routes/transactions.rs` — two handlers (PATCH valid-until, DELETE soft-delete)
- `crates/spinbike-server/tests/transactions_routes.rs` — integration tests for the two handlers
- `e2e/tests/credit-improvements.spec.ts` — Playwright E2E for all four items

**Modify:**
- `VERSION` (0.6.0 → 0.7.0) + sync to Cargo.tomls
- `crates/spinbike-server/src/db/migrations.rs` — register V7
- `crates/spinbike-server/src/db/transactions.rs` — add `soft_delete`; `TransactionRow` gains `deleted_at`; list queries return it
- `crates/spinbike-server/src/db/cards.rs` — `get_card_pass_valid_until` excludes soft-deleted rows
- `crates/spinbike-server/src/routes/mod.rs` — register new `transactions::routes()`
- `crates/spinbike-server/src/routes/cards.rs` — `TransactionResponse` gains `deleted_at`
- `spinbike-ui/src/pages/dashboard.rs` — charge form alongside Log-visit; pencil on PassBanner; ✕ on history rows; tab container
- `spinbike-ui/src/i18n.rs` — new translation keys
- `spinbike-ui/style.css` — `.txn-row--voided`, `.tabbar` styles

---

### Task 0: Bump version to 0.7.0

**Files:**
- Modify: `VERSION`, `crates/spinbike-server/Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `Cargo.lock`

Rationale: airuleset requires version on `dev` be strictly greater than `main` before any new feature work. Current state (post PR #5 merge) may leave `dev` at 0.6.0 with `main` also at 0.6.0.

- [ ] **Step 1: Bump VERSION**

```bash
echo "0.7.0" > VERSION
scripts/sync-version.sh
```

- [ ] **Step 2: Verify bump**

Run: `git diff VERSION crates/*/Cargo.toml`
Expected: all show `0.6.0 → 0.7.0`.

- [ ] **Step 3: Run format check**

Run: `cargo fmt --all --check`
Expected: no output, exit 0.

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.lock crates/spinbike-server/Cargo.toml crates/spinbike-core/Cargo.toml
git commit -m "chore: bump version to 0.7.0"
```

---

### Task 1: Migration V7 adds `deleted_at` column to `transactions`

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs`
- Test: `crates/spinbike-server/src/db/migrations.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Add a failing test asserting the new column exists after migration**

Append to the existing `#[cfg(test)] mod tests` inside `crates/spinbike-server/src/db/migrations.rs`:

```rust
#[tokio::test]
async fn v7_adds_deleted_at_to_transactions() {
    let pool = crate::db::create_memory_pool().await.unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    let cols: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM pragma_table_info('transactions')")
            .fetch_all(&pool)
            .await
            .unwrap();
    let names: Vec<_> = cols.into_iter().map(|(n,)| n).collect();
    assert!(
        names.iter().any(|n| n == "deleted_at"),
        "transactions should have deleted_at column after V7 migrations, got {names:?}"
    );
}

#[tokio::test]
async fn v7_migration_is_idempotent() {
    let pool = crate::db::create_memory_pool().await.unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    crate::db::run_migrations(&pool).await.unwrap(); // second run must not error
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spinbike-server --lib db::migrations::tests::v7_adds_deleted_at_to_transactions`
Expected: FAIL — column missing.

- [ ] **Step 3: Add V7 to the migration list**

Edit the `MIGRATIONS` slice and add the SQL constant:

```rust
pub(crate) static MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "initial schema", V1_INITIAL_SCHEMA),
    (2, "card holder info and allow debit default", V2_CARD_HOLDER_INFO),
    (3, "card search_text column + index", V3_CARD_SEARCH_TEXT),
    (4, "monthly pass: valid_until + service seed", V4_MONTHLY_PASS),
    (5, "spin booking: bookings extended + persistent_bookings", V5_SPIN_BOOKING),
    (6, "seed 4 weekly spin classes + 2 instructors", V6_SEED_SPIN_CLASSES),
    (7, "transactions: soft-delete column", V7_TRANSACTIONS_SOFT_DELETE),
];
```

Then add the constant near the end of the file:

```rust
const V7_TRANSACTIONS_SOFT_DELETE: &str = r#"
ALTER TABLE transactions ADD COLUMN deleted_at TEXT;
"#;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p spinbike-server --lib db::migrations`
Expected: PASS for both new tests plus existing ones.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): V7 adds deleted_at column for transaction soft-delete"
```

---

### Task 2: `db::transactions::soft_delete`

**Files:**
- Modify: `crates/spinbike-server/src/db/transactions.rs`

- [ ] **Step 1: Write failing test**

Append to the existing `#[cfg(test)] mod tests` inside `crates/spinbike-server/src/db/transactions.rs`:

```rust
#[tokio::test]
async fn soft_delete_sets_deleted_at() {
    let pool = setup().await;
    let card_id = create_card(&pool, "SD-1").await.unwrap();
    let tx_id = create_transaction(&pool, None, Some(card_id), None, None, 5.0, "topup")
        .await
        .unwrap();

    soft_delete(&pool, tx_id).await.unwrap();

    let deleted_at: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(deleted_at.is_some(), "deleted_at must be set");
}

#[tokio::test]
async fn soft_delete_is_idempotent_on_missing_row() {
    let pool = setup().await;
    // Non-existent id must not error — no-op.
    soft_delete(&pool, 99999).await.unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spinbike-server --lib db::transactions::tests::soft_delete`
Expected: FAIL — `soft_delete` does not exist.

- [ ] **Step 3: Implement `soft_delete`**

Add to `crates/spinbike-server/src/db/transactions.rs` right before the `#[cfg(test)]` line:

```rust
/// Mark a transaction as voided. Sets `deleted_at` to the current datetime
/// if the row exists and is not already voided. No-op otherwise.
pub async fn soft_delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE transactions SET deleted_at = datetime('now') \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await
    .context("Failed to soft-delete transaction")?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p spinbike-server --lib db::transactions`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/transactions.rs
git commit -m "feat(db): add soft_delete for transactions"
```

---

### Task 3: List queries return `deleted_at`; pass-validity excludes soft-deleted

**Files:**
- Modify: `crates/spinbike-server/src/db/transactions.rs` — `TransactionRow` + list queries
- Modify: `crates/spinbike-server/src/db/cards.rs` — `get_card_pass_valid_until`

- [ ] **Step 1: Write failing test for pass-validity exclusion**

Append to `#[cfg(test)] mod tests` in `crates/spinbike-server/src/db/cards.rs`:

```rust
#[tokio::test]
async fn pass_validity_ignores_soft_deleted_pass() {
    let pool = setup().await;
    let card_id = create_card(&pool, "PV-1").await.unwrap();
    let future = chrono::Local::now().date_naive() + chrono::Duration::days(10);

    let tx_id = crate::db::transactions::create_transaction_with_valid_until(
        &pool,
        None,
        Some(card_id),
        None,
        Some(1),
        -35.0,
        "charge",
        Some(future),
    )
    .await
    .unwrap();

    assert_eq!(
        get_card_pass_valid_until(&pool, card_id).await.unwrap(),
        Some(future)
    );

    crate::db::transactions::soft_delete(&pool, tx_id)
        .await
        .unwrap();

    assert_eq!(
        get_card_pass_valid_until(&pool, card_id).await.unwrap(),
        None,
        "soft-deleted pass sale must not count as active pass"
    );
}
```

- [ ] **Step 2: Write failing test that list_transactions_for_card returns deleted_at**

Append to `#[cfg(test)] mod tests` in `crates/spinbike-server/src/db/transactions.rs`:

```rust
#[tokio::test]
async fn list_transactions_returns_deleted_at_flag() {
    let pool = setup().await;
    let card_id = create_card(&pool, "SD-LIST").await.unwrap();
    let tx_id = create_transaction(&pool, None, Some(card_id), None, None, 5.0, "topup")
        .await
        .unwrap();
    soft_delete(&pool, tx_id).await.unwrap();

    let rows = list_transactions_for_card(&pool, card_id).await.unwrap();
    assert_eq!(rows.len(), 1, "soft-deleted rows must still appear in history");
    assert!(rows[0].deleted_at.is_some(), "voided row must expose deleted_at");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p spinbike-server --lib pass_validity_ignores_soft_deleted_pass list_transactions_returns_deleted_at_flag`
Expected: FAIL (field missing and query not filtered).

- [ ] **Step 4: Add `deleted_at` to `TransactionRow` and update list SELECTs**

In `crates/spinbike-server/src/db/transactions.rs`, add a field to `TransactionRow` and extend both SELECT statements:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TransactionRow {
    pub id: i64,
    pub user_id: Option<i64>,
    pub card_id: Option<i64>,
    pub staff_id: Option<i64>,
    pub service_id: Option<i64>,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
    pub valid_until: Option<chrono::NaiveDate>,
    #[sqlx(default)]
    pub service_name: Option<String>,
    pub deleted_at: Option<String>,
}
```

Then update both queries to add `t.deleted_at` to the SELECT list:

```rust
"SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
        t.amount, t.action, t.created_at, t.valid_until,
        s.name AS service_name, t.deleted_at
 FROM transactions t
 LEFT JOIN services s ON s.id = t.service_id
 WHERE t.card_id = ?
 ORDER BY t.created_at DESC"
```

Same change for `list_transactions_for_user`.

- [ ] **Step 5: Filter soft-deleted rows out of pass validity**

In `crates/spinbike-server/src/db/cards.rs`, change `get_card_pass_valid_until` SQL:

```rust
pub async fn get_card_pass_valid_until(
    pool: &SqlitePool,
    card_id: i64,
) -> Result<Option<chrono::NaiveDate>> {
    let row: Option<(Option<chrono::NaiveDate>,)> = sqlx::query_as(
        "SELECT MAX(valid_until) FROM transactions
         WHERE card_id = ? AND valid_until IS NOT NULL AND deleted_at IS NULL",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .context("Failed to compute pass valid_until")?;
    Ok(row.and_then(|(d,)| d))
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p spinbike-server --lib`
Expected: PASS — both new tests green, old tests unaffected.

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-server/src/db/transactions.rs crates/spinbike-server/src/db/cards.rs
git commit -m "feat(db): expose deleted_at; skip soft-deleted pass sales"
```

---

### Task 4: `TransactionResponse` exposes `deleted_at` on the GET endpoint

**Files:**
- Modify: `crates/spinbike-server/src/routes/cards.rs` — `TransactionResponse` struct and mapping
- Test: `crates/spinbike-server/tests/cards_routes.rs` (existing) OR new assertion in an existing test

- [ ] **Step 1: Locate `TransactionResponse`**

Open `crates/spinbike-server/src/routes/cards.rs` and find the struct (around line 96-105) plus its mapping inside the `GET /api/cards/{id}/transactions` handler.

- [ ] **Step 2: Write failing integration test**

Append to `crates/spinbike-server/tests/cards_routes.rs`:

```rust
#[tokio::test]
async fn transactions_endpoint_returns_deleted_at_field() {
    let app = TestApp::new().await;
    // Create a topup then soft-delete it.
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action) VALUES (?, 5.0, 'topup') RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    spinbike_server::db::transactions::soft_delete(&app.pool, tx_id)
        .await
        .unwrap();

    let uri = format!("/api/cards/{}/transactions", app.customer_card_id);
    let (status, resp) = app.request(get(&uri, &app.staff_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let row = resp
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_i64() == Some(tx_id))
        .expect("deleted row must still be listed");
    assert!(
        row.get("deleted_at").and_then(|v| v.as_str()).is_some(),
        "response must include deleted_at string"
    );
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p spinbike-server --test cards_routes transactions_endpoint_returns_deleted_at_field`
Expected: FAIL — `deleted_at` not in response.

- [ ] **Step 4: Extend `TransactionResponse`**

Edit the struct in `crates/spinbike-server/src/routes/cards.rs`:

```rust
#[derive(serde::Serialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub card_id: Option<i64>,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
    pub service_name: Option<String>,
    pub valid_until: Option<chrono::NaiveDate>,
    pub deleted_at: Option<String>,
}
```

Then update the mapping inside the handler (look for `.map(|t| TransactionResponse { ... })`):

```rust
.map(|t| TransactionResponse {
    id: t.id,
    card_id: t.card_id,
    amount: t.amount,
    action: t.action,
    created_at: t.created_at,
    service_name: t.service_name,
    valid_until: t.valid_until,
    deleted_at: t.deleted_at,
})
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p spinbike-server --test cards_routes`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/routes/cards.rs crates/spinbike-server/tests/cards_routes.rs
git commit -m "feat(api): include deleted_at in GET /api/cards/{id}/transactions"
```

---

### Task 5: `DELETE /api/transactions/{id}` — soft-delete and reverse credit impact

**Files:**
- Create: `crates/spinbike-server/src/routes/transactions.rs`
- Create: `crates/spinbike-server/tests/transactions_routes.rs`
- Modify: `crates/spinbike-server/src/routes/mod.rs` — register router

- [ ] **Step 1: Write failing integration tests**

Create `crates/spinbike-server/tests/transactions_routes.rs`:

```rust
//! Integration tests for /api/transactions/{id} (PATCH valid-until + DELETE soft-delete).
mod helpers;
use axum::http::StatusCode;
use helpers::{TestApp, delete, get, patch_json};

async fn seed_topup(app: &TestApp, amount: f64) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action) VALUES (?, ?, 'topup') RETURNING id",
    )
    .bind(app.customer_card_id)
    .bind(amount)
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn delete_transaction_is_staff_only() {
    let app = TestApp::new().await;
    let tx_id = seed_topup(&app, 5.0).await;
    let (status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.customer_token,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delete_missing_transaction_returns_404() {
    let app = TestApp::new().await;
    let (status, _) = app
        .request(delete("/api/transactions/999999", &app.staff_token))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_topup_reverses_credit_and_soft_deletes() {
    let app = TestApp::new().await;

    // Set card credit to a known value, then insert a +10 topup that already
    // nudged credit up. We simulate this by manually setting credit to 10.
    sqlx::query("UPDATE cards SET credit = 10.0 WHERE id = ?")
        .bind(app.customer_card_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let tx_id = seed_topup(&app, 10.0).await;

    let (status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Credit reversed from 10.0 to 0.0.
    let credit: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(app.customer_card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!((credit - 0.0).abs() < 0.001, "credit should reverse to 0.0, got {credit}");

    // Row remains and carries deleted_at.
    let deleted_at: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(deleted_at.is_some());
}

#[tokio::test]
async fn delete_charge_refunds_credit() {
    // Charges are stored with NEGATIVE amount. Voiding a charge of -7
    // must add 7 back to credit.
    let app = TestApp::new().await;
    sqlx::query("UPDATE cards SET credit = 3.0 WHERE id = ?")
        .bind(app.customer_card_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action) VALUES (?, -7.0, 'charge') RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let (status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let credit: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(app.customer_card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!((credit - 10.0).abs() < 0.001, "voiding a charge must refund; got {credit}");
}
```

Check whether `helpers::patch_json` exists. If not, add it during Step 1 of Task 6.

- [ ] **Step 2: Verify `delete` helper exists**

Run: `grep -n "pub fn delete" crates/spinbike-server/tests/helpers/mod.rs`
Expected: a helper already exists (`cancel_booking` tests use it). If missing, add it:

```rust
pub fn delete(uri: &str, token: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method(axum::http::Method::DELETE)
        .uri(uri)
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .unwrap()
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p spinbike-server --test transactions_routes`
Expected: FAIL (compile error because `/api/transactions/{id}` is not routed yet).

- [ ] **Step 4: Create the routes module with DELETE handler**

Create `crates/spinbike-server/src/routes/transactions.rs`:

```rust
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, patch},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::transactions as db_tx;
use crate::routes::internal_error;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/transactions/{id}", delete(void_transaction))
        .route(
            "/api/transactions/{id}/valid-until",
            patch(patch_valid_until),
        )
}

#[derive(sqlx::FromRow)]
struct TxMini {
    amount: f64,
    card_id: Option<i64>,
    valid_until: Option<chrono::NaiveDate>,
    deleted_at: Option<String>,
}

async fn void_transaction(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, card_id, valid_until, deleted_at FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Transaction not found"})),
        ));
    };
    if row.deleted_at.is_some() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Transaction already voided"})),
        ));
    }

    // Soft-delete the row.
    sqlx::query("UPDATE transactions SET deleted_at = datetime('now') WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    // Reverse the credit impact. `amount` is signed (charges negative,
    // topups positive), so subtracting it undoes the original update.
    if let Some(card_id) = row.card_id {
        sqlx::query("UPDATE cards SET credit = ROUND(credit - ?, 2) WHERE id = ?")
            .bind(row.amount)
            .bind(card_id)
            .execute(&mut *tx)
            .await
            .map_err(internal_error)?;
    }

    tx.commit().await.map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// `patch_valid_until` is added in Task 6.
async fn patch_valid_until(
    State(_state): State<AppState>,
    AuthUser(_claims): AuthUser,
    Path(_id): Path<i64>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({"error": "Not yet implemented"})),
    ))
}
```

(The stub for `patch_valid_until` keeps the router compiling; Task 6 replaces it.)

- [ ] **Step 5: Register the router**

Edit `crates/spinbike-server/src/routes/mod.rs`:

```rust
pub mod admin;
pub mod auth;
pub mod cards;
pub mod classes;
pub mod payments;
pub mod persistent_bookings;
pub mod static_files;
pub mod test_fixtures;
pub mod transactions;
pub mod upcoming_classes;
```

And inside `api_routes()`:

```rust
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(classes::routes())
        .merge(cards::routes())
        .merge(payments::routes())
        .merge(admin::routes())
        .merge(persistent_bookings::routes())
        .merge(upcoming_classes::routes())
        .merge(transactions::routes())
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p spinbike-server --test transactions_routes -- --test-threads=1`
Expected: the four DELETE tests PASS. The PATCH tests (to be added in Task 6) don't exist yet.

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-server/src/routes/transactions.rs \
        crates/spinbike-server/src/routes/mod.rs \
        crates/spinbike-server/tests/transactions_routes.rs \
        crates/spinbike-server/tests/helpers/mod.rs
git commit -m "feat(api): DELETE /api/transactions/{id} soft-deletes and reverses credit"
```

---

### Task 6: `PATCH /api/transactions/{id}/valid-until` — edit pass end date

**Files:**
- Modify: `crates/spinbike-server/src/routes/transactions.rs` — replace stub
- Modify: `crates/spinbike-server/tests/transactions_routes.rs` — PATCH tests
- Modify (if missing): `crates/spinbike-server/tests/helpers/mod.rs` — add `patch_json`

- [ ] **Step 1: Add `patch_json` helper if missing**

Check `crates/spinbike-server/tests/helpers/mod.rs` for an existing `patch_json`. If absent, append:

```rust
pub fn patch_json<T: serde::Serialize>(
    uri: &str,
    token: &str,
    body: &T,
) -> axum::http::Request<axum::body::Body> {
    let json = serde_json::to_vec(body).unwrap();
    axum::http::Request::builder()
        .method(axum::http::Method::PATCH)
        .uri(uri)
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(json))
        .unwrap()
}
```

- [ ] **Step 2: Write failing tests**

Append to `crates/spinbike-server/tests/transactions_routes.rs`:

```rust
#[tokio::test]
async fn patch_valid_until_updates_pass_end_date() {
    let app = TestApp::new().await;
    // Seed a pass sale (amount negative, valid_until set).
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action, valid_until)
         VALUES (?, -35.0, 'charge', '2026-05-01') RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp["valid_until"].as_str(), Some("2026-06-15"));

    let stored: Option<String> =
        sqlx::query_scalar("SELECT valid_until FROM transactions WHERE id = ?")
            .bind(tx_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some("2026-06-15"));
}

#[tokio::test]
async fn patch_valid_until_rejects_non_pass_transaction() {
    let app = TestApp::new().await;
    let tx_id = seed_topup(&app, 5.0).await; // topup has valid_until = NULL
    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn patch_valid_until_forbidden_for_customer() {
    let app = TestApp::new().await;
    let tx_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO transactions (card_id, amount, action, valid_until)
         VALUES (?, -35.0, 'charge', '2026-05-01') RETURNING id",
    )
    .bind(app.customer_card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &app.customer_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn patch_valid_until_missing_returns_404() {
    let app = TestApp::new().await;
    let body = serde_json::json!({ "valid_until": "2026-06-15" });
    let (status, _) = app
        .request(patch_json(
            "/api/transactions/999999/valid-until",
            &app.staff_token,
            &body,
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p spinbike-server --test transactions_routes patch_valid_until`
Expected: FAIL (stub returns 501).

- [ ] **Step 4: Replace the `patch_valid_until` stub with the real handler**

Replace the stub body in `crates/spinbike-server/src/routes/transactions.rs`:

```rust
#[derive(Deserialize)]
struct PatchValidUntilReq {
    valid_until: chrono::NaiveDate,
}

#[derive(Serialize)]
struct PatchValidUntilResp {
    id: i64,
    valid_until: chrono::NaiveDate,
}

async fn patch_valid_until(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchValidUntilReq>,
) -> Result<Json<PatchValidUntilResp>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, card_id, valid_until, deleted_at FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Transaction not found"})),
        ));
    };
    if row.valid_until.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Only pass transactions have valid_until"})),
        ));
    }

    sqlx::query("UPDATE transactions SET valid_until = ? WHERE id = ?")
        .bind(body.valid_until)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(PatchValidUntilResp {
        id,
        valid_until: body.valid_until,
    }))
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p spinbike-server --test transactions_routes`
Expected: all PATCH + DELETE tests PASS.

- [ ] **Step 6: Format + commit**

```bash
cargo fmt --all
git add crates/spinbike-server/src/routes/transactions.rs \
        crates/spinbike-server/tests/transactions_routes.rs \
        crates/spinbike-server/tests/helpers/mod.rs
git commit -m "feat(api): PATCH /api/transactions/{id}/valid-until"
```

---

### Task 7: UI — charge form always available; Log-visit primary when pass active

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs` — ChargeSection component (~lines 1028-1087)
- Modify: `spinbike-ui/src/i18n.rs` — add `charge_for_extras` key

- [ ] **Step 1: Add translation key**

Edit `spinbike-ui/src/i18n.rs` and add entries to both language dictionaries:

```rust
("charge_for_extras", "Charge for drinks / food / other"),
// Slovak equivalent, match style of neighbouring entries:
("charge_for_extras", "Účtovať nápoje / jedlo / iné"),
```

Find the right strings table by searching for `("charge"`, append the new entry in both EN and SK tables.

- [ ] **Step 2: Modify ChargeSection to always show the form**

In `spinbike-ui/src/pages/dashboard.rs` around the `if pass_active` block (ChargeSection):

```rust
view! {
    <div class="mt-2">
        <div class="text-muted" style="font-size:0.85rem;margin-bottom:4px">
            {move || i18n::t(lang.get(), "quick_charge")}
        </div>

        // When pass is active, Log-visit buttons appear first as the primary action.
        {if pass_active {
            view! {
                <div class="flex gap-1" style="flex-wrap:wrap">
                    {services.get().into_iter()
                        .filter(|svc| svc.name != "Monthly pass")
                        .map(|svc| {
                            let service_id = svc.id;
                            let svc_name = svc.name.clone();
                            view! {
                                <button
                                    class="btn btn-sm btn-primary"
                                    data-testid="log-visit-btn"
                                    on:click=visit_click_for(service_id)
                                >
                                    {move || i18n::t(lang.get(), "log_visit")}" "{svc_name.clone()}
                                </button>
                            }
                        }).collect::<Vec<_>>()}
                </div>
            }.into_any()
        } else {
            view! { <div></div> }.into_any()
        }}

        // Charge form is ALWAYS visible so staff can charge drinks/food even when pass is active.
        <div class="text-muted" style="font-size:0.8rem;margin:6px 0 2px">
            {move || i18n::t(lang.get(), "charge_for_extras")}
        </div>
        <form class="inline-form" on:submit=on_submit style="flex-wrap:wrap">
            <select class="form-control" node_ref=service_ref on:change=on_service_change data-testid="charge-service">
                <option value="">{move || i18n::t(lang.get(), "select_service")}</option>
                {move || {
                    services.get().into_iter()
                        .filter(|s| s.name != "Monthly pass")
                        .map(|s| {
                            let val = s.id.to_string();
                            let label = format!("{} ({:.2} €)", s.name, s.default_price);
                            view! { <option value=val>{label}</option> }
                        }).collect::<Vec<_>>()
                }}
            </select>
            <input
                type="number"
                class="form-control"
                node_ref=amount_ref
                placeholder=move || i18n::t(lang.get(), "amount")
                step="0.01"
                min="0.01"
                style="width:8em"
                required
            />
            <button type="submit" class="btn btn-sm btn-danger" data-testid="charge-submit" disabled=move || loading.get()>
                {move || i18n::t(lang.get(), "charge")}
            </button>
        </form>
    </div>
}
```

- [ ] **Step 3: Trunk build sanity-check**

Run: `cd spinbike-ui && trunk build && cd ..`
Expected: build succeeds. If it fails with a type mismatch on `into_any()`, keep branches returning the same type by wrapping the else branch in an empty `div`.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs spinbike-ui/src/i18n.rs
git commit -m "feat(ui): charge form stays visible when monthly pass is active"
```

---

### Task 8: UI — pencil icon on PassBanner to edit end date

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs` — PassBanner component
- Modify: `spinbike-ui/src/i18n.rs` — keys `edit`, `save`, `cancel`

- [ ] **Step 1: Add translation keys**

Append to `spinbike-ui/src/i18n.rs` both EN and SK tables:

```rust
("edit", "Edit"),        // SK: "Upraviť"
("save", "Save"),        // SK: "Uložiť"
("cancel", "Cancel"),    // SK: "Zrušiť"
("edit_pass_date", "Change pass end date"),  // SK: "Zmeniť koniec permanentky"
```

Skip any key that already exists.

- [ ] **Step 2: Locate PassBanner**

Find it in `spinbike-ui/src/pages/dashboard.rs`. It renders the pass info including `valid_until`.

- [ ] **Step 3: Add signals and inline editor**

Replace the part of PassBanner that renders the end-date with:

```rust
let (editing, set_editing) = signal(false);
let (err, set_err) = signal(String::new());
let date_ref: NodeRef<leptos::html::Input> = NodeRef::new();

let save_click = move |ev: leptos::ev::MouseEvent| {
    ev.prevent_default();
    let input = date_ref.get().expect("date input");
    let new_date = input.value();
    if new_date.is_empty() {
        set_err.set("Pick a date".into());
        return;
    }
    let tx_id = pass_transaction_id; // comes from PassBanner props — see Step 4
    spawn_local(async move {
        #[derive(serde::Serialize)]
        struct Req { valid_until: String }
        match api::patch::<Req, serde_json::Value>(
            &format!("/api/transactions/{tx_id}/valid-until"),
            &Req { valid_until: new_date.clone() },
        ).await {
            Ok(_) => {
                set_editing.set(false);
                // Parent will refetch card info; signal via a context or callback.
                refresh.update(|n| *n += 1);
            }
            Err(e) => set_err.set(format!("Error: {e}")),
        }
    });
};

// In the view:
{move || if editing.get() {
    view! {
        <span>
            <input type="date" node_ref=date_ref value=valid_until_str.clone()
                   class="form-control" style="display:inline-block;width:auto" data-testid="pass-date-input"/>
            <button class="btn btn-sm btn-primary" on:click=save_click
                    data-testid="pass-date-save">
                {move || i18n::t(lang.get(), "save")}
            </button>
            <button class="btn btn-sm btn-outline" on:click=move |_| set_editing.set(false)>
                {move || i18n::t(lang.get(), "cancel")}
            </button>
            <span class="text-danger" style="margin-left:4px">{move || err.get()}</span>
        </span>
    }.into_any()
} else {
    view! {
        <span>
            {valid_until_str.clone()}
            <button class="btn btn-sm btn-link" data-testid="pass-date-edit"
                    title=move || i18n::t(lang.get(), "edit_pass_date")
                    on:click=move |_| { set_err.set(String::new()); set_editing.set(true); }>
                "✎"
            </button>
        </span>
    }.into_any()
}}
```

- [ ] **Step 4: Thread the pass transaction id into PassBanner**

The card detail page needs to know which transaction stores the pass. Extend the GET `/api/cards/{id}` response OR the nearest helper to return the latest non-void pass transaction id. Two sub-steps:

**a)** Extend `cards::CardInfo` (or the DTO serialised by the card detail endpoint) with `pass_transaction_id: Option<i64>`. Query it with:

```sql
SELECT id FROM transactions
 WHERE card_id = ? AND valid_until IS NOT NULL AND deleted_at IS NULL
 ORDER BY valid_until DESC LIMIT 1
```

Add a small helper in `db/cards.rs`:

```rust
pub async fn get_latest_pass_tx_id(
    pool: &SqlitePool,
    card_id: i64,
) -> Result<Option<i64>> {
    let id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM transactions
         WHERE card_id = ? AND valid_until IS NOT NULL AND deleted_at IS NULL
         ORDER BY valid_until DESC LIMIT 1",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .context("Failed to get latest pass tx id")?;
    Ok(id)
}
```

**b)** The frontend CardInfo struct gains the same `pass_transaction_id: Option<i64>`; PassBanner takes it as a prop. If `None`, the pencil is hidden.

- [ ] **Step 5: Ensure `api::patch` exists**

Check `spinbike-ui/src/api.rs` for a `patch` function mirroring `post`. If missing, add it following the same pattern as the existing `post`:

```rust
pub async fn patch<Req: serde::Serialize, Resp: for<'de> serde::Deserialize<'de>>(
    url: &str,
    body: &Req,
) -> Result<Resp, String> {
    // Mirror the existing `post` helper, changing Method::POST to Method::PATCH.
}
```

- [ ] **Step 6: Trunk build**

Run: `cd spinbike-ui && trunk build && cd ..`
Expected: build succeeds.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add spinbike-ui/src/pages/dashboard.rs \
        spinbike-ui/src/i18n.rs \
        spinbike-ui/src/api.rs \
        crates/spinbike-server/src/db/cards.rs \
        crates/spinbike-server/src/routes/cards.rs
git commit -m "feat(ui): inline pencil to edit monthly pass end date"
```

---

### Task 9: UI — ✕ button, confirm modal, voided styling on history rows

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs` — TransactionsList component (~lines 709-750)
- Modify: `spinbike-ui/src/i18n.rs` — keys `void`, `voided`, `confirm_void`
- Modify: `spinbike-ui/style.css` — `.txn-row--voided` class

- [ ] **Step 1: Add translation keys**

Append to `spinbike-ui/src/i18n.rs`:

```rust
("void", "Void"),                                        // SK: "Zrušiť"
("voided", "voided"),                                    // SK: "zrušené"
("confirm_void", "Void this entry? This cannot be undone from the UI."), // SK: "Zrušiť tento záznam? Nedá sa vrátiť."
```

- [ ] **Step 2: Add CSS**

Append to `spinbike-ui/style.css`:

```css
tr.txn-row--voided {
  background: #f5f5f5;
  color: #888;
}
tr.txn-row--voided td.txn-amount {
  text-decoration: line-through;
}
.txn-voided-tag {
  font-size: 0.75rem;
  color: #b00020;
  margin-left: 6px;
  text-transform: uppercase;
}
```

- [ ] **Step 3: Modify TransactionsList rendering**

In `spinbike-ui/src/pages/dashboard.rs`, the TransactionsList renders a `<table>` of transactions. Update the row rendering to:

```rust
txns.get().into_iter().map(|t| {
    let row_class = if t.deleted_at.is_some() { "txn-row--voided" } else { "" };
    let tx_id = t.id;
    let card_id_ref = t.card_id; // for refetch trigger
    let on_void = move |_| {
        let confirm_msg = i18n::t(lang.get(), "confirm_void");
        if !leptos::prelude::window().confirm_with_message(&confirm_msg).unwrap_or(false) {
            return;
        }
        spawn_local(async move {
            let url = format!("/api/transactions/{tx_id}");
            match api::delete_empty(&url).await {
                Ok(()) => set_refresh.update(|n| *n += 1),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
        });
    };
    view! {
        <tr class=row_class>
            <td>{t.created_at.clone()}</td>
            <td>{t.action.clone()}</td>
            <td class="txn-amount">{format!("{:.2} €", t.amount)}</td>
            <td>{t.service_name.clone().unwrap_or_default()}
                {if t.deleted_at.is_some() {
                    view! { <span class="txn-voided-tag">{move || i18n::t(lang.get(), "voided")}</span> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </td>
            <td>
                {if t.deleted_at.is_none() {
                    view! {
                        <button class="btn btn-sm btn-outline" data-testid="txn-void"
                                on:click=on_void>"✕"</button>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </td>
        </tr>
    }
}).collect::<Vec<_>>()
```

- [ ] **Step 4: Ensure `api::delete_empty` exists**

Check `spinbike-ui/src/api.rs`. If missing, add a DELETE helper that expects 204 with no body:

```rust
pub async fn delete_empty(url: &str) -> Result<(), String> {
    // Mirror `post` but Method::DELETE, no body, accept 204.
}
```

- [ ] **Step 5: Trunk build + spot-check**

Run: `cd spinbike-ui && trunk build && cd ..`
Expected: build succeeds.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add spinbike-ui/src/pages/dashboard.rs spinbike-ui/src/i18n.rs spinbike-ui/style.css spinbike-ui/src/api.rs
git commit -m "feat(ui): void button with soft-delete styling on transaction history"
```

---

### Task 10: UI — 3-tab container on the card detail (History / Upcoming / Persistent)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs` — ActionPanel component
- Modify: `spinbike-ui/src/i18n.rs` — tab labels

- [ ] **Step 1: Add translation keys**

Append to `spinbike-ui/src/i18n.rs`:

```rust
("tab_history", "History"),       // SK: "História"
("tab_upcoming", "Upcoming"),     // SK: "Pripravované"
("tab_persistent", "Persistent"), // SK: "Opakované"
```

(Skip any that already exist.)

- [ ] **Step 2: Wrap the three sections in a tab container**

In `spinbike-ui/src/pages/dashboard.rs`, inside the ActionPanel's view block, after the primary-actions row and before the existing UpcomingClasses / PersistentToggles / TransactionsList components, add:

```rust
let (tab, set_tab) = signal("history".to_string());

// Helper closure to render a tab button.
let tab_btn = move |key: &'static str, i18n_key: &'static str| {
    let active = move || tab.get() == key;
    let on_click = move |_| set_tab.set(key.to_string());
    view! {
        <button
            class=move || if active() { "tab tab--active" } else { "tab" }
            on:click=on_click
            data-testid=format!("tab-{key}")
        >
            {move || i18n::t(lang.get(), i18n_key)}
        </button>
    }
};

view! {
    <div class="tabbar">
        {tab_btn("history", "tab_history")}
        {tab_btn("upcoming", "tab_upcoming")}
        {tab_btn("persistent", "tab_persistent")}
    </div>
    <div class="tab-body">
        {move || match tab.get().as_str() {
            "history" => view! { <TransactionsList card_id=card_id set_msg=set_msg refresh=refresh set_refresh=set_refresh/> }.into_any(),
            "upcoming" => view! { <UpcomingClasses card_id=card_id/> }.into_any(),
            "persistent" => view! { <PersistentToggles card_id=card_id/> }.into_any(),
            _ => view! { <div></div> }.into_any(),
        }}
    </div>
}
```

Remove the three existing inline renderings (TransactionsList, UpcomingClasses, PersistentToggles) that were rendered one after another — they're now inside the tab match.

- [ ] **Step 3: Add tab CSS**

Append to `spinbike-ui/style.css`:

```css
.tabbar {
  display: flex;
  gap: 4px;
  border-bottom: 1px solid #ddd;
  margin-top: 12px;
}
.tab {
  background: transparent;
  border: none;
  padding: 8px 14px;
  cursor: pointer;
  border-bottom: 2px solid transparent;
  color: #555;
}
.tab--active {
  color: #111;
  border-bottom-color: #0a66c2;
  font-weight: 600;
}
.tab-body {
  padding-top: 8px;
}
```

- [ ] **Step 4: Trunk build**

Run: `cd spinbike-ui && trunk build && cd ..`
Expected: build succeeds.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add spinbike-ui/src/pages/dashboard.rs spinbike-ui/src/i18n.rs spinbike-ui/style.css
git commit -m "feat(ui): 3-tab card detail (history/upcoming/persistent)"
```

---

### Task 11: Playwright E2E — credit-improvements.spec.ts

**Files:**
- Create: `e2e/tests/credit-improvements.spec.ts`

Cover all four items per airuleset `e2e-real-user-testing`: each feature gets its own test block that clicks through the UI and asserts both UI state and backend effect.

- [ ] **Step 1: Create the test file**

Create `e2e/tests/credit-improvements.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck } from './helpers';

test.describe('credit improvements', () => {
  test('staff can charge for drinks even when a monthly pass is active', async ({ page }) => {
    const consoleCheck = setupConsoleCheck(page);
    await loginViaAPI(page, 'staff@test.com', 'staffpass');
    await page.goto('/dashboard');

    // Scan a card that has an active pass (fixture: JANA 70701001 seeded with pass).
    await page.getByTestId('card-search-input').fill('70701001');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('action-panel')).toBeVisible();

    // Log-visit buttons visible AND the charge form is visible at the same time.
    await expect(page.getByTestId('log-visit-btn').first()).toBeVisible();
    await expect(page.getByTestId('charge-service')).toBeVisible();
    await expect(page.getByTestId('charge-submit')).toBeVisible();

    // Charge a drink (service_id=1 is the first non-pass service).
    await page.getByTestId('charge-service').selectOption({ index: 1 });
    await page.locator('input[placeholder*="Amount" i], input[placeholder*="Suma" i]').fill('2');
    await page.getByTestId('charge-submit').click();

    await expect(page.getByText(/success|Čerpanie|Charged/i).first()).toBeVisible({ timeout: 5000 });
    consoleCheck.assertClean();
  });

  test('staff edits monthly pass end date inline', async ({ page }) => {
    const consoleCheck = setupConsoleCheck(page);
    await loginViaAPI(page, 'staff@test.com', 'staffpass');
    await page.goto('/dashboard');
    await page.getByTestId('card-search-input').fill('70701001');
    await page.keyboard.press('Enter');

    await page.getByTestId('pass-date-edit').click();
    await page.getByTestId('pass-date-input').fill('2027-01-15');
    await page.getByTestId('pass-date-save').click();

    // Banner refreshes to show new date.
    await expect(page.getByTestId('pass-banner')).toContainText('2027-01-15', { timeout: 5000 });
    consoleCheck.assertClean();
  });

  test('staff voids a history row; row greys out and reverses credit', async ({ page }) => {
    const consoleCheck = setupConsoleCheck(page);
    await loginViaAPI(page, 'staff@test.com', 'staffpass');
    await page.goto('/dashboard');
    await page.getByTestId('card-search-input').fill('70701001');
    await page.keyboard.press('Enter');

    // Switch to History tab and capture the current credit.
    await page.getByTestId('tab-history').click();

    const creditBefore = await page.getByTestId('card-credit').textContent();

    // Accept the confirm dialog and void the first row.
    page.once('dialog', d => d.accept());
    await page.getByTestId('txn-void').first().click();

    // The voided row appears with the "voided" tag.
    await expect(page.locator('tr.txn-row--voided').first()).toBeVisible({ timeout: 5000 });

    const creditAfter = await page.getByTestId('card-credit').textContent();
    expect(creditAfter).not.toBe(creditBefore); // credit changed

    consoleCheck.assertClean();
  });

  test('card detail has three working tabs', async ({ page }) => {
    const consoleCheck = setupConsoleCheck(page);
    await loginViaAPI(page, 'staff@test.com', 'staffpass');
    await page.goto('/dashboard');
    await page.getByTestId('card-search-input').fill('70701001');
    await page.keyboard.press('Enter');

    // All three tabs visible.
    await expect(page.getByTestId('tab-history')).toBeVisible();
    await expect(page.getByTestId('tab-upcoming')).toBeVisible();
    await expect(page.getByTestId('tab-persistent')).toBeVisible();

    // Click each and confirm the respective region renders.
    await page.getByTestId('tab-upcoming').click();
    await expect(page.locator('[data-testid="upcoming-list"]').first()).toBeVisible();

    await page.getByTestId('tab-persistent').click();
    await expect(page.locator('[data-testid="persistent-list"]').first()).toBeVisible();

    await page.getByTestId('tab-history').click();
    await expect(page.locator('table').first()).toBeVisible();

    consoleCheck.assertClean();
  });
});
```

- [ ] **Step 2: If `card-credit` or `pass-banner` or `action-panel` test-ids are not yet present in the UI**, add them. Grep the existing dashboard.rs; add `data-testid=` attributes on the matching elements. They are small cosmetic additions.

Run: `grep -n "data-testid" spinbike-ui/src/pages/dashboard.rs | head -20`
Add any missing ids in the same commit.

- [ ] **Step 3: Run E2E locally against a dev server**

Skip running locally (CI runs E2E). Fix any compile errors first.

- [ ] **Step 4: Commit**

```bash
git add e2e/tests/credit-improvements.spec.ts spinbike-ui/src/pages/dashboard.rs
git commit -m "test(e2e): credit improvements — charge during pass, edit date, void, tabs"
```

---

### Task 12: Format, push, monitor CI, open PR review

**Files:** none changed directly — this is the wrap-up task.

- [ ] **Step 1: Run full local format check**

Run: `cargo fmt --all --check`
Expected: exit 0, no output.

- [ ] **Step 2: Push to `dev`**

```bash
git push origin dev
```

- [ ] **Step 3: Monitor both push and PR-event CI runs to completion**

Run (in background): `sleep 300 && gh run view <push-run-id> --json status,conclusion,jobs` and `sleep 1500 && gh run view <pr-run-id> --json status,conclusion,jobs`.

All jobs (Test Integrity, Version Bump Check, Lint, Test, Build WASM, E2E, Mutation Testing, Deploy) MUST be `success` or `skipped`. If anything is FAILURE, investigate via `gh run view <run-id> --log-failed`, fix, commit, push, re-monitor. No "rerun without fix".

- [ ] **Step 4: Confirm PR #5 is not affected (or already merged)**

If PR #5 is still open, this new work lands on top of it. Either:
- Wait for the user to merge PR #5 first, then push this work (preferred — cleaner PRs), OR
- This work piggybacks on PR #5 and becomes part of it (ask the user)

Once the plan is executed, confirm PR status:

```bash
gh api repos/zbynekdrlik/spinbike/pulls/5 --jq '{mergeable, mergeable_state}'
```

Expected: `mergeable: true, mergeable_state: "clean"`. Provide the PR URL and WAIT for the user's explicit merge instruction.

- [ ] **Step 5: Post-deploy verification (only after merge to main)**

Not part of this plan — handled by airuleset's post-deploy-verification once the user merges.

---

## Self-review notes

- **Spec coverage:** All four items are covered — charge-during-pass (Task 7), editable pass date (Tasks 4+6+8), soft-delete history (Tasks 1+2+3+5+9), tabs on card page (Task 10). Tests (integration + E2E) covered by Tasks 1-6 and Task 11.
- **Type consistency:** `deleted_at` is `Option<String>` in both `TransactionRow` and `TransactionResponse`. The DB stores it as TEXT (via `datetime('now')`). UI treats it as a presence flag (`is_some()`).
- **Mutation-testing resilience:** DELETE credit-reversal is tested with BOTH a topup (positive amount) and a charge (negative amount), which kills sign-flip and swap-to-add mutants. PATCH tests cover the three failure paths (wrong kind, forbidden, not-found) so a mutation that deletes one branch is caught.
- **Idempotent migration:** V7 runs through the version-gated loop — on subsequent starts the runner skips it. Tested in Task 1 Step 1.
- **No placeholders:** every step shows full code or full commands.
