# SpinBike PWA Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a modern PWA replacing the legacy VB6 spin bike reservation and prepaid card system, hosted on Hetzner VPS.

**Architecture:** Monolith — single Axum binary serving a Leptos WASM frontend via rust-embed. SQLite database. JWT auth with OAuth2 support. WebSocket for live booking updates.

**Tech Stack:** Rust (edition 2024), Axum 0.8, Leptos 0.7 CSR, Trunk, sqlx 0.8 + SQLite, rust-embed 8, serde, tokio, tower-http, gloo-net.

**Design Spec:** `docs/superpowers/specs/2026-04-09-spinbike-pwa-design.md`

---

## File Structure

```
spinbike/
├── Cargo.toml                          # Workspace root
├── Cargo.lock
├── VERSION                             # Single source of truth for version
├── scripts/
│   └── sync-version.sh                 # Syncs VERSION to all Cargo.toml files
├── crates/
│   ├── spinbike-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # Re-exports modules
│   │       ├── models.rs               # User, Card, Booking, etc. (serde models)
│   │       ├── auth.rs                 # Role enum, JWT claims, auth types
│   │       └── ws.rs                   # WebSocket message types (ClientMsg, ServerMsg)
│   └── spinbike-server/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                  # Re-exports, AppState, start_server()
│           ├── bin/
│           │   └── server.rs           # Binary entry point
│           ├── db/
│           │   ├── mod.rs              # Pool creation, migration runner
│           │   ├── migrations.rs       # SQL migration strings (V1..VN)
│           │   ├── users.rs            # User CRUD queries
│           │   ├── cards.rs            # Card CRUD queries
│           │   ├── classes.rs          # ClassTemplate, cancellation, booking queries
│           │   ├── transactions.rs     # Transaction queries
│           │   └── settings.rs         # Settings key/value queries
│           ├── routes/
│           │   ├── mod.rs              # Merges all routers
│           │   ├── auth.rs             # POST /api/auth/register, /login, /oauth/*
│           │   ├── classes.rs          # GET /api/classes, POST /api/bookings, etc.
│           │   ├── cards.rs            # GET/POST /api/cards/*
│           │   ├── payments.rs         # POST /api/payments/*
│           │   ├── admin.rs            # Admin-only routes (templates, instructors, settings, users)
│           │   └── static_files.rs     # rust-embed SPA serving
│           ├── auth/
│           │   ├── mod.rs              # JWT creation/validation, middleware extractor
│           │   └── oauth.rs            # Google/Facebook OAuth2 flows
│           └── ws.rs                   # WebSocket handler, broadcast
├── spinbike-ui/
│   ├── Cargo.toml                      # Excluded from workspace
│   ├── Trunk.toml
│   ├── index.html                      # PWA shell with Trunk directives
│   ├── style.css                       # Global styles
│   ├── manifest.json                   # PWA manifest
│   ├── sw.js                           # Service worker
│   ├── icon-192.png                    # PWA icons
│   ├── icon-512.png
│   └── src/
│       ├── lib.rs                      # WASM entry, mounts Leptos app
│       ├── api.rs                      # HTTP client helpers (fetch wrappers)
│       ├── ws.rs                       # WebSocket client with reconnect
│       ├── auth.rs                     # Auth state (JWT storage, login/logout)
│       ├── router.rs                   # Top-level router, role-based routing
│       ├── components/
│       │   ├── mod.rs
│       │   ├── nav.rs                  # Navigation bar (role-aware)
│       │   ├── class_card.rs           # Single class slot card
│       │   └── day_picker.rs           # Week day selector
│       └── pages/
│           ├── mod.rs
│           ├── schedule.rs             # Class schedule view (customer + staff)
│           ├── my_bookings.rs          # Customer: my bookings list
│           ├── my_balance.rs           # Customer: credit balance + history
│           ├── login.rs                # Login/register page
│           ├── link_card.rs            # Link barcode card to account
│           ├── staff_dashboard.rs      # Staff: class management, walk-ins
│           ├── card_ops.rs             # Staff: card activate, top-up, block
│           ├── payments.rs             # Staff: process payment / storno
│           └── admin.rs                # Admin: templates, instructors, services, users, settings
├── e2e/
│   ├── package.json
│   ├── playwright.config.ts
│   └── tests/
│       ├── schedule.spec.ts            # View schedule, book a class
│       ├── auth.spec.ts                # Register, login, logout
│       ├── staff.spec.ts               # Staff operations
│       └── admin.spec.ts               # Admin operations
└── .github/
    └── workflows/
        └── ci.yml
```

---

### Task 1: Project Scaffolding — Workspace, Core Crate, VERSION

**Files:**
- Create: `Cargo.toml`
- Create: `VERSION`
- Create: `scripts/sync-version.sh`
- Create: `crates/spinbike-core/Cargo.toml`
- Create: `crates/spinbike-core/src/lib.rs`
- Create: `crates/spinbike-core/src/models.rs`
- Create: `crates/spinbike-core/src/auth.rs`
- Create: `crates/spinbike-core/src/ws.rs`

- [ ] **Step 1: Initialize git repo**

```bash
cd /home/newlevel/devel/spinbike
git init
```

- [ ] **Step 2: Create .gitignore**

Create `.gitignore`:
```
/target
**/target
/dist
**/dist
*.swp
*.swo
.env
.superpowers/
zbynek/
```

- [ ] **Step 3: Create VERSION file**

Create `VERSION`:
```
0.1.0
```

- [ ] **Step 4: Create sync-version.sh**

Create `scripts/sync-version.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

VERSION_FILE="$ROOT_DIR/VERSION"
if [[ ! -f "$VERSION_FILE" ]]; then
    echo "ERROR: VERSION file not found at $VERSION_FILE" >&2
    exit 1
fi

VERSION="$(cat "$VERSION_FILE" | tr -d '[:space:]')"
if [[ -z "$VERSION" ]]; then
    echo "ERROR: VERSION file is empty" >&2
    exit 1
fi

echo "Syncing version: $VERSION"

# Update root Cargo.toml [workspace.package] version
sed -i "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$ROOT_DIR/Cargo.toml"

# Update spinbike-ui/Cargo.toml (excluded from workspace, needs manual sync)
UI_CARGO="$ROOT_DIR/spinbike-ui/Cargo.toml"
if [[ -f "$UI_CARGO" ]]; then
    sed -i "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$UI_CARGO"
fi

echo "Done. All version fields set to $VERSION"
```

```bash
chmod +x scripts/sync-version.sh
```

- [ ] **Step 5: Create workspace Cargo.toml**

Create `Cargo.toml`:
```toml
[workspace]
members = [
    "crates/spinbike-core",
    "crates/spinbike-server",
]
exclude = [
    "spinbike-ui",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"

[workspace.dependencies]
spinbike-core = { path = "crates/spinbike-core" }
tokio = { version = "1", features = ["full"] }
axum = { version = "0.8", features = ["ws", "macros"] }
tower-http = { version = "0.6", features = ["cors", "trace", "fs"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
futures = "0.3"
chrono = { version = "0.4", features = ["serde"] }

[profile.release]
lto = true
codegen-units = 1
panic = "abort"
```

- [ ] **Step 6: Create spinbike-core crate**

Create `crates/spinbike-core/Cargo.toml`:
```toml
[package]
name = "spinbike-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
```

Create `crates/spinbike-core/src/lib.rs`:
```rust
pub mod auth;
pub mod models;
pub mod ws;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

Create `crates/spinbike-core/src/auth.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Staff,
    Customer,
}

impl Role {
    pub fn can_manage_templates(&self) -> bool {
        matches!(self, Role::Admin)
    }

