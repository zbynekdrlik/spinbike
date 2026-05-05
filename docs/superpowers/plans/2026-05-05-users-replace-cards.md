# Users Replace Cards — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop the `cards` table; make `users` the canonical customer entity. Replace the desk "Activate New Card" flow with "Add Person". Migrate all legacy data in a single PR with full E2E coverage.

**Architecture:** One sqlx incremental migration recreates `users` (email nullable + new columns: credit, card_code, blocked, allow_debit, company, search_text), recreates `transactions` and `bookings` without `card_id`, drops `cards`. All backend modules (`db/cards.rs`, `routes/cards.rs`, parts of `routes/payments.rs`, `routes/transactions.rs`, `routes/persistent_bookings.rs`, `routes/test_fixtures.rs`) re-keyed to `user_id`. Frontend dashboard replaces Activate-Card form with Add-Person form; all action endpoints rewired; `link_card.rs` page and `/link-card` route deleted; i18n keys swapped. E2E specs and helpers updated to seed via user-create endpoint.

**Tech Stack:** Rust (Axum 0.8, sqlx, SQLite), Leptos 0.7 CSR/WASM, Trunk, Playwright E2E.

**Spec:** `docs/superpowers/specs/2026-05-05-users-replace-cards-design.md` (committed at `ecfd3de`).

**Issue:** [#55](https://github.com/zbynekdrlik/spinbike/issues/55).

---

## Conventions used by every task

- Branch: `dev`. Never push to `main`. Open PR `dev` → `main` after CI green; never merge.
- `git add` always uses explicit paths or `git add -u`. **NEVER `git add -A` or `git add .`**.
- **NO local cargo build / test / clippy / trunk build.** Only `cargo fmt --all --check` is allowed locally. CI is authoritative.
- Each task ends with a single commit using a Conventional Commit-style message ending with the trailer `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>`.
- Slovak strings are UNACCENTED (project convention).
- Visit-row definition in any `last_visit_at` query: `service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))` AND `deleted_at IS NULL`. Never `action='visit'` alone.

---

## Task 1: Bump VERSION 0.13.21 → 0.13.22 (CONTROLLER, not subagent)

**Files:**
- Modify: `VERSION`
- Modify: `Cargo.toml` (auto by sync-version.sh)
- Modify: `spinbike-ui/Cargo.toml` (auto by sync-version.sh)

- [ ] **Step 1: Edit VERSION**

```bash
echo 0.13.22 > VERSION
```

- [ ] **Step 2: Sync to Cargo.toml files**

```bash
bash scripts/sync-version.sh
```

Expected output: `Syncing version: 0.13.22 ... Done. All version fields set to 0.13.22`.

- [ ] **Step 3: Commit**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "$(cat <<'EOF'
chore(version): bump to 0.13.22 for #55 users-replace-cards

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Migration step in db/migrations.rs

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs` (append migration entry + add unit test)

**Why:** Single sqlx migration step #13 implementing the spec's "Migration sequence". Recreates `users`, `transactions`, `bookings`; drops `cards`. Idempotent (runner schema_version gate prevents re-run on production).

### Background facts the implementer must rely on

- `MIGRATIONS` is `&[(i64, &str, &str)]`. Highest current version is **12**. Add **13**.
- Per-migration foreign-keys handling: V8 toggles `PRAGMA foreign_keys=OFF`/ON manually inside its SQL block. Migration #13 must do the same.
- Existing schema state going into migration 13 (composed across V1..V12):
  - `users(id, email NOT NULL UNIQUE, password_hash, name NOT NULL, phone, role NOT NULL DEFAULT 'customer', oauth_provider, oauth_id, created_at)`
  - `cards(id, barcode NOT NULL UNIQUE, user_id, blocked, credit, allow_debit, created_at, first_name, last_name, company, phone, search_text)` plus index `idx_cards_search_text`
  - `transactions(id, user_id, card_id, staff_id, service_id, amount, action, created_at, valid_until, deleted_at, note CHECK length<=200)` — recreated by V11 with the CHECK
  - `bookings(id, template_id NOT NULL, date NOT NULL, user_id NOT NULL, created_by, created_at, cancelled_at, card_id, source, charged_at, charge_transaction_id)` plus unique index `idx_bookings_active`

### Implementation

- [ ] **Step 1: Open the file**

```bash
$EDITOR crates/spinbike-server/src/db/migrations.rs
```

- [ ] **Step 2: Append migration entry to MIGRATIONS array**

Add this entry at the end of the `MIGRATIONS` slice (before the closing `];`):

```rust
    (13, "users replace cards", V13_USERS_REPLACE_CARDS),
```

- [ ] **Step 3: Add the `V13_USERS_REPLACE_CARDS` const at end of file**

```rust
const V13_USERS_REPLACE_CARDS: &str = r#"
PRAGMA foreign_keys = OFF;

-- 1. Recreate users with email nullable + new columns
CREATE TABLE users_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    email           TEXT    UNIQUE,
    name            TEXT    NOT NULL DEFAULT '(no name)',
    password_hash   TEXT,
    phone           TEXT,
    company         TEXT,
    role            TEXT    NOT NULL DEFAULT 'customer',
    oauth_provider  TEXT,
    oauth_id        TEXT,
    credit          REAL    NOT NULL DEFAULT 0.0,
    card_code       TEXT,
    blocked         INTEGER NOT NULL DEFAULT 0,
    allow_debit     INTEGER NOT NULL DEFAULT 0,
    search_text     TEXT,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO users_new (id, email, name, password_hash, phone, role,
                       oauth_provider, oauth_id, created_at)
SELECT id, email, COALESCE(NULLIF(TRIM(name),''), '(no name)'),
       password_hash, phone, role, oauth_provider, oauth_id, created_at
  FROM users;

DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE UNIQUE INDEX idx_users_card_code ON users(card_code) WHERE card_code IS NOT NULL;
CREATE INDEX        idx_users_search_text ON users(search_text);

-- 2. Promote linked cards into existing users rows
UPDATE users SET
    credit      = (SELECT credit      FROM cards WHERE cards.user_id = users.id),
    card_code   = (SELECT barcode     FROM cards WHERE cards.user_id = users.id),
    blocked     = (SELECT blocked     FROM cards WHERE cards.user_id = users.id),
    allow_debit = (SELECT allow_debit FROM cards WHERE cards.user_id = users.id),
    company     = (SELECT company     FROM cards WHERE cards.user_id = users.id),
    search_text = (SELECT search_text FROM cards WHERE cards.user_id = users.id),
    name        = COALESCE(
                    NULLIF(TRIM((SELECT TRIM(COALESCE(first_name,'') || ' ' || COALESCE(last_name,''))
                                  FROM cards WHERE cards.user_id = users.id)), ''),
                    users.name),
    phone       = COALESCE(users.phone,
                           (SELECT phone FROM cards WHERE cards.user_id = users.id))
 WHERE EXISTS (SELECT 1 FROM cards WHERE cards.user_id = users.id);

-- 3. Insert one users row per unlinked legacy card (email=NULL, name placeholder if blank)
INSERT INTO users (email, name, phone, role, credit, card_code,
                   blocked, allow_debit, company, search_text, created_at)
