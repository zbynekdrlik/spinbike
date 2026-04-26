# Legacy Services Backfill + Dual-Language Catalog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore ~7,100 NULL-service legacy transactions and ship a configurable dual-language item catalog (Slovak + English) with stable `kind`-based identifiers for special-purpose services.

**Architecture:** Two new SQLite migrations (V8 services dual-lang + kind, V9 transactions.legacy_backfilled). Code refactor swaps `services.name` for `name_sk`/`name_en` everywhere and replaces `WHERE name='Monthly pass'` with `WHERE kind='monthly_pass'`. New `migrate-legacy --backfill` subcommand walks the legacy `.mdb`, matches prod transactions by `(barcode, created_at, amount)`, and sets `service_id` where currently NULL.

**Tech Stack:** Rust (Axum 0.8 + sqlx + Leptos 0.7 → WASM via Trunk + rust-embed), SQLite, Playwright (E2E), mdbtools (CLI dependency for the backfill).

**Local-build policy (per CLAUDE.md):** Only `cargo fmt --all --check` runs locally. Tests, clippy, build, trunk build run on CI. Each task ends in a commit; verification is via CI green on push. The plan still follows TDD discipline (tests written alongside code in the same task), but RED→GREEN happens on CI, not locally.

---

## File Structure

### New files

| Path | Purpose |
|---|---|
| `crates/spinbike-server/src/db/backfill.rs` | Backfill module: walks `.mdb` and runs idempotent UPDATEs on target DB. Pure logic, callable from the migrate-legacy bin. |
| `e2e/tests/services-admin.spec.ts` | Playwright: admin creates/edits/deactivates a dual-language service. |
| `e2e/tests/card-action-form-language.spec.ts` | Playwright: same service row renders in Slovak vs English. |
| `e2e/tests/legacy-history.spec.ts` | Playwright: card history shows backfilled service labels (3 categories). |

### Modified files

| Path | Change |
|---|---|
| `crates/spinbike-server/src/db/migrations.rs` | Add `V8_SERVICES_DUAL_LANG_KIND`, `V9_TRANSACTIONS_LEGACY_BACKFILL_MARKER`. |
| `crates/spinbike-server/src/routes/admin.rs` | `ServiceRow` / `CreateServiceRequest` / `UpdateServiceRequest` swap `name` for `name_sk`+`name_en`+`kind`. |
| `crates/spinbike-server/src/routes/payments.rs` | 2 sites: `WHERE name='Monthly pass'` → `WHERE kind='monthly_pass'`. |
| `crates/spinbike-server/src/db/transactions.rs` | `TransactionRow.service_name` → `service_name_sk`/`service_name_en`/`service_kind`. SELECTs updated. |
| `crates/spinbike-server/src/db/reports.rs` | Service-grouped reports return `name_sk`+`name_en`. |
| `crates/spinbike-server/src/bin/migrate_legacy.rs` | `map_legacy_service_name` extended; new `--backfill` mode dispatching to `db::backfill`. Lookup uses `name_sk`. |
| `spinbike-ui/src/pages/dashboard/mod.rs` | `ServiceInfo` swaps `name` for `name_sk`+`name_en`+`kind`. New `ServiceInfo::display_name(lang)`. |
| `spinbike-ui/src/pages/dashboard/action_form.rs` | `is_monthly_pass()` from `kind`; `data-kind` attribute on each `<option>`; `display_name(lang)` for option text. |
| `spinbike-ui/src/pages/admin.rs` | `ServicesTab`: 2 name inputs + kind selector on create + read-only kind badge on list. |
| `spinbike-ui/src/pages/dashboard/transactions_list.rs` | Service column uses `display_name(lang)`. |
| `spinbike-ui/src/pages/dashboard/pass_banner.rs` | Detects pass via `service_kind == "monthly_pass"`. |
| `spinbike-ui/src/pages/reports/*` | All service-name renders use `display_name(lang)`. |
| `spinbike-ui/src/i18n.rs` (or wherever the i18n map lives) | Add `service_kind_generic`, `service_kind_monthly_pass`. |
| `e2e/tests/helpers.ts` | `selectMonthlyPass` finds the option via `[data-kind="monthly_pass"]` attribute. |

### Tests

| Path | Coverage |
|---|---|
| `crates/spinbike-server/src/db/migrations.rs` (`#[cfg(test)] mod tests`) | V8 schema shape; V8 preserves ids; V8 partial unique index; V9 column. |
| `crates/spinbike-server/src/db/backfill.rs` (`#[cfg(test)]`) | Idempotency, NULL-guard, ambiguous match, unknown legacy service, orphan card. |
| `crates/spinbike-server/src/bin/migrate_legacy.rs` (`#[cfg(test)]`) | `map_legacy_service_name` extended cases. |
| `crates/spinbike-server/tests/admin_routes.rs` | POST/PUT/GET `/api/admin/services` with `name_sk`/`name_en`/`kind`. PUT cannot change kind. Second `monthly_pass` rejected. |
| `crates/spinbike-server/tests/payments.rs` | Sell pass after kind swap. Rename Monthly-pass `name_en` then sell still succeeds. |
| `crates/spinbike-server/tests/reports.rs` | Reports return both names. |

---

## Task Sequence

The plan is **16 implementation tasks (numbered 1–19 with three "SKIP" placeholders preserved for traceability), then one push task (20) and one PR task (21)**. The backend → frontend changes are tightly coupled (the API shape changes in Task 3 and the frontend deserialization changes in Task 10), so intermediate pushes would temporarily break CI's E2E job. **Commit per task locally; do NOT push until all implementation tasks are committed.** Then a single push at Task 20 triggers one CI cycle that validates everything.

This matches `~/.claude/CLAUDE.md` ci-push-discipline: *"one push, one CI cycle, monitor to completion."*

---

### Task 1: V8 schema migration — services dual-lang + kind

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs:2-32` (MIGRATIONS table) and append a new V8 const after `V7_TRANSACTIONS_SOFT_DELETE`.
- Modify: `crates/spinbike-server/src/db/migrations.rs` `#[cfg(test)] mod tests` (append new test).

- [ ] **Step 1: Append V8 constant**

After the `V7_TRANSACTIONS_SOFT_DELETE` const (around line 204), add:

```rust
const V8_SERVICES_DUAL_LANG_KIND: &str = r#"
CREATE TABLE services_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT    NOT NULL DEFAULT 'generic'
                  CHECK (kind IN ('generic', 'monthly_pass')),
    name_sk       TEXT    NOT NULL,
    name_en       TEXT    NOT NULL,
    default_price REAL    NOT NULL,
    active        INTEGER NOT NULL DEFAULT 1
);

INSERT INTO services_new (id, kind, name_sk, name_en, default_price, active)
SELECT id,
       CASE WHEN name = 'Monthly pass' THEN 'monthly_pass' ELSE 'generic' END,
       CASE name WHEN 'Spinning' THEN 'Spinning'
                 WHEN 'Fitness' THEN 'Fitness'
                 WHEN 'Monthly pass' THEN 'Mesačný preplatok'
                 ELSE name END,
       CASE name WHEN 'Spinning' THEN 'Spinning'
                 WHEN 'Fitness' THEN 'Fitness'
                 WHEN 'Monthly pass' THEN 'Monthly pass'
                 ELSE name END,
       default_price, active
FROM services;

DROP TABLE services;
ALTER TABLE services_new RENAME TO services;

CREATE UNIQUE INDEX idx_services_monthly_pass
    ON services(kind) WHERE kind = 'monthly_pass';

INSERT OR IGNORE INTO services (kind, name_sk, name_en, default_price, active)
VALUES ('generic', 'Občerstvenie',     'Refreshments',        0.0, 1),
       ('generic', 'Doplnky výživy',   'Supplements',         0.0, 1),
       ('generic', 'Aktivácia karty',  'Card activation fee', 0.0, 1);
"#;
```

- [ ] **Step 2: Register V8 in MIGRATIONS array**

In `MIGRATIONS` (top of file, ~line 2-32), append:

```rust
(8, "services_dual_lang_kind", V8_SERVICES_DUAL_LANG_KIND),
```

- [ ] **Step 3: Write tests for V8 schema and seed**

Append in the `tests` module (after the V7 test):

