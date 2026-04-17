# Monthly Pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add "Casova karta" (monthly pass) — staff sells a time-based unlimited-access pass by deducting credit and picking a `valid_until` date; active pass waives per-class charges and surfaces a banner on the dashboard; legacy importer preserves service name + end date.

**Architecture:** Single new column (`transactions.valid_until`) and one new seeded service ("Monthly pass"). Pass status is computed at read time as `MAX(valid_until)` over a card's transactions — no denormalized column on cards. Two new POST routes (`/api/payments/sell-pass`, `/api/payments/log-visit`) plus a `pass` field added to existing card responses. Legacy importer maps service names and parses `EndDate` on write.

**Tech Stack:** Rust 1.x + Axum 0.8, sqlx + SQLite, Leptos 0.7 CSR + Trunk (WASM), chrono (already in workspace deps), Playwright E2E. Test runner: `cargo nextest`. No new crates.

**Scope reference:** `docs/superpowers/specs/2026-04-17-monthly-pass-design.md`

---

## File Map

**Create:**
- `e2e/tests/monthly-pass.spec.ts` — two E2E scenarios (sell+banner+visit, expired)

**Modify:**
- `VERSION`, `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml` — version bump (via `scripts/sync-version.sh`)
- `crates/spinbike-server/src/db/migrations.rs` — add V4 migration
- `crates/spinbike-server/src/db/transactions.rs` — add `valid_until` field on `TransactionRow`, add `create_transaction_with_valid_until` helper, update SELECTs
- `crates/spinbike-server/src/db/cards.rs` — add `get_card_pass_valid_until` helper + tests
- `crates/spinbike-server/src/routes/payments.rs` — add `sell_pass` + `log_visit` handlers, register routes
- `crates/spinbike-server/src/routes/cards.rs` — extend `CardResponse` with `pass`, extend `TransactionResponse` with `valid_until`
- `crates/spinbike-server/src/bin/migrate_legacy.rs` — map service name, parse EndDate, insert with those fields
- `spinbike-ui/src/pages/dashboard.rs` — `CardInfo` gets `pass` field, add banner, sell-pass modal, visit buttons, history styling

---

## Task 1: Bump version to 0.4.0

**Files:**
- Modify: `VERSION`, `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml` (via script)

This is the first commit on `dev` for this feature. CI has a version-bump check — if `dev` version ≤ `main` version, the PR fails. Bump FIRST, before any code.

- [ ] **Step 1: Write new VERSION**

Run: `echo "0.4.0" > VERSION`

- [ ] **Step 2: Sync version across all Cargo.toml files**

Run: `scripts/sync-version.sh`

Expected: all five `Cargo.toml` files and the workspace `Cargo.toml` now show `version = "0.4.0"`.

- [ ] **Step 3: Verify no other changes snuck in**

Run: `git diff --stat`
Expected: only `VERSION` and `*Cargo.toml` files changed.

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump version to 0.4.0 for monthly pass feature"
```

---

## Task 2: DB migration V4 — valid_until column + Monthly pass service

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs`

**Context:** Migrations are registered as `(version, description, sql)` tuples. V3 is the last one. The new migration both adds the `valid_until` column (`TEXT`, nullable, ISO-8601 date-only) and seeds the "Monthly pass" service row. Using `INSERT OR IGNORE` on the seed makes the migration safe to re-run.

- [ ] **Step 1: Write failing integration test asserting the column and service exist**