SELECT
    NULL,
    COALESCE(NULLIF(TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')), ''),
             '(no name)'),
    c.phone, 'customer', c.credit, c.barcode, c.blocked,
    c.allow_debit, c.company, c.search_text, c.created_at
FROM cards c WHERE c.user_id IS NULL;

-- 4. Backfill cards.user_id for the freshly-created users (so step 5 maps cleanly)
UPDATE cards SET user_id = (SELECT id FROM users WHERE users.card_code = cards.barcode)
 WHERE user_id IS NULL;

-- 5. Backfill transactions.user_id where missing
UPDATE transactions
   SET user_id = (SELECT user_id FROM cards WHERE cards.id = transactions.card_id)
 WHERE user_id IS NULL AND card_id IS NOT NULL;

-- 6. Recreate transactions without card_id (and user_id NOT NULL)
CREATE TABLE transactions_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    staff_id    INTEGER REFERENCES users(id),
    service_id  INTEGER REFERENCES services(id),
    amount      REAL    NOT NULL,
    action      TEXT    NOT NULL,
    valid_until TEXT,
    deleted_at  TEXT,
    note        TEXT CHECK (note IS NULL OR length(note) <= 200),
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO transactions_new
       (id, user_id, staff_id, service_id, amount, action,
        valid_until, deleted_at, note, created_at)
SELECT id, user_id, staff_id, service_id, amount, action,
       valid_until, deleted_at, note, created_at
  FROM transactions;

DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;

-- 7. Recreate bookings without card_id (preserve all other columns)
CREATE TABLE bookings_new (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id           INTEGER NOT NULL REFERENCES class_templates(id),
    date                  TEXT    NOT NULL,
    user_id               INTEGER NOT NULL REFERENCES users(id),
    created_by            INTEGER REFERENCES users(id),
    source                TEXT,
    charged_at            TEXT,
    charge_transaction_id INTEGER REFERENCES transactions(id),
    created_at            TEXT    NOT NULL DEFAULT (datetime('now')),
    cancelled_at          TEXT
);

INSERT INTO bookings_new
       (id, template_id, date, user_id, created_by, source,
        charged_at, charge_transaction_id, created_at, cancelled_at)
SELECT id, template_id, date, user_id, created_by, source,
       charged_at, charge_transaction_id, created_at, cancelled_at
  FROM bookings;

DROP TABLE bookings;
ALTER TABLE bookings_new RENAME TO bookings;

CREATE UNIQUE INDEX idx_bookings_active
    ON bookings(template_id, date, user_id)
    WHERE cancelled_at IS NULL;

-- 8. Drop cards
DROP TABLE cards;

PRAGMA foreign_keys = ON;
"#;
```

- [ ] **Step 4: Add a unit test in the same file**

Inside the existing `#[cfg(test)] mod tests { ... }` block (find it; if absent, add at end of file), append:

```rust
    #[tokio::test]
    async fn migration_13_users_replace_cards_full_round_trip() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Apply migrations 1..=12 only (simulate pre-#13 state)
        sqlx::query("CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')))")
            .execute(&pool).await.unwrap();
        for &(v, _desc, sql) in MIGRATIONS.iter().take_while(|(v, _, _)| *v <= 12) {
            sqlx::query(sql).execute(&pool).await.unwrap_or_else(|e| panic!("v{v}: {e}"));
            sqlx::query("INSERT INTO schema_version(version) VALUES (?)")
                .bind(v).execute(&pool).await.unwrap();
        }

        // Seed: 1 user already linked + 1 unlinked named card + 1 unlinked nameless card
        let staff_id: i64 = sqlx::query_scalar(
            "INSERT INTO users(email,name,role) VALUES('staff@x','Staff','staff') RETURNING id"
        ).fetch_one(&pool).await.unwrap();

        let alice_user: i64 = sqlx::query_scalar(
            "INSERT INTO users(email,name,role) VALUES('alice@x','Alice Old','customer') RETURNING id"
        ).fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO cards(barcode,user_id,blocked,credit,allow_debit,
                              first_name,last_name,company,phone,search_text)
             VALUES('CODE1', ?, 0, 12.50, 0, 'Alice', 'Linked', 'Acme', '111', 'alice linked acme')"
        ).bind(alice_user).execute(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO cards(barcode,user_id,blocked,credit,allow_debit,
                              first_name,last_name,company,phone,search_text)
             VALUES('CODE2', NULL, 0, -3.00, 1, 'Bob', 'Lonely', NULL, '222', 'bob lonely')"
        ).execute(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO cards(barcode,user_id,blocked,credit,allow_debit,
                              first_name,last_name,company,phone,search_text)
             VALUES('CODE3', NULL, 1, 0.0, 0, NULL, NULL, NULL, NULL, NULL)"
        ).execute(&pool).await.unwrap();

        // Insert a transaction tied to bob's card
        let bob_card_id: i64 = sqlx::query_scalar(
            "SELECT id FROM cards WHERE barcode='CODE2'"
        ).fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO transactions(card_id, staff_id, amount, action)
             VALUES(?, ?, -1.50, 'charge')"
        ).bind(bob_card_id).bind(staff_id).execute(&pool).await.unwrap();

        // Apply V13
        let v13_sql = MIGRATIONS.iter().find(|(v,_,_)| *v == 13).unwrap().2;
        sqlx::query(v13_sql).execute(&pool).await
            .unwrap_or_else(|e| panic!("V13: {e}"));

        // Assertions
        let users_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(users_total, 4, "staff + alice + bob + nameless");

        let alice_credit: f64 = sqlx::query_scalar(
            "SELECT credit FROM users WHERE email='alice@x'"
        ).fetch_one(&pool).await.unwrap();
        assert!((alice_credit - 12.50).abs() < 1e-9);

        let alice_card: String = sqlx::query_scalar(
            "SELECT card_code FROM users WHERE email='alice@x'"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(alice_card, "CODE1");

        let bob: (Option<String>, f64, i64, String) = {
            let row: (Option<String>, f64, i64, String) = sqlx::query_as(
                "SELECT email, credit, blocked, name FROM users WHERE card_code='CODE2'"
            ).fetch_one(&pool).await.unwrap();
            row
        };
        assert!(bob.0.is_none(), "bob has no email");
        assert!((bob.1 - (-3.00)).abs() < 1e-9);
        assert_eq!(bob.2, 0);
        assert_eq!(bob.3, "Bob Lonely");

        let nameless_name: String = sqlx::query_scalar(
            "SELECT name FROM users WHERE card_code='CODE3'"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(nameless_name, "(no name)");

        // Bob's transaction now references bob's user_id (not card)
        let bob_user: i64 = sqlx::query_scalar(
            "SELECT id FROM users WHERE card_code='CODE2'"
        ).fetch_one(&pool).await.unwrap();
        let txn_user: i64 = sqlx::query_scalar(
            "SELECT user_id FROM transactions LIMIT 1"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(txn_user, bob_user);

        // cards table is gone
        let cards_table: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='cards'"
        ).fetch_optional(&pool).await.unwrap();
        assert!(cards_table.is_none(), "cards table must be dropped");

        // transactions has no card_id column
        let cols: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_info('transactions')"
        ).fetch_all(&pool).await.unwrap();
        assert!(!cols.contains(&"card_id".to_string()), "transactions.card_id must be dropped");

        // bookings has no card_id column
        let bcols: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_info('bookings')"
        ).fetch_all(&pool).await.unwrap();
        assert!(!bcols.contains(&"card_id".to_string()), "bookings.card_id must be dropped");

        // PRAGMA integrity_check
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(integrity, "ok");
    }
```

- [ ] **Step 5: Local format check**

```bash
cargo fmt --all --check
```

If it fails: `cargo fmt --all` then retry.

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "$(cat <<'EOF'
feat(db): migration #13 — users replace cards (#55)

Recreate users with email nullable + credit/card_code/blocked/allow_debit/company/search_text.
Promote linked cards into users; insert legacy unlinked cards as users with NULL email and
'(no name)' placeholder where first+last are blank. Backfill transactions.user_id; drop
transactions.card_id and bookings.card_id; drop cards table.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: db/users.rs helpers + delete db/cards.rs

**Files:**
- Modify: `crates/spinbike-server/src/db/users.rs` (extend with all helpers ported from cards.rs)
- Delete: `crates/spinbike-server/src/db/cards.rs`
- Modify: `crates/spinbike-server/src/db/mod.rs` (drop `pub mod cards;`)

### Helpers to add to db/users.rs

Use these exact signatures (all `pub async fn` returning `sqlx::Result<...>` unless noted). Function bodies must mirror cards.rs counterparts but query `users` table on `id` and `card_code` (not `cards` on `id` and `barcode`).

- [ ] **Step 1: Extend `UserRow` struct**

Replace the existing struct definition with:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: Option<String>,
    pub name: String,
    pub password_hash: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub role: String,
    pub oauth_provider: Option<String>,
    pub oauth_id: Option<String>,
    pub credit: f64,
    pub card_code: Option<String>,
    pub blocked: bool,
    pub allow_debit: bool,
    pub search_text: Option<String>,
    pub created_at: String,
}
```

- [ ] **Step 2: Update `create_user` to accept the new optional fields**

Update signature and body to:

```rust
#[allow(clippy::too_many_arguments)]
pub async fn create_user(
    pool: &SqlitePool,
    email: Option<&str>,
    password_hash: Option<&str>,
    name: &str,
    phone: Option<&str>,
    company: Option<&str>,
    card_code: Option<&str>,
    role: &str,
    initial_credit: Option<f64>,
    oauth_provider: Option<&str>,
    oauth_id: Option<&str>,
) -> sqlx::Result<i64> {
    let search_text = compute_search_text(Some(name), None, company, card_code);
    let credit = initial_credit.unwrap_or(0.0);
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (email, password_hash, name, phone, company,
                            card_code, role, credit, oauth_provider, oauth_id, search_text)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id"
    )
    .bind(email)
    .bind(password_hash)
    .bind(name)
    .bind(phone)
    .bind(company)
    .bind(card_code)
    .bind(role)
    .bind(credit)
    .bind(oauth_provider)
    .bind(oauth_id)
    .bind(&search_text)
    .fetch_one(pool)
    .await?;
    Ok(id)
}
```

- [ ] **Step 3: Add helpers (port from cards.rs, all keyed by user_id)**

Add these functions to db/users.rs. Use the cards.rs equivalents as reference but rename `barcode` → `card_code`, `cards.id` → `users.id`, drop `first_name/last_name` arguments (use single `name`):

```rust
pub async fn search_users(pool: &SqlitePool, query: &str, limit: i64)
    -> sqlx::Result<Vec<UserRow>>;