```rust
#[tokio::test]
async fn v8_services_have_dual_lang_and_kind() {
    use crate::db::{create_memory_pool, run_migrations};
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();

    // Schema: name_sk, name_en, kind, default_price, active
    let cols: Vec<(String,)> = sqlx::query_as("PRAGMA table_info(services)")
        .fetch_all(&pool).await.unwrap();
    let names: Vec<&str> = cols.iter().map(|r| r.0.as_str()).collect();
    for col in ["id", "kind", "name_sk", "name_en", "default_price", "active"] {
        assert!(names.contains(&col), "missing column {col} in services");
    }
    assert!(!names.contains(&"name"), "old `name` column must be dropped");

    // Existing rows preserved with correct ids and dual-lang
    let rows: Vec<(i64, String, String, String, f64, i64)> = sqlx::query_as(
        "SELECT id, kind, name_sk, name_en, default_price, active FROM services ORDER BY id"
    ).fetch_all(&pool).await.unwrap();
    let by_kind: std::collections::HashMap<&str, &(i64, String, String, String, f64, i64)> =
        rows.iter().map(|r| (r.1.as_str(), r)).collect();
    let pass = by_kind.get("monthly_pass").expect("monthly_pass row");
    assert_eq!(pass.2, "Mesačný preplatok");
    assert_eq!(pass.3, "Monthly pass");

    // Three new generic rows seeded
    for n_sk in ["Občerstvenie", "Doplnky výživy", "Aktivácia karty"] {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM services WHERE name_sk = ?"
        ).bind(n_sk).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "service '{n_sk}' should be seeded once");
    }
}

#[tokio::test]
async fn v8_only_one_monthly_pass_allowed() {
    use crate::db::{create_memory_pool, run_migrations};
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();

    // Inserting a second monthly_pass must fail (partial unique index).
    let res = sqlx::query(
        "INSERT INTO services (kind, name_sk, name_en, default_price)
         VALUES ('monthly_pass', 'Druhý preplatok', 'Second pass', 35.0)"
    ).execute(&pool).await;
    assert!(res.is_err(), "partial unique index on kind='monthly_pass' must reject duplicates");
}

#[tokio::test]
async fn v8_kind_check_constraint_rejects_unknown() {
    use crate::db::{create_memory_pool, run_migrations};
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();

    let res = sqlx::query(
        "INSERT INTO services (kind, name_sk, name_en, default_price)
         VALUES ('foobar', 'X', 'Y', 1.0)"
    ).execute(&pool).await;
    assert!(res.is_err(), "kind CHECK constraint must reject 'foobar'");
}
```

- [ ] **Step 4: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): V8 services dual-language + kind enum

Migration adds name_sk, name_en, kind columns; preserves Spinning,
Fitness, Monthly pass rows with stable ids; seeds Občerstvenie,
Doplnky výživy, Aktivácia karty; partial unique index keeps
kind='monthly_pass' singleton."
```

---

### Task 2: V9 schema migration — transactions.legacy_backfilled marker

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs` (append V9 const, register in MIGRATIONS, add test).

- [ ] **Step 1: Append V9 constant**

After the V8 const, add:

```rust
const V9_TRANSACTIONS_LEGACY_BACKFILL_MARKER: &str = r#"
ALTER TABLE transactions ADD COLUMN legacy_backfilled INTEGER NOT NULL DEFAULT 0;
"#;
```

- [ ] **Step 2: Register V9 in MIGRATIONS**

```rust
(9, "transactions_legacy_backfill_marker", V9_TRANSACTIONS_LEGACY_BACKFILL_MARKER),
```

- [ ] **Step 3: Write test**

In the tests module:

```rust
#[tokio::test]
async fn v9_transactions_have_legacy_backfilled_column() {
    use crate::db::{create_memory_pool, run_migrations};
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();

    let cols: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(transactions)")
            .fetch_all(&pool).await.unwrap();
    let lb = cols.iter().find(|c| c.1 == "legacy_backfilled")
        .expect("legacy_backfilled column missing");
    assert_eq!(lb.2, "INTEGER");
    assert_eq!(lb.3, 1, "should be NOT NULL");
}
```

- [ ] **Step 4: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): V9 transactions.legacy_backfilled marker

Adds NOT NULL DEFAULT 0 marker column. Used by migrate-legacy
--backfill to identify rows it set, enabling targeted rollback."
```

---

### Task 3: Backend `ServiceRow` and admin routes — dual-lang + kind

**Files:**
- Modify: `crates/spinbike-server/src/routes/admin.rs:60-130` (struct definitions) and the handlers `list_services`, `create_service`, `update_service` around lines 445-540.
- Modify: `crates/spinbike-server/tests/admin_routes.rs` (add tests).

- [ ] **Step 1: Update struct definitions**

Replace the three structs (around lines 63-130):

```rust
#[derive(Debug, serde::Deserialize)]
pub struct CreateServiceRequest {
    pub name_sk: String,
    pub name_en: String,
    pub default_price: f64,
    /// Optional. Defaults to "generic". Only "generic" or "monthly_pass" accepted.
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ServiceRow {
    pub id: i64,
    pub kind: String,
    pub name_sk: String,
    pub name_en: String,
    pub default_price: f64,
    pub active: i64,
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateServiceRequest {
    pub name_sk: Option<String>,
    pub name_en: Option<String>,
    pub default_price: Option<f64>,
    pub active: Option<bool>,
    // NOTE: `kind` is intentionally absent — it's read-only after create.
}
```

- [ ] **Step 2: Update list/create/update handlers**

Around line 445-540 in `admin.rs`:

```rust
async fn list_services(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<ServiceRow>>, (StatusCode, Json<serde_json::Value>)> {
    require_role_one_of(&user, &[Role::Staff, Role::Admin])?;
    let rows = sqlx::query_as::<_, ServiceRow>(
        "SELECT id, kind, name_sk, name_en, default_price, active FROM services ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;
    Ok(Json(rows))
}

async fn create_service(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateServiceRequest>,
) -> Result<(StatusCode, Json<ServiceRow>), (StatusCode, Json<serde_json::Value>)> {
    require_role(&user, Role::Admin)?;
    if body.name_sk.trim().is_empty() || body.name_en.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name_sk and name_en are required"})),
        ));
    }
    let kind = body.kind.as_deref().unwrap_or("generic");
    if !matches!(kind, "generic" | "monthly_pass") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "kind must be 'generic' or 'monthly_pass'"})),
        ));
    }
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO services (kind, name_sk, name_en, default_price) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(kind)
    .bind(&body.name_sk)
    .bind(&body.name_en)
    .bind(body.default_price)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        // Partial unique index on monthly_pass surfaces here.
        if e.to_string().contains("UNIQUE constraint") {
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "a monthly_pass service already exists"})),
            )
        } else {
            internal_error(e)
        }
    })?;
    Ok((
        StatusCode::CREATED,
        Json(ServiceRow {
            id,
            kind: kind.to_string(),
            name_sk: body.name_sk,
            name_en: body.name_en,
            default_price: body.default_price,
            active: 1,
        }),
    ))
}

async fn update_service(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateServiceRequest>,
) -> Result<Json<ServiceRow>, (StatusCode, Json<serde_json::Value>)> {
    require_role(&user, Role::Admin)?;
    let existing = sqlx::query_as::<_, ServiceRow>(
        "SELECT id, kind, name_sk, name_en, default_price, active FROM services WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "service not found"})),
    ))?;

    let name_sk = body.name_sk.unwrap_or(existing.name_sk);
    let name_en = body.name_en.unwrap_or(existing.name_en);
    let default_price = body.default_price.unwrap_or(existing.default_price);
    let active: i64 = body
        .active
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(existing.active);

    sqlx::query(
        "UPDATE services SET name_sk=?, name_en=?, default_price=?, active=? WHERE id=?",
    )
    .bind(&name_sk)
    .bind(&name_en)
    .bind(default_price)
    .bind(active)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(ServiceRow {
        id,
        kind: existing.kind, // unchanged: kind is read-only after create
        name_sk,
        name_en,
        default_price,
        active,
    }))
}
```

- [ ] **Step 3: Add integration tests in `tests/admin_routes.rs`**

Append (the file already has helper imports and an admin token helper):

```rust
#[tokio::test]
async fn create_and_list_services_with_dual_language() {
    let app = crate::helpers::test_app().await;
    let token = crate::helpers::admin_token(&app).await;

    // Create
    let resp = app
        .post("/api/admin/services")
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "name_sk": "Voda",
            "name_en": "Water",
            "default_price": 1.0
        }))
        .send()
        .await;
    assert_eq!(resp.status(), 201);
    let row: serde_json::Value = resp.json().await;
    assert_eq!(row["kind"], "generic");
    assert_eq!(row["name_sk"], "Voda");
    assert_eq!(row["name_en"], "Water");

    // List includes seeded rows
    let resp = app.get("/api/admin/services").bearer_auth(&token).send().await;
    let rows: Vec<serde_json::Value> = resp.json().await;
    assert!(rows.iter().any(|r| r["kind"] == "monthly_pass"));
    assert!(rows.iter().any(|r| r["name_sk"] == "Občerstvenie"));
    assert!(rows.iter().any(|r| r["name_sk"] == "Doplnky výživy"));
    assert!(rows.iter().any(|r| r["name_sk"] == "Aktivácia karty"));
}

