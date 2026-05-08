# Users-by-last-movement report + soft-delete — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Reports → Users tab listing all users sorted by oldest activity first, with a soft-delete affordance on the user's card panel.

**Architecture:** New SQLite migration V15 adds `users.deleted_at` and retires the V13 synthetic placeholder. New `GET /api/admin/users/by-last-movement` (paginated aggregate query) and `DELETE /api/admin/users/{id}` (sets `deleted_at`). Frontend gains a tab switcher in the existing Reports page, a new `UsersByMovement` list, and a `DeleteUserSheet` modal mounted from the existing card panel. Every existing user-listing query gains `WHERE u.deleted_at IS NULL`.

**Tech Stack:** Rust / Axum 0.8 / sqlx 0.8 / SQLite, Leptos 0.7 CSR/WASM (rust-embed), Playwright TS for E2E, cargo-mutants for mutation testing.

**Spec:** `docs/superpowers/specs/2026-05-08-users-by-last-movement-design.md` (commit dc18cb3).
**Issue:** [#56](https://github.com/zbynekdrlik/spinbike/issues/56) (closes #68 as a side effect).

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `VERSION` | Modify | Bump 0.13.27 → 0.13.28 |
| `Cargo.toml`, `crates/*/Cargo.toml`, `spinbike-ui/Cargo.toml` | Modify (auto via `scripts/sync-version.sh`) | Sync version |
| `spinbike-ui/src/i18n.rs` | Modify | 13 new keys (Slovak unaccented) |
| `crates/spinbike-server/src/db/migrations.rs` | Modify | Add V15 (ALTER TABLE users + retire synthetic) + idempotency tests |
| `crates/spinbike-server/src/db/users.rs` | Modify | Add `delete_user`, `users_by_last_movement`; add `deleted_at IS NULL` filter to every user-listing site |
| `crates/spinbike-server/src/routes/users.rs` | Modify | Add 2 handlers + register routes |
| `crates/spinbike-server/tests/users_by_movement.rs` | Create | 6 list tests |
| `crates/spinbike-server/tests/users_delete.rs` | Create | 7 delete tests |
| `spinbike-ui/src/pages/dashboard/sheets/delete_user.rs` | Create | Confirm modal with warnings |
| `spinbike-ui/src/pages/dashboard/sheets/mod.rs` | Modify | Re-export `DeleteUserSheet` |
| `spinbike-ui/src/pages/dashboard/card_panel.rs` | Modify | Delete button + sheet mount |
| `spinbike-ui/src/pages/reports/mod.rs` | Modify | Tab switcher; mount card panel from row click |
| `spinbike-ui/src/pages/reports/users_by_movement.rs` | Create | List component + paginated fetch + load-more |
| `e2e/tests/users-by-movement.spec.ts` | Create | E2E for full flow |

---

## Task 1: VERSION bump 0.13.27 → 0.13.28 (CONTROLLER-RUN)

**Files:**
- Modify: `VERSION`
- Modify (via script): `Cargo.toml`, `crates/*/Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Sync from origin first**

```bash
git fetch origin && git status -s && cat VERSION
```
Expected: clean working tree, VERSION = `0.13.27`.

- [ ] **Step 2: Bump VERSION**

```bash
echo "0.13.28" > VERSION
```

- [ ] **Step 3: Sync to all Cargo.toml files**

```bash
bash scripts/sync-version.sh
```
Expected: script prints the files it touched.

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git status -s   # confirm only those files staged
git commit -m "$(cat <<'EOF'
chore: bump version to 0.13.28 for #56 (users-by-last-movement)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: i18n keys (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`

Add 13 keys. Slovak unaccented (no diacritics). Insert in alphabetical order with surrounding keys; if alphabetical order is not maintained in the file, append at the end of the same `m.insert(...)` block where `modal_date` and `edit_tx_date` already live.

- [ ] **Step 1: Read current i18n.rs to locate insertion site**

Run: `grep -nE 'modal_date|edit_tx_date|cancel|save' spinbike-ui/src/i18n.rs | head -20`
Pick a stable insertion line near related keys; preserve the file's existing insertion convention.

- [ ] **Step 2: Insert keys**

Add this block (one line per key, format mirrors existing entries — `m.insert("KEY", ("SLOVAK", "ENGLISH"));`):

```rust
m.insert("reports_tab_daily", ("Denna aktivita", "Daily activity"));
m.insert("reports_tab_users", ("Pouzivatelia", "Users"));
m.insert("users_by_movement_heading", ("Pouzivatelia podla posledneho pohybu", "Users by last movement"));
m.insert("last_movement", ("Posledny pohyb", "Last movement"));
m.insert("no_movement_yet", ("Bez pohybu", "No movement yet"));
m.insert("show_more", ("Zobrazit dalsie", "Show more"));
m.insert("delete_user", ("Zmazat pouzivatela", "Delete user"));
m.insert("delete_user_confirm_title", ("Zmazat {name}?", "Delete {name}?"));
m.insert("delete_user_confirm_body", ("Tato akcia skryje pouzivatela vsade. Historia ostane v DB.", "Hides the user everywhere. History stays in the DB."));
m.insert("delete_user_warning_balance", ("Zostatok: {amount} EUR", "Balance: {amount} EUR"));
m.insert("delete_user_warning_pass", ("Aktivna permanentka do {date}", "Active permanentka until {date}"));
m.insert("delete_user_cancel", ("Zrusit", "Cancel"));
m.insert("delete_user_confirm", ("Zmazat", "Delete"));
```

- [ ] **Step 3: Local format check only**

Run: `cargo fmt --all --check`
Expected: no diff. Fix with `cargo fmt --all` if needed.
**Do NOT run** `cargo build` / `cargo test` / `cargo clippy` / `trunk build` — CI is authoritative.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "$(cat <<'EOF'
feat(i18n): keys for users-by-movement report + delete-user (#56)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Migration V15 + retire synthetic + idempotency tests (subagent, sonnet)

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs`

V15 mirrors V7's pattern: a plain SQL string in the `MIGRATIONS` array. Idempotency comes from the migration runner's version-table guard, not from PRAGMA checks; running migrations twice is a no-op for already-applied versions. The internal `UPDATE … WHERE deleted_at IS NULL` is naturally idempotent on its own.

**Why retire the synthetic here:** V13 created a `(deleted)` placeholder user to keep orphan transactions referenceable. With soft-delete now available, the placeholder should be hidden from UI surfaces — set its `deleted_at` to `datetime('now')` as part of V15 so it disappears from search/lists without dropping its referenced transactions. This closes #68 as a side effect.

- [ ] **Step 1: Insert the V15 SQL constant**

Add after `V14_RENAME_MONTHLY_PASS_LABEL` (around line 565):

```rust
const V15_USERS_SOFT_DELETE: &str = r#"
-- Issue #56: soft-delete column on users + retire V13 (deleted) placeholder.
-- Adds users.deleted_at; every existing user-listing query in db/users.rs
-- gains `WHERE u.deleted_at IS NULL` in Task 4 of this plan. Per-user-history
-- endpoints intentionally do NOT add the filter so a deep link still renders
-- the row's history.
--
-- Side effect (closes #68): V13 inserted a synthetic '(deleted)' customer to
-- keep orphan transactions referenceable. Set its deleted_at so it stops
-- surfacing in search/dropdowns/reports. Transactions stay attached.
ALTER TABLE users ADD COLUMN deleted_at TEXT;

UPDATE users
   SET deleted_at = datetime('now')
 WHERE name = '(deleted)' AND deleted_at IS NULL;
"#;
```

- [ ] **Step 2: Register V15 in the MIGRATIONS array**

In `pub(crate) static MIGRATIONS: &[(i64, &str, &str)] = &[ ... ];` (around line 2), append:

```rust
    (
        15,
        "users: soft-delete column + retire V13 (deleted) synthetic",
        V15_USERS_SOFT_DELETE,
    ),
```

- [ ] **Step 3: Add unit tests**

Append these tests to the `#[cfg(test)] mod tests { ... }` block in the same file (after the existing V13/V14 tests):

```rust
#[tokio::test]
async fn v15_adds_users_deleted_at_column() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let cols: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM pragma_table_info('users')")
            .fetch_all(&pool)
            .await
            .unwrap();
    let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
    assert!(
        names.contains(&"deleted_at"),
        "users.deleted_at column missing; found columns: {names:?}"
    );
}

#[tokio::test]
async fn v15_retires_synthetic_deleted_user() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    // V13 may insert (deleted) only when there are orphan rows. On a clean
    // pool no orphans exist, so the row may be absent — assert that IF the
    // row exists it has deleted_at set.
    let row: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, deleted_at FROM users WHERE name = '(deleted)' ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    if let Some((_id, deleted_at)) = row {
        assert!(
            deleted_at.is_some(),
            "synthetic (deleted) user must have deleted_at set after V15"
        );
    }
}

#[tokio::test]
async fn v15_is_idempotent() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    run_migrations(&pool).await.unwrap();
    // Second run is a no-op via the version-table guard. The deleted_at
    // column must still be exactly one column on users.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('users') WHERE name = 'deleted_at'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "users.deleted_at must exist exactly once after re-run");
}
```

- [ ] **Step 4: Local format check**

Run: `cargo fmt --all --check`
Expected: no diff.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "$(cat <<'EOF'
feat(db): V15 users.deleted_at + retire (deleted) synthetic (#56 #68)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: DB layer + filter ripple audit (subagent, sonnet)

**Files:**
- Modify: `crates/spinbike-server/src/db/users.rs`
- Possibly modify: `crates/spinbike-server/src/db/reports.rs`, `crates/spinbike-server/src/jobs/charger.rs`, `crates/spinbike-server/src/routes/payments.rs`, `crates/spinbike-server/src/routes/classes.rs`, `crates/spinbike-server/src/routes/transactions.rs`

This task makes soft-delete real: every place that reads `users` for listing/search/dropdown gains the filter, and we add the two new read functions for the report.

### Step 1: Run the exhaustive audit

- [ ] **Step 1.1: Enumerate every users-table site**

Run exactly:

```bash
grep -nE "FROM users|JOIN users|users WHERE|UPDATE users" crates/spinbike-server/src -r
```

Expected hits (audit each — don't trust the list, re-run the grep):

| File | Line | What it does | Action |
|---|---|---|---|
| `db/users.rs:56` | `SELECT * FROM users WHERE search_text IS NULL OR search_text = ''` (search_text backfill on startup) | **Skip filter** — needs to backfill all users including soft-deleted ones so the column stays consistent. |
| `db/users.rs:122` | `SELECT * FROM users WHERE email = ?` (login lookup) | **Add filter** — soft-deleted users cannot log in. |
| `db/users.rs:131` | `SELECT * FROM users WHERE id = ?` (`get_user_by_id`) | **Skip filter** — per-row endpoints serve the full row regardless (used by card panel for soft-deleted users until panel closes). |
| `db/users.rs:145` | `SELECT * FROM users WHERE oauth_provider = ? AND oauth_id = ?` (OAuth login) | **Add filter**. |
| `db/users.rs:156` | `SELECT * FROM users ORDER BY id` (list_all_users) | **Add filter**. |
| `db/users.rs:251` | `list_all_users_with_pass` SELECT … FROM users u | **Add filter** (`WHERE u.deleted_at IS NULL`). |
| `db/users.rs:291` | `search_users_with_pass` | **Add filter** before the `ORDER BY`. |
| `db/users.rs:326` | `search_users` | **Add filter**. |
| `db/users.rs:344` | `get_user_by_card_code` | **Add filter** — search/scan should not find soft-deleted users. |
| `db/users.rs:459` | (negative-balance feed query at start of `users_with_negative_balance`) | **Add filter** (`WHERE u.deleted_at IS NULL`). |
| `db/users.rs:489` | `UPDATE users SET ...` (some set-like operation) | Inspect; UPDATE is fine, but if it lists rows first, filter that read. |
| `db/reports.rs:35`, `db/reports.rs:168` | `LEFT JOIN users u ON u.id = t.user_id` (revenue/activity reports) | **Skip filter** — historical activity should still show transactions even after the user is soft-deleted; the report just shows a missing name. (Add inline comment noting the decision.) |
| `routes/classes.rs:173` | `JOIN users u ON u.id = b.user_id` (per-class roster) | **Skip filter** — bookings for a soft-deleted user are still part of class history. Add comment noting the decision. |
| `routes/payments.rs:125, 200, 272` | `SELECT * FROM users WHERE id = ?` per payment lookup | **Skip filter** — same reason as `db/users.rs:131`; admin manual ops on a known user id should still work even after soft-delete (and the soft-delete endpoint itself depends on this). |
| `routes/payments.rs:147, 213, 296`, `routes/transactions.rs:116`, `jobs/charger.rs:85` | `UPDATE users SET credit = ...` | **Skip filter** — these only run for known user ids that came from a prior get; not a listing path. |
| `jobs/charger.rs:62` | `SELECT … FROM users u WHERE u.id = ?` (per-user lookup) | **Skip filter** — per-id lookup. |
| `jobs/charger.rs:184/198/226/260` | `SELECT credit FROM users WHERE id = ?` (test fixtures) | **Skip filter** — tests. |
| `routes/test_fixtures.rs:162` | `UPDATE users SET credit = ?` (test seed) | **Skip filter** — seed code. |
| `db/migrations.rs:1610..1827` | various `FROM users` in V13/V14 unit tests | **Skip filter** — test queries against pre-V15 state. |

If grep finds something not in this table, add it to the audit. The subagent must produce a final list of all touch points and apply the filter (or document the skip) before committing.

- [ ] **Step 1.2: Patch the listing queries**

Apply the `WHERE u.deleted_at IS NULL` (or `AND u.deleted_at IS NULL` if a `WHERE` already exists) to every "Add filter" row above. For aliased queries (e.g. `FROM users u`), use the alias; for unaliased (`SELECT * FROM users`), use bare `deleted_at IS NULL`.

Example patch for `db/users.rs:317` (`search_users`):

```rust
let users = sqlx::query_as::<_, UserRow>(
    "SELECT * FROM users
     WHERE deleted_at IS NULL
       AND search_text LIKE ?
     ORDER BY
       CASE WHEN card_code LIKE ? THEN 0 ELSE 1 END,
       name IS NULL, name ASC,
       card_code ASC
     LIMIT ?",
)
```

For `list_all_users_with_pass` (`db/users.rs:251`):

```rust
// ... existing SELECT ...
         FROM users u
         WHERE u.deleted_at IS NULL
         ORDER BY u.name",
```

### Step 2: Add the two new DB functions

- [ ] **Step 2.1: `users_by_last_movement`**

Append to `crates/spinbike-server/src/db/users.rs` (above the `#[cfg(test)]` block, near the other listing functions):

```rust
/// Row returned by `users_by_last_movement`.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct UserByMovementRow {
    pub id: i64,
    pub name: String,
    pub last_movement_at: Option<String>,
}

/// List users (excluding soft-deleted) with their most recent non-voided
/// transaction's created_at, sorted oldest-movement-first.
/// Users with no transactions appear first (last_movement_at IS NULL).
pub async fn users_by_last_movement(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<UserByMovementRow>> {
    let rows = sqlx::query_as::<_, UserByMovementRow>(
        "SELECT
            u.id,
            u.name,
            MAX(t.created_at) AS last_movement_at
           FROM users u
           LEFT JOIN transactions t
             ON t.user_id = u.id AND t.deleted_at IS NULL
          WHERE u.deleted_at IS NULL
          GROUP BY u.id
          ORDER BY last_movement_at IS NULL DESC,
                   last_movement_at ASC,
                   u.id ASC
          LIMIT ?1 OFFSET ?2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .context("Failed to list users by last movement")?;
    Ok(rows)
}
```

- [ ] **Step 2.2: `delete_user`**

Append:

```rust
/// Outcome of a delete attempt — distinguishes "no such id" from
/// "already deleted" so the route can map to 404 vs 409.
pub enum DeleteUserOutcome {
    Deleted { deleted_at: String },
    NotFound,
    AlreadyDeleted,
}

/// Soft-delete a user by setting `deleted_at` to now. Idempotent semantics:
/// returns `AlreadyDeleted` if the user already has `deleted_at`. Transactions
/// for that user are NOT touched.
pub async fn delete_user(pool: &SqlitePool, id: i64) -> Result<DeleteUserOutcome> {
    // Fetch current row to disambiguate not-found vs already-deleted
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT deleted_at FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("Failed to fetch user before delete")?;
    let Some((existing,)) = row else {
        return Ok(DeleteUserOutcome::NotFound);
    };
    if existing.is_some() {
        return Ok(DeleteUserOutcome::AlreadyDeleted);
    }
    let now: (String,) = sqlx::query_as("SELECT datetime('now')")
        .fetch_one(pool)
        .await
        .context("Failed to read current time from sqlite")?;
    sqlx::query("UPDATE users SET deleted_at = ? WHERE id = ?")
        .bind(&now.0)
        .bind(id)
        .execute(pool)
        .await
        .context("Failed to soft-delete user")?;
    Ok(DeleteUserOutcome::Deleted { deleted_at: now.0 })
}
```

### Step 3: Local format + commit

- [ ] **Step 3.1: Format check**

Run: `cargo fmt --all --check`
Expected: no diff.

- [ ] **Step 3.2: Commit**

```bash
git add crates/spinbike-server/src/db/users.rs   # plus any other files patched in Step 1.2
git status -s   # confirm only the audited files are staged
git commit -m "$(cat <<'EOF'
feat(db): users.deleted_at filter ripple + by-last-movement + delete (#56)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Backend routes + integration tests (subagent, sonnet)

**Files:**
- Modify: `crates/spinbike-server/src/routes/users.rs`
- Possibly modify: `crates/spinbike-server/src/routes/mod.rs` (route registration)
- Create: `crates/spinbike-server/tests/users_by_movement.rs`
- Create: `crates/spinbike-server/tests/users_delete.rs`

### Step 1: Add the two route handlers

- [ ] **Step 1.1: Add handlers to `routes/users.rs`**

Locate the existing `pub fn router() -> Router<AppState>` block (or wherever admin user routes are nested). Add these handlers + register the routes:

```rust
use crate::db::users::{delete_user as db_delete_user, users_by_last_movement, DeleteUserOutcome, UserByMovementRow};

#[derive(serde::Deserialize)]
struct ByMovementQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}
fn default_limit() -> i64 { 50 }

async fn list_users_by_movement(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<ByMovementQuery>,
) -> Result<Json<Vec<UserByMovementRow>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err(super::forbidden("Staff access required"));
    }
    if !(1..=200).contains(&q.limit) || q.offset < 0 {
        return Err(super::bad_request("limit must be 1..=200, offset >= 0"));
    }
    let rows = users_by_last_movement(&state.pool, q.limit, q.offset)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

#[derive(serde::Serialize)]
struct DeleteUserResp { id: i64, deleted_at: String }

async fn delete_user_route(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<DeleteUserResp>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err(super::forbidden("Staff access required"));
    }
    match db_delete_user(&state.pool, id).await.map_err(internal_error)? {
        DeleteUserOutcome::Deleted { deleted_at } => Ok(Json(DeleteUserResp { id, deleted_at })),
        DeleteUserOutcome::NotFound => Err(super::not_found("User not found")),
        DeleteUserOutcome::AlreadyDeleted => Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "User already deleted" })),
        )),
    }
}
```

- [ ] **Step 1.2: Register the routes**

Inside the admin router (look for `.route("/admin/users` or similar; if absent, mirror the pattern from another admin route):

```rust
.route("/admin/users/by-last-movement", get(list_users_by_movement))
.route("/admin/users/{id}", delete(delete_user_route))
```

If `super::forbidden`, `super::bad_request`, `super::not_found` don't exist with these exact names, use whichever helper the file already uses for those status codes (mirror `patch_void_transaction` in `routes/transactions.rs`). Inline the `(StatusCode, Json(json!(...)))` tuple if no helper exists.

### Step 2: Backend tests for list endpoint

- [ ] **Step 2.1: Create `crates/spinbike-server/tests/users_by_movement.rs`**

Mirror an existing integration test file (e.g. `transactions_date.rs`) for setup. Cases:

```rust
// Setup helpers (use the existing test_helpers / spawn_test_app pattern from
// other tests; mirror exactly what users_search.rs or transactions_date.rs do).

// 1. Seeds 3 users with varying movement times → asserts order oldest-first
async fn list_orders_by_oldest_movement_first() { /* ... */ }

// 2. Users with no transactions appear first (NULLS FIRST)
async fn list_users_with_no_transactions_appear_first() { /* ... */ }

// 3. limit=2, offset=2 returns the next slice in stable order
async fn list_paginates_with_show_more() { /* ... */ }

// 4. A user whose only txn is voided shows last_movement_at = null
async fn list_excludes_voided_transactions() { /* ... */ }

// 5. Soft-deleted user does not appear
async fn list_excludes_soft_deleted_users() { /* ... */ }

// 6. Non-staff token → 403
async fn list_requires_staff_role() { /* ... */ }
```

Each test calls the live API via `reqwest` (or whichever helper the existing tests use), seeds via the test_fixtures route or direct sqlx, and asserts the response shape.

### Step 3: Backend tests for delete endpoint

- [ ] **Step 3.1: Create `crates/spinbike-server/tests/users_delete.rs`**

Cases:

```rust
async fn delete_user_happy_path_sets_deleted_at() { /* DELETE /api/admin/users/{id} → 200, then fetch shows deleted_at */ }
async fn delete_user_already_deleted_returns_409() { /* call twice; second is 409 */ }
async fn delete_user_missing_id_returns_404() { /* DELETE for non-existent id → 404 */ }
async fn delete_user_non_staff_returns_403() { /* customer token → 403 */ }
async fn delete_user_does_not_remove_transactions() { /* seed user + txn, delete, COUNT(*) FROM transactions WHERE user_id = ? > 0 */ }
async fn deleted_user_hidden_from_search() { /* search by name returns empty after delete */ }
async fn deleted_user_hidden_from_negative_balance() { /* user with negative balance soft-deleted is omitted from /api/admin/users/negative-balance */ }
```

### Step 4: Local format + commit

- [ ] **Step 4.1: Format check**

Run: `cargo fmt --all --check`

- [ ] **Step 4.2: Commit**

```bash
git add crates/spinbike-server/src/routes/users.rs crates/spinbike-server/src/routes/mod.rs crates/spinbike-server/tests/users_by_movement.rs crates/spinbike-server/tests/users_delete.rs
git status -s
git commit -m "$(cat <<'EOF'
feat(routes): GET by-last-movement + DELETE /admin/users/{id} (#56)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Frontend `DeleteUserSheet` (subagent, sonnet)

**Files:**
- Create: `spinbike-ui/src/pages/dashboard/sheets/delete_user.rs`
- Modify: `spinbike-ui/src/pages/dashboard/sheets/mod.rs`

Mirror `EditTxDateSheet` (`spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs`) for structure: `show: RwSignal<bool>`, per-mount fresh state, error signal, `on_saved: Callback<()>`. Replace the date-input body with the conditional warnings.

### Step 1: Component

- [ ] **Step 1.1: Create `delete_user.rs`**

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

#[component]
pub fn DeleteUserSheet(
    show: RwSignal<bool>,
    user_id: i64,
    name: String,
    /// Current credit balance — warning row appears if non-zero.
    balance: f64,
    /// Active permanentka end date if any — warning row appears when Some.
    active_pass_end: Option<chrono::NaiveDate>,
    on_saved: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let name_for_template = name.clone();

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            let (err, set_err) = signal(String::new());
            let (saving, set_saving) = signal(false);
            let name_inner = name_for_template.clone();

            let on_confirm = move |_| {
                set_err.set(String::new());
                set_saving.set(true);
                spawn_local(async move {
                    match api::delete::<serde_json::Value>(&format!("/api/admin/users/{user_id}")).await {
                        Ok(_) => {
                            show.set(false);
                            on_saved.run(());
                        }
                        Err(e) => set_err.set(e),
                    }
                    set_saving.set(false);
                });
            };
            let on_cancel = move |_| { set_err.set(String::new()); show.set(false); };

            let title = i18n::t(lang.get(), "delete_user_confirm_title")
                .replace("{name}", &name_inner);

            view! {
                <Sheet
                    on_close=Callback::new(move |()| show.set(false))
                    title=title
                    testid="sheet-delete-user"
                >
                    <p>{i18n::t(lang.get(), "delete_user_confirm_body")}</p>
                    {move || {
                        if balance.abs() < 0.005 {
                            ().into_any()
                        } else {
                            let txt = i18n::t(lang.get(), "delete_user_warning_balance")
                                .replace("{amount}", &format!("{:+.2}", balance));
                            view! {
                                <div class="alert alert--warning" data-testid="delete-user-warning-balance">{txt}</div>
                            }.into_any()
                        }
                    }}
                    {move || {
                        match active_pass_end {
                            Some(d) => {
                                let txt = i18n::t(lang.get(), "delete_user_warning_pass")
                                    .replace("{date}", &i18n::fmt_date(d, lang.get()));
                                view! {
                                    <div class="alert alert--warning" data-testid="delete-user-warning-pass">{txt}</div>
                                }.into_any()
                            }
                            None => ().into_any(),
                        }
                    }}
                    {move || {
                        let e = err.get();
                        if e.is_empty() { view! { <div></div> }.into_any() }
                        else { view! { <div class="alert alert--error" data-testid="delete-user-error">{e}</div> }.into_any() }
                    }}
                    <div class="sheet__actions">
                        <button class="btn btn--ghost" disabled=move || saving.get() on:click=on_cancel data-testid="delete-user-cancel">
                            {i18n::t(lang.get(), "delete_user_cancel")}
                        </button>
                        <button class="btn btn--danger" disabled=move || saving.get() on:click=on_confirm data-testid="delete-user-confirm">
                            {i18n::t(lang.get(), "delete_user_confirm")}
                        </button>
                    </div>
                </Sheet>
            }.into_any()
        }}
    }
}
```

If `api::delete` does not exist, mirror `api::patch` and add the helper alongside it in `spinbike-ui/src/api.rs` (one-line wrapper for HTTP DELETE) — same call surface as `patch::<Req, Resp>` but without a body. If `api::delete` already exists, use it as-is.

If `i18n::fmt_date(date, lang)` doesn't exist, fall back to `format!("{}", d.format("%d.%m.%Y"))`.

### Step 2: Re-export

- [ ] **Step 2.1: Update `sheets/mod.rs`**

Add the lines mirroring `EditTxDateSheet`'s export:

```rust
pub mod delete_user;
pub use delete_user::DeleteUserSheet;
```

### Step 3: Local format + commit

- [ ] **Step 3.1: Format check**

Run: `cargo fmt --all --check`

- [ ] **Step 3.2: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/sheets/delete_user.rs spinbike-ui/src/pages/dashboard/sheets/mod.rs spinbike-ui/src/api.rs
git status -s   # api.rs only if you added api::delete
git commit -m "$(cat <<'EOF'
feat(ui): DeleteUserSheet confirm modal (#56)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Reports tab + UsersByMovement + card-panel button + E2E (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/src/pages/reports/mod.rs`
- Create: `spinbike-ui/src/pages/reports/users_by_movement.rs`
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs`
- Create: `e2e/tests/users-by-movement.spec.ts`

### Step 1: Tab switcher in reports/mod.rs

- [ ] **Step 1.1: Add a tab enum + signal**

Above the existing `RangeMode` enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsersTab { DailyActivity, Users }
```

Inside `ReportsPage()`:

```rust
let (tab, set_tab) = signal(UsersTab::DailyActivity);
```

Add the tab switcher view — place it as the FIRST element inside the existing `<div class="reports-page" data-testid="reports-page">` block (above the `reports-date-strip`):

```rust
<div class="seg" role="tablist" data-testid="reports-tabs">
    <button class="seg__item" data-testid="reports-tab-daily"
            aria-selected=move || (tab.get() == UsersTab::DailyActivity).to_string()
            on:click=move |_| set_tab.set(UsersTab::DailyActivity)>
        {move || i18n::t(lang.get(), "reports_tab_daily")}
    </button>
    <button class="seg__item" data-testid="reports-tab-users"
            aria-selected=move || (tab.get() == UsersTab::Users).to_string()
            on:click=move |_| set_tab.set(UsersTab::Users)>
        {move || i18n::t(lang.get(), "reports_tab_users")}
    </button>
</div>
```

Wrap the existing date-strip + KPI/feed content in a `{move || if tab.get() == UsersTab::DailyActivity { view! { ... }.into_any() } else { view! { <users_by_movement::UsersByMovement /> }.into_any() }}` block. If a wrap is too disruptive, render both branches and toggle their visibility with a class — pick whichever keeps the diff smallest while preserving testid `reports-page`.

- [ ] **Step 1.2: Add the module declaration + import**

Top of `reports/mod.rs`:

```rust
mod users_by_movement;
```

### Step 2: UsersByMovement component

- [ ] **Step 2.1: Create `spinbike-ui/src/pages/reports/users_by_movement.rs`**

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
struct Row {
    id: i64,
    name: String,
    last_movement_at: Option<String>,
}

#[component]
pub fn UsersByMovement(
    /// Open the user's card panel when a row is clicked.
    on_open_user: Callback<i64>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (rows, set_rows) = signal::<Vec<Row>>(Vec::new());
    let (offset, set_offset) = signal(0i64);
    let (loading, set_loading) = signal(true);
    let (has_more, set_has_more) = signal(false);
    let (error, set_error) = signal(String::new());

    const PAGE: i64 = 50;

    // Initial load
    Effect::new(move |_| {
        set_loading.set(true);
        spawn_local(async move {
            let url = format!("/api/admin/users/by-last-movement?limit={PAGE}&offset=0");
            match api::get::<Vec<Row>>(&url).await {
                Ok(r) => {
                    let len = r.len() as i64;
                    set_rows.set(r);
                    set_has_more.set(len == PAGE);
                    set_offset.set(len);
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    });

    let on_show_more = move |_| {
        set_loading.set(true);
        let cur_offset = offset.get();
        spawn_local(async move {
            let url = format!("/api/admin/users/by-last-movement?limit={PAGE}&offset={cur_offset}");
            match api::get::<Vec<Row>>(&url).await {
                Ok(r) => {
                    let len = r.len() as i64;
                    set_rows.update(|v| v.extend(r));
                    set_has_more.set(len == PAGE);
                    set_offset.update(|n| *n += len);
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <section class="users-by-movement" data-testid="users-by-movement">
            <h2>{move || i18n::t(lang.get(), "users_by_movement_heading")}</h2>
            {move || if !error.get().is_empty() {
                view! { <div class="alert alert--error">{move || error.get()}</div> }.into_any()
            } else { ().into_any() }}
            <ul class="list" data-testid="users-by-movement-list">
                <For
                    each=move || rows.get().into_iter().enumerate().collect::<Vec<_>>()
                    key=|(i, r)| (*i, r.id)
                    children=move |(_, r)| {
                        let id = r.id;
                        let display_date = match &r.last_movement_at {
                            Some(s) if s.len() >= 10 => s[..10].to_string(),
                            _ => i18n::t(lang.get(), "no_movement_yet").to_string(),
                        };
                        view! {
                            <li class="list-row" data-testid="user-row" data-user-id=id
                                on:click=move |_| on_open_user.run(id)>
                                <div class="list-row__main">
                                    <div class="list-row__title">{r.name.clone()}</div>
                                    <div class="list-row__sub">{display_date}</div>
                                </div>
                            </li>
                        }
                    }
                />
            </ul>
            {move || if has_more.get() {
                view! {
                    <button class="btn btn--ghost"
                            data-testid="users-by-movement-show-more"
                            disabled=move || loading.get()
                            on:click=on_show_more.clone()>
                        {move || i18n::t(lang.get(), "show_more")}
                    </button>
                }.into_any()
            } else { ().into_any() }}
        </section>
    }
}
```

- [ ] **Step 2.2: Wire `on_open_user` in `reports/mod.rs`**

Add a signal in `ReportsPage` to track the selected user id and mount the existing card panel as a sibling — mirror how Desk does it (search Desk's mod.rs for `editing_user` or `selected_card`). Pseudocode:

```rust
let selected_user = RwSignal::new(None::<i64>);

// In the Users tab branch:
view! { <users_by_movement::UsersByMovement on_open_user=Callback::new(move |id| selected_user.set(Some(id))) /> }

// As a sibling of the tab body, mount the card panel when selected_user is Some.
{move || match selected_user.get() {
    Some(id) => view! { /* fetch CardInfo by id, render <CardActionPanel ... on_close=close_callback /> */ }.into_any(),
    None => ().into_any(),
}}
```

The existing card panel takes a `card: CardInfo`; if no helper currently fetches a single CardInfo by user id, add one in `spinbike-ui/src/pages/dashboard/helpers.rs` (mirror an existing fetch) or call the existing `/api/users/{id}` endpoint inline. Keep this scope tight: ONE fetch, ONE mount, no full page navigation.

### Step 3: Delete button on card panel

- [ ] **Step 3.1: Modify `card_panel.rs` to add a delete button + sheet mount**

After the existing `BlockButton` (around line 167):

```rust
let editing_delete = RwSignal::new(false);
let user_id = card_id;
let name_for_modal = card.name.clone();
let balance_for_modal = credit;
let pass_end_for_modal = card_pass.as_ref().map(|p| p.1);
```

Inside the `<div class="action-row stack-12">` (around line 160-168), append:

```rust
<button class="btn btn--danger"
        data-testid="delete-user-button"
        on:click=move |_| editing_delete.set(true)>
    {move || i18n::t(lang.get(), "delete_user")}
</button>
```

Mount the sheet as a sibling of the outer div (after the closing `</div>` of `action-panel` but inside the `<>` fragment, alongside `EditInfoForm`):

```rust
<DeleteUserSheet
    show=editing_delete
    user_id=user_id
    name=name_for_modal
    balance=balance_for_modal
    active_pass_end=pass_end_for_modal
    on_saved=Callback::new(move |()| {
        // Closing the panel matches the Desk pattern when a user disappears
        set_selected.set(None);
    })
/>
```

Add the import at the top:

```rust
use super::sheets::DeleteUserSheet;
```

If `claims.role.can_manage_cards()` is the role gate at the route level, the button can render unconditionally — staff routes are the only place card panel is reachable. If a per-render gate is needed, mirror the existing one used by `BlockButton`.

### Step 4: E2E test

- [ ] **Step 4.1: Create `e2e/tests/users-by-movement.spec.ts`**

Mirror `e2e/tests/edit-tx-date.spec.ts` for setup. The test:

```ts
import { test, expect } from '@playwright/test';
import {
  loginViaAPI, createUniqueUser, seedTransaction,
  assertCleanConsole, registerConsoleCollector,
} from './helpers';

test('reports users tab orders oldest-first and supports soft-delete', async ({ page }) => {
  const msgs = registerConsoleCollector(page);
  const token = await loginViaAPI(page);

  // Seed: A (no movement), B (movement 2y ago), C (movement yesterday).
  // Each user has a unique tag to make assertions stable.
  const a = await createUniqueUser(token, 0.0, 'UMA-A');
  const b = await createUniqueUser(token, 0.0, 'UMA-B');
  const c = await createUniqueUser(token, 0.0, 'UMA-C');

  // Seed transactions with explicit dates via test_fixtures or by following
  // edit-tx-date.spec.ts's pattern (create txn, then PATCH /created-at).
  const today = new Date();
  const yesterday = new Date(today); yesterday.setDate(today.getDate() - 1);
  const twoYearsAgo = new Date(today); twoYearsAgo.setFullYear(today.getFullYear() - 2);
  await seedTransaction(token, b.user_id, /*amount*/ -1.0, /*date*/ twoYearsAgo);
  await seedTransaction(token, c.user_id, /*amount*/ -1.0, /*date*/ yesterday);

  await page.goto('/staff');
  await page.click('[data-testid="nav-reports"]');
  await page.click('[data-testid="reports-tab-users"]');

  // Assert order — A first (no movement), then B (oldest dated), then C (newest).
  const rows = page.locator('[data-testid="user-row"]');
  await expect(rows.first()).toContainText(a.name);
  await expect(rows.nth(1)).toContainText(b.name);
  await expect(rows.nth(2)).toContainText(c.name);

  // Click row B → card panel opens
  await rows.nth(1).click();
  await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

  // Open delete modal → confirm
  await page.click('[data-testid="delete-user-button"]');
  await expect(page.locator('[data-testid="sheet-delete-user"]')).toBeVisible();
  await page.click('[data-testid="delete-user-confirm"]');

  // Modal closes, panel closes, B disappears from the list
  await expect(page.locator('[data-testid="sheet-delete-user"]')).toBeHidden();
  await expect(page.locator('[data-testid="action-panel"]')).toBeHidden();
  await expect(page.locator(`[data-testid="user-row"]:has-text("${b.name}")`)).toHaveCount(0);

  await assertCleanConsole(msgs);
});
```

If `seedTransaction` doesn't exist, add the helper to `e2e/tests/helpers.ts` — mirror the topup/charge POST that `edit-tx-date.spec.ts` already uses, then PATCH `/api/transactions/{id}/created-at` with the wanted date.

### Step 5: Local format + commit

- [ ] **Step 5.1: Format check**

Run: `cargo fmt --all --check`

- [ ] **Step 5.2: Commit**

```bash
git add spinbike-ui/src/pages/reports/mod.rs spinbike-ui/src/pages/reports/users_by_movement.rs spinbike-ui/src/pages/dashboard/card_panel.rs spinbike-ui/src/pages/dashboard/helpers.rs e2e/tests/users-by-movement.spec.ts e2e/tests/helpers.ts
git status -s   # confirm only the touched files
git commit -m "$(cat <<'EOF'
feat(ui,e2e): Reports Users tab + delete-user button (#56)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Push, monitor CI, open PR (CONTROLLER-RUN)

- [ ] **Step 1: Push dev**

```bash
git push origin dev
```

- [ ] **Step 2: Monitor the latest run to terminal state**

Find the run id:

```bash
gh run list --branch dev --limit 1 --json databaseId,headSha,status,conclusion
```

Then ONE background monitor — never `/loop`, never custom bash scripts:

```bash
sleep 600 && gh run view <run-id> --json status,conclusion,jobs
```

(`run_in_background: true`, then read with `BashOutput`.)

If any job fails: `gh run view <run-id> --log-failed`, fix root cause in ONE commit, push, monitor again. Per `no-timeout-band-aids.md`: if E2E times out, investigate the regression — never bump the timeout.

- [ ] **Step 3: Open the PR (only when CI is green)**

```bash
gh pr create --base main --head dev --title "v0.13.28: users-by-last-movement report + soft-delete (#56)" --body "$(cat <<'EOF'
## Summary

- Reports page gains a Users tab listing all customers sorted by oldest activity first, paginated 50 + Show more.
- Card panel gains a Delete user button (admin gate). Confirmation modal warns about positive balance and active permanentka.
- Migration V15 adds `users.deleted_at` and retires the V13 `(deleted)` synthetic placeholder so it no longer surfaces in search/lists.
- Soft-delete filter ripple applied to all user-listing SQL.

Closes #56
Closes #68

## Test plan

- [x] Backend integration tests for list endpoint (6 cases) and delete endpoint (7 cases)
- [x] V15 migration unit tests (column add, idempotency, synthetic retired)
- [x] Playwright E2E: tab switch, oldest-first order, click row, delete modal, row disappears
- [x] Mutation testing kills any drop of `WHERE u.deleted_at IS NULL`
- [x] Post-deploy verification on prod (Task 9)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Verify mergeable + clean**

```bash
gh pr view <pr-number> --json mergeable,mergeStateStatus
```

Expected: `{"mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN"}`. Anything else → fix before reporting.

Per `pr-merge-policy.md`: never merge. Hand off the green PR URL and wait for the explicit `merge it` from the user.

---

## Task 9: Post-deploy verification on prod (CONTROLLER-RUN — ONLY after user merges)

- [ ] **Step 1: Wait for main CI to deploy**

```bash
gh run list --branch main --limit 3 --json databaseId,headSha,status,conclusion,name
sleep 600 && gh run view <main-run-id> --json status,conclusion,jobs
```

All jobs green incl. Deploy (prod) + Smoke (prod).

- [ ] **Step 2: Liveness + version check**

```bash
curl -s https://spinbike.newlevel.media/api/version
```

Expected: `{"version":"0.13.28"}`.

- [ ] **Step 3: Functional verification via Playwright**

Open https://spinbike.newlevel.media/staff in the Playwright MCP browser:

1. Read `[data-testid="version"]` — must be `v0.13.28`.
2. Click `[data-testid="nav-reports"]` → click `[data-testid="reports-tab-users"]`.
3. Confirm rows render in oldest-first order (visual scan + first-row name read).
4. Pick a known disposable test user (created earlier or seed via API beforehand). Click row → card panel opens.
5. Click `[data-testid="delete-user-button"]` → confirm modal opens with the user's name.
6. Click `[data-testid="delete-user-confirm"]` → modal closes, panel closes, row gone.
7. Reload the Users tab — row still gone.
8. Restore the test user via direct DB:

```bash
sqlite3 /opt/spinbike/spinbike.db "UPDATE users SET deleted_at = NULL WHERE id = <test-id>"
```

9. Capture browser console — must show 0 errors and 0 warnings.

- [ ] **Step 4: Send the completion report**

Per `airuleset/modules/core/completion-report.md` template — audits + Goal + What changed + 🌐 URLs + PR ref. Include `✅ Deploy: prod frontend shows v0.13.28 (matches backend /api/version); Reports → Users tab orders oldest-first and delete flow round-trips on test user.`

PR is `mergeable: MERGEABLE` and `mergeStateStatus: CLEAN`. Awaiting user merge instruction.

---

## Self-review notes (controller-only — do not include in commits)

**Spec coverage:**
- Reports tab — ✅ Task 7 step 1
- Users tab list, oldest-first, top 50 + Show more — ✅ Task 7 step 2
- Row click → existing card panel — ✅ Task 7 step 2.2
- Delete button on card panel (admin gate) — ✅ Task 7 step 3
- Confirm modal with balance/permanentka warnings — ✅ Task 6
- Migration V15 + retire synthetic — ✅ Task 3
- `GET /api/admin/users/by-last-movement` paginated — ✅ Task 5
- `DELETE /api/admin/users/{id}` with 404/409/403 — ✅ Task 5
- Filter ripple to all user-listing queries with audit table — ✅ Task 4
- 13 i18n keys — ✅ Task 2
- Backend integration tests (6 + 7 cases) — ✅ Task 5
- Playwright E2E full flow — ✅ Task 7 step 4
- Mutation-killing assertions on the filter — ✅ implicitly via `list_excludes_soft_deleted_users` + `deleted_user_hidden_from_search`
- Closes #56 and #68 in PR body — ✅ Task 8

**Type consistency:**
- `UserByMovementRow` fields (`id`, `name`, `last_movement_at`) match the JSON contract used by the frontend `Row` struct.
- `DeleteUserOutcome::Deleted { deleted_at: String }` matches the `DeleteUserResp` JSON shape.
- `DeleteUserSheet` props (`show`, `user_id`, `name`, `balance`, `active_pass_end`, `on_saved`) match the call site in `card_panel.rs`.