pub async fn search_users_with_pass(pool: &SqlitePool, query: &str, limit: i64)
    -> sqlx::Result<Vec<(UserRow, Option<(i64, chrono::NaiveDate)>, Option<String>)>>;

pub async fn get_user_by_card_code(pool: &SqlitePool, code: &str)
    -> sqlx::Result<Option<UserRow>>;

pub async fn list_all_users_with_pass(pool: &SqlitePool)
    -> sqlx::Result<Vec<(UserRow, Option<(i64, chrono::NaiveDate)>, Option<String>)>>;

pub async fn update_credit(pool: &SqlitePool, user_id: i64, delta: f64) -> sqlx::Result<()>;

pub async fn set_blocked(pool: &SqlitePool, user_id: i64, blocked: bool) -> sqlx::Result<()>;

pub async fn set_allow_debit(pool: &SqlitePool, user_id: i64, allow: bool) -> sqlx::Result<()>;

pub async fn get_user_pass_valid_until(pool: &SqlitePool, user_id: i64)
    -> sqlx::Result<Option<chrono::NaiveDate>>;

pub async fn get_user_pass_tx(pool: &SqlitePool, user_id: i64)
    -> sqlx::Result<Option<(i64, chrono::NaiveDate)>>;

pub async fn list_negative_balance(pool: &SqlitePool)
    -> sqlx::Result<Vec<NegativeBalanceUserRow>>;

pub async fn update_user_info(
    pool: &SqlitePool,
    user_id: i64,
    name: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
    company: Option<&str>,
    card_code: Option<&str>,
) -> sqlx::Result<()>;

pub fn round_cents(value: f64) -> f64 { (value * 100.0).round() / 100.0 }

pub async fn backfill_search_text(pool: &SqlitePool) -> sqlx::Result<usize>;

pub fn compute_search_text(
    name: Option<&str>,
    _unused: Option<&str>,
    company: Option<&str>,
    card_code: Option<&str>,
) -> String {
    let parts = [name, company, card_code];
    let joined: String = parts.iter().filter_map(|p| *p).collect::<Vec<_>>().join(" ");
    normalize_search(&joined)
}

pub fn normalize_search(s: &str) -> String { /* port from cards.rs */ }
```

- [ ] **Step 4: Add `NegativeBalanceUserRow` struct**

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NegativeBalanceUserRow {
    pub id: i64,
    pub card_code: Option<String>,
    pub credit: f64,
    pub blocked: bool,
    pub name: String,
    pub email: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub last_payment_at: Option<String>,
    pub pass_valid_until: Option<String>,
    pub pass_tx_id: Option<i64>,
}
```

- [ ] **Step 5: list_negative_balance SQL — use the visit definition correctly**

```rust
pub async fn list_negative_balance(pool: &SqlitePool) -> sqlx::Result<Vec<NegativeBalanceUserRow>> {
    sqlx::query_as::<_, NegativeBalanceUserRow>(
        "SELECT
            u.id, u.card_code, u.credit, u.blocked, u.name, u.email, u.company,
            (SELECT MAX(t.created_at) FROM transactions t
                WHERE t.user_id = u.id
                  AND t.deleted_at IS NULL
                  AND t.service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))
            ) AS last_visit_at,
            (SELECT MAX(t.created_at) FROM transactions t
                WHERE t.user_id = u.id
                  AND t.action = 'topup'
                  AND t.amount > 0
                  AND t.deleted_at IS NULL
            ) AS last_payment_at,
            (SELECT MAX(valid_until) FROM transactions
                WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
                WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
                ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id
         FROM users u
         WHERE u.credit < 0
         ORDER BY u.credit ASC"
    )
    .fetch_all(pool)
    .await
}
```

- [ ] **Step 6: Delete db/cards.rs**

```bash
git rm crates/spinbike-server/src/db/cards.rs
```

- [ ] **Step 7: Drop the `cards` module from `db/mod.rs`**

In `crates/spinbike-server/src/db/mod.rs`, remove the line `pub mod cards;`.

- [ ] **Step 8: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 9: Commit**