#[tokio::test]
async fn put_service_cannot_change_kind() {
    let app = crate::helpers::test_app().await;
    let token = crate::helpers::admin_token(&app).await;

    let pass_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
        .fetch_one(&app.pool).await.unwrap();

    // PUT with `kind` payload — server ignores it.
    let resp = app
        .put(&format!("/api/admin/services/{pass_id}"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name_sk": "Renamed", "kind": "generic" }))
        .send()
        .await;
    assert_eq!(resp.status(), 200);
    let row: serde_json::Value = resp.json().await;
    assert_eq!(row["kind"], "monthly_pass", "kind must remain monthly_pass");
}

#[tokio::test]
async fn create_second_monthly_pass_rejected() {
    let app = crate::helpers::test_app().await;
    let token = crate::helpers::admin_token(&app).await;

    let resp = app
        .post("/api/admin/services")
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "name_sk": "Druhý",
            "name_en": "Second",
            "default_price": 35.0,
            "kind": "monthly_pass"
        }))
        .send()
        .await;
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn create_service_with_invalid_kind_rejected() {
    let app = crate::helpers::test_app().await;
    let token = crate::helpers::admin_token(&app).await;

    let resp = app
        .post("/api/admin/services")
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "name_sk": "X", "name_en": "Y", "default_price": 1.0, "kind": "foobar"
        }))
        .send()
        .await;
    assert_eq!(resp.status(), 400);
}
```

If `tests/helpers/mod.rs` does not yet expose `pool` on the test app, expose it (one line: `pub pool: SqlitePool` field on the test wrapper). Match the existing helper API conventions.

- [ ] **Step 4: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/routes/admin.rs crates/spinbike-server/tests/admin_routes.rs
git commit -m "feat(api): dual-language services + kind on /api/admin/services

ServiceRow exposes kind, name_sk, name_en. POST accepts optional kind
(default 'generic'); PUT ignores kind (read-only). Conflict on second
monthly_pass; 400 on invalid kind."
```

---

### Task 4: Backend payments — switch monthly_pass lookup to kind

**Files:**
- Modify: `crates/spinbike-server/src/routes/payments.rs:79, 279` (2 sites).
- Modify: `crates/spinbike-server/tests/payments.rs` (add regression test).

- [ ] **Step 1: Replace the two name-based lookups**

`payments.rs:79` area — the charge handler currently has:

```rust
let is_pass: Option<bool> =
    sqlx::query_scalar("SELECT name = 'Monthly pass' FROM services WHERE id = ?")
        .bind(sid).fetch_optional(&state.pool).await.map_err(internal_error)?;
```

Replace with:

```rust
let is_pass: Option<bool> =
    sqlx::query_scalar("SELECT kind = 'monthly_pass' FROM services WHERE id = ?")
        .bind(sid).fetch_optional(&state.pool).await.map_err(internal_error)?;
```

`payments.rs:279` area — sell-pass handler currently:

```rust
let service_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name = 'Monthly pass'")
    .fetch_one(&state.pool).await.map_err(internal_error)?;
```

Replace with:

```rust
let service_id: i64 =
    sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&state.pool).await.map_err(internal_error)?;
```

- [ ] **Step 2: Add rename-and-still-sell regression test**

Append in `tests/payments.rs`:

```rust
#[tokio::test]
async fn sell_pass_works_after_admin_renames_pass() {
    let app = crate::helpers::test_app().await;
    let admin = crate::helpers::admin_token(&app).await;
    let staff = crate::helpers::staff_token(&app).await;

    // Activate a card so we have something to sell against.
    let card_id = crate::helpers::activate_card(&app, &staff, "PASS-RN-1", 50.0).await;

    // Admin renames the Monthly pass — both languages.
    let pass_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
        .fetch_one(&app.pool).await.unwrap();
    let resp = app.put(&format!("/api/admin/services/{pass_id}"))
        .bearer_auth(&admin)
        .json(&serde_json::json!({ "name_sk": "Permanentka", "name_en": "Membership" }))
        .send().await;
    assert_eq!(resp.status(), 200);

    // Sell pass must still succeed (lookup is by kind, not name).
    let resp = app.post("/api/payments/sell-pass")
        .bearer_auth(&staff)
        .json(&serde_json::json!({
            "card_id": card_id,
            "price": 35.0,
            "valid_until": "2026-12-31"
        }))
        .send().await;
    assert_eq!(resp.status(), 200, "sell-pass must work after rename");
}
```

- [ ] **Step 3: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 4: Commit**

```bash
git add crates/spinbike-server/src/routes/payments.rs crates/spinbike-server/tests/payments.rs
git commit -m "refactor(payments): identify Monthly pass by kind, not name

Both /charge and /sell-pass now look up the pass via kind='monthly_pass'.
Admin can rename name_sk/name_en freely without breaking sell-pass."
```

---

### Task 5: Backend transactions/reports SELECTs — return dual names + kind

**Files:**
- Modify: `crates/spinbike-server/src/db/transactions.rs:5-20, 80-90` (`TransactionRow` struct + the SELECT).
- Modify: `crates/spinbike-server/src/db/reports.rs` (any SELECT joining services).
- Modify: `crates/spinbike-server/tests/transactions_routes.rs` and `tests/reports.rs` (assertions).

- [ ] **Step 1: Update `TransactionRow`**

In `db/transactions.rs:5-20`, change the struct:

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
    pub service_name_sk: Option<String>,
    #[sqlx(default)]
    pub service_name_en: Option<String>,
    #[sqlx(default)]
    pub service_kind: Option<String>,
    pub deleted_at: Option<String>,
}
```

- [ ] **Step 2: Update the SELECT around line 82-90**

```rust
"SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
        t.amount, t.action, t.created_at, t.valid_until,
        s.name_sk AS service_name_sk,
        s.name_en AS service_name_en,
        s.kind    AS service_kind,
        t.deleted_at
   FROM transactions t
   LEFT JOIN services s ON s.id = t.service_id
  WHERE t.card_id = ? AND t.deleted_at IS NULL
  ORDER BY t.id DESC"
```

- [ ] **Step 3: Update routes/transactions.rs serialization**

Find any `pub struct TransactionView` or response wrapper in `routes/transactions.rs` that surfaces `service_name`. Replace with `service_name_sk`/`service_name_en`/`service_kind`. (If a single field is exposed, expose all three.)

- [ ] **Step 4: Update reports.rs SELECTs**

Search for `s.name` in `db/reports.rs` and `routes/reports.rs`:

```bash
grep -n "s\.name\|services\.name" crates/spinbike-server/src/db/reports.rs crates/spinbike-server/src/routes/reports.rs
```

For each hit, change `s.name AS service_name` to `s.name_sk AS service_name_sk, s.name_en AS service_name_en` and update the receiving struct accordingly.

- [ ] **Step 5: Update integration tests**

In `tests/transactions_routes.rs`, find assertions referencing `service_name` and update:

```rust
// BEFORE
assert_eq!(txn["service_name"], "Spinning");
// AFTER
assert_eq!(txn["service_name_sk"], "Spinning");
assert_eq!(txn["service_name_en"], "Spinning");
```

In `tests/reports.rs`, similar substitution.

- [ ] **Step 6: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-server/src/db/transactions.rs \
        crates/spinbike-server/src/routes/transactions.rs \
        crates/spinbike-server/src/db/reports.rs \
        crates/spinbike-server/src/routes/reports.rs \
        crates/spinbike-server/tests/transactions_routes.rs \
        crates/spinbike-server/tests/reports.rs
git commit -m "refactor(api): transactions and reports return dual-language service

TransactionRow exposes service_name_sk, service_name_en, service_kind.
Reports' service-grouped output carries both names so the UI picks per
Lang. Tests updated accordingly."
```

---

### Task 6: SKIP — push deferred

Originally a "Push Phase A" task. Backend and frontend are tightly coupled (the API shape changes here, the frontend deserialization changes in Task 10). Pushing here would temporarily break E2E. Commit-only at this point; the single push is at Task 20.

Move to Task 7.

---

### Task 7: Migrator — extend service mapping and lookup by name_sk

**Files:**
- Modify: `crates/spinbike-server/src/bin/migrate_legacy.rs:74-81` (`map_legacy_service_name`) and `:258-264` (service-id lookup).
- Add tests in the existing `#[cfg(test)] mod tests`.

- [ ] **Step 1: Extend `map_legacy_service_name`**

```rust
/// Map legacy service names (Slovak, from MS Access `serviceTab`) to
/// the Slovak `name_sk` of the corresponding seeded service.
fn map_legacy_service_name(name: &str) -> Option<&'static str> {
    match name.trim() {
        "Casova karta"     => Some("Mesačný preplatok"),
        "Fitnes"           => Some("Fitness"),
        "Spinbike"         => Some("Spinning"),
        "Doplnky Vyzivy"   => Some("Doplnky výživy"),
        "Obcerstvenie"     => Some("Občerstvenie"),
        "AktivaciaKarty"   => Some("Aktivácia karty"),
        // "Storno" deliberately NOT mapped — action='storno' already labels it.
        // "Iont" had zero historical sales — YAGNI.
        _ => None,
    }
}
```