Add to `crates/spinbike-server/src/db/migrations.rs` at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use crate::db::{create_memory_pool, run_migrations};

    #[tokio::test]
    async fn v4_adds_valid_until_column() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // PRAGMA table_info(transactions) returns one row per column with name/type.
        let cols: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM pragma_table_info('transactions')")
                .fetch_all(&pool)
                .await
                .unwrap();
        let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
        assert!(
            names.contains(&"valid_until"),
            "transactions.valid_until column missing; found columns: {names:?}"
        );
    }

    #[tokio::test]
    async fn v4_seeds_monthly_pass_service() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (name, price, active): (String, f64, i64) = sqlx::query_as(
            "SELECT name, default_price, active FROM services WHERE name = 'Monthly pass'",
        )
        .fetch_one(&pool)
        .await
        .expect("Monthly pass service must be seeded by V4");
        assert_eq!(name, "Monthly pass");
        assert_eq!(price, 35.0);
        assert_eq!(active, 1);
    }

    #[tokio::test]
    async fn v4_is_idempotent() {
        // Running migrations twice must not fail (second run should be no-op).
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM services WHERE name = 'Monthly pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "Monthly pass must be seeded exactly once");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p spinbike-server --test-threads 1 migrations::tests`

Expected: three failures — column `valid_until` missing, and the service row doesn't exist.

- [ ] **Step 3: Add V4 migration**

In `crates/spinbike-server/src/db/migrations.rs`, change the `MIGRATIONS` slice to include V4, and append the SQL constant at the bottom (before the `#[cfg(test)] mod tests` block):

```rust
pub(crate) static MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "initial schema", V1_INITIAL_SCHEMA),
    (
        2,
        "card holder info and allow debit default",
        V2_CARD_HOLDER_INFO,
    ),
    (3, "card search_text column + index", V3_CARD_SEARCH_TEXT),
    (4, "monthly pass: valid_until + service seed", V4_MONTHLY_PASS),
];
```

Add new constant after `V3_CARD_SEARCH_TEXT`:

```rust
// Monthly pass (Casova karta): records the pass expiry date on the purchase
// transaction row. NULL for every transaction except monthly-pass charges.
// Service is seeded idempotently so re-running migrations is safe.
const V4_MONTHLY_PASS: &str = r#"
ALTER TABLE transactions ADD COLUMN valid_until TEXT;
INSERT OR IGNORE INTO services (name, default_price, active) VALUES ('Monthly pass', 35.0, 1);
"#;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p spinbike-server --test-threads 1 migrations::tests`
Expected: all three pass.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): add V4 migration for transactions.valid_until + Monthly pass service"
```

---

## Task 3: Extend TransactionRow with valid_until

**Files:**
- Modify: `Cargo.toml`, `crates/spinbike-server/src/db/transactions.rs`

**Context:** The `TransactionRow` struct is the DB-level representation. It needs a `valid_until: Option<NaiveDate>`. The existing `list_transactions_for_card` and `list_transactions_for_user` SELECTs must include the new column. A helper `create_transaction_with_valid_until` lets the sell-pass route write the date.

sqlx needs the `chrono` feature enabled to natively bind/decode `NaiveDate` — currently only `["runtime-tokio", "sqlite"]` are enabled, so we must add `chrono`.

- [ ] **Step 1: Enable sqlx chrono feature**

In the workspace root `Cargo.toml`, update the `sqlx` line:

```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono"] }
```

- [ ] **Step 2: Write failing test**

Append to the existing `mod tests` block in `crates/spinbike-server/src/db/transactions.rs`:

```rust
    #[tokio::test]
    async fn transaction_stores_and_retrieves_valid_until() {
        let pool = setup().await;
        let card_id = create_card(&pool, "VU-1").await.unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        create_transaction_with_valid_until(
            &pool, None, Some(card_id), None, Some(1), -35.0, "charge", Some(date),
        )
        .await
        .unwrap();

        let rows = list_transactions_for_card(&pool, card_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].valid_until, Some(date));
    }

    #[tokio::test]
    async fn transaction_without_valid_until_reads_back_as_none() {
        let pool = setup().await;
        let card_id = create_card(&pool, "VU-2").await.unwrap();
        create_transaction(&pool, None, Some(card_id), None, None, 10.0, "topup")
            .await
            .unwrap();
        let rows = list_transactions_for_card(&pool, card_id).await.unwrap();
        assert_eq!(rows[0].valid_until, None);
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo nextest run -p spinbike-server --test-threads 1 transactions::tests`
Expected: compile error on `valid_until` field and `create_transaction_with_valid_until`.

- [ ] **Step 4: Add `valid_until` to `TransactionRow` and expand SELECTs**

In `crates/spinbike-server/src/db/transactions.rs`:

Replace the `TransactionRow` struct:

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
    // ISO-8601 date (YYYY-MM-DD). Set only for monthly-pass charges.
    pub valid_until: Option<chrono::NaiveDate>,
    // Joined from services — None when the transaction wasn't tied to a service.
    #[sqlx(default)]
    pub service_name: Option<String>,
}
```

Update both list queries to select `valid_until`. Replace `list_transactions_for_card`:

```rust
pub async fn list_transactions_for_card(
    pool: &SqlitePool,
    card_id: i64,
) -> Result<Vec<TransactionRow>> {
    let txns = sqlx::query_as::<_, TransactionRow>(
        "SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
                t.amount, t.action, t.created_at, t.valid_until,
                s.name AS service_name
         FROM transactions t
         LEFT JOIN services s ON s.id = t.service_id
         WHERE t.card_id = ?
         ORDER BY t.created_at DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .context("Failed to list transactions for card")?;
    Ok(txns)
}
```

Replace `list_transactions_for_user` the same way (add `t.valid_until` to the SELECT).

- [ ] **Step 5: Add the write helper**

Append to `transactions.rs`, below `create_transaction`:

```rust
#[allow(clippy::too_many_arguments)]
pub async fn create_transaction_with_valid_until(
    pool: &SqlitePool,
    user_id: Option<i64>,
    card_id: Option<i64>,
    staff_id: Option<i64>,
    service_id: Option<i64>,
    amount: f64,
    action: &str,
    valid_until: Option<chrono::NaiveDate>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, valid_until)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(user_id)
    .bind(card_id)
    .bind(staff_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .bind(valid_until)
    .fetch_one(pool)
    .await
    .context("Failed to create transaction with valid_until")?;
    Ok(id)
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo nextest run -p spinbike-server --test-threads 1 transactions::tests`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/spinbike-server/src/db/transactions.rs
git commit -m "feat(db): thread valid_until through TransactionRow and write helper"
```

---

## Task 4: Pass-status helper on db::cards

**Files:**
- Modify: `crates/spinbike-server/src/db/cards.rs`

**Context:** Pass status is derived, not stored. `get_card_pass_valid_until` returns the latest `valid_until` across a card's transactions — `None` if the card has never purchased a pass. Callers compare it to today's date to decide active/expired.

- [ ] **Step 1: Write failing tests**

Append to the existing `mod tests` in `crates/spinbike-server/src/db/cards.rs`:

```rust
    #[tokio::test]
    async fn pass_valid_until_none_when_no_pass_purchased() {
        let pool = setup().await;
        let card_id = create_card(&pool, "NO-PASS").await.unwrap();
        let result = get_card_pass_valid_until(&pool, card_id).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn pass_valid_until_returns_max_across_multiple_passes() {
        use crate::db::transactions::create_transaction_with_valid_until;
        let pool = setup().await;
        let card_id = create_card(&pool, "MULTI-PASS").await.unwrap();
        // Two pass purchases — the later one wins.
        let d1 = chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let d2 = chrono::NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        create_transaction_with_valid_until(
            &pool, None, Some(card_id), None, Some(1), -35.0, "charge", Some(d1),
        ).await.unwrap();
        create_transaction_with_valid_until(
            &pool, None, Some(card_id), None, Some(1), -35.0, "charge", Some(d2),
        ).await.unwrap();

        let result = get_card_pass_valid_until(&pool, card_id).await.unwrap();
        assert_eq!(result, Some(d2), "MAX(valid_until) must win regardless of insert order");
    }

    #[tokio::test]
    async fn pass_valid_until_ignores_non_pass_transactions() {
        use crate::db::transactions::create_transaction;
        let pool = setup().await;
        let card_id = create_card(&pool, "CHARGE-ONLY").await.unwrap();
        create_transaction(&pool, None, Some(card_id), None, Some(1), -5.0, "charge").await.unwrap();
        create_transaction(&pool, None, Some(card_id), None, None, 20.0, "topup").await.unwrap();
        let result = get_card_pass_valid_until(&pool, card_id).await.unwrap();
        assert_eq!(result, None, "non-pass transactions must not produce a valid_until");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p spinbike-server --test-threads 1 cards::tests::pass_`
Expected: compile error — `get_card_pass_valid_until` not defined.

- [ ] **Step 3: Implement the helper**

Append to `crates/spinbike-server/src/db/cards.rs` (after `set_allow_debit`):

```rust
/// Return the latest `valid_until` across a card's transactions, or `None` if
/// the card has never had a monthly-pass purchase. Callers compare against
/// today's date to determine whether the pass is active or expired.
pub async fn get_card_pass_valid_until(
    pool: &SqlitePool,
    card_id: i64,
) -> Result<Option<chrono::NaiveDate>> {
    let row: Option<(Option<chrono::NaiveDate>,)> = sqlx::query_as(
        "SELECT MAX(valid_until) FROM transactions
         WHERE card_id = ? AND valid_until IS NOT NULL",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .context("Failed to compute pass valid_until")?;
    Ok(row.and_then(|(d,)| d))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p spinbike-server --test-threads 1 cards::tests::pass_`
Expected: three passes.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/cards.rs
git commit -m "feat(db): get_card_pass_valid_until helper (MAX across transactions)"
```

---

## Task 5: POST /api/payments/sell-pass

**Files:**
- Modify: `crates/spinbike-server/src/routes/payments.rs`

**Context:** Follows the same pattern as `charge` — begin tx, read card, validate, debit credit, insert transaction. Differences: resolves the monthly-pass service id by name, requires `valid_until` in request body, rejects past dates. Amount is stored as negative (charge convention).

- [ ] **Step 1: Write failing integration tests**

Integration tests live under `crates/spinbike-server/tests/` and use the shared `helpers::TestApp` (see `tests/payments.rs` for the idiom). The TestApp exposes `.seed_card(barcode, credit, ...)`, `.pool` for direct SQL, `.staff_token`, and `.request(req)` which returns `(StatusCode, serde_json::Value)`. Request builders: `post_json(uri, &token, &body)` and `get(uri, &token)`.

Create `crates/spinbike-server/tests/monthly_pass.rs`:

```rust
//! Integration tests for /api/payments/sell-pass, /api/payments/log-visit,
//! and the `pass` field on CardResponse.

mod helpers;

use helpers::{TestApp, get, post_json};
use serde_json::json;

async fn set_blocked(app: &TestApp, card_id: i64) {
    sqlx::query("UPDATE cards SET blocked = 1 WHERE id = ?")
        .bind(card_id)
        .execute(&app.pool)
        .await
        .unwrap();
}

async fn card_credit(app: &TestApp, card_id: i64) -> f64 {
    sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn service_id(app: &TestApp, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM services WHERE name = ?")
        .bind(name)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn sell_pass_debits_credit_and_records_valid_until() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("SELL-PASS-1", 50.0, None, None, None, None).await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-05-17" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "body = {resp}");
    assert_eq!(resp["new_credit"].as_f64().unwrap(), 15.0);
    assert_eq!(resp["valid_until"], "2030-05-17");

    assert_eq!(card_credit(&app, card_id).await, 15.0);

    let tx_id = resp["transaction_id"].as_i64().unwrap();
    let (amount, valid_until, service_id): (f64, Option<chrono::NaiveDate>, i64) = sqlx::query_as(
        "SELECT amount, valid_until, service_id FROM transactions WHERE id = ?",
    )
    .bind(tx_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(amount, -35.0, "monthly pass amount stored as negative (ledger convention)");
    assert_eq!(
        valid_until,
        Some(chrono::NaiveDate::from_ymd_opt(2030, 5, 17).unwrap())
    );
    let pass_svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
        .fetch_one(&app.pool).await.unwrap();
    assert_eq!(service_id, pass_svc_id);
}

#[tokio::test]
async fn sell_pass_rejects_past_valid_until() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("SELL-PAST", 100.0, None, None, None, None).await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2020-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sell_pass_rejects_negative_price() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("SELL-NEG", 100.0, None, None, None, None).await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": -1.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sell_pass_rejects_blocked_card() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("SELL-BLOCKED", 100.0, None, None, None, None).await;
    set_blocked(&app, card_id).await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}
```

The `set_blocked`, `card_credit`, `service_id` helpers are declared locally at the top of `monthly_pass.rs` (not added to the shared `helpers/mod.rs`) so this test file is self-contained.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p spinbike-server --test monthly_pass`
Expected: four failures — `/api/payments/sell-pass` returns 404.

- [ ] **Step 3: Implement the handler**

In `crates/spinbike-server/src/routes/payments.rs`, add the request struct after `ChargeRequest`:

```rust
#[derive(Deserialize)]
pub struct SellPassRequest {
    pub card_id: i64,
    pub price: f64,
    pub valid_until: chrono::NaiveDate,
}

#[derive(Serialize)]
pub struct SellPassResponse {
    pub transaction_id: i64,
    pub new_credit: f64,
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
}
```

Register the route in `routes()`:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/payments/charge", post(charge))
        .route("/api/payments/storno", post(storno))
        .route("/api/payments/sell-pass", post(sell_pass))
        .route("/api/payments/log-visit", post(log_visit))
}
```

Append `sell_pass` handler at the bottom of the file:

```rust
async fn sell_pass(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SellPassRequest>,
) -> Result<Json<SellPassResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }
    if body.price < 0.0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Price must be zero or greater"})),
        ));
    }
    let today = chrono::Local::now().date_naive();
    if body.valid_until <= today {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "valid_until must be in the future"})),
        ));
    }

    let price = cards::round_cents(body.price);

    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let card = sqlx::query_as::<_, cards::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;
    if card.blocked != 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Card is blocked"})),
        ));
    }

    // Resolve Monthly pass service id by name (seeded by V4 migration).
    let service_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
        .fetch_one(&mut *tx)
        .await
        .map_err(internal_error)?;

    sqlx::query("UPDATE cards SET credit = ROUND(credit - ?, 2) WHERE id = ?")
        .bind(price)
        .bind(body.card_id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, valid_until)
         VALUES (?, ?, ?, ?, ?, 'charge', ?)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind(Some(service_id))
    .bind(-price)
    .bind(body.valid_until)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    let new_credit = cards::round_cents(card.credit - price);
    let days_remaining = (body.valid_until - today).num_days() as i32;

    Ok(Json(SellPassResponse {
        transaction_id: tx_id,
        new_credit,
        valid_until: body.valid_until,
        days_remaining,
    }))
}
```

Note: `log_visit` is added in Task 6 — leaving `.route("/api/payments/log-visit", post(log_visit))` registered now means Task 5 won't compile until Task 6 is done. To keep Task 5 shippable standalone, register `log_visit` in Task 6 instead; remove that line from this task's `routes()` edit:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/payments/charge", post(charge))
        .route("/api/payments/storno", post(storno))
        .route("/api/payments/sell-pass", post(sell_pass))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p spinbike-server --test monthly_pass`
Expected: four passes.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/routes/payments.rs crates/spinbike-server/tests/monthly_pass.rs
git commit -m "feat(api): POST /api/payments/sell-pass deducts credit and sets valid_until"
```

---

## Task 6: POST /api/payments/log-visit

**Files:**
- Modify: `crates/spinbike-server/src/routes/payments.rs`

**Context:** Writes a zero-amount `visit` transaction tied to a service (Spinning or Fitness). Rejects if the card has no active pass — staff should use `/charge` in that case. No credit change.

- [ ] **Step 1: Append failing tests to the integration test file**

Append to `crates/spinbike-server/tests/monthly_pass.rs`:

```rust
#[tokio::test]
async fn log_visit_writes_zero_amount_when_pass_active() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("VISIT-1", 50.0, None, None, None, None).await;

    // Sell a pass first (relies on Task 5's handler)
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let spinning_id = service_id(&app, "Spinning").await;
    let (status, resp) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "card_id": card_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let tx_id = resp["transaction_id"].as_i64().unwrap();
    let (amount, action, service_id_val): (f64, String, i64) = sqlx::query_as(
        "SELECT amount, action, service_id FROM transactions WHERE id = ?",
    )
    .bind(tx_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(amount, 0.0);
    assert_eq!(action, "visit");
    assert_eq!(service_id_val, spinning_id);

    // Credit unchanged (50 - 35 = 15)
    assert_eq!(card_credit(&app, card_id).await, 15.0);
}

#[tokio::test]
async fn log_visit_rejects_card_without_active_pass() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("VISIT-2", 50.0, None, None, None, None).await;
    let spinning_id = service_id(&app, "Spinning").await;

    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "card_id": card_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn log_visit_rejects_card_with_expired_pass() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("VISIT-3", 50.0, None, None, None, None).await;

    // Insert an expired pass transaction directly via SQL
    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
        .fetch_one(&app.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO transactions (card_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, -35.0, 'charge', ?, datetime('now'))",
    )
    .bind(card_id)
    .bind(pass_svc)
    .bind(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap())
    .execute(&app.pool).await.unwrap();

    let spinning_id = service_id(&app, "Spinning").await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/log-visit",
            &app.staff_token,
            &json!({ "card_id": card_id, "service_id": spinning_id }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p spinbike-server --test monthly_pass`
Expected: three failures — `/api/payments/log-visit` returns 404 or compile error.

- [ ] **Step 3: Implement the handler**

In `crates/spinbike-server/src/routes/payments.rs`, add request/response types (near the other structs):

```rust
#[derive(Deserialize)]
pub struct LogVisitRequest {
    pub card_id: i64,
    pub service_id: i64,
}

#[derive(Serialize)]
pub struct LogVisitResponse {
    pub transaction_id: i64,
}
```

Register the route:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/payments/charge", post(charge))
        .route("/api/payments/storno", post(storno))
        .route("/api/payments/sell-pass", post(sell_pass))
        .route("/api/payments/log-visit", post(log_visit))
}
```

Append handler:

```rust
async fn log_visit(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<LogVisitRequest>,
) -> Result<Json<LogVisitResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let today = chrono::Local::now().date_naive();
    let valid_until = cards::get_card_pass_valid_until(&state.pool, body.card_id)
        .await
        .map_err(internal_error)?;
    match valid_until {
        Some(d) if d >= today => {} // active — OK
        _ => {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Card has no active monthly pass; use /api/payments/charge"
                })),
            ));
        }
    }

    let tx_id = crate::db::transactions::create_transaction(
        &state.pool,
        None,
        Some(body.card_id),
        Some(claims.sub),
        Some(body.service_id),
        0.0,
        "visit",
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(LogVisitResponse {
        transaction_id: tx_id,
    }))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p spinbike-server --test monthly_pass`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/routes/payments.rs crates/spinbike-server/tests/monthly_pass.rs
git commit -m "feat(api): POST /api/payments/log-visit records zero-EUR visit for pass holders"
```