    pub fn can_manage_cards(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_book_for_others(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_cancel_any_booking(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_process_payments(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_cancel_class(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_manage_users(&self) -> bool {
        matches!(self, Role::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub email: String,
    pub role: Role,
    pub exp: i64,
    pub iat: i64,
}
```

Create `crates/spinbike-core/src/models.rs`:
```rust
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::auth::Role;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub phone: Option<String>,
    pub role: Role,
    pub oauth_provider: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: i64,
    pub name: String,
    pub default_price: f64,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionAction {
    Credit,
    Debit,
    Activation,
    Storno,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub user_id: Option<i64>,
    pub card_id: Option<i64>,
    pub staff_id: Option<i64>,
    pub service_id: Option<i64>,
    pub amount: f64,
    pub action: TransactionAction,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instructor {
    pub id: i64,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassTemplate {
    pub id: i64,
    pub weekday: u8,
    pub start_time: String,
    pub duration_minutes: i32,
    pub instructor_id: i64,
    pub capacity: i32,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassOccurrence {
    pub template: ClassTemplate,
    pub date: String,
    pub instructor_name: String,
    pub booked: i32,
    pub cancelled: bool,
    pub user_booked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Booking {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub user_id: i64,
    pub user_name: Option<String>,
    pub created_by: i64,
    pub created_at: String,
    pub cancelled_at: Option<String>,
}
```

Create `crates/spinbike-core/src/ws.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ClientMsg {
    Ping,
    SubscribeSchedule { date: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerMsg {
    BookingUpdate {
        template_id: i64,
        date: String,
        booked: i32,
        capacity: i32,
    },
    ClassCancelled {
        template_id: i64,
        date: String,
    },
    Pong,
}
```

- [ ] **Step 7: Verify it compiles**

```bash
cd /home/newlevel/devel/spinbike
cargo check -p spinbike-core
```

Expected: compiles without errors.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock VERSION scripts/ crates/spinbike-core/ .gitignore
git commit -m "feat: scaffold workspace with spinbike-core crate

Add workspace structure, VERSION file, sync script, and core crate
with domain models, auth types, and WebSocket message definitions."
```

---

### Task 2: Server Crate — Database, Migrations, Pool

**Files:**
- Create: `crates/spinbike-server/Cargo.toml`
- Create: `crates/spinbike-server/src/lib.rs`
- Create: `crates/spinbike-server/src/db/mod.rs`
- Create: `crates/spinbike-server/src/db/migrations.rs`

- [ ] **Step 1: Write database pool test**

Create `crates/spinbike-server/src/db/mod.rs`:
```rust
pub mod migrations;

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::path::Path;
use std::str::FromStr;

pub async fn create_pool(db_path: &Path) -> Result<SqlitePool> {
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let options = SqliteConnectOptions::from_str(&url)?
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .create_if_missing(true)
        .pragma("foreign_keys", "1");

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    Ok(pool)
}

pub async fn create_memory_pool() -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")?
        .pragma("foreign_keys", "1");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)")
        .execute(pool)
        .await?;

    let current: i32 = sqlx::query("SELECT COALESCE(MAX(version), 0) as v FROM schema_version")
        .fetch_one(pool)
        .await
        .map(|r| r.get("v"))?;

    let all_migrations = migrations::all();

    for &(version, sql) in all_migrations {
        if current < version {
            tracing::info!("Running migration V{version}");
            let mut tx = pool.begin().await?;
            for statement in sql.split(';') {
                let trimmed = statement.trim();
                if !trimmed.is_empty() {
                    sqlx::query(trimmed).execute(&mut *tx).await?;
                }
            }
            sqlx::query("INSERT INTO schema_version (version) VALUES (?1)")
                .bind(version)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            tracing::info!("Migration V{version} complete");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_migrations_run_on_fresh_db() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Verify all tables exist
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"cards".to_string()));
        assert!(tables.contains(&"services".to_string()));
        assert!(tables.contains(&"transactions".to_string()));
        assert!(tables.contains(&"instructors".to_string()));
        assert!(tables.contains(&"class_templates".to_string()));
        assert!(tables.contains(&"class_cancellations".to_string()));
        assert!(tables.contains(&"bookings".to_string()));
        assert!(tables.contains(&"settings".to_string()));
    }

    #[tokio::test]
    async fn test_migrations_are_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Running again should not error
        run_migrations(&pool).await.unwrap();
    }
}
```

- [ ] **Step 2: Write migration V1 SQL**

Create `crates/spinbike-server/src/db/migrations.rs`:
```rust
pub fn all() -> &'static [(i32, &'static str)] {
    &[
        (1, V1_INITIAL_SCHEMA),
    ]
}

const V1_INITIAL_SCHEMA: &str = r#"
CREATE TABLE users (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    email           TEXT NOT NULL UNIQUE,
    password_hash   TEXT,
    name            TEXT NOT NULL,
    phone           TEXT,
    role            TEXT NOT NULL DEFAULT 'customer',
    oauth_provider  TEXT,
    oauth_id        TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE cards (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    barcode         TEXT NOT NULL UNIQUE,
    user_id         INTEGER REFERENCES users(id),
    blocked         INTEGER NOT NULL DEFAULT 0,
    credit          REAL NOT NULL DEFAULT 0.0,
    allow_debit     INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE services (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    default_price   REAL NOT NULL DEFAULT 0.0,
    active          INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE transactions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id         INTEGER REFERENCES users(id),
    card_id         INTEGER REFERENCES cards(id),
    staff_id        INTEGER REFERENCES users(id),
    service_id      INTEGER REFERENCES services(id),
    amount          REAL NOT NULL,
    action          TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE instructors (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    active          INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE class_templates (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    weekday             INTEGER NOT NULL,
    start_time          TEXT NOT NULL,
    duration_minutes    INTEGER NOT NULL DEFAULT 60,
    instructor_id       INTEGER NOT NULL REFERENCES instructors(id),
    capacity            INTEGER NOT NULL DEFAULT 10,
    active              INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE class_cancellations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id     INTEGER NOT NULL REFERENCES class_templates(id),
    date            TEXT NOT NULL,
    reason          TEXT,
    cancelled_by    INTEGER NOT NULL REFERENCES users(id),
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(template_id, date)
);

CREATE TABLE bookings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id     INTEGER NOT NULL REFERENCES class_templates(id),
    date            TEXT NOT NULL,
    user_id         INTEGER NOT NULL REFERENCES users(id),
    created_by      INTEGER NOT NULL REFERENCES users(id),
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    cancelled_at    TEXT
);

CREATE UNIQUE INDEX idx_bookings_active
    ON bookings(template_id, date, user_id)
    WHERE cancelled_at IS NULL;

CREATE TABLE settings (
    key     TEXT PRIMARY KEY,
    value   TEXT NOT NULL
);

INSERT INTO services (name, default_price) VALUES ('Spinning', 5.0);
INSERT INTO services (name, default_price) VALUES ('Fitness', 5.0);
INSERT INTO settings (key, value) VALUES ('bike_count', '10');
INSERT INTO settings (key, value) VALUES ('center_name', 'Squash Centrum Smizany')
"#;
```

- [ ] **Step 3: Create server Cargo.toml and lib.rs**

Create `crates/spinbike-server/Cargo.toml`:
```toml
[package]
name = "spinbike-server"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
spinbike-core = { workspace = true }
axum = { workspace = true }
tower-http = { workspace = true }
tokio = { workspace = true }
sqlx = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
futures = { workspace = true }
chrono = { workspace = true }
rust-embed = { version = "8", features = ["include-exclude"] }
mime_guess = "2"
jsonwebtoken = "9"
argon2 = "0.5"
rand = "0.8"

[[bin]]
name = "spinbike-server"
path = "src/bin/server.rs"
```

Create `crates/spinbike-server/src/lib.rs`:
```rust
pub mod db;

use sqlx::SqlitePool;
use tokio::sync::broadcast;
use spinbike_core::ws::ServerMsg;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub event_tx: broadcast::Sender<ServerMsg>,
    pub jwt_secret: String,
}
```

Create `crates/spinbike-server/src/bin/server.rs`:
```rust
fn main() {
    // Placeholder — will be implemented in Task 4
    println!("spinbike-server placeholder");
}
```

- [ ] **Step 4: Run the tests**

```bash
cd /home/newlevel/devel/spinbike
cargo test -p spinbike-server
```

Expected: 2 tests pass (`test_migrations_run_on_fresh_db`, `test_migrations_are_idempotent`).

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/
git commit -m "feat: add spinbike-server crate with SQLite migrations

V1 migration creates all tables: users, cards, services, transactions,
instructors, class_templates, class_cancellations, bookings, settings.
Includes in-memory pool for tests."
```

---

### Task 3: Database Query Modules

**Files:**
- Create: `crates/spinbike-server/src/db/users.rs`
- Create: `crates/spinbike-server/src/db/cards.rs`
- Create: `crates/spinbike-server/src/db/classes.rs`
- Create: `crates/spinbike-server/src/db/transactions.rs`
- Create: `crates/spinbike-server/src/db/settings.rs`
- Modify: `crates/spinbike-server/src/db/mod.rs`

- [ ] **Step 1: Write failing tests for user queries**

Add to `crates/spinbike-server/src/db/mod.rs` (in the modules section):
```rust
pub mod users;
pub mod cards;
pub mod classes;
pub mod transactions;
pub mod settings;
```

Create `crates/spinbike-server/src/db/users.rs`:
```rust
use anyhow::Result;
use sqlx::SqlitePool;

pub async fn create_user(
    pool: &SqlitePool,
    email: &str,
    password_hash: Option<&str>,
    name: &str,
    phone: Option<&str>,
    role: &str,
    oauth_provider: Option<&str>,
    oauth_id: Option<&str>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, name, phone, role, oauth_provider, oauth_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         RETURNING id"
    )
    .bind(email)
    .bind(password_hash)
    .bind(name)
    .bind(phone)
    .bind(role)
    .bind(oauth_provider)
    .bind(oauth_id)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

pub async fn get_user_by_email(pool: &SqlitePool, email: &str) -> Result<Option<UserRow>> {
    let row = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE email = ?1")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn get_user_by_id(pool: &SqlitePool, id: i64) -> Result<Option<UserRow>> {
    let row = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn get_user_by_oauth(pool: &SqlitePool, provider: &str, oauth_id: &str) -> Result<Option<UserRow>> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE oauth_provider = ?1 AND oauth_id = ?2"
    )
    .bind(provider)
    .bind(oauth_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_users(pool: &SqlitePool) -> Result<Vec<UserRow>> {
    let rows = sqlx::query_as::<_, UserRow>("SELECT * FROM users ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn update_user_role(pool: &SqlitePool, user_id: i64, role: &str) -> Result<()> {
    sqlx::query("UPDATE users SET role = ?1 WHERE id = ?2")
        .bind(role)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub password_hash: Option<String>,
    pub name: String,
    pub phone: Option<String>,
    pub role: String,
    pub oauth_provider: Option<String>,
    pub oauth_id: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    #[tokio::test]
    async fn test_create_and_get_user() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let id = create_user(&pool, "test@example.com", Some("hash123"), "Test User", None, "customer", None, None)
            .await.unwrap();
        assert!(id > 0);

        let user = get_user_by_email(&pool, "test@example.com").await.unwrap().unwrap();
        assert_eq!(user.name, "Test User");
        assert_eq!(user.role, "customer");
    }

    #[tokio::test]
    async fn test_duplicate_email_fails() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        create_user(&pool, "dup@test.com", Some("h"), "A", None, "customer", None, None).await.unwrap();
        let result = create_user(&pool, "dup@test.com", Some("h"), "B", None, "customer", None, None).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Write card queries with tests**

Create `crates/spinbike-server/src/db/cards.rs`:
```rust
use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CardRow {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
    pub created_at: String,
}

pub async fn create_card(pool: &SqlitePool, barcode: &str, credit: f64) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO cards (barcode, credit) VALUES (?1, ?2) RETURNING id"
    )
    .bind(barcode)
    .bind(credit)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn get_card_by_barcode(pool: &SqlitePool, barcode: &str) -> Result<Option<CardRow>> {
    let row = sqlx::query_as::<_, CardRow>("SELECT * FROM cards WHERE barcode = ?1")
        .bind(barcode)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn get_card_by_user(pool: &SqlitePool, user_id: i64) -> Result<Option<CardRow>> {
    let row = sqlx::query_as::<_, CardRow>("SELECT * FROM cards WHERE user_id = ?1")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn link_card_to_user(pool: &SqlitePool, card_id: i64, user_id: i64) -> Result<()> {
    sqlx::query("UPDATE cards SET user_id = ?1 WHERE id = ?2")
        .bind(user_id)
        .bind(card_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_credit(pool: &SqlitePool, card_id: i64, amount: f64) -> Result<f64> {
    let new_credit: f64 = sqlx::query_scalar(
        "UPDATE cards SET credit = credit + ?1 WHERE id = ?2 RETURNING credit"
    )
    .bind(amount)
    .bind(card_id)
    .fetch_one(pool)
    .await?;
    Ok(new_credit)
}

pub async fn set_blocked(pool: &SqlitePool, card_id: i64, blocked: bool) -> Result<()> {
    sqlx::query("UPDATE cards SET blocked = ?1 WHERE id = ?2")
        .bind(blocked)
        .bind(card_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_allow_debit(pool: &SqlitePool, card_id: i64, allow: bool) -> Result<()> {
    sqlx::query("UPDATE cards SET allow_debit = ?1 WHERE id = ?2")
        .bind(allow)
        .bind(card_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    #[tokio::test]
    async fn test_create_and_get_card() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let id = create_card(&pool, "70701001", 50.0).await.unwrap();
        let card = get_card_by_barcode(&pool, "70701001").await.unwrap().unwrap();
        assert_eq!(card.id, id);
        assert_eq!(card.credit, 50.0);
        assert!(!card.blocked);
    }

    #[tokio::test]
    async fn test_update_credit() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let id = create_card(&pool, "70701002", 100.0).await.unwrap();
        let new_credit = update_credit(&pool, id, -30.0).await.unwrap();
        assert_eq!(new_credit, 70.0);

        let new_credit = update_credit(&pool, id, 10.0).await.unwrap();
        assert_eq!(new_credit, 80.0);
    }

    #[tokio::test]
    async fn test_link_card_to_user() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let user_id = crate::db::users::create_user(&pool, "u@test.com", Some("h"), "U", None, "customer", None, None).await.unwrap();
        let card_id = create_card(&pool, "70701003", 0.0).await.unwrap();

        link_card_to_user(&pool, card_id, user_id).await.unwrap();
        let card = get_card_by_user(&pool, user_id).await.unwrap().unwrap();
        assert_eq!(card.barcode, "70701003");
    }
}
```

- [ ] **Step 3: Write class/booking queries with tests**

Create `crates/spinbike-server/src/db/classes.rs`:
```rust
use anyhow::{Result, bail};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassTemplateRow {
    pub id: i64,
    pub weekday: i32,
    pub start_time: String,
    pub duration_minutes: i32,
    pub instructor_id: i64,
    pub capacity: i32,
    pub active: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BookingRow {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub user_id: i64,
    pub created_by: i64,
    pub created_at: String,
    pub cancelled_at: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CancellationRow {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub reason: Option<String>,
    pub cancelled_by: i64,
    pub created_at: String,
}

pub async fn create_template(
    pool: &SqlitePool,
    weekday: i32,
    start_time: &str,
    duration_minutes: i32,
    instructor_id: i64,
    capacity: i32,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity)
         VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id"
    )
    .bind(weekday)
    .bind(start_time)
    .bind(duration_minutes)
    .bind(instructor_id)
    .bind(capacity)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_active_templates(pool: &SqlitePool) -> Result<Vec<ClassTemplateRow>> {
    let rows = sqlx::query_as::<_, ClassTemplateRow>(
        "SELECT * FROM class_templates WHERE active = 1 ORDER BY weekday, start_time"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn cancel_occurrence(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
    reason: Option<&str>,
    cancelled_by: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO class_cancellations (template_id, date, reason, cancelled_by)
         VALUES (?1, ?2, ?3, ?4)"
    )
    .bind(template_id)
    .bind(date)
    .bind(reason)
    .bind(cancelled_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn is_occurrence_cancelled(pool: &SqlitePool, template_id: i64, date: &str) -> Result<bool> {
    let count: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM class_cancellations WHERE template_id = ?1 AND date = ?2"
    )
    .bind(template_id)
    .bind(date)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn create_booking(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
    user_id: i64,
    created_by: i64,
) -> Result<i64> {
    // Check capacity
    let template = sqlx::query_as::<_, ClassTemplateRow>(
        "SELECT * FROM class_templates WHERE id = ?1"
    )
    .bind(template_id)
    .fetch_optional(pool)
    .await?;

    let template = match template {
        Some(t) => t,
        None => bail!("Class template not found"),
    };

    let booked: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL"
    )
    .bind(template_id)
    .bind(date)
    .fetch_one(pool)
    .await?;

    if booked >= template.capacity {
        bail!("Class is full ({}/{})", booked, template.capacity);
    }

    let id = sqlx::query_scalar(
        "INSERT INTO bookings (template_id, date, user_id, created_by)
         VALUES (?1, ?2, ?3, ?4) RETURNING id"
    )
    .bind(template_id)
    .bind(date)
    .bind(user_id)
    .bind(created_by)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

pub async fn cancel_booking(pool: &SqlitePool, booking_id: i64) -> Result<()> {
    sqlx::query("UPDATE bookings SET cancelled_at = datetime('now') WHERE id = ?1 AND cancelled_at IS NULL")
        .bind(booking_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_booking_count(pool: &SqlitePool, template_id: i64, date: &str) -> Result<i32> {
    let count: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL"
    )
    .bind(template_id)
    .bind(date)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn list_bookings_for_class(pool: &SqlitePool, template_id: i64, date: &str) -> Result<Vec<BookingRow>> {
    let rows = sqlx::query_as::<_, BookingRow>(
        "SELECT * FROM bookings WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL ORDER BY created_at"
    )
    .bind(template_id)
    .bind(date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_user_bookings(pool: &SqlitePool, user_id: i64) -> Result<Vec<BookingRow>> {
    let rows = sqlx::query_as::<_, BookingRow>(
        "SELECT * FROM bookings WHERE user_id = ?1 AND cancelled_at IS NULL AND date >= date('now') ORDER BY date, template_id"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> (SqlitePool, i64, i64) {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let instructor_id: i64 = sqlx::query_scalar(
            "INSERT INTO instructors (name) VALUES ('Judita') RETURNING id"
        ).fetch_one(&pool).await.unwrap();

        let user_id = crate::db::users::create_user(
            &pool, "rider@test.com", Some("h"), "Rider", None, "customer", None, None
        ).await.unwrap();

        (pool, instructor_id, user_id)
    }

    #[tokio::test]
    async fn test_create_template_and_booking() {
        let (pool, instructor_id, user_id) = setup().await;

        let template_id = create_template(&pool, 0, "17:00", 60, instructor_id, 10).await.unwrap();
        let booking_id = create_booking(&pool, template_id, "2026-04-14", user_id, user_id).await.unwrap();

        let count = get_booking_count(&pool, template_id, "2026-04-14").await.unwrap();
        assert_eq!(count, 1);

        let bookings = list_bookings_for_class(&pool, template_id, "2026-04-14").await.unwrap();
        assert_eq!(bookings.len(), 1);
        assert_eq!(bookings[0].user_id, user_id);
    }

    #[tokio::test]
    async fn test_capacity_enforcement() {
        let (pool, instructor_id, _) = setup().await;

        let template_id = create_template(&pool, 0, "17:00", 60, instructor_id, 2).await.unwrap();

        // Create 2 users and fill capacity
        let u1 = crate::db::users::create_user(&pool, "a@t.com", Some("h"), "A", None, "customer", None, None).await.unwrap();
        let u2 = crate::db::users::create_user(&pool, "b@t.com", Some("h"), "B", None, "customer", None, None).await.unwrap();
        let u3 = crate::db::users::create_user(&pool, "c@t.com", Some("h"), "C", None, "customer", None, None).await.unwrap();

        create_booking(&pool, template_id, "2026-04-14", u1, u1).await.unwrap();
        create_booking(&pool, template_id, "2026-04-14", u2, u2).await.unwrap();

        // Third booking should fail
        let result = create_booking(&pool, template_id, "2026-04-14", u3, u3).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("full"));
    }

    #[tokio::test]
    async fn test_cancel_booking_frees_spot() {
        let (pool, instructor_id, user_id) = setup().await;

        let template_id = create_template(&pool, 0, "17:00", 60, instructor_id, 1).await.unwrap();
        let booking_id = create_booking(&pool, template_id, "2026-04-14", user_id, user_id).await.unwrap();

        cancel_booking(&pool, booking_id).await.unwrap();

        let count = get_booking_count(&pool, template_id, "2026-04-14").await.unwrap();
        assert_eq!(count, 0);

        // Now another user can book
        let u2 = crate::db::users::create_user(&pool, "new@t.com", Some("h"), "New", None, "customer", None, None).await.unwrap();
        create_booking(&pool, template_id, "2026-04-14", u2, u2).await.unwrap();
    }

    #[tokio::test]
    async fn test_cancel_occurrence() {
        let (pool, instructor_id, _) = setup().await;
        let admin_id = crate::db::users::create_user(&pool, "admin@t.com", Some("h"), "Admin", None, "admin", None, None).await.unwrap();

        let template_id = create_template(&pool, 0, "17:00", 60, instructor_id, 10).await.unwrap();

        assert!(!is_occurrence_cancelled(&pool, template_id, "2026-04-14").await.unwrap());

        cancel_occurrence(&pool, template_id, "2026-04-14", Some("Instructor sick"), admin_id).await.unwrap();

        assert!(is_occurrence_cancelled(&pool, template_id, "2026-04-14").await.unwrap());
        // Other dates unaffected
        assert!(!is_occurrence_cancelled(&pool, template_id, "2026-04-21").await.unwrap());
    }

    #[tokio::test]
    async fn test_duplicate_booking_rejected() {
        let (pool, instructor_id, user_id) = setup().await;

        let template_id = create_template(&pool, 0, "17:00", 60, instructor_id, 10).await.unwrap();
        create_booking(&pool, template_id, "2026-04-14", user_id, user_id).await.unwrap();

        // Same user, same class, same date — should fail (unique index)
        let result = create_booking(&pool, template_id, "2026-04-14", user_id, user_id).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 4: Write transaction and settings queries**

Create `crates/spinbike-server/src/db/transactions.rs`:
```rust
use anyhow::Result;
use sqlx::SqlitePool;

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
}

pub async fn create_transaction(
    pool: &SqlitePool,
    user_id: Option<i64>,
    card_id: Option<i64>,
    staff_id: Option<i64>,
    service_id: Option<i64>,
    amount: f64,
    action: &str,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id"
    )
    .bind(user_id)
    .bind(card_id)
    .bind(staff_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_transactions_for_card(pool: &SqlitePool, card_id: i64) -> Result<Vec<TransactionRow>> {
    let rows = sqlx::query_as::<_, TransactionRow>(
        "SELECT * FROM transactions WHERE card_id = ?1 ORDER BY created_at DESC"
    )
    .bind(card_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_transactions_for_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<TransactionRow>> {
    let rows = sqlx::query_as::<_, TransactionRow>(
        "SELECT * FROM transactions WHERE user_id = ?1 ORDER BY created_at DESC"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    #[tokio::test]
    async fn test_create_and_list_transactions() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let user_id = crate::db::users::create_user(&pool, "u@t.com", Some("h"), "U", None, "customer", None, None).await.unwrap();
        let card_id = crate::db::cards::create_card(&pool, "70701001", 100.0).await.unwrap();

        create_transaction(&pool, Some(user_id), Some(card_id), None, Some(1), -5.0, "debit").await.unwrap();
        create_transaction(&pool, Some(user_id), Some(card_id), None, Some(1), -5.0, "debit").await.unwrap();

        let txns = list_transactions_for_card(&pool, card_id).await.unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].action, "debit");
    }
}
```

Create `crates/spinbike-server/src/db/settings.rs`:
```rust
use anyhow::Result;
use sqlx::SqlitePool;

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = ?1"
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(value)
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value"
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_bike_count(pool: &SqlitePool) -> Result<i32> {
    let val = get_setting(pool, "bike_count").await?;
    Ok(val.and_then(|v| v.parse().ok()).unwrap_or(10))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    #[tokio::test]
    async fn test_settings_seeded() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let name = get_setting(&pool, "center_name").await.unwrap().unwrap();
        assert_eq!(name, "Squash Centrum Smizany");

        let bikes = get_bike_count(&pool).await.unwrap();
        assert_eq!(bikes, 10);
    }

    #[tokio::test]
    async fn test_upsert_setting() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        set_setting(&pool, "bike_count", "15").await.unwrap();
        let bikes = get_bike_count(&pool).await.unwrap();
        assert_eq!(bikes, 15);
    }
}
```

- [ ] **Step 5: Run all tests**

```bash
cargo test -p spinbike-server
```

Expected: all tests pass (migrations + users + cards + classes + transactions + settings).

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/db/
git commit -m "feat: add database query modules with full test coverage

Users, cards, classes/bookings, transactions, and settings CRUD.
Capacity enforcement, booking deduplication, class cancellation."
```

---

### Task 4: Auth Module — JWT, Password Hashing, Middleware

**Files:**
- Create: `crates/spinbike-server/src/auth/mod.rs`
- Create: `crates/spinbike-server/src/auth/oauth.rs`
- Modify: `crates/spinbike-server/src/lib.rs`

- [ ] **Step 1: Write JWT and password hashing module with tests**

Create `crates/spinbike-server/src/auth/mod.rs`:
```rust
pub mod oauth;

use anyhow::Result;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::SaltString;
use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::rngs::OsRng;
use serde_json::json;
use spinbike_core::auth::{Claims, Role};
use crate::AppState;

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn create_token(secret: &str, user_id: i64, email: &str, role: Role) -> Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        email: email.to_string(),
        role,
        iat: now,
        exp: now + 86400 * 7, // 7 days
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn validate_token(secret: &str, token: &str) -> Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

/// Axum extractor that validates JWT from Authorization header
pub struct AuthUser(pub Claims);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        let token = match auth_header {
            Some(t) => t,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "Missing authorization token"})),
                ).into_response());
            }
        };

        match validate_token(&app_state.jwt_secret, token) {
            Ok(claims) => Ok(AuthUser(claims)),
            Err(_) => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid or expired token"})),
            ).into_response()),
        }
    }
}

use axum::extract::FromRef;

impl FromRef<AppState> for AppState {
    fn from_ref(state: &AppState) -> Self {
        state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hash_and_verify() {
        let hash = hash_password("mypassword").unwrap();
        assert!(verify_password("mypassword", &hash));
        assert!(!verify_password("wrongpassword", &hash));
    }

    #[test]
    fn test_jwt_create_and_validate() {
        let secret = "test-secret-key";
        let token = create_token(secret, 42, "user@test.com", Role::Customer).unwrap();
        let claims = validate_token(secret, &token).unwrap();
        assert_eq!(claims.sub, 42);
        assert_eq!(claims.email, "user@test.com");
        assert_eq!(claims.role, Role::Customer);
    }

    #[test]
    fn test_jwt_invalid_secret_fails() {
        let token = create_token("secret1", 1, "a@b.com", Role::Customer).unwrap();
        let result = validate_token("secret2", &token);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Create OAuth placeholder**

Create `crates/spinbike-server/src/auth/oauth.rs`:
```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OAuthCallback {
    pub code: String,
    pub state: Option<String>,
}

#[derive(Debug)]
pub struct OAuthUserInfo {
    pub provider: String,
    pub oauth_id: String,
    pub email: String,
    pub name: String,
}

// Google and Facebook OAuth implementations will be added
// when configuring the OAuth client IDs on the VPS.
// For now, email+password auth is the primary flow.
```

- [ ] **Step 3: Update lib.rs to include auth module**

Update `crates/spinbike-server/src/lib.rs`:
```rust
pub mod auth;
pub mod db;

use sqlx::SqlitePool;
use tokio::sync::broadcast;
use spinbike_core::ws::ServerMsg;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub event_tx: broadcast::Sender<ServerMsg>,
    pub jwt_secret: String,
}
```

- [ ] **Step 4: Run all tests**

```bash
cargo test -p spinbike-server
cargo test -p spinbike-core
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/auth/ crates/spinbike-server/src/lib.rs
git commit -m "feat: add auth module with JWT, argon2 password hashing, Axum extractor

Includes AuthUser extractor for protected routes, password hashing/verification,
token creation/validation. OAuth placeholder for Google/Facebook."
```

---

### Task 5: API Routes — Auth Endpoints

**Files:**
- Create: `crates/spinbike-server/src/routes/mod.rs`
- Create: `crates/spinbike-server/src/routes/auth.rs`
- Create: `crates/spinbike-server/src/routes/static_files.rs`

- [ ] **Step 1: Create route modules**

Create `crates/spinbike-server/src/routes/mod.rs`:
```rust
pub mod auth;
pub mod static_files;

use axum::Router;
use crate::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
}
```

Create `crates/spinbike-server/src/routes/auth.rs`:
```rust
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use spinbike_core::auth::Role;
use crate::auth::{create_token, hash_password, verify_password, AuthUser};
use crate::db::users;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", axum::routing::get(me))
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: String,
    pub phone: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role: Role,
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Check if email already exists
    if users::get_user_by_email(&state.pool, &req.email).await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?
        .is_some()
    {
        return Err((StatusCode::CONFLICT, Json(json!({"error": "Email already registered"}))));
    }

    let password_hash = hash_password(&req.password)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to hash password"}))))?;

    let user_id = users::create_user(
        &state.pool, &req.email, Some(&password_hash), &req.name, req.phone.as_deref(), "customer", None, None,
    )
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to create user"}))))?;

    let token = create_token(&state.jwt_secret, user_id, &req.email, Role::Customer)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to create token"}))))?;

    Ok(Json(AuthResponse {
        token,
        user: UserResponse { id: user_id, email: req.email, name: req.name, role: Role::Customer },
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user = users::get_user_by_email(&state.pool, &req.email)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid email or password"}))))?;

    let password_hash = user.password_hash.as_deref()
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "Use OAuth to log in"}))))?;

    if !verify_password(&req.password, password_hash) {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid email or password"}))));
    }

    let role: Role = serde_json::from_str(&format!("\"{}\"", user.role))
        .unwrap_or(Role::Customer);

    let token = create_token(&state.jwt_secret, user.id, &user.email, role)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to create token"}))))?;

    Ok(Json(AuthResponse {
        token,
        user: UserResponse { id: user.id, email: user.email, name: user.name, role },
    }))
}

async fn me(
    AuthUser(claims): AuthUser,
) -> Json<UserResponse> {
    Json(UserResponse {
        id: claims.sub,
        email: claims.email,
        name: String::new(), // Will be populated from DB in a future iteration
        role: claims.role,
    })
}
```

- [ ] **Step 2: Create static file serving**

Create `crates/spinbike-server/src/routes/static_files.rs`:
```rust
use axum::{
    body::Body,
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::Embed;
use crate::AppState;

#[derive(Embed)]
#[folder = "../../spinbike-ui/dist/"]
struct Assets;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(serve_index))
        .route("/assets/{*path}", get(serve_asset))
        .route("/{*path}", get(serve_spa_route))
}

async fn serve_index() -> impl IntoResponse {
    serve_embedded_file("index.html")
}

async fn serve_asset(Path(path): Path<String>) -> impl IntoResponse {
    serve_embedded_file(&format!("assets/{}", path))
}

async fn serve_spa_route(Path(path): Path<String>) -> Response {
    // If it has a file extension, serve the file directly
    if path.contains('.') {
        return serve_embedded_file(&path);
    }
    // Otherwise, serve index.html for SPA client-side routing
    serve_embedded_file("index.html")
}

fn serve_embedded_file(path: &str) -> Response {
    if let Some(file) = Assets::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache, must-revalidate"
        };
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, cache)
            .body(Body::from(file.data.into_owned()))
            .unwrap()
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not found"))
            .unwrap()
    }
}
```

- [ ] **Step 3: Update routes mod to include static files**

Update `crates/spinbike-server/src/routes/mod.rs`:
```rust
pub mod auth;
pub mod static_files;

use axum::Router;
use crate::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
}

pub fn all_routes() -> Router<AppState> {
    Router::new()
        .merge(api_routes())
        .merge(static_files::routes())
}
```

- [ ] **Step 4: Update lib.rs to include routes**

Update `crates/spinbike-server/src/lib.rs`:
```rust
pub mod auth;
pub mod db;
pub mod routes;

use anyhow::Result;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use spinbike_core::ws::ServerMsg;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub event_tx: broadcast::Sender<ServerMsg>,
    pub jwt_secret: String,
}

pub async fn start_server(pool: SqlitePool, port: u16, jwt_secret: String) -> Result<()> {
    let (event_tx, _) = broadcast::channel(256);
    let state = AppState { pool, event_tx, jwt_secret };

    let app = routes::all_routes()
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

- [ ] **Step 5: Update server binary**

Update `crates/spinbike-server/src/bin/server.rs`:
```rust
use std::path::PathBuf;
use spinbike_server::db;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("spinbike_server=info".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting SpinBike Server v{}", spinbike_core::VERSION);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let db_path = std::env::var("DATABASE_PATH")
        .unwrap_or_else(|_| "spinbike.db".to_string());

    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "dev-secret-change-in-production".to_string());

    let pool = db::create_pool(&PathBuf::from(&db_path)).await?;
    db::run_migrations(&pool).await?;

    spinbike_server::start_server(pool, port, jwt_secret).await?;

    Ok(())
}
```

- [ ] **Step 6: Create minimal UI dist for compilation**

```bash
mkdir -p spinbike-ui/dist
echo "<html><body>placeholder</body></html>" > spinbike-ui/dist/index.html
```

- [ ] **Step 7: Verify it compiles**

```bash
cargo check -p spinbike-server
```

- [ ] **Step 8: Commit**

```bash
git add crates/spinbike-server/ spinbike-ui/dist/index.html
git commit -m "feat: add API routes for auth (register, login, me) and static file serving

Axum server with JWT auth, argon2 passwords, rust-embed SPA serving.
Includes server binary entry point with env-based configuration."
```

---

### Task 6: API Routes — Classes, Bookings, Cards, Payments, Admin

**Files:**
- Create: `crates/spinbike-server/src/routes/classes.rs`
- Create: `crates/spinbike-server/src/routes/cards.rs`
- Create: `crates/spinbike-server/src/routes/payments.rs`
- Create: `crates/spinbike-server/src/routes/admin.rs`
- Modify: `crates/spinbike-server/src/routes/mod.rs`

- [ ] **Step 1: Create class/booking routes**

Create `crates/spinbike-server/src/routes/classes.rs`:
```rust
use axum::{extract::State, http::StatusCode, routing::{get, post, delete}, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::auth::AuthUser;
use crate::db::classes;
use crate::AppState;
use spinbike_core::ws::ServerMsg;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/classes", get(list_classes))
        .route("/api/bookings", post(create_booking))
        .route("/api/bookings/{id}", delete(cancel_booking))
        .route("/api/my/bookings", get(my_bookings))
}

#[derive(Deserialize)]
pub struct ClassesQuery {
    pub from: String,  // ISO date
    pub to: String,    // ISO date
}

#[derive(Serialize)]
pub struct ClassResponse {
    pub template_id: i64,
    pub date: String,
    pub weekday: i32,
    pub start_time: String,
    pub duration_minutes: i32,
    pub instructor_name: String,
    pub capacity: i32,
    pub booked: i32,
    pub cancelled: bool,
    pub user_booked: bool,
    pub user_booking_id: Option<i64>,
}

async fn list_classes(
    State(state): State<AppState>,
    auth: Option<AuthUser>,
    axum::extract::Query(query): axum::extract::Query<ClassesQuery>,
) -> Result<Json<Vec<ClassResponse>>, (StatusCode, Json<serde_json::Value>)> {
    let templates = classes::list_active_templates(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;

    let user_id = auth.map(|a| a.0.sub);

    let from = chrono::NaiveDate::parse_from_str(&query.from, "%Y-%m-%d")
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid from date"}))))?;
    let to = chrono::NaiveDate::parse_from_str(&query.to, "%Y-%m-%d")
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid to date"}))))?;

    let mut results = Vec::new();

    let mut current = from;
    while current <= to {
        let weekday = current.weekday().num_days_from_monday() as i32;

        for template in &templates {
            if template.weekday != weekday {
                continue;
            }

            let date_str = current.format("%Y-%m-%d").to_string();

            let cancelled = classes::is_occurrence_cancelled(&state.pool, template.id, &date_str)
                .await
                .unwrap_or(false);

            let booked = classes::get_booking_count(&state.pool, template.id, &date_str)
                .await
                .unwrap_or(0);

            let (user_booked, user_booking_id) = if let Some(uid) = user_id {
                let bookings = classes::list_bookings_for_class(&state.pool, template.id, &date_str)
                    .await
                    .unwrap_or_default();
                let user_booking = bookings.iter().find(|b| b.user_id == uid);
                (user_booking.is_some(), user_booking.map(|b| b.id))
            } else {
                (false, None)
            };

            let instructor_name = sqlx::query_scalar::<_, String>(
                "SELECT name FROM instructors WHERE id = ?1"
            )
            .bind(template.instructor_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "Unknown".to_string());

            results.push(ClassResponse {
                template_id: template.id,
                date: date_str,
                weekday: template.weekday,
                start_time: template.start_time.clone(),
                duration_minutes: template.duration_minutes,
                instructor_name,
                capacity: template.capacity,
                booked,
                cancelled,
                user_booked,
                user_booking_id,
            });
        }

        current += chrono::Duration::days(1);
    }

    Ok(Json(results))
}

#[derive(Deserialize)]
pub struct CreateBookingRequest {
    pub template_id: i64,
    pub date: String,
    pub user_id: Option<i64>, // Staff can book for others
}

async fn create_booking(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<CreateBookingRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let target_user = if let Some(uid) = req.user_id {
        if !claims.role.can_book_for_others() {
            return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Cannot book for others"}))));
        }
        uid
    } else {
        claims.sub
    };

    // Check if class is cancelled
    if classes::is_occurrence_cancelled(&state.pool, req.template_id, &req.date)
        .await
        .unwrap_or(false)
    {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Class is cancelled"}))));
    }

    let booking_id = classes::create_booking(&state.pool, req.template_id, &req.date, target_user, claims.sub)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))))?;

    // Broadcast booking update
    let booked = classes::get_booking_count(&state.pool, req.template_id, &req.date).await.unwrap_or(0);
    let template = classes::list_active_templates(&state.pool).await.unwrap_or_default();
    let capacity = template.iter().find(|t| t.id == req.template_id).map(|t| t.capacity).unwrap_or(0);

    let _ = state.event_tx.send(ServerMsg::BookingUpdate {
        template_id: req.template_id,
        date: req.date,
        booked,
        capacity,
    });

    Ok(Json(json!({"id": booking_id})))
}

async fn cancel_booking(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    axum::extract::Path(booking_id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Fetch the booking to check ownership
    let booking = sqlx::query_as::<_, classes::BookingRow>(
        "SELECT * FROM bookings WHERE id = ?1 AND cancelled_at IS NULL"
    )
    .bind(booking_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Booking not found"}))))?;

    if booking.user_id != claims.sub && !claims.role.can_cancel_any_booking() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Cannot cancel this booking"}))));
    }

    classes::cancel_booking(&state.pool, booking_id)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to cancel"}))))?;

    // Broadcast update
    let booked = classes::get_booking_count(&state.pool, booking.template_id, &booking.date).await.unwrap_or(0);
    let templates = classes::list_active_templates(&state.pool).await.unwrap_or_default();
    let capacity = templates.iter().find(|t| t.id == booking.template_id).map(|t| t.capacity).unwrap_or(0);

    let _ = state.event_tx.send(ServerMsg::BookingUpdate {
        template_id: booking.template_id,
        date: booking.date,
        booked,
        capacity,
    });

    Ok(Json(json!({"ok": true})))
}

async fn my_bookings(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<classes::BookingRow>>, (StatusCode, Json<serde_json::Value>)> {
    let bookings = classes::list_user_bookings(&state.pool, claims.sub)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;
    Ok(Json(bookings))
}
```

- [ ] **Step 2: Create card routes**

Create `crates/spinbike-server/src/routes/cards.rs`:
```rust
use axum::{extract::State, http::StatusCode, routing::{get, post}, Json, Router};
use serde::Deserialize;
use serde_json::json;
use crate::auth::AuthUser;
use crate::db::{cards, transactions};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards/link", post(link_card))
        .route("/api/cards/lookup/{barcode}", get(lookup_card))
        .route("/api/cards/activate", post(activate_card))
        .route("/api/cards/topup", post(topup_card))
        .route("/api/cards/block", post(block_card))
        .route("/api/my/balance", get(my_balance))
}

#[derive(Deserialize)]
pub struct LinkCardRequest {
    pub barcode: String,
}

async fn link_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<LinkCardRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let card = cards::get_card_by_barcode(&state.pool, &req.barcode)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Card not found"}))))?;

    if card.user_id.is_some() {
        return Err((StatusCode::CONFLICT, Json(json!({"error": "Card already linked to an account"}))));
    }

    cards::link_card_to_user(&state.pool, card.id, claims.sub)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to link card"}))))?;

    Ok(Json(json!({"ok": true, "credit": card.credit})))
}

async fn lookup_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    axum::extract::Path(barcode): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Staff only"}))));
    }

    let card = cards::get_card_by_barcode(&state.pool, &barcode)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;

    match card {
        Some(c) => Ok(Json(json!({
            "found": true,
            "id": c.id,
            "barcode": c.barcode,
            "credit": c.credit,
            "blocked": c.blocked,
            "allow_debit": c.allow_debit,
            "user_id": c.user_id,
        }))),
        None => Ok(Json(json!({"found": false}))),
    }
}

#[derive(Deserialize)]
pub struct ActivateCardRequest {
    pub barcode: String,
    pub credit: f64,
}

async fn activate_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<ActivateCardRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Staff only"}))));
    }

    if cards::get_card_by_barcode(&state.pool, &req.barcode).await.ok().flatten().is_some() {
        return Err((StatusCode::CONFLICT, Json(json!({"error": "Card already exists"}))));
    }

    let card_id = cards::create_card(&state.pool, &req.barcode, req.credit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    transactions::create_transaction(&state.pool, None, Some(card_id), Some(claims.sub), None, req.credit, "activation")
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to log transaction"}))))?;

    Ok(Json(json!({"id": card_id})))
}

#[derive(Deserialize)]
pub struct TopupRequest {
    pub card_id: i64,
    pub amount: f64,
}

async fn topup_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<TopupRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Staff only"}))));
    }

    let new_credit = cards::update_credit(&state.pool, req.card_id, req.amount)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to update credit"}))))?;

    transactions::create_transaction(&state.pool, None, Some(req.card_id), Some(claims.sub), None, req.amount, "credit")
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to log transaction"}))))?;

    Ok(Json(json!({"credit": new_credit})))
}

#[derive(Deserialize)]
pub struct BlockCardRequest {
    pub card_id: i64,
    pub blocked: bool,
}

async fn block_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<BlockCardRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Staff only"}))));
    }

    cards::set_blocked(&state.pool, req.card_id, req.blocked)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to update card"}))))?;

    Ok(Json(json!({"ok": true})))
}

async fn my_balance(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let card = cards::get_card_by_user(&state.pool, claims.sub)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;

    match card {
        Some(c) => {
            let txns = transactions::list_transactions_for_card(&state.pool, c.id)
                .await
                .unwrap_or_default();
            Ok(Json(json!({
                "card_linked": true,
                "credit": c.credit,
                "barcode": c.barcode,
                "transactions": txns,
            })))
        }
        None => Ok(Json(json!({"card_linked": false}))),
    }
}
```

- [ ] **Step 3: Create payment routes**

Create `crates/spinbike-server/src/routes/payments.rs`:
```rust
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::json;
use crate::auth::AuthUser;
use crate::db::{cards, transactions};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/payments/charge", post(charge))
        .route("/api/payments/storno", post(storno))
}

#[derive(Deserialize)]
pub struct ChargeRequest {
    pub card_id: i64,
    pub service_id: i64,
    pub amount: f64,
}

async fn charge(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<ChargeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Staff only"}))));
    }

    let card = cards::get_card_by_barcode(&state.pool, "")
        .await
        .ok()
        .flatten();

    // Get card by ID instead
    let card = sqlx::query_as::<_, cards::CardRow>("SELECT * FROM cards WHERE id = ?1")
        .bind(req.card_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Card not found"}))))?;

    if card.blocked {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Card is blocked"}))));
    }

    if card.credit < req.amount && !card.allow_debit {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Insufficient credit", "credit": card.credit}))));
    }

    let new_credit = cards::update_credit(&state.pool, req.card_id, -req.amount)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to charge"}))))?;

    transactions::create_transaction(
        &state.pool, card.user_id, Some(req.card_id), Some(claims.sub), Some(req.service_id), -req.amount, "debit",
    )
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to log transaction"}))))?;

    Ok(Json(json!({"credit": new_credit})))
}

async fn storno(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<ChargeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Staff only"}))));
    }

    let new_credit = cards::update_credit(&state.pool, req.card_id, req.amount)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to refund"}))))?;

    transactions::create_transaction(
        &state.pool, None, Some(req.card_id), Some(claims.sub), Some(req.service_id), req.amount, "storno",
    )
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to log transaction"}))))?;

    Ok(Json(json!({"credit": new_credit})))
}
```

- [ ] **Step 4: Create admin routes**

Create `crates/spinbike-server/src/routes/admin.rs`:
```rust
use axum::{extract::State, http::StatusCode, routing::{get, post, put, delete}, Json, Router};
use serde::Deserialize;
use serde_json::json;
use crate::auth::AuthUser;
use crate::db::{classes, settings, users};
use crate::AppState;
use spinbike_core::ws::ServerMsg;

pub fn routes() -> Router<AppState> {
    Router::new()
        // Class templates
        .route("/api/admin/templates", get(list_templates).post(create_template))
        .route("/api/admin/templates/{id}", delete(deactivate_template))
        .route("/api/admin/cancel-class", post(cancel_class))
        // Instructors
        .route("/api/admin/instructors", get(list_instructors).post(create_instructor))
        // Services
        .route("/api/admin/services", get(list_services).post(create_service))
        // Settings
        .route("/api/admin/settings", get(get_settings).put(update_setting))
        // Users
        .route("/api/admin/users", get(list_users))
        .route("/api/admin/users/{id}/role", put(update_role))
}

fn require_admin(claims: &spinbike_core::auth::Claims) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_templates() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Admin only"}))));
    }
    Ok(())
}

fn require_staff(claims: &spinbike_core::auth::Claims) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Staff only"}))));
    }
    Ok(())
}

// --- Templates ---

async fn list_templates(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<classes::ClassTemplateRow>>, (StatusCode, Json<serde_json::Value>)> {
    require_staff(&claims)?;
    let templates = classes::list_active_templates(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;
    Ok(Json(templates))
}

#[derive(Deserialize)]
pub struct CreateTemplateRequest {
    pub weekday: i32,
    pub start_time: String,
    pub duration_minutes: i32,
    pub instructor_id: i64,
    pub capacity: i32,
}

async fn create_template(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<CreateTemplateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let id = classes::create_template(&state.pool, req.weekday, &req.start_time, req.duration_minutes, req.instructor_id, req.capacity)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({"id": id})))
}

async fn deactivate_template(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    sqlx::query("UPDATE class_templates SET active = 0 WHERE id = ?1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct CancelClassRequest {
    pub template_id: i64,
    pub date: String,
    pub reason: Option<String>,
}

async fn cancel_class(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<CancelClassRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_staff(&claims)?;
    classes::cancel_occurrence(&state.pool, req.template_id, &req.date, req.reason.as_deref(), claims.sub)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let _ = state.event_tx.send(ServerMsg::ClassCancelled {
        template_id: req.template_id,
        date: req.date,
    });

    Ok(Json(json!({"ok": true})))
}

// --- Instructors ---

async fn list_instructors(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (i64, String, bool)>(
        "SELECT id, name, active FROM instructors ORDER BY name"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;

    let instructors: Vec<_> = rows.iter().map(|(id, name, active)| {
        json!({"id": id, "name": name, "active": active})
    }).collect();

    Ok(Json(instructors))
}

#[derive(Deserialize)]
pub struct CreateInstructorRequest {
    pub name: String,
}

async fn create_instructor(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<CreateInstructorRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let id: i64 = sqlx::query_scalar("INSERT INTO instructors (name) VALUES (?1) RETURNING id")
        .bind(&req.name)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;
    Ok(Json(json!({"id": id})))
}

// --- Services ---

async fn list_services(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (i64, String, f64, bool)>(
        "SELECT id, name, default_price, active FROM services ORDER BY name"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;

    let services: Vec<_> = rows.iter().map(|(id, name, price, active)| {
        json!({"id": id, "name": name, "default_price": price, "active": active})
    }).collect();

    Ok(Json(services))
}

#[derive(Deserialize)]
pub struct CreateServiceRequest {
    pub name: String,
    pub default_price: f64,
}

async fn create_service(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<CreateServiceRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO services (name, default_price) VALUES (?1, ?2) RETURNING id"
    )
    .bind(&req.name)
    .bind(req.default_price)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;
    Ok(Json(json!({"id": id})))
}

// --- Settings ---

async fn get_settings(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let rows = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings ORDER BY key")
        .fetch_all(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;

    let settings: serde_json::Map<String, serde_json::Value> = rows.into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect();

    Ok(Json(json!(settings)))
}

#[derive(Deserialize)]
pub struct UpdateSettingRequest {
    pub key: String,
    pub value: String,
}

async fn update_setting(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<UpdateSettingRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    settings::set_setting(&state.pool, &req.key, &req.value)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;
    Ok(Json(json!({"ok": true})))
}

// --- Users ---

async fn list_users(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let rows = users::list_users(&state.pool)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;

    let users: Vec<_> = rows.iter().map(|u| {
        json!({"id": u.id, "email": u.email, "name": u.name, "role": u.role, "created_at": u.created_at})
    }).collect();

    Ok(Json(users))
}

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

async fn update_role(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    axum::extract::Path(user_id): axum::extract::Path<i64>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    users::update_user_role(&state.pool, user_id, &req.role)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"}))))?;
    Ok(Json(json!({"ok": true})))
}
```

- [ ] **Step 5: Update routes mod to include all routes**

Update `crates/spinbike-server/src/routes/mod.rs`:
```rust
pub mod admin;
pub mod auth;
pub mod cards;
pub mod classes;
pub mod payments;
pub mod static_files;

use axum::Router;
use crate::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(classes::routes())
        .merge(cards::routes())
        .merge(payments::routes())
        .merge(admin::routes())
}

pub fn all_routes() -> Router<AppState> {
    Router::new()
        .merge(api_routes())
        .merge(static_files::routes())
}
```

- [ ] **Step 6: Verify it compiles**

```bash
cargo check -p spinbike-server
```

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-server/src/routes/
git commit -m "feat: add all API routes — classes, bookings, cards, payments, admin

Full REST API with role-based access control. WebSocket broadcast
on booking changes and class cancellations."
```

---

### Task 7: WebSocket Handler

**Files:**
- Create: `crates/spinbike-server/src/ws.rs`
- Modify: `crates/spinbike-server/src/routes/mod.rs`
- Modify: `crates/spinbike-server/src/lib.rs`

- [ ] **Step 1: Create WebSocket handler**

Create `crates/spinbike-server/src/ws.rs`:
```rust
use axum::{
    extract::{State, ws::{Message, WebSocket, WebSocketUpgrade}},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use crate::AppState;
use spinbike_core::ws::{ClientMsg, ServerMsg};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_tx.subscribe();

    tracing::info!("WebSocket client connected");

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(_) => continue,
            };
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMsg>(&text) {
                        match client_msg {
                            ClientMsg::Ping => {
                                // Pong is handled by broadcast, but we could respond directly
                            }
                            ClientMsg::SubscribeSchedule { .. } => {
                                // Future: per-date subscriptions for efficiency
                            }
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // When either task finishes, abort the other
    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    tracing::info!("WebSocket client disconnected");
}
```

- [ ] **Step 2: Add WebSocket route**

Update `crates/spinbike-server/src/routes/mod.rs` — add to `api_routes()`:
```rust
pub mod admin;
pub mod auth;
pub mod cards;
pub mod classes;
pub mod payments;
pub mod static_files;

use axum::{routing::get, Router};
use crate::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(classes::routes())
        .merge(cards::routes())
        .merge(payments::routes())
        .merge(admin::routes())
        .route("/api/ws", get(crate::ws::ws_handler))
}

pub fn all_routes() -> Router<AppState> {
    Router::new()
        .merge(api_routes())
        .merge(static_files::routes())
}
```

- [ ] **Step 3: Update lib.rs to include ws module**

Update `crates/spinbike-server/src/lib.rs` — add `pub mod ws;` after `pub mod routes;`.

- [ ] **Step 4: Verify it compiles**

```bash
cargo check -p spinbike-server
```

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/ws.rs crates/spinbike-server/src/routes/mod.rs crates/spinbike-server/src/lib.rs
git commit -m "feat: add WebSocket handler for live booking updates

Broadcasts BookingUpdate and ClassCancelled events to all connected clients."
```

---

### Task 8: Leptos UI — Project Setup, Router, Auth Pages

**Files:**
- Create: `spinbike-ui/Cargo.toml`
- Create: `spinbike-ui/Trunk.toml`
- Create: `spinbike-ui/index.html`
- Create: `spinbike-ui/style.css`
- Create: `spinbike-ui/manifest.json`
- Create: `spinbike-ui/sw.js`
- Create: `spinbike-ui/src/lib.rs`
- Create: `spinbike-ui/src/api.rs`
- Create: `spinbike-ui/src/auth.rs`
- Create: `spinbike-ui/src/ws.rs`
- Create: `spinbike-ui/src/router.rs`
- Create: `spinbike-ui/src/pages/mod.rs`
- Create: `spinbike-ui/src/pages/login.rs`
- Create: `spinbike-ui/src/components/mod.rs`
- Create: `spinbike-ui/src/components/nav.rs`

This is a large task. The subagent should create all files, build with Trunk, and verify the WASM output exists.

- [ ] **Step 1: Create Cargo.toml**

Create `spinbike-ui/Cargo.toml`:
```toml
[package]
name = "spinbike-ui"
version = "0.1.0"
edition = "2024"
license = "MIT"

[workspace]

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
spinbike-core = { path = "../crates/spinbike-core" }
leptos = { version = "0.7", features = ["csr"] }
leptos_router = "0.7"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
console_error_panic_hook = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
gloo-net = { version = "0.6", features = ["http", "websocket"] }
gloo-timers = "0.3"
gloo-utils = "0.2"
web-sys = { version = "0.3", features = ["console", "Location", "Window", "Storage", "HtmlInputElement"] }
js-sys = "0.3"
futures = "0.3"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
```

- [ ] **Step 2: Create Trunk.toml and index.html**

Create `spinbike-ui/Trunk.toml`:
```toml
[build]
target = "index.html"
dist = "dist"

[watch]
watch = ["src", "index.html", "style.css"]
ignore = ["./target", "./dist"]

[serve]
address = "127.0.0.1"
port = 8081
open = false
```

Create `spinbike-ui/index.html`:
```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
    <title>SpinBike</title>
    <link data-trunk rel="rust" data-wasm-opt="z" />
    <link data-trunk rel="css" href="style.css" />
    <link data-trunk rel="copy-file" href="manifest.json" />
    <link data-trunk rel="copy-file" href="sw.js" />
    <link rel="manifest" href="/manifest.json" />
    <link rel="icon" type="image/svg+xml" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><text y='.9em' font-size='90'>🚴</text></svg>" />
    <meta name="theme-color" content="#0f172a" />
    <meta name="apple-mobile-web-app-capable" content="yes" />
</head>
<body>
    <div id="app-shell" style="display:flex;align-items:center;justify-content:center;min-height:100vh;background:#0f172a;color:#94a3b8;font-family:system-ui;">
        Loading SpinBike...
    </div>
    <script>
    if ('serviceWorker' in navigator) {
        navigator.serviceWorker.register('/sw.js');
    }
    </script>
</body>
</html>
```

- [ ] **Step 3: Create PWA manifest and service worker**

Create `spinbike-ui/manifest.json`:
```json
{
    "name": "SpinBike",
    "short_name": "SpinBike",
    "start_url": "/",
    "display": "standalone",
    "background_color": "#0f172a",
    "theme_color": "#0f172a",
    "description": "Spin bike class booking and management"
}
```

Create `spinbike-ui/sw.js`:
```javascript
const CACHE_NAME = 'spinbike-v1';

self.addEventListener('install', (event) => {
    self.skipWaiting();
});

self.addEventListener('activate', (event) => {
    event.waitUntil(
        caches.keys().then((names) =>
            Promise.all(names.filter((n) => n !== CACHE_NAME).map((n) => caches.delete(n)))
        )
    );
});

self.addEventListener('fetch', (event) => {
    if (event.request.url.includes('/api/')) return;
    event.respondWith(
        caches.match(event.request).then((cached) =>
            cached || fetch(event.request).then((response) => {
                if (response.ok && event.request.method === 'GET') {
                    const clone = response.clone();
                    caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
                }
                return response;
            }).catch(() => cached)
        )
    );
});
```

- [ ] **Step 4: Create minimal CSS**

Create `spinbike-ui/style.css`:
```css
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
    --bg: #0f172a;
    --surface: #1e293b;
    --border: #334155;
    --text: #e2e8f0;
    --text-muted: #94a3b8;
    --primary: #22c55e;
    --primary-dark: #16a34a;
    --danger: #ef4444;
    --info: #3b82f6;
    --warning: #f59e0b;
}

body {
    font-family: system-ui, -apple-system, sans-serif;
    background: var(--bg);
    color: var(--text);
    min-height: 100vh;
}

a { color: var(--primary); text-decoration: none; }

.container { max-width: 600px; margin: 0 auto; padding: 16px; }

/* Navigation */
.nav { display: flex; justify-content: space-between; align-items: center; padding: 12px 16px; background: var(--surface); border-bottom: 1px solid var(--border); }
.nav-brand { font-weight: 700; font-size: 1.1rem; }
.nav-links { display: flex; gap: 12px; }
.nav-link { color: var(--text-muted); font-size: 0.9rem; padding: 4px 8px; border-radius: 4px; }
.nav-link:hover, .nav-link.active { color: var(--primary); }

/* Forms */
.form-group { margin-bottom: 16px; }
.form-label { display: block; font-size: 0.85rem; color: var(--text-muted); margin-bottom: 4px; }
.form-input { width: 100%; padding: 10px 12px; background: var(--bg); border: 1px solid var(--border); border-radius: 6px; color: var(--text); font-size: 1rem; }
.form-input:focus { outline: none; border-color: var(--primary); }

/* Buttons */
.btn { display: inline-flex; align-items: center; justify-content: center; padding: 10px 20px; border: none; border-radius: 6px; font-size: 0.9rem; font-weight: 600; cursor: pointer; transition: background 0.15s; }
.btn-primary { background: var(--primary); color: #000; }
.btn-primary:hover { background: var(--primary-dark); }
.btn-danger { background: var(--danger); color: #fff; }
.btn-ghost { background: transparent; color: var(--text-muted); border: 1px solid var(--border); }
.btn-sm { padding: 6px 12px; font-size: 0.8rem; }
.btn:disabled { opacity: 0.5; cursor: not-allowed; }
.btn-block { width: 100%; }

/* Class cards */
.class-card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 14px; margin-bottom: 8px; }
.class-card.available { border-color: var(--primary); }
.class-card.booked { border-color: var(--info); background: #172554; }
.class-card.full { opacity: 0.6; border-color: var(--danger); }
.class-card.cancelled { opacity: 0.4; text-decoration: line-through; }
.class-header { display: flex; justify-content: space-between; align-items: center; }
.class-time { font-weight: 700; font-size: 1.05rem; }
.class-instructor { font-size: 0.85rem; color: var(--text-muted); }
.class-status { font-size: 0.85rem; font-weight: 600; }
.class-participants { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 8px; padding-top: 8px; border-top: 1px solid var(--border); }
.participant-tag { background: var(--bg); padding: 2px 8px; border-radius: 12px; font-size: 0.75rem; color: var(--text-muted); }

/* Day picker */
.day-picker { display: flex; gap: 6px; margin-bottom: 16px; overflow-x: auto; }
.day-btn { padding: 6px 14px; border-radius: 20px; background: var(--surface); border: 1px solid var(--border); color: var(--text-muted); font-size: 0.85rem; cursor: pointer; white-space: nowrap; }
.day-btn.active { background: var(--primary); color: #000; border-color: var(--primary); font-weight: 600; }

/* Alerts */
.alert { padding: 10px 14px; border-radius: 6px; margin-bottom: 12px; font-size: 0.9rem; }
.alert-error { background: #451a1a; border: 1px solid var(--danger); color: #fca5a5; }
.alert-success { background: #14532d; border: 1px solid var(--primary); color: #86efac; }

/* Page title */
.page-title { font-size: 1.3rem; font-weight: 700; margin-bottom: 16px; }

/* Utility */
.text-muted { color: var(--text-muted); }
.text-sm { font-size: 0.85rem; }
.mt-2 { margin-top: 8px; }
.mt-4 { margin-top: 16px; }
.flex { display: flex; }
.gap-2 { gap: 8px; }
.items-center { align-items: center; }
.justify-between { justify-content: space-between; }
```

- [ ] **Step 5: Create Leptos app entry point and auth state**

Create `spinbike-ui/src/lib.rs`:
```rust
pub mod api;
pub mod auth;
pub mod components;
pub mod pages;
pub mod router;
pub mod ws;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(router::App);

    if let Some(shell) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("app-shell"))
    {
        shell.remove();
    }
}
```

Create `spinbike-ui/src/auth.rs`:
```rust
use leptos::prelude::*;
use spinbike_core::auth::Role;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub token: String,
    pub user_id: i64,
    pub email: String,
    pub name: String,
    pub role: Role,
}

pub fn get_auth() -> Option<AuthState> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let json = storage.get_item("auth").ok()??;
    serde_json::from_str(&json).ok()
}

pub fn set_auth(state: &AuthState) {
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let json = serde_json::to_string(state).unwrap_or_default();
        let _ = storage.set_item("auth", &json);
    }
}

pub fn clear_auth() {
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let _ = storage.remove_item("auth");
    }
}

pub fn get_token() -> Option<String> {
    get_auth().map(|a| a.token)
}

pub fn is_staff_or_admin() -> bool {
    get_auth()
        .map(|a| matches!(a.role, Role::Admin | Role::Staff))
        .unwrap_or(false)
}

pub fn is_admin() -> bool {
    get_auth()
        .map(|a| matches!(a.role, Role::Admin))
        .unwrap_or(false)
}
```

Create `spinbike-ui/src/api.rs`:
```rust
use gloo_net::http::Request;
use serde::de::DeserializeOwned;
use crate::auth::get_token;

pub async fn get<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let mut req = Request::get(url);
    if let Some(token) = get_token() {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(text);
    }
    resp.json().await.map_err(|e| e.to_string())
}

pub async fn post<T: DeserializeOwned>(url: &str, body: &impl serde::Serialize) -> Result<T, String> {
    let mut req = Request::post(url);
    if let Some(token) = get_token() {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let resp = req
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(body).unwrap())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(text);
    }
    resp.json().await.map_err(|e| e.to_string())
}

pub async fn delete<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let mut req = Request::delete(url);
    if let Some(token) = get_token() {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(text);
    }
    resp.json().await.map_err(|e| e.to_string())
}

pub async fn put<T: DeserializeOwned>(url: &str, body: &impl serde::Serialize) -> Result<T, String> {
    let mut req = Request::put(url);
    if let Some(token) = get_token() {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let resp = req
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(body).unwrap())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(text);
    }
    resp.json().await.map_err(|e| e.to_string())
}
```

Create `spinbike-ui/src/ws.rs`:
```rust
use gloo_net::websocket::Message;
use gloo_net::websocket::futures::WebSocket;
use gloo_timers::callback::Timeout;
use wasm_bindgen_futures::spawn_local;
use futures::StreamExt;
use spinbike_core::ws::ServerMsg;

pub fn ws_url() -> String {
    let location = gloo_utils::window().location();
    let protocol = location.protocol().unwrap_or_else(|_| "http:".into());
    let ws_proto = if protocol == "https:" { "wss:" } else { "ws:" };
    let host = location.host().unwrap_or_else(|_| "127.0.0.1:8080".into());
    format!("{ws_proto}//{host}/api/ws")
}

pub fn connect(on_message: impl Fn(ServerMsg) + 'static) {
    connect_with_backoff(on_message, 1000);
}

fn connect_with_backoff(on_message: impl Fn(ServerMsg) + 'static, delay_ms: u32) {
    let url = ws_url();
    let ws = match WebSocket::open(&url) {
        Ok(ws) => ws,
        Err(_) => {
            schedule_reconnect(on_message, delay_ms);
            return;
        }
    };

    let (_write, mut read) = ws.split();
    let on_message = std::rc::Rc::new(on_message);
    let on_msg_clone = on_message.clone();

    spawn_local(async move {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(server_msg) = serde_json::from_str::<ServerMsg>(&text) {
                        on_msg_clone(&server_msg);
                    }
                }
                Ok(Message::Bytes(_)) => {}
                Err(_) => break,
            }
        }
        // Disconnected — reconnect
        let on_msg_reconnect = on_message.clone();
        schedule_reconnect(move |msg| on_msg_reconnect(&msg), 1000);
    });
}

fn schedule_reconnect(on_message: impl Fn(ServerMsg) + 'static, delay_ms: u32) {
    let next_delay = (delay_ms * 2).min(30_000);
    let timeout = Timeout::new(delay_ms, move || {
        connect_with_backoff(on_message, next_delay);
    });
    timeout.forget();
}
```

Create `spinbike-ui/src/router.rs`:
```rust
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use crate::components::nav::Nav;
use crate::pages;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Nav />
            <div class="container">
                <Routes fallback=|| view! { <p>"Page not found"</p> }>
                    <Route path=path!("/") view=pages::schedule::SchedulePage />
                    <Route path=path!("/login") view=pages::login::LoginPage />
                    <Route path=path!("/register") view=pages::login::RegisterPage />
                    <Route path=path!("/my/bookings") view=pages::my_bookings::MyBookingsPage />
                    <Route path=path!("/my/balance") view=pages::my_balance::MyBalancePage />
                    <Route path=path!("/link-card") view=pages::link_card::LinkCardPage />
                    <Route path=path!("/staff") view=pages::staff_dashboard::StaffDashboardPage />
                    <Route path=path!("/staff/cards") view=pages::card_ops::CardOpsPage />
                    <Route path=path!("/staff/payments") view=pages::payments::PaymentsPage />
                    <Route path=path!("/admin") view=pages::admin::AdminPage />
                </Routes>
            </div>
        </Router>
    }
}
```

Create `spinbike-ui/src/components/mod.rs`:
```rust
pub mod nav;
pub mod class_card;
pub mod day_picker;
```

Create `spinbike-ui/src/components/nav.rs`:
```rust
use leptos::prelude::*;
use crate::auth;

#[component]
pub fn Nav() -> impl IntoView {
    let (logged_in, _) = signal(auth::get_auth().is_some());
    let (is_staff, _) = signal(auth::is_staff_or_admin());
    let (is_admin, _) = signal(auth::is_admin());

    view! {
        <nav class="nav">
            <a href="/" class="nav-brand">"SpinBike"</a>
            <div class="nav-links">
                <a href="/" class="nav-link">"Schedule"</a>
                <Show when=move || logged_in.get()>
                    <a href="/my/bookings" class="nav-link">"My Bookings"</a>
                    <a href="/my/balance" class="nav-link">"Balance"</a>
                </Show>
                <Show when=move || is_staff.get()>
                    <a href="/staff" class="nav-link">"Staff"</a>
                </Show>
                <Show when=move || is_admin.get()>
                    <a href="/admin" class="nav-link">"Admin"</a>
                </Show>
                <Show when=move || !logged_in.get()>
                    <a href="/login" class="nav-link">"Login"</a>
                </Show>
                <Show when=move || logged_in.get()>
                    <a href="/login" class="nav-link"
                        on:click=move |_| { auth::clear_auth(); }>"Logout"</a>
                </Show>
            </div>
        </nav>
    }
}
```

Create `spinbike-ui/src/pages/mod.rs`:
```rust
pub mod admin;
pub mod card_ops;
pub mod link_card;
pub mod login;
pub mod my_balance;
pub mod my_bookings;
pub mod payments;
pub mod schedule;
pub mod staff_dashboard;
```

Create `spinbike-ui/src/pages/login.rs`:
```rust
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;
use crate::{api, auth};

#[derive(Serialize)]
struct LoginBody { email: String, password: String }

#[derive(Serialize)]
struct RegisterBody { email: String, password: String, name: String, phone: Option<String> }

#[derive(Deserialize)]
struct AuthResponse {
    token: String,
    user: UserResponse,
}

#[derive(Deserialize)]
struct UserResponse {
    id: i64,
    email: String,
    name: String,
    role: spinbike_core::auth::Role,
}

#[component]
pub fn LoginPage() -> impl IntoView {
    let (error, set_error) = signal(String::new());
    let (email, set_email) = signal(String::new());
    let (password, set_password) = signal(String::new());

    let on_submit = move |_| {
        let email = email.get();
        let password = password.get();
        set_error.set(String::new());

        spawn_local(async move {
            let body = LoginBody { email: email.clone(), password };
            match api::post::<AuthResponse>("/api/auth/login", &body).await {
                Ok(resp) => {
                    auth::set_auth(&auth::AuthState {
                        token: resp.token,
                        user_id: resp.user.id,
                        email: resp.user.email,
                        name: resp.user.name,
                        role: resp.user.role,
                    });
                    let _ = web_sys::window().unwrap().location().set_href("/");
                }
                Err(e) => set_error.set(e),
            }
        });
    };

    view! {
        <div class="mt-4">
            <h1 class="page-title">"Login"</h1>
            <Show when=move || !error.get().is_empty()>
                <div class="alert alert-error">{move || error.get()}</div>
            </Show>
            <div class="form-group">
                <label class="form-label">"Email"</label>
                <input class="form-input" type="email"
                    on:input=move |ev| set_email.set(event_target_value(&ev))
                    prop:value=email />
            </div>
            <div class="form-group">
                <label class="form-label">"Password"</label>
                <input class="form-input" type="password"
                    on:input=move |ev| set_password.set(event_target_value(&ev))
                    prop:value=password />
            </div>
            <button class="btn btn-primary btn-block" on:click=on_submit>"Login"</button>
            <p class="text-muted text-sm mt-2">"Don't have an account? " <a href="/register">"Register"</a></p>
        </div>
    }
}

#[component]
pub fn RegisterPage() -> impl IntoView {
    let (error, set_error) = signal(String::new());
    let (email, set_email) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (name, set_name) = signal(String::new());

    let on_submit = move |_| {
        let email = email.get();
        let password = password.get();
        let name = name.get();
        set_error.set(String::new());

        spawn_local(async move {
            let body = RegisterBody { email: email.clone(), password, name: name.clone(), phone: None };
            match api::post::<AuthResponse>("/api/auth/register", &body).await {
                Ok(resp) => {
                    auth::set_auth(&auth::AuthState {
                        token: resp.token,
                        user_id: resp.user.id,
                        email: resp.user.email,
                        name: resp.user.name,
                        role: resp.user.role,
                    });
                    let _ = web_sys::window().unwrap().location().set_href("/");
                }
                Err(e) => set_error.set(e),
            }
        });
    };

    view! {
        <div class="mt-4">
            <h1 class="page-title">"Register"</h1>
            <Show when=move || !error.get().is_empty()>
                <div class="alert alert-error">{move || error.get()}</div>
            </Show>
            <div class="form-group">
                <label class="form-label">"Name"</label>
                <input class="form-input" type="text"
                    on:input=move |ev| set_name.set(event_target_value(&ev))
                    prop:value=name />
            </div>
            <div class="form-group">
                <label class="form-label">"Email"</label>
                <input class="form-input" type="email"
                    on:input=move |ev| set_email.set(event_target_value(&ev))
                    prop:value=email />
            </div>
            <div class="form-group">
                <label class="form-label">"Password"</label>
                <input class="form-input" type="password"
                    on:input=move |ev| set_password.set(event_target_value(&ev))
                    prop:value=password />
            </div>
            <button class="btn btn-primary btn-block" on:click=on_submit>"Register"</button>
            <p class="text-muted text-sm mt-2">"Already have an account? " <a href="/login">"Login"</a></p>
        </div>
    }
}
```

- [ ] **Step 6: Create placeholder pages**

Create placeholder files for each remaining page. Each should export a `#[component]` that returns a basic view with the page title. The subagent should create these files:

`spinbike-ui/src/pages/schedule.rs` — "Schedule" page title + "Coming soon" text
`spinbike-ui/src/pages/my_bookings.rs` — "My Bookings" page
`spinbike-ui/src/pages/my_balance.rs` — "My Balance" page
`spinbike-ui/src/pages/link_card.rs` — "Link Card" page
`spinbike-ui/src/pages/staff_dashboard.rs` — "Staff Dashboard" page
`spinbike-ui/src/pages/card_ops.rs` — "Card Operations" page
`spinbike-ui/src/pages/payments.rs` — "Payments" page
`spinbike-ui/src/pages/admin.rs` — "Admin" page

And placeholder component files:
`spinbike-ui/src/components/class_card.rs` — empty component
`spinbike-ui/src/components/day_picker.rs` — empty component

Each placeholder page follows this pattern:
```rust
use leptos::prelude::*;

#[component]
pub fn SchedulePage() -> impl IntoView {
    view! {
        <div class="mt-4">
            <h1 class="page-title">"Schedule"</h1>
            <p class="text-muted">"Coming soon"</p>
        </div>
    }
}
```

- [ ] **Step 7: Install Trunk and build WASM**

```bash
# Install trunk if not present
which trunk || cargo install trunk

# Add wasm target
rustup target add wasm32-unknown-unknown

# Build
cd spinbike-ui
trunk build
```

Expected: `spinbike-ui/dist/` contains `index.html` and `.wasm` file.

- [ ] **Step 8: Verify server compiles with real UI dist**

```bash
cd /home/newlevel/devel/spinbike
cargo check -p spinbike-server
```

- [ ] **Step 9: Commit**

```bash
git add spinbike-ui/
git commit -m "feat: add Leptos WASM frontend with PWA support

Login/register pages, router with role-based navigation, WebSocket
client, API helpers, CSS theme. Placeholder pages for all features."
```

---

### Task 9: UI — Schedule Page, Class Cards, Day Picker, Booking Flow

**Files:**
- Modify: `spinbike-ui/src/pages/schedule.rs`
- Modify: `spinbike-ui/src/components/class_card.rs`
- Modify: `spinbike-ui/src/components/day_picker.rs`

This is the core customer experience. The subagent should implement the full schedule view with functional booking — this is where the brainstorming mockup becomes real. The page fetches classes from `/api/classes?from=&to=`, renders class cards with booking buttons, and handles the book/cancel flow. The WebSocket client should update booking counts live.

The subagent should follow the mockup from the brainstorming session (slot cards with time, instructor, availability, BOOK/FULL/BOOKED states). Build it, then trunk build to verify.

- [ ] **Step 1: Implement DayPicker component**
- [ ] **Step 2: Implement ClassCard component**
- [ ] **Step 3: Implement SchedulePage with API calls and booking**
- [ ] **Step 4: Build with Trunk and verify**
- [ ] **Step 5: Commit**

---

### Task 10: UI — My Bookings, My Balance, Link Card Pages

**Files:**
- Modify: `spinbike-ui/src/pages/my_bookings.rs`
- Modify: `spinbike-ui/src/pages/my_balance.rs`
- Modify: `spinbike-ui/src/pages/link_card.rs`

Implement the customer self-service pages:
- **My Bookings**: fetch from `/api/my/bookings`, list upcoming with cancel buttons
- **My Balance**: fetch from `/api/my/balance`, show credit + transaction history table
- **Link Card**: barcode input field, POST to `/api/cards/link`

- [ ] **Step 1: Implement My Bookings page**
- [ ] **Step 2: Implement My Balance page**
- [ ] **Step 3: Implement Link Card page**
- [ ] **Step 4: Build with Trunk and verify**
- [ ] **Step 5: Commit**

---

### Task 11: UI — Staff Pages (Dashboard, Card Ops, Payments)

**Files:**
- Modify: `spinbike-ui/src/pages/staff_dashboard.rs`
- Modify: `spinbike-ui/src/pages/card_ops.rs`
- Modify: `spinbike-ui/src/pages/payments.rs`

Implement staff-facing pages:
- **Staff Dashboard**: same schedule view but with participant names, walk-in booking, class cancellation
- **Card Ops**: barcode lookup, activate new card, top up credit, block/unblock
- **Payments**: select customer card, select service, enter amount, charge/storno

- [ ] **Step 1: Implement Staff Dashboard**
- [ ] **Step 2: Implement Card Operations page**
- [ ] **Step 3: Implement Payments page**
- [ ] **Step 4: Build with Trunk and verify**
- [ ] **Step 5: Commit**

---

### Task 12: UI — Admin Page

**Files:**
- Modify: `spinbike-ui/src/pages/admin.rs`

Implement admin pages with tabs/sections:
- **Class Templates**: list, create, delete recurring weekly schedule
- **Instructors**: list, add, deactivate
- **Services**: list, add, edit
- **Users**: list, change roles
- **Settings**: view/edit key-value settings

- [ ] **Step 1: Implement Admin page with all sections**
- [ ] **Step 2: Build with Trunk and verify**
- [ ] **Step 3: Commit**

---

### Task 13: Data Migration Tool

**Files:**
- Create: `crates/spinbike-server/src/bin/migrate_legacy.rs`
- Modify: `crates/spinbike-server/Cargo.toml`

CLI tool that reads the legacy Access DB (`db.mdb`) using the CSV exports or mdbtools and imports into SQLite.

- [ ] **Step 1: Add binary to Cargo.toml**

Add to `crates/spinbike-server/Cargo.toml`:
```toml
[[bin]]
name = "migrate-legacy"
path = "src/bin/migrate_legacy.rs"
```

- [ ] **Step 2: Write migration tool**

The tool should:
1. Read card data from the Access DB (via mdbtools shell or pre-exported CSV)
2. Import cards into SQLite (barcode, credit EUR, blocked, allow_debit)
3. Import transaction history
4. Import instructors
5. Seed default services (Spinning, Fitness)
6. Create initial admin account

- [ ] **Step 3: Test with the actual db.mdb**

```bash
cargo run --bin migrate-legacy -- --db-path zbynek/spining_extracted/spining/db/db.mdb --output spinbike-test.db
```

- [ ] **Step 4: Commit**

---

### Task 14: CI Pipeline

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write CI workflow**

Create `.github/workflows/ci.yml` following the patterns from iem-mixer:
- test-integrity (check for #[ignore], continue-on-error)
- lint (fmt + clippy, with placeholder dist/ for rust-embed)
- test (cargo test server + core)
- build-wasm (trunk build, upload artifact)
- e2e (download wasm artifact, build server, Playwright tests)
- check-version-bump (on PRs only)

- [ ] **Step 2: Commit**

---

### Task 15: E2E Tests — Playwright

**Files:**
- Create: `e2e/package.json`
- Create: `e2e/playwright.config.ts`
- Create: `e2e/tests/auth.spec.ts`
- Create: `e2e/tests/schedule.spec.ts`
- Create: `e2e/tests/staff.spec.ts`
- Create: `e2e/tests/admin.spec.ts`

- [ ] **Step 1: Set up Playwright project**

```bash
cd e2e
npm init -y
npm install -D @playwright/test
npx playwright install chromium
```

- [ ] **Step 2: Write playwright.config.ts**

```typescript
import { defineConfig } from '@playwright/test';

export default defineConfig({
    testDir: './tests',
    timeout: 30000,
    use: {
        baseURL: 'http://localhost:8080',
        headless: true,
    },
    webServer: {
        command: 'cd .. && cargo run --bin spinbike-server',
        port: 8080,
        reuseExistingServer: true,
        timeout: 120000,
    },
});
```

- [ ] **Step 3: Write auth E2E test**

Test: register a new user, login, verify nav shows logged-in state, logout.

- [ ] **Step 4: Write schedule E2E test**

Test: view schedule (public), login as customer, book a class, verify booking appears, cancel booking.

- [ ] **Step 5: Write staff E2E test**

Test: login as staff, look up a card, process a payment, add a walk-in booking.

- [ ] **Step 6: Write admin E2E test**

Test: login as admin, create a class template, add an instructor, verify template appears in schedule.

- [ ] **Step 7: Run all E2E tests**

```bash
cd e2e && npx playwright test
```

- [ ] **Step 8: Commit**

---

### Task 16: CLAUDE.md and Final Polish

**Files:**
- Create: `CLAUDE.md`

- [ ] **Step 1: Create project CLAUDE.md**

Create `CLAUDE.md` with project-specific instructions:
- How to build (cargo check, trunk build)
- How to run locally (cargo run --bin spinbike-server)
- How to run tests (cargo test, cd e2e && npx playwright test)
- Version bumping (VERSION file + scripts/sync-version.sh)
- Branch workflow (two-branch: main + dev)
- CI notes (placeholder dist/ for rust-embed)

- [ ] **Step 2: Run full test suite**

```bash
cargo fmt --all --check
cargo test -p spinbike-core -p spinbike-server
cd spinbike-ui && trunk build --release
cd ../e2e && npx playwright test
```

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add CLAUDE.md with project build and test instructions"
```