- [ ] **Step 2: Update the service-id lookup**

Around line 258-264, the loop that builds `service_ids` currently keys on the old `name`. Change the SELECT to `name_sk`:

```rust
let service_ids: std::collections::HashMap<String, i64> =
    sqlx::query_as::<_, (String, i64)>("SELECT name_sk, id FROM services")
        .fetch_all(&pool)
        .await
        .context("Failed to load services for legacy mapping")?
        .into_iter()
        .collect();
```

The matching call site `map_legacy_service_name(legacy_service).and_then(|n| service_ids.get(n).copied())` continues to work — the returned name now matches `name_sk`.

- [ ] **Step 3: Update the importer integration test**

The existing `importer_preserves_service_and_end_date` test asserts `Some("Monthly pass")`. Change to `Some("Mesačný preplatok")` to match the new schema:

```rust
let row: (Option<String>, Option<chrono::NaiveDate>) = sqlx::query_as(
    "SELECT s.name_sk, t.valid_until FROM transactions t
     LEFT JOIN services s ON s.id = t.service_id WHERE t.card_id = 1",
)
.fetch_one(&pool).await.unwrap();
assert_eq!(row.0.as_deref(), Some("Mesačný preplatok"));
```

- [ ] **Step 4: Add new mapping tests**

```rust
#[test]
fn map_legacy_service_extended_names() {
    assert_eq!(map_legacy_service_name("Doplnky Vyzivy"), Some("Doplnky výživy"));
    assert_eq!(map_legacy_service_name("Obcerstvenie"),   Some("Občerstvenie"));
    assert_eq!(map_legacy_service_name("AktivaciaKarty"), Some("Aktivácia karty"));
    assert_eq!(map_legacy_service_name("Storno"), None, "Storno not mapped — action carries it");
}
```

- [ ] **Step 5: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/bin/migrate_legacy.rs
git commit -m "feat(migrator): map Doplnky výživy, Občerstvenie, Aktivácia karty

map_legacy_service_name now covers six legacy services. Lookup keys on
name_sk to match the new schema. Future re-imports correctly link
~7,100 previously-stripped transactions."
```

---

### Task 8: Backfill module + CLI subcommand

**Files:**
- Create: `crates/spinbike-server/src/db/backfill.rs`
- Modify: `crates/spinbike-server/src/db/mod.rs` (add `pub mod backfill;`)
- Modify: `crates/spinbike-server/src/bin/migrate_legacy.rs` (CLI dispatch + new arg parser branch)

- [ ] **Step 1: Create `db/backfill.rs` skeleton + struct definitions**

```rust
//! In-place legacy backfill: walk the .mdb `Data` table and set
//! transactions.service_id where currently NULL, matching by
//! (barcode, created_at, amount). Idempotent.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use sqlx::SqlitePool;
use tracing::{info, warn};

#[derive(Debug, Default)]
pub struct BackfillReport {
    pub matched: u32,
    pub already_set: u32,
    pub unmatched: u32,
    pub orphan_card: u32,
    pub unknown_service: u32,
    pub ambiguous: u32,
    pub per_service: HashMap<String, ServiceCounts>,
}

#[derive(Debug, Default)]
pub struct ServiceCounts {
    pub matched: u32,
    pub already_set: u32,
    pub unmatched: u32,
    pub ambiguous: u32,
}

/// Map a legacy action string to whether it should be considered for backfill.
/// Returns false for actions that legitimately have no service.
pub(crate) fn legacy_action_has_service(action: &str) -> bool {
    !matches!(
        action.trim().trim_matches('"'),
        "Novy kredit" | "Kredit" | "AKTIVACIA" | "BLOKOVANA"
    )
}