---

## Task 7: Extend card and transaction API responses with pass / valid_until

**Files:**
- Modify: `crates/spinbike-server/src/routes/cards.rs`

**Context:** The UI needs to know a card's current pass status on every fetch (search, lookup, list, activate) and needs `valid_until` in the transaction history for the styled "charge · until DD.MM" display. Adding a `pass` field to `CardResponse` and `valid_until` to `TransactionResponse` keeps API responses self-contained (no second fetch).

- [ ] **Step 1: Write failing integration test**

Append to `crates/spinbike-server/tests/monthly_pass.rs`:

```rust
#[tokio::test]
async fn card_response_includes_pass_field_when_pass_active() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("PASS-RESP-1", 50.0, None, None, None, None).await;
    let (status, _) = app
        .request(post_json(
            "/api/payments/sell-pass",
            &app.staff_token,
            &json!({ "card_id": card_id, "price": 35.0, "valid_until": "2030-01-01" }),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (status, body) = app
        .request(get("/api/cards/lookup/PASS-RESP-1", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["pass"]["valid_until"], "2030-01-01");
    let days = body["pass"]["days_remaining"].as_i64().unwrap();
    assert!(days > 0, "days_remaining must be positive for an active pass");
}

#[tokio::test]
async fn card_response_pass_field_is_null_when_no_pass() {
    let app = TestApp::new().await;
    app.seed_card("NO-PASS-RESP", 10.0, None, None, None, None).await;
    let (status, body) = app
        .request(get("/api/cards/lookup/NO-PASS-RESP", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(body["pass"].is_null(), "pass must be null when card has no pass");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p spinbike-server --test monthly_pass card_response_`