```bash
git add crates/spinbike-server/src/db/users.rs crates/spinbike-server/src/db/mod.rs
git add -u crates/spinbike-server/src/db/cards.rs
git commit -m "$(cat <<'EOF'
refactor(db): port cards helpers to users; delete cards module (#55)

UserRow extended with credit/card_code/blocked/allow_debit/company/search_text.
Helpers ported: search, search_with_pass, get_by_card_code, list_negative_balance,
update_credit/blocked/allow_debit, pass_valid_until/tx, update_user_info, search-text helpers.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: routes/users.rs (replaces routes/cards.rs)

**Files:**
- Create: `crates/spinbike-server/src/routes/users.rs`
- Delete: `crates/spinbike-server/src/routes/cards.rs`
- Modify: `crates/spinbike-server/src/routes/mod.rs`

### Endpoints to provide (all Staff-gated except `/api/my/balance` which is owner-or-staff)

| Method | Path | Replaces |
|---|---|---|
| GET    | `/api/users` (Staff)             | GET /api/cards |
| GET    | `/api/users/search` (Staff)      | GET /api/cards/search |
| POST   | `/api/users` (Staff)             | POST /api/cards/activate |
| GET    | `/api/users/lookup/{code}` (Staff) | GET /api/cards/lookup/{barcode} |
| POST   | `/api/users/topup` (Staff)       | POST /api/cards/topup |
| POST   | `/api/users/block` (Staff)       | POST /api/cards/block |
| GET    | `/api/users/negative-balance` (Staff) | GET /api/cards/negative-balance |
| PUT    | `/api/users/{id}` (Staff)        | PUT /api/cards/{id} |
| GET    | `/api/users/{id}/transactions` (Staff or owner) | GET /api/cards/{id}/transactions |
| GET    | `/api/users/{id}/stats` (Staff)  | GET /api/cards/{id}/stats |
| GET    | `/api/my/balance` (any auth user) | unchanged path |

- [ ] **Step 1: Create routes/users.rs**

Open `crates/spinbike-server/src/routes/cards.rs` as a reference. Build `routes/users.rs` mirroring its handler bodies, with these substitutions:
  - struct field `card_id: i64` → `user_id: i64`
  - struct field `barcode: String` → `card_code: Option<String>` for create-flow; `code: String` for lookup
  - response struct `CardResponse` → `UserResponse` with fields `id, email, name, phone, company, card_code, credit, blocked, allow_debit, role, last_visit_at, pass`
  - response struct `NegativeBalanceCardResponse` → `NegativeBalanceUserResponse` matching `NegativeBalanceUserRow` shape with `pass: Option<CardPass>`
  - `db::cards::*` calls → `db::users::*` equivalents

Full file outline (the implementer fills handler bodies exactly per cards.rs):

```rust
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use crate::AppState;
use crate::auth::{AuthUser, parse_role};
use crate::db::users;
use crate::routes::internal_error;
use spinbike_core::auth::Role;

#[derive(Serialize, Clone)]
pub struct UserResponse {
    pub id: i64,
    pub email: Option<String>,
    pub name: String,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub card_code: Option<String>,
    pub credit: f64,
    pub blocked: bool,
    pub allow_debit: bool,
    pub role: String,
    pub last_visit_at: Option<String>,
    pub pass: Option<CardPass>,
}

#[derive(Serialize, Clone)]
pub struct CardPass {
    pub transaction_id: i64,
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
    pub label: String,
}

#[derive(Serialize, Clone)]
pub struct NegativeBalanceUserResponse {
    pub id: i64,
    pub card_code: Option<String>,
    pub credit: f64,
    pub blocked: bool,
    pub name: String,
    pub email: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub last_payment_at: Option<String>,
    pub pass: Option<CardPass>,
}

#[derive(Deserialize)]
pub struct SearchQuery { pub q: Option<String>, pub limit: Option<i64> }

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub card_code: Option<String>,
    pub initial_credit: Option<f64>,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub card_code: Option<String>,
}

#[derive(Deserialize)]
pub struct TopupRequest {
    pub user_id: i64,
    pub amount: f64,
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct BlockRequest {
    pub user_id: i64,
    pub blocked: bool,
}

#[derive(Serialize)]
pub struct BalanceResponse {
    pub user_id: i64,
    pub credit: f64,
    pub card_code: Option<String>,
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub topups_total: f64,
    pub charges_total: f64,
    pub visits_total: i64,
    pub last_visit_at: Option<String>,
}

#[derive(Serialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub action: String,
    pub amount: f64,
    pub service_name: Option<String>,
    pub note: Option<String>,
    pub valid_until: Option<chrono::NaiveDate>,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/users",                    get(list_users).post(create_user))
        .route("/api/users/search",             get(search_users))
        .route("/api/users/lookup/{code}",      get(lookup_user))
        .route("/api/users/topup",              post(topup_user))
        .route("/api/users/block",              post(block_user))
        .route("/api/users/negative-balance",   get(negative_balance))
        .route("/api/users/{id}",               put(update_user))
        .route("/api/users/{id}/transactions",  get(user_transactions))
        .route("/api/users/{id}/stats",         get(user_stats))
        .route("/api/my/balance",               get(my_balance))
}

// Handlers below mirror routes/cards.rs counterparts. Substitute every card_id with
// user_id, every cards::* DB call with users::*, every CardResponse with UserResponse.