fn export_table(mdb_path: &Path, table: &str) -> Result<String> {
    let output = Command::new("mdb-export")
        .arg(mdb_path)
        .arg(table)
        .output()
        .with_context(|| format!("Failed to run mdb-export for table '{table}'"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("mdb-export failed for table '{table}': {stderr}");
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("mdb-export output for '{table}' is not valid UTF-8"))
}

pub fn map_legacy_service_name(name: &str) -> Option<&'static str> {
    match name.trim() {
        "Casova karta"   => Some("Mesačný preplatok"),
        "Fitnes"         => Some("Fitness"),
        "Spinbike"       => Some("Spinning"),
        "Doplnky Vyzivy" => Some("Doplnky výživy"),
        "Obcerstvenie"   => Some("Občerstvenie"),
        "AktivaciaKarty" => Some("Aktivácia karty"),
        _ => None,
    }
}
```

- [ ] **Step 2: Implement the run() entry point**

Append to `backfill.rs`:

```rust
/// Run the in-place backfill. Idempotent: only updates rows where
/// service_id IS NULL. Sets legacy_backfilled = 1 alongside service_id
/// so a targeted rollback is possible.
pub async fn run(pool: &SqlitePool, mdb_path: &Path) -> Result<BackfillReport> {
    info!("Loading services from target DB...");
    let service_ids: HashMap<String, i64> =
        sqlx::query_as::<_, (String, i64)>("SELECT name_sk, id FROM services")
            .fetch_all(pool)
            .await
            .context("Failed to load services from target")?
            .into_iter()
            .collect();

    info!("Reading legacy card table from {}", mdb_path.display());
    let card_csv = export_table(mdb_path, "card")?;
    let mut card_reader = csv::Reader::from_reader(Cursor::new(&card_csv));
    let mut legacy_card_to_barcode: HashMap<String, String> = HashMap::new();
    for result in card_reader.records() {
        let r = result.context("parse legacy card row")?;
        let id = r.get(0).unwrap_or("").trim().to_string();
        let barcode = r.get(1).unwrap_or("").trim().to_string();
        if !id.is_empty() && !barcode.is_empty() {
            legacy_card_to_barcode.insert(id, barcode);
        }
    }
    info!("Mapped {} legacy cards to barcodes", legacy_card_to_barcode.len());

    info!("Reading legacy Data table...");
    let data_csv = export_table(mdb_path, "Data")?;
    let mut data_reader = csv::Reader::from_reader(Cursor::new(&data_csv));

    let mut report = BackfillReport::default();

    for result in data_reader.records() {
        let r = result.context("parse legacy Data row")?;
        // Header: id_data,id_card,user,action,service,suma_SK,Date,EndDate,suma
        let legacy_card_id = r.get(1).unwrap_or("").trim().to_string();
        let action = r.get(3).unwrap_or("").trim();
        let legacy_service = r.get(4).unwrap_or("").trim().trim_matches('"').to_string();
        let date = r.get(6).unwrap_or("").trim().to_string();
        let amount_eur: f64 = r.get(8).unwrap_or("0").trim().parse().unwrap_or(0.0);

        if !legacy_action_has_service(action) {
            continue;
        }
        if legacy_service.is_empty() {
            continue;
        }

        let barcode = match legacy_card_to_barcode.get(&legacy_card_id) {
            Some(bc) => bc,
            None => {
                report.orphan_card += 1;
                continue;
            }
        };

        let new_name = match map_legacy_service_name(&legacy_service) {
            Some(n) => n,
            None => {
                warn!("unknown legacy service '{legacy_service}' on row card={legacy_card_id}");
                report.unknown_service += 1;
                continue;
            }
        };
        let svc_id = match service_ids.get(new_name) {
            Some(id) => *id,
            None => {
                warn!("target DB has no service named '{new_name}' (legacy '{legacy_service}')");
                report.unknown_service += 1;
                continue;
            }
        };

        // UPDATE prod transactions matching (barcode, created_at, amount) where service_id IS NULL.
        // Prod amounts are stored negative for debits; legacy `suma` is positive.
        let updated_ids: Vec<(i64,)> = sqlx::query_as(
            "UPDATE transactions
                SET service_id = ?, legacy_backfilled = 1
              WHERE id IN (
                SELECT t.id
                  FROM transactions t
                  JOIN cards c ON c.id = t.card_id
                 WHERE c.barcode = ?
                   AND t.created_at = ?
                   AND ABS(t.amount + ?) < 0.005
                   AND t.service_id IS NULL
              )
              RETURNING id",
        )
        .bind(svc_id)
        .bind(barcode)
        .bind(&date)
        .bind(amount_eur)
        .fetch_all(pool)
        .await
        .context("backfill UPDATE failed")?;

        let bucket = report
            .per_service
            .entry(new_name.to_string())
            .or_default();
        match updated_ids.len() {
            0 => {
                // Either already-set on a prior run, or no prod row exists for this legacy row.
                // Distinguish by querying without the NULL guard.
                let exists: Option<i64> = sqlx::query_scalar(
                    "SELECT t.id FROM transactions t
                       JOIN cards c ON c.id = t.card_id
                      WHERE c.barcode = ? AND t.created_at = ?
                        AND ABS(t.amount + ?) < 0.005
                      LIMIT 1",
                )
                .bind(barcode).bind(&date).bind(amount_eur)
                .fetch_optional(pool).await.context("probe failed")?;
                if exists.is_some() {
                    report.already_set += 1;
                    bucket.already_set += 1;
                } else {
                    report.unmatched += 1;
                    bucket.unmatched += 1;
                }
            }
            1 => {
                report.matched += 1;
                bucket.matched += 1;
            }
            n => {
                report.matched += n as u32;
                bucket.matched += n as u32;
                report.ambiguous += 1;
                bucket.ambiguous += 1;
                warn!(
                    "ambiguous: legacy row card={legacy_card_id} date={date} amount={amount_eur} matched {n} prod rows: {:?}",
                    updated_ids.iter().map(|(i,)| *i).collect::<Vec<_>>()
                );
            }
        }
    }

    info!("=== Backfill summary ===");
    for (svc, c) in &report.per_service {
        info!(
            "  {svc}: matched={} already-set={} unmatched={} ambiguous={}",
            c.matched, c.already_set, c.unmatched, c.ambiguous
        );
    }
    info!(
        "  TOTAL: matched={} already-set={} unmatched={} ambiguous={} orphan_card={} unknown_service={}",
        report.matched, report.already_set, report.unmatched,
        report.ambiguous, report.orphan_card, report.unknown_service
    );

    Ok(report)
}
```

- [ ] **Step 3: Add unit tests in `backfill.rs`**

Append:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};
    use sqlx::SqlitePool;
    use std::io::Write;

    /// Write a minimal CSV-shaped fake .mdb is not feasible in tests. Instead, we test
    /// the SQL-level update logic directly against a seeded DB by calling the same
    /// UPDATE the run() function uses, in a small helper.
    async fn seed_target(pool: &SqlitePool) -> i64 {
        // Card with one NULL-service transaction matching legacy row.
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, allow_debit) VALUES ('LEG-1', 1) RETURNING id"
        ).fetch_one(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (?, -1.66, 'debit', '11/06/08 21:31:04')"
        ).bind(card_id).execute(pool).await.unwrap();
        card_id
    }

    async fn doplnky_service_id(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT id FROM services WHERE name_sk = 'Doplnky výživy'")
            .fetch_one(pool).await.unwrap()
    }

    /// Helper that runs the backfill UPDATE for a single matched row.
    async fn backfill_one(pool: &SqlitePool, svc_id: i64, barcode: &str, date: &str, amount_eur: f64)
        -> Vec<i64>
    {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "UPDATE transactions
                SET service_id = ?, legacy_backfilled = 1
              WHERE id IN (
                SELECT t.id FROM transactions t
                  JOIN cards c ON c.id = t.card_id
                 WHERE c.barcode = ? AND t.created_at = ?
                   AND ABS(t.amount + ?) < 0.005
                   AND t.service_id IS NULL
              ) RETURNING id"
        ).bind(svc_id).bind(barcode).bind(date).bind(amount_eur)
         .fetch_all(pool).await.unwrap();
        rows.into_iter().map(|(i,)| i).collect()
    }

    #[tokio::test]
    async fn backfill_idempotent_first_run_matches_second_does_nothing() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        seed_target(&pool).await;
        let svc_id = doplnky_service_id(&pool).await;

        let first = backfill_one(&pool, svc_id, "LEG-1", "11/06/08 21:31:04", 1.66).await;
        assert_eq!(first.len(), 1, "first run should match the row");

        let second = backfill_one(&pool, svc_id, "LEG-1", "11/06/08 21:31:04", 1.66).await;
        assert_eq!(second.len(), 0, "second run must not match (NULL guard)");

        let svc: Option<i64> = sqlx::query_scalar(
            "SELECT service_id FROM transactions WHERE card_id = ?"
        ).bind(seed_target_card_id_for(&pool).await).fetch_one(&pool).await.unwrap();
        assert_eq!(svc, Some(svc_id));
    }

    async fn seed_target_card_id_for(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT id FROM cards WHERE barcode = 'LEG-1'")
            .fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn backfill_skips_post_import_sales_with_existing_service() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, allow_debit) VALUES ('LEG-2', 1) RETURNING id"
        ).fetch_one(&pool).await.unwrap();
        let fitness_id: i64 = sqlx::query_scalar(
            "SELECT id FROM services WHERE name_sk = 'Fitness'"
        ).fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, created_at)
             VALUES (?, ?, -1.66, 'debit', '11/06/08 21:31:04')"
        ).bind(card_id).bind(fitness_id).execute(&pool).await.unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let updated = backfill_one(&pool, svc_id, "LEG-2", "11/06/08 21:31:04", 1.66).await;
        assert_eq!(updated.len(), 0, "row already has service_id; must not be touched");

        let svc_after: Option<i64> = sqlx::query_scalar(
            "SELECT service_id FROM transactions WHERE card_id = ?"
        ).bind(card_id).fetch_one(&pool).await.unwrap();
        assert_eq!(svc_after, Some(fitness_id), "service_id must remain Fitness");
    }

    #[tokio::test]
    async fn backfill_ambiguous_match_updates_all() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, allow_debit) VALUES ('LEG-3', 1) RETURNING id"
        ).fetch_one(&pool).await.unwrap();
        // Two prod rows with identical key (same second, same amount).
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (?, -1.66, 'debit', '11/06/08 21:31:04'), (?, -1.66, 'debit', '11/06/08 21:31:04')"
        ).bind(card_id).bind(card_id).execute(&pool).await.unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let updated = backfill_one(&pool, svc_id, "LEG-3", "11/06/08 21:31:04", 1.66).await;
        assert_eq!(updated.len(), 2, "ambiguous: both rows updated to same service_id");
    }

    #[test]
    fn legacy_action_has_service_excludes_topups_and_blocks() {
        assert!(!legacy_action_has_service("Novy kredit"));
        assert!(!legacy_action_has_service("\"Novy kredit\""));
        assert!(!legacy_action_has_service("AKTIVACIA"));
        assert!(!legacy_action_has_service("BLOKOVANA"));
        assert!(legacy_action_has_service("Debet"));
        assert!(legacy_action_has_service("Storno"));
    }

    #[test]
    fn map_legacy_service_name_covers_all_six() {
        assert_eq!(map_legacy_service_name("Fitnes"), Some("Fitness"));
        assert_eq!(map_legacy_service_name("Spinbike"), Some("Spinning"));
        assert_eq!(map_legacy_service_name("Casova karta"), Some("Mesačný preplatok"));
        assert_eq!(map_legacy_service_name("Doplnky Vyzivy"), Some("Doplnky výživy"));
        assert_eq!(map_legacy_service_name("Obcerstvenie"), Some("Občerstvenie"));
        assert_eq!(map_legacy_service_name("AktivaciaKarty"), Some("Aktivácia karty"));
        assert_eq!(map_legacy_service_name("Storno"), None);
        assert_eq!(map_legacy_service_name("Iont"), None);
    }
}
```

Drop the unused `Write` import if the linter complains.

- [ ] **Step 4: Wire `backfill` module**

In `crates/spinbike-server/src/db/mod.rs`, add `pub mod backfill;`.

- [ ] **Step 5: Wire `--backfill` CLI mode in `migrate_legacy.rs`**

Update `parse_args` to accept `--backfill` flag and a `--target` alias for `--output`:

```rust
fn parse_args() -> Result<Mode> {
    let args: Vec<String> = std::env::args().collect();
    let mut mdb_path: Option<PathBuf> = None;
    let mut target: Option<PathBuf> = None;
    let mut backfill = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mdb-path" => {
                i += 1;
                mdb_path = Some(PathBuf::from(args.get(i).context("--mdb-path requires a value")?));
            }
            "--output" | "--target" => {
                i += 1;
                target = Some(PathBuf::from(args.get(i).context("--output/--target requires a value")?));
            }
            "--backfill" => backfill = true,
            other => bail!("Unknown argument: {other}"),
        }
        i += 1;
    }

    let mdb_path = mdb_path.context("Missing required argument: --mdb-path <path>")?;
    let target = target.context("Missing required argument: --output/--target <path>")?;
    if !mdb_path.exists() {
        bail!("MDB file not found: {}", mdb_path.display());
    }

    Ok(if backfill {
        Mode::Backfill { mdb_path, target }
    } else {
        Mode::FreshImport { mdb_path, target }
    })
}

enum Mode {
    FreshImport { mdb_path: PathBuf, target: PathBuf },
    Backfill { mdb_path: PathBuf, target: PathBuf },
}
```