Expected: two failures — `pass` field missing.

- [ ] **Step 3: Update `CardResponse` with pass info**

In `crates/spinbike-server/src/routes/cards.rs`, add struct:

```rust
#[derive(Serialize)]
pub struct CardPass {
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
}
```

Update `CardResponse` to include `pass: Option<CardPass>`:

```rust
#[derive(Serialize)]
pub struct CardResponse {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
    pub pass: Option<CardPass>,
}
```

The `From<&CardRow>` impl can't compute `pass` (it needs DB access). Delete that impl and write an async constructor instead:

```rust
// Replaces `impl From<&db::CardRow> for CardResponse`.
async fn card_response_from_row(
    pool: &sqlx::SqlitePool,
    c: &db::CardRow,
) -> anyhow::Result<CardResponse> {
    let today = chrono::Local::now().date_naive();
    let pass = db::get_card_pass_valid_until(pool, c.id)
        .await?
        .map(|d| CardPass {
            valid_until: d,
            days_remaining: (d - today).num_days() as i32,
        });
    Ok(CardResponse {
        id: c.id,
        barcode: c.barcode.clone(),
        user_id: c.user_id,
        blocked: c.blocked != 0,
        credit: c.credit,
        allow_debit: c.allow_debit != 0,
        first_name: c.first_name.clone(),
        last_name: c.last_name.clone(),
        company: c.company.clone(),
        phone: c.phone.clone(),
        pass,
    })
}
```

Replace every `CardResponse::from(&card)` and `cards.iter().map(CardResponse::from).collect()` call in the file with awaited calls:

```rust
// Single card:
let body = card_response_from_row(&state.pool, &card).await.map_err(internal_error)?;
Ok(Json(body))

// List:
let mut out = Vec::with_capacity(cards.len());
for c in &cards {
    out.push(card_response_from_row(&state.pool, c).await.map_err(internal_error)?);
}
Ok(Json(out))
```

Callsites to update: `search_cards`, `list_cards`, `link_card`, `lookup_card`, `activate_card`, `topup_card`, `block_card`, `update_card`. Grep for `CardResponse::from` to find them all.

- [ ] **Step 4: Update `TransactionResponse` with valid_until**

In the same file, update:

```rust
#[derive(Serialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub card_id: Option<i64>,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
    pub service_name: Option<String>,
    pub valid_until: Option<chrono::NaiveDate>,
}
```

Find where `TransactionResponse` is built from `TransactionRow` (likely `card_transactions` handler and maybe `my_balance`). Add `valid_until: t.valid_until` to the struct literal.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo nextest run -p spinbike-server`
Expected: all pass including the two new tests.

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/routes/cards.rs crates/spinbike-server/tests/monthly_pass.rs
git commit -m "feat(api): include pass on CardResponse and valid_until on TransactionResponse"
```

---

## Task 8: Legacy importer — preserve service_id + valid_until

**Files:**
- Modify: `crates/spinbike-server/src/bin/migrate_legacy.rs`

**Context:** The `Data` CSV has columns `id_data, id_card, user, action, service, suma_SK, Date, EndDate, suma`. The current importer reads columns 1, 3, 6, 8 (id_card, action, Date, suma). Must also read 4 (service) and 7 (EndDate), map service names, parse EndDate, and insert via the V4-aware INSERT.

- [ ] **Step 1: Write failing unit test for the EndDate parser**