async fn list_users(...)        { /* port from list_cards */ }
async fn search_users(...)      { /* port from search_cards */ }
async fn create_user(...)       { /* port from activate_card; allow email=None */ }
async fn lookup_user(...)       { /* port from lookup_card */ }
async fn topup_user(...)        { /* port from topup_card */ }
async fn block_user(...)        { /* port from block_card */ }
async fn negative_balance(...)  { /* port from negative_balance */ }
async fn update_user(...)       { /* port from update_card */ }
async fn user_transactions(...) { /* port from card_transactions */ }
async fn user_stats(...)        { /* port from card_stats */ }
async fn my_balance(...)        { /* port from my_balance, key on claims.sub user_id */ }
```

The `create_user` handler MUST:
- Validate `name.trim()` non-empty (400 if empty).
- If `email.is_some()`: validate it contains `@` and `.` (400 if not), check duplicate via `users::get_user_by_email` (409 if exists).
- If `card_code.is_some()`: check duplicate via `users::get_user_by_card_code` (409 if exists).
- Call `db::users::create_user(pool, email, None, name, phone, company, card_code, "customer", initial_credit, None, None)`.
- Return 201 with full UserResponse.

The `topup_user` handler MUST:
- Reject non-positive amounts with 400.
- Refuse to top up blocked accounts with 403.
- Append a `topup` transaction with `user_id`, `amount`, `note`.
- Update `users.credit` with delta = +amount.
- Return UserResponse.

The `update_user` handler MUST:
- Validate email format if present (400 on bad).
- Reject duplicate email/card_code (409).
- Persist via `db::users::update_user_info`.

- [ ] **Step 2: Wire route in `routes/mod.rs`**

In `crates/spinbike-server/src/routes/mod.rs`:
- Remove line `pub mod cards;`
- Add line `pub mod users;`
- Update the `Router::new().merge(...)` chain that registers all submodules: `merge(cards::routes())` → `merge(users::routes())`.

- [ ] **Step 3: Delete routes/cards.rs**

```bash
git rm crates/spinbike-server/src/routes/cards.rs
```

- [ ] **Step 4: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/routes/users.rs crates/spinbike-server/src/routes/mod.rs
git add -u crates/spinbike-server/src/routes/cards.rs
git commit -m "$(cat <<'EOF'
refactor(routes): introduce /api/users; drop /api/cards (#55)

Endpoints ported: list, search, lookup, create (replaces activate-card; email optional),
update, topup, block, negative-balance, transactions, stats. /api/my/balance keyed to
claims.sub user_id.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: routes/payments.rs rewired to user_id

**Files:**
- Modify: `crates/spinbike-server/src/routes/payments.rs`

### Rewrites

- `ChargeRequest.card_id: i64` → `user_id: i64`
- `StornoRequest.card_id` → `user_id`
- `SellPassRequest.card_id` → `user_id`
- `LogVisitRequest.card_id` → `user_id`
- All handler bodies: substitute `db::cards::update_credit(pool, card_id, ...)` with `db::users::update_credit(pool, user_id, ...)`. Substitute card-pass lookup, blocked check, allow_debit check, etc. with users equivalents.
- The "active monthly pass" gate in `log_visit` and `charge`: query `users::get_user_pass_valid_until(pool, user_id)`.
- Insert into `transactions(user_id, staff_id, service_id, amount, action, valid_until, note)` — drop `card_id` from INSERT.

- [ ] **Step 1: Open the file and update each request struct**

Apply rename `card_id` → `user_id` in `ChargeRequest`, `StornoRequest`, `SellPassRequest`, `LogVisitRequest`.

- [ ] **Step 2: Update each handler body to query and update users**

Walk through every reference to:
- `db::cards::update_credit(...)`
- `db::cards::get_card_by_id(...)` (if present) → `db::users::get_user_by_id(...)`
- `db::cards::get_card_pass_valid_until(...)` → `db::users::get_user_pass_valid_until(...)`
- INSERT INTO transactions(... card_id ...): remove the card_id column.

- [ ] **Step 3: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 4: Commit**

```bash
git add crates/spinbike-server/src/routes/payments.rs
git commit -m "$(cat <<'EOF'
refactor(payments): charge/storno/sell-pass/log-visit keyed on user_id (#55)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: routes/transactions.rs + routes/persistent_bookings.rs + routes/upcoming_classes.rs + routes/classes.rs — drop card_id refs

**Files:**
- Modify: `crates/spinbike-server/src/routes/transactions.rs`
- Modify: `crates/spinbike-server/src/routes/persistent_bookings.rs`
- Modify: `crates/spinbike-server/src/routes/upcoming_classes.rs`
- Modify: `crates/spinbike-server/src/routes/classes.rs`

- [ ] **Step 1: For each file, grep for `card_id` and rewrite**

```bash
grep -nE 'card_id|cards\.' crates/spinbike-server/src/routes/transactions.rs \
                                  crates/spinbike-server/src/routes/persistent_bookings.rs \
                                  crates/spinbike-server/src/routes/upcoming_classes.rs \
                                  crates/spinbike-server/src/routes/classes.rs
```

For each match: remove the `card_id` selection/binding/struct field. The `transactions` and `bookings` tables no longer have `card_id`. Replace any join through `cards` with the user_id directly (already on transactions/bookings post-migration).

- [ ] **Step 2: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 3: Commit**

```bash
git add -u crates/spinbike-server/src/routes/transactions.rs \
            crates/spinbike-server/src/routes/persistent_bookings.rs \
            crates/spinbike-server/src/routes/upcoming_classes.rs \
            crates/spinbike-server/src/routes/classes.rs
git commit -m "$(cat <<'EOF'
refactor(routes): drop card_id from transactions/bookings/classes (#55)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: routes/test_fixtures.rs — `seed_user` replaces `seed_credit`

**Files:**
- Modify: `crates/spinbike-server/src/routes/test_fixtures.rs`

- [ ] **Step 1: Replace `seed_credit` route with `seed_user`**

Drop the `/api/test-fixtures/seed-credit` route + handler. Add:

```rust
#[derive(Deserialize)]
struct SeedUserRequest {
    name: String,
    email: Option<String>,
    phone: Option<String>,
    company: Option<String>,
    card_code: Option<String>,
    credit: Option<f64>,
}

async fn seed_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedUserRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "forbidden"}))));
    }
    let user_id = db::users::create_user(
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
    ).await.map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({"user_id": user_id}))))
}
```

Update `seed_transactions` to take `user_id` instead of `card_id` in its `SeedEntry` struct, and INSERT into `transactions` without `card_id`.

- [ ] **Step 2: Update routes() registration**

```rust
.route("/api/test-fixtures/seed-user", post(seed_user))
```

- [ ] **Step 3: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 4: Commit**

```bash
git add crates/spinbike-server/src/routes/test_fixtures.rs
git commit -m "$(cat <<'EOF'
refactor(test-fixtures): seed_user replaces seed_credit; seed_transactions keyed by user_id (#55)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: tests/users_routes.rs (replaces tests/cards_routes.rs)

**Files:**
- Create: `crates/spinbike-server/tests/users_routes.rs`
- Delete: `crates/spinbike-server/tests/cards_routes.rs`

- [ ] **Step 1: Port every test from `cards_routes.rs` to user-keyed semantics**

Each existing test in cards_routes.rs maps 1:1:
- `test_topup_amount_validation` → exercises `/api/users/topup`, asserts 400 on amount<=0.
- `test_list_cards_staff_only` → `test_list_users_staff_only` against `/api/users`, assert 403 for customer.
- `test_search_diacritics` → `/api/users/search`, seed names with accented chars, assert results.
- `test_negative_balance` → `/api/users/negative-balance`, sorted ASC, includes pass+blocked.
- `test_activate_card_duplicate_barcode_conflict` → `test_create_user_duplicate_card_code` via `/api/users` POST.
- `test_create_user_duplicate_email_conflict` (NEW): POST same email twice → 409.
- `test_create_user_email_optional` (NEW): POST with no email → 201, response email is null.
- `test_update_card_info` → `test_update_user_info` via PUT `/api/users/{id}`.
- `test_card_transactions_ledger` → `/api/users/{id}/transactions`.
- `test_block_unblock_toggle` → `/api/users/block`.
- Soft-delete + route registration tests: same shape against new routes.

Each test uses the helper pattern:
```rust
let (server, pool) = setup_app().await;
let token = staff_token(&server).await;
let resp = server.post("/api/users")
    .header("Authorization", format!("Bearer {}", token))
    .json(&serde_json::json!({"name": "Alice", "email": "a@b"}))
    .send().await;
assert_eq!(resp.status_code(), 201);
```

Add tests targeting mutation pressure points:
- `test_create_user_blocked_field_round_trip` — POST then GET, verify `blocked` value flows through.
- `test_negative_balance_excludes_zero` — create user with credit=0, assert NOT in list.
- `test_negative_balance_includes_minus_one_cent` — credit=-0.01, assert IN list.

- [ ] **Step 2: Delete tests/cards_routes.rs**

```bash
git rm crates/spinbike-server/tests/cards_routes.rs
```

- [ ] **Step 3: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 4: Commit**

```bash
git add crates/spinbike-server/tests/users_routes.rs
git add -u crates/spinbike-server/tests/cards_routes.rs
git commit -m "$(cat <<'EOF'
test(routes): port cards_routes.rs to users_routes.rs (#55)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: UI dashboard — Add Person form replaces Activate Card

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs`
- Modify: `spinbike-ui/src/pages/dashboard/activate_card_form.rs` — DELETE if it's a separate file; or rewrite as `add_person_form.rs`.
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs` (action endpoint paths + body shapes)
- Modify: `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` (endpoint + types)
- Modify: any search-results sub-component under `spinbike-ui/src/pages/dashboard/` (endpoint to `/api/users/search`)

- [ ] **Step 1: Replace the `show_activate` signal**

In `spinbike-ui/src/pages/dashboard/mod.rs` around line 169:
```rust
let (show_add_person, set_show_add_person) = signal(false);
```
Search and replace `show_activate` → `show_add_person` and `set_show_activate` → `set_show_add_person` in this file only.

- [ ] **Step 2: Replace the activate button + form**

Around lines 489-507, replace:
```rust
<button
    class="btn btn--ghost btn--compact"
    on:click=move |_| set_show_add_person.update(|v| *v = !*v)
>
    {move || if show_add_person.get() {
        i18n::t(lang.get(), "hide_add_person")
    } else {
        i18n::t(lang.get(), "add_person")
    }}
</button>
```
And:
```rust
{move || {
    if show_add_person.get() {
        view! { <AddPersonForm set_selected=set_selected set_msg=set_msg set_show=set_show_add_person /> }.into_any()
    } else { view! { <span></span> }.into_any() }
}}
```

- [ ] **Step 3: Create AddPersonForm component**

In `spinbike-ui/src/pages/dashboard/add_person_form.rs` (new file), create the component. It posts to `/api/users` with `{name, email?, phone?, company?, card_code?}`. On 201: shows success banner, populates `set_selected`, closes the form.

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use crate::api;
use crate::i18n::{self, Lang};
use super::CardInfo;

#[derive(Serialize)]
struct CreateUserReq {
    name: String,
    email: Option<String>,
    phone: Option<String>,
    company: Option<String>,
    card_code: Option<String>,
}

#[derive(Deserialize, Clone)]
struct UserResp {
    id: i64,
    email: Option<String>,
    name: String,
    phone: Option<String>,
    company: Option<String>,
    card_code: Option<String>,
    credit: f64,
    blocked: bool,
    allow_debit: bool,
}

#[component]
pub fn AddPersonForm(
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    set_show: WriteSignal<bool>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang");
    let (name, set_name) = signal(String::new());
    let (email, set_email) = signal(String::new());
    let (phone, set_phone) = signal(String::new());
    let (company, set_company) = signal(String::new());
    let (card_code, set_card_code) = signal(String::new());
    let (err, set_err) = signal(String::new());
    let (loading, set_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if loading.get_untracked() { return; }
        let n = name.get_untracked();
        if n.trim().is_empty() {
            set_err.set(i18n::t(lang.get_untracked(), "name_required").to_string());
            return;
        }
        set_err.set(String::new());
        set_loading.set(true);
        let to_opt = |s: String| if s.trim().is_empty() { None } else { Some(s.trim().to_string()) };
        let body = CreateUserReq {
            name: n.trim().to_string(),
            email: to_opt(email.get_untracked()),
            phone: to_opt(phone.get_untracked()),
            company: to_opt(company.get_untracked()),
            card_code: to_opt(card_code.get_untracked()),
        };
        spawn_local(async move {
            match api::post::<CreateUserReq, UserResp>("/api/users", &body).await {
                Ok(u) => {
                    set_msg.set(i18n::tf(lang.get_untracked(), "add_person_ok_format", &[&u.name]));
                    set_selected.set(Some(CardInfo {
                        id: u.id,
                        card_code: u.card_code.clone(),
                        name: u.name.clone(),
                        email: u.email,
                        phone: u.phone,
                        company: u.company,
                        credit: u.credit,
                        blocked: u.blocked,
                        allow_debit: u.allow_debit,
                        ..Default::default()
                    }));
                    set_show.set(false);
                }
                Err(e) => set_err.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <form class="add-person-form" on:submit=on_submit>
            // name (required)
            <label>{move || i18n::t(lang.get(), "name")}
                <input type="text" required prop:value=move || name.get()
                       on:input=move |ev| set_name.set(event_target_value(&ev)) />
            </label>
            // email (optional)
            <label>{move || i18n::t(lang.get(), "email")}" "{move || i18n::t(lang.get(), "optional_paren")}
                <input type="email" prop:value=move || email.get()
                       on:input=move |ev| set_email.set(event_target_value(&ev)) />
            </label>
            // phone, company, card_code (optional)
            <label>{move || i18n::t(lang.get(), "phone")}" "{move || i18n::t(lang.get(), "optional_paren")}
                <input type="text" prop:value=move || phone.get()
                       on:input=move |ev| set_phone.set(event_target_value(&ev)) />
            </label>
            <label>{move || i18n::t(lang.get(), "company")}" "{move || i18n::t(lang.get(), "optional_paren")}
                <input type="text" prop:value=move || company.get()
                       on:input=move |ev| set_company.set(event_target_value(&ev)) />
            </label>
            <label>{move || i18n::t(lang.get(), "card_code")}" "{move || i18n::t(lang.get(), "optional_paren")}
                <input type="text" prop:value=move || card_code.get()
                       on:input=move |ev| set_card_code.set(event_target_value(&ev)) />
            </label>
            <button type="submit" data-testid="add-person-submit" class="btn btn--primary" disabled=move || loading.get()>
                {move || i18n::t(lang.get(), "add_person_submit")}
            </button>
            {move || if !err.get().is_empty() {
                view! { <p class="alert-error">{err.get()}</p> }.into_any()
            } else { view! { <span></span> }.into_any() }}
        </form>
    }
}

fn event_target_value(ev: &web_sys::Event) -> String {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}
```

Add `pub mod add_person_form;` to the `mod.rs` for the `dashboard` directory.

- [ ] **Step 4: Update action_form.rs endpoint paths**

In `spinbike-ui/src/pages/dashboard/action_form.rs`:

Find every reference to `/api/payments/charge`, `/api/payments/storno`, `/api/payments/sell-pass`, `/api/payments/log-visit`, `/api/cards/topup` and update the request body to use `user_id` instead of `card_id`. The path strings stay the same.

The visit_click_for closure (lines 242-303) — change body struct field `card_id` to `user_id`.

The do_charge function (lines 130-240) — update ChargeRequest and SellPassRequest body fields.

The do_topup closure (lines 91-128) — POST to `/api/users/topup` with `user_id` (replaces `/api/cards/topup`).

- [ ] **Step 5: Update negative_balance_list.rs**

In `spinbike-ui/src/pages/dashboard/negative_balance_list.rs`:
- Endpoint switches to `/api/users/negative-balance`.
- Struct `NegativeBalanceCard` → `NegativeBalanceUser`. Drop `first_name`/`last_name` fields, replace with `name: String`. Drop `barcode`, replace with `card_code: Option<String>`.
- `neg_to_card_info` becomes `neg_to_card_info` (still useful — produces a CardInfo for the dashboard panel) but populates `name` and `card_code`.

- [ ] **Step 6: Update search dropdown component**

Find the search-results component (likely `pages/dashboard/search_results.rs` or inline in `mod.rs`). The endpoint `/api/cards/search` → `/api/users/search`. Response struct field renames as above. `helpers::result_row_class(highlighted, credit)` continues to work.

- [ ] **Step 7: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 8: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/mod.rs \
        spinbike-ui/src/pages/dashboard/add_person_form.rs \
        spinbike-ui/src/pages/dashboard/action_form.rs \
        spinbike-ui/src/pages/dashboard/negative_balance_list.rs
git add -u spinbike-ui/src/pages/dashboard/
git commit -m "$(cat <<'EOF'
feat(ui): Add Person form replaces Activate Card; dashboard rewired to /api/users (#55)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: UI cleanup — i18n + helpers + router + delete link_card.rs

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`
- Modify: `spinbike-ui/src/pages/dashboard/helpers.rs`
- Modify: `spinbike-ui/src/router.rs`
- Delete: `spinbike-ui/src/pages/link_card.rs`
- Modify: any `pub mod link_card;` in `spinbike-ui/src/pages/mod.rs`

- [ ] **Step 1: i18n.rs — add new keys**

Append to the `TRANSLATIONS` static initializer (after the existing card-related keys):

```rust
    // #55 Add Person flow
    m.insert("add_person",          ("Pridat osobu", "Add Person"));
    m.insert("hide_add_person",     ("Skryt formular", "Hide form"));
    m.insert("add_person_submit",   ("Ulozit osobu", "Save Person"));
    m.insert("add_person_ok_format", ("Osoba pridana: {}", "Person added: {}"));
    m.insert("name_required",       ("Meno je povinne", "Name is required"));
    m.insert("optional_paren",      ("(volitelne)", "(optional)"));
    m.insert("card_code",           ("Kod karty", "Card code"));
    m.insert("card_code_optional",  ("Kod karty (volitelne)", "Card code (optional)"));
```

- [ ] **Step 2: i18n.rs — remove obsolete keys**

Delete these `m.insert(...)` lines:

- `"activate_new_card"` (line ~324)
- `"activate"` (line ~327)
- `"activate_ok"` (line ~379)
- `"hide_activate"` (~line 364)
- `"barcode"` (line ~343) — only if no other consumer; otherwise keep.
- `"card_barcode"` (~line 308)
- `"card_barcode_label"` (~line 410)
- `"scan_barcode"` (~line 411)
- `"link_card"` (~line 307) — link-card page is deleted.
- `"card_has_no_user"` (~line 402)
- `"new_card_barcode"` if present.

For each removed key, search the codebase for usage with `grep -rn "i18n::t(.*\"<key>\"" spinbike-ui/src/`. If a remaining usage exists, update it to a different key or remove the dead UI element.

- [ ] **Step 3: helpers.rs — simplify `full_name_or_fallback`**

In `spinbike-ui/src/pages/dashboard/helpers.rs`, the `full_name_or_fallback` function currently combines first_name + last_name + company. Simplify to:

```rust
pub fn user_display_name(name: &str, fallback_company: Option<&str>, fallback_card_code: Option<&str>) -> String {
    let trimmed = name.trim();
    if !trimmed.is_empty() && trimmed != "(no name)" {
        return trimmed.to_string();
    }
    if let Some(c) = fallback_company.filter(|s| !s.trim().is_empty()) {
        return c.trim().to_string();
    }
    if let Some(code) = fallback_card_code.filter(|s| !s.trim().is_empty()) {
        return format!("[{}]", code.trim());
    }
    "(no name)".to_string()
}
```

Update or delete the existing `full_name_or_fallback` and its call sites. Update wasm-bindgen tests accordingly.

- [ ] **Step 4: Delete link_card.rs page**

```bash
git rm spinbike-ui/src/pages/link_card.rs
```

In `spinbike-ui/src/pages/mod.rs`, remove `pub mod link_card;`.

- [ ] **Step 5: router.rs — drop `/link-card` route**

Open `spinbike-ui/src/router.rs`. Remove:
```rust
<Route path=path!("/link-card") view=LinkCardPage />
```
And the `use crate::pages::link_card::LinkCardPage;` import.

- [ ] **Step 6: Local format check**

```bash
cargo fmt --all --check
```

- [ ] **Step 7: Commit**

```bash
git add spinbike-ui/src/i18n.rs \
        spinbike-ui/src/pages/dashboard/helpers.rs \
        spinbike-ui/src/router.rs
git add -u spinbike-ui/src/pages/link_card.rs spinbike-ui/src/pages/mod.rs
git commit -m "$(cat <<'EOF'
feat(ui): i18n add_person keys; drop activate_/link_card surface (#55)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: E2E specs — rewrite to user-centric API + add add-person.spec.ts

**Files:**
- Modify: `e2e/tests/helpers.ts` — replace `activateUniqueCard` with `createUniqueUser`
- Modify: every spec under `e2e/tests/` that calls `/api/cards/activate`, `/api/payments/charge`, `/api/payments/topup`, `/api/payments/log-visit`, `/api/payments/sell-pass`, `/api/cards/topup`, `/api/cards/search`, `/api/cards/negative-balance`, or `/api/test-fixtures/seed-credit`. Per the Step-1 grep, this set includes (non-exhaustive but at minimum):
  - `negative-balance.spec.ts`
  - `last-visit-display.spec.ts`
  - `card-action-form.spec.ts`, `card-action-form-language.spec.ts`, `card-search-keyboard.spec.ts`
  - `credit-improvements.spec.ts`
  - `dashboard.spec.ts`, `dashboard-button-layout.spec.ts`, `desk-ux.spec.ts`
  - `legacy-history.spec.ts`
  - `log-visit-class-only.spec.ts`
  - `monthly-pass.spec.ts`, `monthly-pass-expired.spec.ts`
  - `no-predefined-prices.spec.ts`
  - `per-card-overview.spec.ts`
  - `post-backfill-history.spec.ts`
  - `redesign-history-pagination.spec.ts`, `redesign-sheets.spec.ts`, `redesign-theme.spec.ts`
  - `sell-pass-price-input.spec.ts`
  - `txn-note.spec.ts`
  - `visit-button-feedback.spec.ts`
- Create: `e2e/tests/add-person.spec.ts`

- [ ] **Step 1: Grep the exact set of specs to rewrite**

```bash
grep -lE '/api/(cards|payments|test-fixtures/seed-credit)' e2e/tests/*.ts
```

The output is the working set. Apply uniform substitutions:
- `/api/cards/activate` → `/api/users` (POST). Body: `{name, email?, phone?, company?, card_code?, initial_credit?}` instead of `{barcode, initial_credit, first_name, last_name, company, phone}`.
- `/api/payments/charge` → unchanged path; body `card_id` → `user_id`.
- `/api/payments/topup`, `/api/cards/topup` → `/api/users/topup` (POST); body `card_id` → `user_id`.
- `/api/payments/log-visit` → unchanged path; body `card_id` → `user_id`.
- `/api/payments/sell-pass` → unchanged path; body `card_id` → `user_id`.
- `/api/cards/search` → `/api/users/search` (GET).
- `/api/cards/negative-balance` → `/api/users/negative-balance` (GET).
- `/api/test-fixtures/seed-credit` → `/api/test-fixtures/seed-user`.

- [ ] **Step 2: Rewrite `e2e/tests/helpers.ts`**

Replace `activateUniqueCard` with `createUniqueUser`:

```typescript
export async function createUniqueUser(
    token: string,
    initialCredit: number,
    prefix: string = 'AF',
): Promise<{ user_id: number; name: string; email: string }> {
    const tag = Math.random().toString(36).slice(2, 10).toUpperCase();
    const lastName = `${prefix}${tag}`;
    const email = `${prefix.toLowerCase()}.${tag.toLowerCase()}@e2e.local`;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${token}`,
        },
        body: JSON.stringify({
            name: `Test ${lastName}`,
            email,
            initial_credit: initialCredit,
        }),
    });
    if (!resp.ok) throw new Error(`createUniqueUser failed: ${resp.status} ${await resp.text()}`);
    const json = await resp.json();
    return { user_id: json.id, name: json.name, email: json.email };
}
```

(Adjust BASE_URL handling per existing helpers.ts conventions.)

- [ ] **Step 3: For each spec in the working set, apply substitutions**

The mechanical pattern is the same per file. Where the spec asserts on a card row in a search dropdown, switch the assertion target text from the legacy first/last-name format to the new `name` field rendering.

- [ ] **Step 4: Delete any link-card-related spec if it exists**

```bash
ls e2e/tests/link-card*.spec.ts 2>/dev/null && git rm e2e/tests/link-card*.spec.ts
```

- [ ] **Step 5: Create `e2e/tests/add-person.spec.ts`**

```typescript
import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    setEnglishLanguage,
    loginViaAPI,
} from './helpers';