In `main`, dispatch:

```rust
match parse_args()? {
    Mode::FreshImport { mdb_path, target } => run_fresh_import(mdb_path, target).await,
    Mode::Backfill    { mdb_path, target } => {
        if !target.exists() {
            bail!("--backfill requires an existing target DB: {}", target.display());
        }
        let pool = db::create_pool(&target).await?;
        db::run_migrations(&pool).await?;
        let report = db::backfill::run(&pool, &mdb_path).await?;
        info!("Backfill done: matched={} already_set={} unmatched={} ambiguous={}",
              report.matched, report.already_set, report.unmatched, report.ambiguous);
        Ok(())
    }
}
```

Wrap the existing `main` body that does the fresh import into `async fn run_fresh_import(mdb_path: PathBuf, target: PathBuf) -> Result<()>` — paste-move, no logic change.

- [ ] **Step 6: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-server/src/db/backfill.rs \
        crates/spinbike-server/src/db/mod.rs \
        crates/spinbike-server/src/bin/migrate_legacy.rs
git commit -m "feat(migrator): in-place backfill subcommand

migrate-legacy --backfill --mdb-path X --target Y walks legacy Data,
matches prod transactions by (barcode, created_at, amount) where
service_id IS NULL, sets service_id and legacy_backfilled=1.
Idempotent. Reports matched/already_set/unmatched/ambiguous per service."
```

---

### Task 9: SKIP — push deferred

Originally "Push Phase B". Same reasoning as Task 6 — frontend hasn't been updated yet, so any push at this point would break E2E. Commit-only; single push at Task 20.

Move to Task 10.

---

### Task 10: Frontend `ServiceInfo` model + `display_name` helper

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs:67-72`

- [ ] **Step 1: Update struct and add helper**

Replace the existing struct:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ServiceInfo {
    pub id: i64,
    pub kind: String,
    pub name_sk: String,
    pub name_en: String,
    pub default_price: f64,
    #[serde(default = "default_active")]
    pub active: i64,
}

fn default_active() -> i64 { 1 }

impl ServiceInfo {
    pub fn display_name(&self, lang: crate::i18n::Lang) -> &str {
        match lang {
            crate::i18n::Lang::Sk => &self.name_sk,
            crate::i18n::Lang::En => &self.name_en,
        }
    }
    pub fn is_monthly_pass(&self) -> bool {
        self.kind == "monthly_pass"
    }
}
```

- [ ] **Step 2: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "feat(ui): ServiceInfo gains kind, name_sk, name_en + display_name helper"
```

Note: this commit will fail compile in CI because consumers still use `service.name`. That is expected — Tasks 11–14 update those consumers. The implementer can EITHER bundle Tasks 10–14 into one commit, OR push a single Phase C push at the end of Task 14.

---

### Task 11: Frontend `ActionForm` — kind-aware option markers

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs`

- [ ] **Step 1: Replace name-based monthly-pass detection**

Find the constant and the helper that look like:

```rust
const MONTHLY_PASS_NAME: &str = "Monthly pass";
let is_monthly_pass = move || {
    selected.with(|s| s.as_ref().map(|sv| sv.name == MONTHLY_PASS_NAME).unwrap_or(false))
};
```

Replace with kind-based detection (use the helper from Task 10):

```rust
let is_monthly_pass = move || {
    selected.with(|s| s.as_ref().map(|sv| sv.is_monthly_pass()).unwrap_or(false))
};
```

Drop the `MONTHLY_PASS_NAME` constant entirely (and its `pub use`/imports if any).

- [ ] **Step 2: Update option rendering with display_name + data-kind**

Find the `services.get().iter().map(|s| view! { <option ...>...</option> })` block. Replace option markup to render via `display_name(lang)` and to expose `data-kind`:

```rust
{services.get().iter().map(|s| {
    let label = format!("{} ({:.2} €)", s.display_name(lang.get()), s.default_price);
    view! {
        <option value=s.id.to_string() data-kind=s.kind.clone()>
            {label}
        </option>
    }
}).collect::<Vec<_>>()}
```

Keep the existing `selected` binding logic; only the option element changes.

- [ ] **Step 3: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): ActionForm uses ServiceInfo.kind + display_name(lang)

Detect Monthly pass by kind=='monthly_pass' instead of name match.
Each <option> exposes data-kind for E2E selectors. Option label uses
display_name(lang) so dropdown re-renders on language switch."
```

---

### Task 12: Frontend admin `ServicesTab` — dual-name inputs + kind

**Files:**
- Modify: `spinbike-ui/src/pages/admin.rs` — the `ServicesTab` component (around the existing service form and the `api::put` call sites referencing `name`).

- [ ] **Step 1: Update local Req types and form fields**

Find the `#[derive(Serialize)] struct Req { ... name: ... }` declarations inside `ServicesTab` and split into create/update variants:

```rust
#[derive(serde::Serialize)]
struct CreateReq<'a> {
    name_sk: &'a str,
    name_en: &'a str,
    default_price: f64,
    kind: &'a str,
}

#[derive(serde::Serialize, Default)]
struct UpdateReq<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name_sk: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_en: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active: Option<bool>,
}
```

- [ ] **Step 2: Replace the create form view**

Wherever the create form lives, change the single name input to two inputs and add a kind selector:

```rust
view! {
    <div class="row gap">
        <input
            type="text"
            placeholder=move || i18n::t(lang.get(), "service_name_sk")
            on:input=move |ev| set_new_name_sk.set(event_target_value(&ev))
            data-testid="service-name-sk-input"
        />
        <input
            type="text"
            placeholder=move || i18n::t(lang.get(), "service_name_en")
            on:input=move |ev| set_new_name_en.set(event_target_value(&ev))
            data-testid="service-name-en-input"
        />
        <input
            type="number"
            step="0.01"
            placeholder="0.00"
            on:input=move |ev| set_new_price.set(event_target_value(&ev).parse().unwrap_or(0.0))
            data-testid="service-price-input"
        />
        <select
            on:change=move |ev| set_new_kind.set(event_target_value(&ev))
            data-testid="service-kind-select"
        >
            <option value="generic" selected>
                {move || i18n::t(lang.get(), "service_kind_generic")}
            </option>
            <option
                value="monthly_pass"
                disabled=move || existing_kinds.get().contains(&"monthly_pass".to_string())
            >
                {move || i18n::t(lang.get(), "service_kind_monthly_pass")}
            </option>
        </select>
        <button on:click=move |_| { /* POST CreateReq */ } data-testid="service-create-btn">
            {move || i18n::t(lang.get(), "create")}
        </button>
    </div>
}
```

`existing_kinds` is a derived signal computed from the loaded service list:

```rust
let existing_kinds = Memo::new(move |_| {
    services.get().iter().map(|s| s.kind.clone()).collect::<Vec<_>>()
});
```

- [ ] **Step 3: Replace the row rendering and edit handlers**

In the list-render block, where each row currently shows `s.name`, render two columns plus a kind badge:

```rust
view! {
    <tr>
        <td>
            <input value=s.name_sk.clone() on:change=update_name_sk />
        </td>
        <td>
            <input value=s.name_en.clone() on:change=update_name_en />
        </td>
        <td>
            <span class=move || format!("badge badge--{}", s.kind)>
                {move || i18n::t(lang.get(), &format!("service_kind_{}", s.kind))}
            </span>
        </td>
        <td><input type="number" step="0.01" value=format!("{:.2}", s.default_price) /></td>
        <td><input type="checkbox" prop:checked=s.active != 0 on:change=toggle_active /></td>
    </tr>
}
```

Update the `api::put` call sites — they previously passed `Req { name: Some(...), ... }`. Now use `UpdateReq { name_sk: Some(...), ... }` etc. The same handler pattern applies, just with the new field names.

- [ ] **Step 4: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 5: Commit**

```bash
git add spinbike-ui/src/pages/admin.rs
git commit -m "feat(ui): admin ServicesTab — dual-language inputs and kind selector

Create form has separate Slovak / English name inputs plus a kind
selector. The kind selector hides 'Monthly pass' when one already
exists. List rows show a kind badge; renaming names_sk/name_en is
edit-in-place. PUT never sends kind."
```

---