Append to `crates/spinbike-server/src/bin/migrate_legacy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_end_date_valid() {
        assert_eq!(
            parse_legacy_end_date("12/05/08 00:00:00").unwrap(),
            Some(chrono::NaiveDate::from_ymd_opt(2008, 12, 5).unwrap())
        );
    }

    #[test]
    fn parse_end_date_empty_is_none() {
        assert_eq!(parse_legacy_end_date("").unwrap(), None);
        assert_eq!(parse_legacy_end_date("   ").unwrap(), None);
    }

    #[test]
    fn parse_end_date_garbage_is_none() {
        assert_eq!(parse_legacy_end_date("not a date").unwrap(), None);
    }

    #[test]
    fn map_legacy_service_known_names() {
        assert_eq!(map_legacy_service_name("Casova karta"), Some("Monthly pass"));
        assert_eq!(map_legacy_service_name("Fitnes"), Some("Fitness"));
        assert_eq!(map_legacy_service_name("Spinbike"), Some("Spinning"));
    }

    #[test]
    fn map_legacy_service_unknown_returns_none() {
        assert_eq!(map_legacy_service_name("Something else"), None);
        assert_eq!(map_legacy_service_name(""), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p spinbike-server --bin migrate-legacy tests::`
Expected: compile error — both helpers undefined.

- [ ] **Step 3: Implement helpers**

Add to `migrate_legacy.rs` (near other pure functions):

```rust
/// Map legacy service names (Slovak, from MS Access `serviceTab`) to
/// the service names seeded in the new system.
fn map_legacy_service_name(name: &str) -> Option<&'static str> {
    match name.trim() {
        "Casova karta" => Some("Monthly pass"),
        "Fitnes" => Some("Fitness"),
        "Spinbike" => Some("Spinning"),
        _ => None,
    }
}

/// Parse legacy EndDate strings in `MM/DD/YY HH:MM:SS` format
/// (e.g. "12/05/08 00:00:00") to a `NaiveDate`. Blank/unparsable → None.
fn parse_legacy_end_date(s: &str) -> anyhow::Result<Option<chrono::NaiveDate>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    // Two-digit year — chrono parses "08" as year 8 without `%y` flag.
    match chrono::NaiveDateTime::parse_from_str(trimmed, "%m/%d/%y %H:%M:%S") {
        Ok(dt) => Ok(Some(dt.date())),
        Err(_) => Ok(None),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p spinbike-server --bin migrate-legacy tests::`
Expected: five passes.

- [ ] **Step 5: Wire helpers into the import loop**

In `migrate_legacy.rs`, before the `for result in data_reader.records()` loop, load a service name → id map once:

```rust
let service_ids: std::collections::HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
    "SELECT name, id FROM services",
)
.fetch_all(&pool)
.await
.context("Failed to load services for legacy mapping")?
.into_iter()
.collect();
```

Inside the loop, after reading `action` and `amount_eur`, read the additional columns and resolve the service_id + valid_until:

```rust
let legacy_service = record.get(4).context("Missing service column")?.trim();
let end_date_raw = record.get(7).context("Missing EndDate column")?.trim();

let service_id: Option<i64> = map_legacy_service_name(legacy_service)
    .and_then(|new_name| service_ids.get(new_name).copied());
let valid_until = parse_legacy_end_date(end_date_raw)?;
```

Replace the existing `INSERT INTO transactions (...)` to also bind `service_id` and `valid_until`:

```rust
sqlx::query(
    "INSERT INTO transactions (card_id, amount, action, created_at, service_id, valid_until)
     VALUES (?, ?, ?, ?, ?, ?)",
)
.bind(new_card_id)
.bind(amount_eur)
.bind(mapped_action)
.bind(date)
.bind(service_id)
.bind(valid_until)
.execute(&pool)
.await
.with_context(|| {
    format!(
        "Failed to insert transaction: card={legacy_card_id}, action={action}"
    )
})?;
```

- [ ] **Step 6: Write a small end-to-end importer test fixture**

Add to the `#[cfg(test)] mod tests` block:

```rust
    #[tokio::test]
    async fn importer_preserves_service_and_end_date() {
        use crate::db::{create_memory_pool, run_migrations};
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed a card row so id 1 exists in the new DB.
        sqlx::query("INSERT INTO cards (id, barcode, allow_debit, search_text) VALUES (1, 'C1', 1, 'c1')")
            .execute(&pool).await.unwrap();

        // Mimic what the import loop does for one row.
        let service_ids: std::collections::HashMap<String, i64> =
            sqlx::query_as::<_, (String, i64)>("SELECT name, id FROM services")
                .fetch_all(&pool).await.unwrap()
                .into_iter().collect();

        let service_id = map_legacy_service_name("Casova karta")
            .and_then(|n| service_ids.get(n).copied())
            .unwrap();
        let valid_until = parse_legacy_end_date("12/05/08 00:00:00").unwrap();

        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at, service_id, valid_until)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(1_i64).bind(-19.92).bind("charge").bind("11/06/08 21:20:24")
        .bind(Some(service_id)).bind(valid_until)
        .execute(&pool).await.unwrap();

        let row: (Option<String>, Option<chrono::NaiveDate>) = sqlx::query_as(
            "SELECT s.name, t.valid_until FROM transactions t
             LEFT JOIN services s ON s.id = t.service_id WHERE t.card_id = 1",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(row.0.as_deref(), Some("Monthly pass"));
        assert_eq!(row.1, Some(chrono::NaiveDate::from_ymd_opt(2008, 12, 5).unwrap()));
    }
```

`create_memory_pool` / `run_migrations` must be accessible to the binary's test module. If they aren't `pub`, promote their visibility in `crates/spinbike-server/src/db/mod.rs` (they likely already are since other tests use them).

- [ ] **Step 7: Run all importer tests to confirm**

Run: `cargo nextest run -p spinbike-server --bin migrate-legacy`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/spinbike-server/src/bin/migrate_legacy.rs
git commit -m "feat(migrate): preserve legacy service name and EndDate in imported transactions"
```

---

## Task 9: UI — CardInfo gains pass, active/expired banner

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs`

**Context:** The UI currently shows name/credit/history. Adding a `pass` field on `CardInfo` plus a banner component that renders one of three states (none, active, expired). Visit buttons and sell-pass modal come in later tasks.

- [ ] **Step 1: Extend CardInfo struct**

At the top of `spinbike-ui/src/pages/dashboard.rs`, below the `CardInfo` struct, add:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
struct CardPass {
    valid_until: chrono::NaiveDate,
    days_remaining: i32,
}
```

Update `CardInfo`:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
struct CardInfo {
    id: i64,
    barcode: String,
    #[allow(dead_code)]
    user_id: Option<i64>,
    blocked: bool,
    credit: f64,
    #[allow(dead_code)]
    allow_debit: bool,
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    last_name: Option<String>,
    #[serde(default)]
    company: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    pass: Option<CardPass>,
}
```

- [ ] **Step 2: Add banner component**

In the same file, add a new component `PassBanner` near `ActionPanel` (find the `#[component]` macro). Place it above `ActionPanel`:

```rust
#[component]
fn PassBanner(pass: Option<CardPass>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    match pass {
        None => view! { <div></div> }.into_any(),
        Some(p) if p.days_remaining >= 0 => {
            let date_str = p.valid_until.format("%d.%m.%Y").to_string();
            view! {
                <div class="pass-banner pass-banner-ok" data-testid="pass-banner-active">
                    <div class="pass-banner-title">
                        {move || i18n::pass_valid_until(lang.get())}" "{date_str.clone()}
                    </div>
                    <div class="pass-banner-sub">
                        {p.days_remaining}" "{move || i18n::pass_days_remaining(lang.get())}
                    </div>
                </div>
            }
            .into_any()
        }
        Some(p) => {
            let date_str = p.valid_until.format("%d.%m.%Y").to_string();
            let days_ago = -p.days_remaining;
            view! {
                <div class="pass-banner pass-banner-expired" data-testid="pass-banner-expired">
                    <div class="pass-banner-title">
                        {move || i18n::pass_expired(lang.get())}" "{days_ago}" "
                        {move || i18n::pass_days_ago(lang.get())}
                    </div>
                    <div class="pass-banner-sub">
                        {move || i18n::pass_last_valid_until(lang.get())}" "{date_str.clone()}
                    </div>
                </div>
            }
            .into_any()
        }
    }
}
```

- [ ] **Step 3: Add i18n strings**