const RUN_TAG = `ADDP${Math.random().toString(36).slice(2, 6).toUpperCase()}`;

test.describe('Add Person flow (#55)', () => {
    test('CEO can add a new person at the desk and find them in search', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        await loginViaAPI(page, baseURL!, 'staff@spinbike.local', 'staff-password');
        await setEnglishLanguage(page);

        await page.goto('/staff');
        await page.getByRole('button', { name: /add person/i }).click();

        const fullName = `Anna Test ${RUN_TAG}`;
        const email = `anna.${RUN_TAG.toLowerCase()}@e2e.local`;

        await page.getByLabel(/name/i).fill(fullName);
        await page.getByLabel(/email/i).fill(email);
        await page.getByLabel(/phone/i).fill('+421900111222');
        await page.getByLabel(/company/i).fill('TestCo');
        await page.getByTestId('add-person-submit').click();

        // Success banner
        await expect(page.locator('.alert-success'))
            .toContainText(`Person added: ${fullName}`, { timeout: 5000 });

        // New person appears in search
        await page.getByPlaceholder(/search/i).fill(RUN_TAG);
        await expect(page.locator('[data-testid="search-result"]', { hasText: fullName }))
            .toBeVisible({ timeout: 5000 });

        assertCleanConsole(messages);
    });

    test('Add Person rejects empty name', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        await loginViaAPI(page, baseURL!, 'staff@spinbike.local', 'staff-password');
        await setEnglishLanguage(page);

        await page.goto('/staff');
        await page.getByRole('button', { name: /add person/i }).click();
        await page.getByTestId('add-person-submit').click();

        await expect(page.locator('.alert-error'))
            .toContainText(/required/i, { timeout: 2000 });

        assertCleanConsole(messages);
    });
});
```

- [ ] **Step 6: Commit**

```bash
git add e2e/tests/add-person.spec.ts e2e/tests/helpers.ts
git add -u e2e/tests/
git commit -m "$(cat <<'EOF'
test(e2e): rewrite specs for /api/users; add add-person.spec.ts (#55)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Push, monitor CI to terminal state, open PR (CONTROLLER, not subagent)

- [ ] **Step 1: Push to dev**

```bash
git push origin dev
```

- [ ] **Step 2: Monitor CI to terminal state**

```bash
RUN_ID=$(gh run list --branch dev --limit 1 --json databaseId -q '.[0].databaseId')
sleep 600 && gh run view "$RUN_ID" --json status,conclusion,jobs
```

If any job fails: `gh run view "$RUN_ID" --log-failed`, fix root cause in ONE batched commit, push, monitor again. NEVER blindly re-run failed CI; investigate first.

- [ ] **Step 3: Verify ALL jobs ✅**

Required green: Test Integrity, Lint, Build WASM (UI), Test, Test (UI), E2E Tests, Mutation Testing, Deploy (dev), Smoke (dev), Version Bump Check.

- [ ] **Step 4: Open PR `dev` → `main`**

```bash
gh pr create --base main --head dev \
  --title "feat: users replace cards (#55)" \
  --body "$(cat <<'EOF'
## Summary

Architectural rework: drop `cards` table, make `users` the canonical customer entity. Replace
the desk "Activate New Card" flow with "Add Person". Single PR with full data migration; no
parallel old/new paths.

Closes #55.

## Changes

- DB migration #13 recreates users (email nullable + credit/card_code/blocked/allow_debit/company/search_text), recreates transactions and bookings without card_id, drops cards.
- Backend: `db/cards.rs` and `routes/cards.rs` deleted; helpers ported into `db/users.rs`; new `routes/users.rs`. Payment endpoints rekeyed to `user_id`.
- Frontend: `Add Person` form replaces `Activate Card`. `link_card` page + `/link-card` route deleted. i18n keys swapped.
- Tests: `tests/cards_routes.rs` ported to `tests/users_routes.rs`. New `e2e/tests/add-person.spec.ts`. Existing E2E specs rewritten to `/api/users`.
- Migration validation pending dev DB sanity check (Task 12 below).

## Test plan

- [ ] CI all green (lint, test, build, E2E, mutation, deploy-dev, smoke-dev)
- [ ] Manual sanity on dev DB: `users` count = `cards` count + pre-existing user count; spot-check 3 known rows
- [ ] Open dev dashboard, click "Add Person", create a person, search by name, click row → action panel opens with credit
- [ ] Negative-balance list still renders and matches users table
- [ ] Last-visit display still renders against migrated transactions

## Out of scope

Email collection / invitation flow, magic-link / password-reset auth, online top-up — to be tracked in follow-up issues.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Verify mergeable + clean**

```bash
PR=$(gh pr list --head dev --base main --json number -q '.[0].number')
gh api "repos/zbynekdrlik/spinbike/pulls/$PR" --jq '{mergeable: .mergeable, mergeable_state: .mergeable_state}'
```

Expect `{mergeable: true, mergeable_state: "clean"}`. If not, investigate (sync with main, fix conflicts).

---

## Task 13: Validate migration on prod-synced dev DB (CONTROLLER, after CI green)

- [ ] **Step 1: Find dev DB path**

```bash
ssh -q ${SPINBIKE_DEV_HOST:-localhost} "ls -la /var/lib/spinbike-dev/ 2>/dev/null || sudo find / -name 'spinbike*.db' 2>/dev/null | head"
```
or directly on local machine: `find / -name 'spinbike*.db' 2>/dev/null | head` (per project memory `feedback_prod_dev_same_machine.md`).

- [ ] **Step 2: Compare counts**

```bash
DEV_DB=/var/lib/spinbike-dev/spinbike.db   # adjust path
sqlite3 "$DEV_DB" "SELECT
  (SELECT COUNT(*) FROM users WHERE card_code IS NOT NULL) AS users_with_card_code,
  (SELECT COUNT(*) FROM users) AS users_total,
  (SELECT COUNT(*) FROM transactions WHERE user_id IS NULL) AS orphaned_txns;"
```

Expected: `users_with_card_code` ≈ original `cards` count; `orphaned_txns = 0`.

- [ ] **Step 3: Spot-check 3 known customers**

Look up by name (e.g., a CEO-known regular). Confirm credit, card_code, blocked match what staff dashboard shows.

```bash
sqlite3 "$DEV_DB" "SELECT id, name, email, card_code, credit, blocked
                   FROM users WHERE name LIKE '%<known-surname>%' LIMIT 5;"
```

- [ ] **Step 4: Document findings in PR description**

`gh pr comment <PR> --body "Dev migration validation: ..."`

---

## Task 14: Post-deploy verification (CONTROLLER, ONLY after user merges)

- [ ] **Step 1: Wait for main CI green**

```bash
RUN_ID=$(gh run list --branch main --limit 1 --json databaseId -q '.[0].databaseId')
sleep 600 && gh run view "$RUN_ID" --json status,conclusion,jobs
```

All green required (lint, test, build, E2E, deploy-prod, smoke-prod).

- [ ] **Step 2: Verify version on prod DOM**

```bash
curl -s https://spinbike.newlevel.media/api/version  # expect "0.13.22"
```

Then via Playwright: navigate https://spinbike.newlevel.media/login, read `[data-testid="version"]`, assert text contains `v0.13.22`. If still showing prior version → service-worker cache; clear via `navigator.serviceWorker.getRegistrations()`+ unregister + Cache API delete + reload with cache-bust query.

- [ ] **Step 3: Functional check on prod**

Login as staff. Navigate to Desk. Search by name (a known legacy customer). Click row. Confirm panel renders credit, card_code, last_visit, pass info.

Click "Add Person". Fill name + email + phone + company. Submit. Confirm success banner + presence in search.

- [ ] **Step 4: Repeat on dev**

Same checks against https://spinbike-dev.newlevel.media.

- [ ] **Step 5: Send completion report**

Per `completion-report.md` template. PR was merged → no further commits required.

---

## End

When all tasks ✅ and PR is mergeable + clean, send the completion report. **Never merge the PR yourself; wait for the user's explicit "merge it" instruction.**