### Task 13: Frontend transactions list, pass banner, reports — display_name

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/transactions_list.rs`
- Modify: `spinbike-ui/src/pages/dashboard/pass_banner.rs`
- Modify: `spinbike-ui/src/pages/reports/*.rs` (all sites that render service name)

- [ ] **Step 1: Update transaction view struct**

In `transactions_list.rs`, the local `TransactionView` struct (or wherever the rows are deserialized) currently has `service_name: Option<String>`. Replace with three fields:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
struct TransactionView {
    pub id: i64,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
    pub service_name_sk: Option<String>,
    pub service_name_en: Option<String>,
    pub service_kind: Option<String>,
    pub valid_until: Option<String>,
}

impl TransactionView {
    fn service_label(&self, lang: crate::i18n::Lang) -> Option<&str> {
        match lang {
            crate::i18n::Lang::Sk => self.service_name_sk.as_deref(),
            crate::i18n::Lang::En => self.service_name_en.as_deref(),
        }
    }
}
```

The render block changes from `txn.service_name.clone()` to `txn.service_label(lang.get()).map(|s| s.to_string())`.

- [ ] **Step 2: Update `pass_banner.rs`**

Find any code that says something like `if txn.service_name == Some("Monthly pass".to_string())`. Replace with kind-based detection:

```rust
if txn.service_kind.as_deref() == Some("monthly_pass") { ... }
```

If pass_banner reads from a separate `CardPass` API shape that doesn't carry kind, leave it alone — that endpoint doesn't depend on the rename.

- [ ] **Step 3: Update reports**

Search `spinbike-ui/src/pages/reports/`:

```bash
grep -rn "service_name" spinbike-ui/src/pages/reports/
```

For each hit, mirror the Step 1 substitution — three fields + a `service_label(lang)` accessor.

- [ ] **Step 4: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 5: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/transactions_list.rs \
        spinbike-ui/src/pages/dashboard/pass_banner.rs \
        spinbike-ui/src/pages/reports/
git commit -m "feat(ui): transactions/banner/reports use display_name(lang)

All three consumers receive name_sk + name_en + kind from the API and
pick the right label per current Lang. Pass banner detects pass via
kind, not name."
```

---

### Task 14: Frontend i18n keys

**Files:**
- Modify: `spinbike-ui/src/i18n.rs` (or wherever the i18n maps live; if it's e.g. `spinbike-ui/src/i18n/mod.rs` follow that path).

- [ ] **Step 1: Add 5 new keys**

In both the SK and EN maps:

```rust
// SK
("service_kind_generic",      "Položka"),
("service_kind_monthly_pass", "Mesačný preplatok"),
("service_name_sk",           "Slovenský názov"),
("service_name_en",           "Anglický názov"),
("create",                    "Vytvoriť"),

// EN
("service_kind_generic",      "Item"),
("service_kind_monthly_pass", "Monthly pass"),
("service_name_sk",           "Slovak name"),
("service_name_en",           "English name"),
("create",                    "Create"),
```

(If `create` already exists in the map, skip that entry and use the existing key in Task 12.)

- [ ] **Step 2: Run formatter**

```bash
cargo fmt --all
```

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "feat(ui): i18n keys for service_kind_* and dual-name input labels"
```

---

### Task 15: SKIP — push deferred

Originally "Push Phase C". E2E specs in Tasks 16–19 will fail without their helper update + new specs. Commit-only; single push at Task 20.

Move to Task 16.

---

### Task 16: E2E helper — `selectMonthlyPass` uses data-kind

**Files:**
- Modify: `e2e/tests/helpers.ts` (the existing `selectMonthlyPass` function).

- [ ] **Step 1: Replace text-matching with attribute-matching**

Replace the existing function:

```typescript
/**
 * Select the Monthly pass option in the unified card-action service dropdown.
 * The option carries data-kind="monthly_pass" so we don't need to match the
 * visible label (which varies by Lang and includes the price).
 */
export async function selectMonthlyPass(page: Page): Promise<void> {
    const value = await page
        .locator('[data-testid="charge-service"] option[data-kind="monthly_pass"]')
        .first()
        .getAttribute('value');
    if (!value) throw new Error('Monthly pass option not found (data-kind="monthly_pass")');
    await page.locator('[data-testid="charge-service"]').selectOption(value);
}
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/helpers.ts
git commit -m "test(e2e): selectMonthlyPass — use data-kind attribute, not visible text

Robust across Lang switches and price formatting differences."
```

---

### Task 17: New E2E test — services-admin (dual-language CRUD)

**Files:**
- Create: `e2e/tests/services-admin.spec.ts`

- [ ] **Step 1: Write the test**

```typescript
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Admin services — dual-language CRUD', () => {
    test('creates, lists, and deactivates a dual-language service', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin?tab=services');

        // Create
        const suffix = Date.now().toString();
        const skName = `TestSk${suffix}`;
        const enName = `TestEn${suffix}`;

        await page.locator('[data-testid="service-name-sk-input"]').fill(skName);
        await page.locator('[data-testid="service-name-en-input"]').fill(enName);
        await page.locator('[data-testid="service-price-input"]').fill('1.50');
        await page.locator('[data-testid="service-create-btn"]').click();

        // The new row appears with both names and a 'generic' kind badge.
        const row = page.locator(`tr:has-text("${skName}")`);
        await expect(row).toBeVisible();
        await expect(row.locator('.badge--generic')).toBeVisible();

        // English name is also rendered.
        await expect(row).toContainText(enName);

        // Switch UI language. Both columns remain visible (admin shows both at once).
        const toggle = page.locator('[data-testid="lang-toggle"]');
        if (await toggle.count()) {
            await toggle.click();
            await expect(row).toContainText(skName);
            await expect(row).toContainText(enName);
        }

        assertCleanConsole(msgs);
    });

    test('cannot create a second monthly_pass via UI', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin?tab=services');

        // The monthly_pass option in the kind selector is disabled when one exists.
        const passOption = page.locator(
            '[data-testid="service-kind-select"] option[value="monthly_pass"]'
        );
        await expect(passOption).toBeDisabled();

        assertCleanConsole(msgs);
    });
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/services-admin.spec.ts
git commit -m "test(e2e): admin services dual-language CRUD + monthly_pass uniqueness"
```

---

### Task 18: New E2E test — language toggle reflects in dropdown

**Files:**
- Create: `e2e/tests/card-action-form-language.spec.ts`

- [ ] **Step 1: Write the test**

```typescript
import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(token: string, suffix: string): Promise<string> {
    const barcode = `LNG-${suffix}`;
    const lastName = `Lang${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: 50, first_name: 'L', last_name: lastName }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status}`);
    return lastName;
}