In `spinbike-ui/src/i18n.rs`, add functions (follow the file's existing pattern — match on `Lang::En` / `Lang::Sk`):

```rust
pub fn pass_valid_until(lang: Lang) -> &'static str {
    match lang { Lang::En => "✓ Monthly pass valid until", Lang::Sk => "✓ Mesačný lístok platný do" }
}
pub fn pass_days_remaining(lang: Lang) -> &'static str {
    match lang { Lang::En => "days remaining · unlimited access", Lang::Sk => "dní zostáva · neobmedzený prístup" }
}
pub fn pass_expired(lang: Lang) -> &'static str {
    match lang { Lang::En => "Monthly pass expired", Lang::Sk => "Mesačný lístok expiroval pred" }
}
pub fn pass_days_ago(lang: Lang) -> &'static str {
    match lang { Lang::En => "days ago", Lang::Sk => "dňami" }
}
pub fn pass_last_valid_until(lang: Lang) -> &'static str {
    match lang { Lang::En => "Last valid until", Lang::Sk => "Naposledy platný do" }
}
```

- [ ] **Step 4: Render the banner at the top of ActionPanel**

Find `ActionPanel` component body. Locate where the credit balance renders — insert the banner just before it (so banner is above credit):

```rust
<PassBanner pass=card.pass.clone() />
```

Pass `card.pass.clone()` from whatever signal holds the selected card.

- [ ] **Step 5: Add CSS**

In `spinbike-ui/style/main.scss` (or whatever SCSS/CSS file is already in use — check the existing imports), append:

```css
.pass-banner { padding: 0.85rem 1rem; border-radius: 6px; margin-bottom: 1rem; }
.pass-banner-ok { background: #1b4a2a; color: #9bf3b4; border: 1px solid #2c7a4a; }
.pass-banner-expired { background: #5a1c1c; color: #ffb3b3; border: 1px solid #a33; }
.pass-banner-title { font-size: 1.05rem; font-weight: 600; margin-bottom: 0.25rem; }
.pass-banner-sub { font-size: 0.85rem; opacity: 0.85; }
```

- [ ] **Step 6: Run local build**

Run: `cd spinbike-ui && trunk build` (from repo root, adjust path)
Expected: compiles without errors. Visual verification deferred to E2E test in Task 13.

- [ ] **Step 7: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs spinbike-ui/src/i18n.rs spinbike-ui/style/main.scss
git commit -m "feat(ui): card action panel shows active/expired monthly pass banner"
```

---

## Task 10: UI — sell-pass modal

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs`

**Context:** New button in the "Sell service" section, opens a modal with price (editable, default 35.00) and date picker (default = max(current valid_until, today) + 30 days), confirm calls `POST /api/payments/sell-pass`.

- [ ] **Step 1: Add sell-pass button next to existing charge buttons**

Find the section in `ActionPanel` that renders existing charge buttons (look for the `ChargeSection` or inline buttons for services). Add after them a new button:

```rust
<button
    class="btn btn-pass"
    data-testid="sell-pass-btn"
    on:click=move |_| set_show_sell_pass.set(true)
>
    {move || i18n::sell_monthly_pass(lang.get())} " 35.00"
</button>
```

- [ ] **Step 2: Add state signal for the modal**

In `ActionPanel`:

```rust
let (show_sell_pass, set_show_sell_pass) = signal(false);
```

- [ ] **Step 3: Add i18n strings**

In `spinbike-ui/src/i18n.rs`:

```rust
pub fn sell_monthly_pass(lang: Lang) -> &'static str {
    match lang { Lang::En => "Sell monthly pass", Lang::Sk => "Predať mesačný lístok" }
}
pub fn modal_price(lang: Lang) -> &'static str {
    match lang { Lang::En => "Price (EUR)", Lang::Sk => "Cena (EUR)" }
}
pub fn modal_valid_until(lang: Lang) -> &'static str {
    match lang { Lang::En => "Valid until", Lang::Sk => "Platný do" }
}
pub fn modal_confirm(lang: Lang) -> &'static str {
    match lang { Lang::En => "Sell pass", Lang::Sk => "Predať" }
}
pub fn modal_cancel(lang: Lang) -> &'static str {
    match lang { Lang::En => "Cancel", Lang::Sk => "Zrušiť" }
}
```

- [ ] **Step 4: Add SellPassModal component**

Above `ActionPanel`:

```rust
#[component]
fn SellPassModal(
    card: CardInfo,
    set_selected: WriteSignal<Option<CardInfo>>,
    show: ReadSignal<bool>,
    set_show: WriteSignal<bool>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let today = chrono::Local::now().date_naive();
    // Default valid_until: max(current valid_until, today) + 30 days.
    let default_date = card
        .pass
        .as_ref()
        .map(|p| if p.valid_until > today { p.valid_until } else { today })
        .unwrap_or(today)
        + chrono::Duration::days(30);

    let (price, set_price) = signal(35.0f64);
    let (valid_until, set_valid_until) = signal(default_date);
    let (err, set_err) = signal(String::new());

    let card_id = card.id;

    let on_confirm = move |_| {
        let p = price.get();
        let vu = valid_until.get();
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { card_id: i64, price: f64, valid_until: chrono::NaiveDate }
            #[derive(serde::Deserialize)]
            struct Resp { new_credit: f64, valid_until: chrono::NaiveDate, days_remaining: i32 }
            match api::post::<Req, Resp>("/api/payments/sell-pass", &Req { card_id, price: p, valid_until: vu }).await {
                Ok(r) => {
                    set_selected.update(|opt| {
                        if let Some(c) = opt.as_mut() {
                            c.credit = r.new_credit;
                            c.pass = Some(CardPass { valid_until: r.valid_until, days_remaining: r.days_remaining });
                        }
                    });
                    set_show.set(false);
                }
                Err(e) => set_err.set(format!("{e}")),
            }
        });
    };

    view! {
        <Show when=move || show.get() fallback=|| view! { <div></div> }>
            <div class="modal-overlay" data-testid="sell-pass-modal">
                <div class="modal">
                    <h3>{move || i18n::sell_monthly_pass(lang.get())}</h3>
                    <label>{move || i18n::modal_price(lang.get())}</label>
                    <input
                        type="number" step="0.01" min="0"
                        data-testid="sell-pass-price"
                        prop:value=move || format!("{:.2}", price.get())
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<f64>() { set_price.set(v); }
                        }
                    />
                    <label>{move || i18n::modal_valid_until(lang.get())}</label>
                    <input
                        type="date"
                        data-testid="sell-pass-date"
                        prop:value=move || valid_until.get().format("%Y-%m-%d").to_string()
                        on:input=move |ev| {
                            let s = event_target_value(&ev);
                            if let Ok(d) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                                set_valid_until.set(d);
                            }
                        }
                    />
                    <Show when=move || !err.get().is_empty() fallback=|| view! { <div></div> }>
                        <div class="err">{move || err.get()}</div>
                    </Show>
                    <div class="modal-buttons">
                        <button class="btn" on:click=move |_| set_show.set(false)>
                            {move || i18n::modal_cancel(lang.get())}
                        </button>
                        <button class="btn btn-primary" data-testid="sell-pass-confirm" on:click=on_confirm>
                            {move || i18n::modal_confirm(lang.get())}
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
```

`event_target_value` is a small helper — if not already imported, add it near the top:

```rust
fn event_target_value(ev: &web_sys::Event) -> String {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default()
}
```

- [ ] **Step 5: Render the modal in ActionPanel**

Inside `ActionPanel` view, somewhere outside the row grid:

```rust
<SellPassModal
    card=card.clone()
    set_selected=set_selected
    show=show_sell_pass
    set_show=set_show_sell_pass
/>
```

- [ ] **Step 6: Add modal CSS**

In `spinbike-ui/style/main.scss`:

```css
.btn-pass { background: #3a5e2a; border-color: #5a8a3a; color: #fff; }
.modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.6); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: #1e1e24; border: 1px solid #333; border-radius: 8px; padding: 1.5rem; min-width: 320px; }
.modal label { display: block; margin-top: 0.75rem; margin-bottom: 0.25rem; color: #aaa; font-size: 0.85rem; }
.modal input { width: 100%; padding: 0.5rem; background: #2a2a32; color: #eee; border: 1px solid #444; border-radius: 4px; }
.modal-buttons { display: flex; gap: 0.5rem; justify-content: flex-end; margin-top: 1rem; }
```

- [ ] **Step 7: Trunk build**

Run: `cd spinbike-ui && trunk build`
Expected: compiles.

- [ ] **Step 8: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs spinbike-ui/src/i18n.rs spinbike-ui/style/main.scss
git commit -m "feat(ui): modal to sell monthly pass with editable price + date picker"
```

---

## Task 11: UI — visit buttons vs charge buttons based on pass state

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs`

**Context:** When a card has an active pass, the per-class charge buttons should read "Log visit" and call `/api/payments/log-visit` instead of `/api/payments/charge`. Expired or no pass → existing charge flow untouched.

- [ ] **Step 1: Determine active-pass helper**

Near the top of `dashboard.rs`, add:

```rust
fn pass_is_active(card: &CardInfo) -> bool {
    card.pass.as_ref().map(|p| p.days_remaining >= 0).unwrap_or(false)
}
```

- [ ] **Step 2: Modify the charge buttons in ActionPanel / ChargeSection**

Find where existing charge buttons iterate over services (look for mentions of `services.get()` and `charge`). For each service button, branch on `pass_is_active(&card)`:

```rust
<Show when=move || pass_is_active(&card) fallback=move || {
    view! {
        <button class="btn" on:click=charge_click(svc.id, svc.default_price)>
            {svc.name.clone()} " " {format!("{:.2}", svc.default_price)}
        </button>
    }
}>
    <button class="btn btn-primary" data-testid="log-visit-btn" on:click=visit_click(svc.id)>
        {move || i18n::log_visit(lang.get())} " " {svc.name.clone()}
    </button>
</Show>
```

Define `visit_click` in the same scope — it's analogous to the charge click handler but posts to `/api/payments/log-visit`:

```rust
let visit_click = move |service_id: i64| {
    let card_id = card.id;
    move |_| {
        let lang_val = lang.get_untracked();
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { card_id: i64, service_id: i64 }
            #[derive(serde::Deserialize)]
            struct Resp { transaction_id: i64 }
            match api::post::<Req, Resp>("/api/payments/log-visit", &Req { card_id, service_id }).await {
                Ok(_) => { /* history auto-refreshes — see step 3 */ }
                Err(_) => { /* surface to err signal if one is in scope */ }
            }
        });
    }
};
```

- [ ] **Step 3: Refresh history signal after a successful visit**

Find the signal that holds transaction history for the selected card (search for `Vec<TxnInfo>` in `dashboard.rs`). After a successful visit, trigger a re-fetch — mirror whatever pattern exists for the existing charge flow. Example pattern (exact names depend on what's already there):

```rust
let reload_history = /* whatever signal/function the charge flow already uses */;
// inside Ok(_) arm:
reload_history();
```

If nothing like this exists, add a signal and an Effect that re-fetches on a "dirty" counter change.

- [ ] **Step 4: Add i18n string**

In `i18n.rs`:

```rust
pub fn log_visit(lang: Lang) -> &'static str {
    match lang { Lang::En => "Log visit", Lang::Sk => "Zaznamenať návštevu" }
}
```

- [ ] **Step 5: Trunk build**

Run: `cd spinbike-ui && trunk build`

- [ ] **Step 6: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs spinbike-ui/src/i18n.rs
git commit -m "feat(ui): active pass swaps charge buttons for zero-EUR visit buttons"
```

---

## Task 12: UI — history row shows valid_until + visit styling

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs`

**Context:** History rows for pass purchases should append the end date to the action cell ("charge · until 15.05"); visit rows (amount 0 with action "visit") get a distinct color.

- [ ] **Step 1: Extend TxnInfo with valid_until**

At the top of `dashboard.rs`, update `TxnInfo`:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
struct TxnInfo {
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    card_id: Option<i64>,
    amount: f64,
    action: String,
    created_at: String,
    #[serde(default)]
    service_name: Option<String>,
    #[serde(default)]
    valid_until: Option<chrono::NaiveDate>,
}
```

- [ ] **Step 2: Format the action column**

Find the history row rendering (look for `tx.action` inside a `<tr>`). Replace the action cell with:

```rust
<td>
    {tx.action.clone()}
    {tx.valid_until.map(|d| format!(" · until {}", d.format("%d.%m"))).unwrap_or_default()}
</td>
```

- [ ] **Step 3: Style visit rows**

Add a CSS class on the row when `tx.action == "visit"`:

```rust
<tr class=move || if tx.action == "visit" { "txn-row-visit" } else { "txn-row" }>
    ...
</tr>
```

- [ ] **Step 4: Add CSS**

In `style/main.scss`:

```css
.txn-row-visit { color: #88ccff; }
.txn-row-visit td { font-style: italic; }
```

- [ ] **Step 5: Trunk build**

Run: `cd spinbike-ui && trunk build`

- [ ] **Step 6: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs spinbike-ui/style/main.scss
git commit -m "feat(ui): history row shows pass end date + visit rows styled distinctly"
```

---

## Task 13: E2E — sell pass + banner + visit logging

**Files:**
- Create: `e2e/tests/monthly-pass.spec.ts`

**Context:** Full user flow: staff logs in, searches card, sells pass, sees banner, logs visit, verifies history. Asserts zero browser console errors.

- [ ] **Step 1: Write the test**

Create `e2e/tests/monthly-pass.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

test.describe('Monthly pass — sell, banner, visit', () => {
    test('sell pass → banner appears → visit logs 0 EUR row', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, 'http://localhost:8099', 'staff@test.com', 'staff123');
        await page.goto('/staff');

        // Pick the first card from the seeded test set
        const searchInput = page.locator('input[type="search"]');
        await searchInput.waitFor();
        await searchInput.focus();
        await page.keyboard.type('TestCorp', { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();

        // Top up so the card has credit for the pass
        await page.locator('text=+50').click();
        await expect(page.locator('[data-testid="action-panel"]')).toContainText('50.00');

        // Open the sell-pass modal
        await page.locator('[data-testid="sell-pass-btn"]').click();
        const modal = page.locator('[data-testid="sell-pass-modal"]');
        await expect(modal).toBeVisible();

        // Default price should be 35, date should be today + 30
        const priceInput = page.locator('[data-testid="sell-pass-price"]');
        await expect(priceInput).toHaveValue('35.00');

        // Confirm
        await page.locator('[data-testid="sell-pass-confirm"]').click();
        await expect(modal).not.toBeVisible();

        // Banner appears
        const banner = page.locator('[data-testid="pass-banner-active"]');
        await expect(banner).toBeVisible();
        await expect(banner).toContainText('Monthly pass valid until');
        await expect(banner).toContainText('days remaining');

        // Credit dropped by 35
        await expect(page.locator('[data-testid="action-panel"]')).toContainText('15.00');

        // Charge buttons are now "Log visit" buttons
        const visitBtn = page.locator('[data-testid="log-visit-btn"]').first();
        await expect(visitBtn).toBeVisible();
        await visitBtn.click();

        // History shows a visit row with 0.00 amount
        await expect(page.locator('.txn-row-visit')).toContainText('visit');
        await expect(page.locator('.txn-row-visit')).toContainText('0.00');

        assertCleanConsole(consoleMessages);
    });
});
```

- [ ] **Step 2: Run locally (optional, requires backend on :8099)**

Run: `cd e2e && npx playwright test tests/monthly-pass.spec.ts`
Expected: passes end-to-end against the backend.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/monthly-pass.spec.ts
git commit -m "test(e2e): sell monthly pass, see banner, log zero-EUR visit"
```

---

## Task 14: E2E — expired pass state

**Files:**
- Create: `e2e/tests/monthly-pass-expired.spec.ts`

**Context:** Seed a card via API that has a pass with `valid_until` in the past. Assert red banner appears, charge buttons are back to paid mode (not "Log visit"), sell-pass button highlighted.

The expired state needs a card whose only pass is in the past. There's no API to backdate a pass, so this task adds a test-only fixture endpoint gated on `SPINBIKE_TEST_MODE=1`. The E2E test calls that endpoint to seed the card, then verifies the UI.

- [ ] **Step 1: Write the E2E test**

Create `e2e/tests/monthly-pass-expired.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

test.describe('Monthly pass — expired state', () => {
    test('expired pass → red banner, charge buttons return to paid mode', async ({ page, request }) => {
        const consoleMessages = setupConsoleCheck(page);
        const baseURL = 'http://localhost:8099';
        await loginViaAPI(page, baseURL, 'staff@test.com', 'staff123');

        const cardBarcode = 'EXPIRED-PASS-CARD';
        const seedResp = await request.post(`${baseURL}/api/test/seed-expired-pass`, {
            data: { barcode: cardBarcode, valid_until: '2020-01-01' },
        });
        expect(seedResp.ok()).toBeTruthy();

        await page.goto('/staff');
        const searchInput = page.locator('input[type="search"]');
        await searchInput.focus();
        await page.keyboard.type(cardBarcode, { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();

        const banner = page.locator('[data-testid="pass-banner-expired"]');
        await expect(banner).toBeVisible();
        await expect(banner).toContainText('expired');
        await expect(banner).toContainText('days ago');

        await expect(page.locator('[data-testid="log-visit-btn"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="sell-pass-btn"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});
```

- [ ] **Step 2: Add the test-only seed endpoint**

In `crates/spinbike-server/src/routes/`, add a new file `test_fixtures.rs` (or extend an existing test-only router) with:

```rust
use axum::{extract::State, Json, Router, http::StatusCode, routing::post};
use serde::Deserialize;
use crate::AppState;

#[derive(Deserialize)]
pub struct SeedExpiredPassRequest {
    pub barcode: String,
    pub valid_until: chrono::NaiveDate,
}

pub fn routes() -> Router<AppState> {
    // Only registered when SPINBIKE_TEST_MODE=1.
    Router::new().route("/api/test/seed-expired-pass", post(seed_expired_pass))
}

async fn seed_expired_pass(
    State(state): State<AppState>,
    Json(body): Json<SeedExpiredPassRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use crate::db::cards;
    let card_id = cards::create_card(&state.pool, &body.barcode)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let pass_service_id: i64 =
        sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    sqlx::query(
        "INSERT INTO transactions (card_id, service_id, amount, action, valid_until, created_at)
         VALUES (?, ?, ?, 'charge', ?, datetime('now'))",
    )
    .bind(card_id)
    .bind(pass_service_id)
    .bind(-35.0)
    .bind(body.valid_until)
    .execute(&state.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "card_id": card_id })))
}
```

Register in the main router (`crates/spinbike-server/src/main.rs` or wherever routers are merged):

```rust
let mut app = Router::new()
    // ... existing routes ...
    ;
if std::env::var("SPINBIKE_TEST_MODE").ok().as_deref() == Some("1") {
    app = app.merge(test_fixtures::routes());
}
```

The E2E job must set `SPINBIKE_TEST_MODE=1` before starting the server. Check `.github/workflows/ci.yml` — add the env var to the E2E step if not already present.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/monthly-pass-expired.spec.ts crates/spinbike-server/src/routes/test_fixtures.rs crates/spinbike-server/src/main.rs .github/workflows/ci.yml
git commit -m "test(e2e): expired monthly pass shows red banner and paid charge buttons"
```

---

## Final verification

After all tasks, before opening the PR:

- [ ] **Local lint gate:** `cargo fmt --all --check` (fix with `cargo fmt --all` if needed)
- [ ] **Push and monitor CI:** `git push origin dev`, then watch all jobs reach green including mutation testing, E2E, and deploy.
- [ ] **Post-deploy verification:** open https://spinbike.newlevel.media in Playwright, log in, sell a pass, see banner, log visit. Check browser console is clean.
- [ ] **PR description:** list the eight backend commits + four UI commits + two E2E test commits, link back to the spec doc, include the E2E table required by completion-report rules.