async function openCardByLastName(page: Page, lastName: string) {
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Card action form — service dropdown is language-aware', () => {
    test('Refreshments shows in EN and Občerstvenie shows in SK', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const lastName = await activateUniqueCard(token, Date.now().toString());

        // Default language in tests is English (set by setEnglishLanguage in loginViaAPI).
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const optionsEn = await page.locator('[data-testid="charge-service"] option').allTextContents();
        expect(optionsEn.some(o => o.includes('Refreshments'))).toBe(true);

        // Switch to Slovak. The toggle UI may live elsewhere; set localStorage and reload.
        await page.evaluate(() => localStorage.setItem('spinbike_lang', 'sk'));
        await page.reload();
        await openCardByLastName(page, lastName);

        const optionsSk = await page.locator('[data-testid="charge-service"] option').allTextContents();
        expect(optionsSk.some(o => o.includes('Občerstvenie'))).toBe(true);
        expect(optionsSk.some(o => o.includes('Doplnky výživy'))).toBe(true);

        assertCleanConsole(msgs);
    });
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/card-action-form-language.spec.ts
git commit -m "test(e2e): service dropdown re-renders in chosen language"
```

---

### Task 19: New E2E test — legacy history shows backfilled labels

**Files:**
- Create: `e2e/tests/legacy-history.spec.ts`

- [ ] **Step 1: Write the test**

This test seeds via SQL fixture using a test-only HTTP route. The repo already has a test-fixtures pattern (`crates/spinbike-server/src/routes/test_fixtures.rs`) — confirm it exposes a way to inject a transaction with a specific `service_id`. If not, add one in Task 19a (next sub-step) before this test.

```typescript
import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function seedCardWithBackfilledHistory(token: string, lastName: string): Promise<void> {
    // Activate fresh card.
    const barcode = `LH-${lastName}`;
    let r = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: 0, first_name: 'L', last_name: lastName }),
    });
    if (!r.ok) throw new Error(`activate: ${r.status}`);

    // Seed three transactions, one per backfilled service kind, via a test-only route.
    // (Adds 'Občerstvenie', 'Doplnky výživy', 'Aktivácia karty'.)
    r = await fetch(`${BASE_URL}/api/test-fixtures/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [
                { amount: -1.66, action: 'debit', service_name_sk: 'Občerstvenie' },
                { amount: -3.15, action: 'debit', service_name_sk: 'Doplnky výživy' },
                { amount: -2.50, action: 'debit', service_name_sk: 'Aktivácia karty' },
            ],
        }),
    });
    if (!r.ok) throw new Error(`seed: ${r.status}`);
}

async function openCardByLastName(page: Page, lastName: string) {
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test('card history shows backfilled service categories', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
    const lastName = `Hist${Date.now()}`;
    await seedCardWithBackfilledHistory(token, lastName);

    await page.goto('/staff');
    await openCardByLastName(page, lastName);

    // History list shows all three categories (English by default in tests).
    const history = page.locator('[data-testid="transactions-list"]');
    await expect(history).toContainText('Refreshments');
    await expect(history).toContainText('Supplements');
    await expect(history).toContainText('Card activation fee');

    assertCleanConsole(msgs);
});
```

- [ ] **Step 2: If needed, extend test-fixtures route**

If `routes/test_fixtures.rs` lacks the `seed-transactions` endpoint, add it (test-only, gated behind a debug feature or a `#[cfg(any(test, debug_assertions))]`). Pattern:

```rust
#[derive(serde::Deserialize)]
struct SeedTxnsRequest {
    barcode: String,
    entries: Vec<SeedEntry>,
}
#[derive(serde::Deserialize)]
struct SeedEntry {
    amount: f64,
    action: String,
    service_name_sk: String,
}

async fn seed_transactions(
    State(state): State<AppState>,
    Json(body): Json<SeedTxnsRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let card_id: i64 = sqlx::query_scalar("SELECT id FROM cards WHERE barcode = ?")
        .bind(&body.barcode).fetch_one(&state.pool).await
        .map_err(|_| (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"card not found"}))))?;
    for e in body.entries {
        let svc_id: Option<i64> = sqlx::query_scalar("SELECT id FROM services WHERE name_sk = ?")
            .bind(&e.service_name_sk).fetch_optional(&state.pool).await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":"db"}))))?;
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, legacy_backfilled)
             VALUES (?, ?, ?, ?, 1)"
        ).bind(card_id).bind(svc_id).bind(e.amount).bind(&e.action)
         .execute(&state.pool).await
         .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":"db"}))))?;
    }
    Ok(StatusCode::NO_CONTENT)
}
```

Register the route under `/api/test-fixtures/seed-transactions` only when running in dev/test mode (the existing fixtures router already does this — append the new route to it).

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/legacy-history.spec.ts \
        crates/spinbike-server/src/routes/test_fixtures.rs
git commit -m "test(e2e): legacy history shows backfilled service labels

Test-only seed endpoint inserts transactions with legacy_backfilled=1
mapped to Občerstvenie / Doplnky výživy / Aktivácia karty.
History card view renders all three by display_name(lang)."
```

---

### Task 20: Single push + monitor CI to all-green

This is the only push in the plan. By this point, Tasks 1–5 (backend), 7–8 (migrator + backfill), 10–14 (frontend), and 16–19 (E2E) have all been committed locally. Push them as one batch so CI runs once.

- [ ] **Step 1: Verify the local branch state**

```bash
git status                     # working tree clean
git log --oneline origin/dev..dev | wc -l   # ~17 commits ahead
```

If anything is uncommitted, finish the prior task before pushing.

- [ ] **Step 2: Sync with main one more time (defensive)**

```bash
git fetch origin
git merge --no-edit origin/main || true   # no-op if already up to date
```

- [ ] **Step 3: Push**

```bash
git push origin dev
```

- [ ] **Step 4: Identify the latest run**

```bash
gh run list --branch dev --limit 3
```

- [ ] **Step 5: Monitor to terminal state (single sleep, no polling)**

```bash
RUN_ID=<run-id-from-step-4>
sleep 300 && gh run view $RUN_ID --json status,conclusion,jobs
```

Expected: ALL jobs `conclusion=success`:
- Lint
- Test (Rust unit + integration including new admin/payments/reports tests; backfill unit tests; migrator extended tests)
- Build WASM (frontend compiles with new ServiceInfo + display_name)
- E2E (all existing specs + services-admin, card-action-form-language, legacy-history)
- Mutation Testing (in-diff)
- Test Integrity
- Version Bump Check

If any job fails: `gh run view $RUN_ID --log-failed`, fix root cause in a single commit batching all related fixes, push, re-monitor with another `sleep 300 && gh run view`. **Never** propose `--admin` / bypass / "merge despite". Fix the gate (per `~/.claude/CLAUDE.md` autonomous-quality-discipline).

---

### Task 21: Open PR and verify mergeable

- [ ] **Step 1: Open PR from dev to main**

```bash
git fetch origin
gh pr create --base main --head dev --title "Legacy services backfill + dual-language item catalog" \
  --body "$(cat <<'EOF'
## Summary
- Restores ~7,100 NULL-service legacy transactions via in-place backfill (`migrate-legacy --backfill`)
- Adds dual-language service catalog: `name_sk` + `name_en`, with stable `kind` enum
- Seeds three new sellable categories: Občerstvenie / Doplnky výživy / Aktivácia karty
- Replaces `WHERE name='Monthly pass'` with `WHERE kind='monthly_pass'` (admin can now rename freely)

## Test plan
- [x] CI green (Lint, Test, Build WASM, E2E, Mutation Testing, Test Integrity, Version Bump)
- [x] Unit tests cover: V8/V9 schema; backfill idempotent / NULL-guard / ambiguous match; legacy_action filter
- [x] Integration tests cover: dual-lang admin CRUD, monthly_pass uniqueness, kind read-only on PUT, sell-pass after rename
- [x] E2E tests cover: services-admin dual-lang, dropdown re-renders per Lang, legacy history shows backfilled labels
- [ ] Post-deploy: open admin → 5+ services in dual-lang, sell pass on test card (banner appears), sell refreshment on test card, history shows it
- [ ] Run backfill on a copy of prod first, verify counts (~7,100 matched), then run against prod

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Verify mergeable + clean**

```bash
PR_NUMBER=$(gh pr view dev --json number --jq .number)
gh api repos/zbynekdrlik/spinbike/pulls/$PR_NUMBER --jq '{mergeable: .mergeable, mergeable_state: .mergeable_state}'
```

Required output: `{"mergeable": true, "mergeable_state": "clean"}`. If `behind`, sync; if `dirty`, fix conflicts; if `unstable`/`blocked`, the gate is failing — investigate.

- [ ] **Step 3: Surface PR URL to the user; wait for explicit "merge it"**

Per `pr-merge-policy.md`, never merge without explicit user instruction.

---

## Operational rollout (post-merge — NOT part of the implementation plan)

These are operational steps the user runs after PR merges to main. The implementer documents them in the PR body but does not execute them.

1. CI deploys to prod automatically on push to main.
2. SSH to prod, copy `/var/lib/spinbike/spinbike.db` to a local scratch path.
3. Run backfill against the COPY:
   ```
   migrate-legacy --backfill --mdb-path zbynek/latest/db/db.mdb --target ./scratch.db
   ```
   Expected report: `matched ≈ 7100`, `unmatched ≈ 0`, `ambiguous ≤ a handful`.
4. Verify on the copy: `sqlite3 scratch.db "SELECT COUNT(*) FROM transactions WHERE service_id IS NULL"` should drop by ~7,100.
5. If counts look right, run backfill against prod:
   ```
   migrate-legacy --backfill --mdb-path zbynek/latest/db/db.mdb --target /var/lib/spinbike/spinbike.db
   ```
   No server restart required (NULL-guard prevents races).
6. Open a card known to have legacy snack purchases — confirm history rows show "Občerstvenie" / "Doplnky výživy".

**Backout:** `UPDATE transactions SET service_id = NULL WHERE legacy_backfilled = 1;` reverses only what backfill did. Schema rollback = restore from daily DB backup.

After ≥2 weeks of stable operation, a follow-up PR can drop the `legacy_backfilled` column.

---

## Spec coverage check (writing-plans self-review)

| Spec section | Covered by |
|---|---|
| Schema migration (services dual-lang + kind) | Task 1 |
| Schema migration (transactions.legacy_backfilled) | Task 2 |
| Migrator `map_legacy_service_name` extension | Task 7 |
| Backfill subcommand | Task 8 |
| Backend admin routes (CreateRequest, UpdateRequest, ServiceRow) | Task 3 |
| Backend payments — kind lookup | Task 4 |
| Backend transactions/reports SELECTs | Task 5 |
| Frontend `ServiceInfo` + `display_name` helper | Task 10 |
| Frontend ActionForm — kind detection + data-kind | Task 11 |
| Frontend admin ServicesTab — dual-name + kind selector | Task 12 |
| Frontend transactions / pass banner / reports | Task 13 |
| Frontend i18n keys | Task 14 |
| E2E helper `selectMonthlyPass` data-kind | Task 16 |
| E2E services-admin spec | Task 17 |
| E2E language-toggle spec | Task 18 |
| E2E legacy-history spec | Task 19 |
| Unit tests: migrator + backfill | Tasks 7, 8 |
| Integration tests: admin / payments / reports | Tasks 3, 4, 5 |
| Risks & rollout | Operational rollout section above |
| Backout: `legacy_backfilled = 1` predicate | Task 2 schema + Task 8 implementation |

All spec requirements have a task. No gaps.
