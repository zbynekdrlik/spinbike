/// Each migration is (version, description, sql).
pub(crate) static MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "initial schema", V1_INITIAL_SCHEMA),
    (
        2,
        "card holder info and allow debit default",
        V2_CARD_HOLDER_INFO,
    ),
    (3, "card search_text column + index", V3_CARD_SEARCH_TEXT),
    (
        4,
        "monthly pass: valid_until + service seed",
        V4_MONTHLY_PASS,
    ),
    (
        5,
        "spin booking: bookings extended + persistent_bookings",
        V5_SPIN_BOOKING,
    ),
    (
        6,
        "seed 4 weekly spin classes + 2 instructors",
        V6_SEED_SPIN_CLASSES,
    ),
    (
        7,
        "transactions: soft-delete column",
        V7_TRANSACTIONS_SOFT_DELETE,
    ),
    (8, "services_dual_lang_kind", V8_SERVICES_DUAL_LANG_KIND),
    (
        9,
        "transactions: legacy_backfilled marker",
        V9_TRANSACTIONS_LEGACY_BACKFILL_MARKER,
    ),
    (
        10,
        "transactions: free-text note column",
        V10_TRANSACTIONS_NOTE_COLUMN,
    ),
    (
        11,
        "transactions: note length CHECK",
        V11_TRANSACTIONS_NOTE_CHECK,
    ),
    (
        12,
        "transactions: normalize legacy actions to new convention",
        V12_NORMALIZE_LEGACY_ACTIONS,
    ),
    (13, "users replace cards", V13_USERS_REPLACE_CARDS),
    (
        14,
        "rename monthly_pass label: Mesačný preplatok → Mesačná permanentka",
        V14_RENAME_MONTHLY_PASS_LABEL,
    ),
];

const V1_INITIAL_SCHEMA: &str = r#"
CREATE TABLE users (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    email       TEXT    NOT NULL UNIQUE,
    password_hash TEXT,
    name        TEXT    NOT NULL,
    phone       TEXT,
    role        TEXT    NOT NULL DEFAULT 'customer',
    oauth_provider TEXT,
    oauth_id    TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE cards (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    barcode     TEXT    NOT NULL UNIQUE,
    user_id     INTEGER REFERENCES users(id),
    blocked     INTEGER NOT NULL DEFAULT 0,
    credit      REAL    NOT NULL DEFAULT 0.0,
    allow_debit INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE services (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT    NOT NULL,
    default_price REAL    NOT NULL,
    active        INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE transactions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER REFERENCES users(id),
    card_id     INTEGER REFERENCES cards(id),
    staff_id    INTEGER REFERENCES users(id),
    service_id  INTEGER REFERENCES services(id),
    amount      REAL    NOT NULL,
    action      TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE instructors (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    name   TEXT    NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE class_templates (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    weekday          INTEGER NOT NULL,
    start_time       TEXT    NOT NULL,
    duration_minutes INTEGER NOT NULL DEFAULT 60,
    instructor_id    INTEGER REFERENCES instructors(id),
    capacity         INTEGER NOT NULL DEFAULT 10,
    active           INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE class_cancellations (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id  INTEGER NOT NULL REFERENCES class_templates(id),
    date         TEXT    NOT NULL,
    reason       TEXT,
    cancelled_by INTEGER REFERENCES users(id),
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE(template_id, date)
);

CREATE TABLE bookings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id INTEGER NOT NULL REFERENCES class_templates(id),
    date        TEXT    NOT NULL,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    created_by  INTEGER REFERENCES users(id),
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    cancelled_at TEXT
);

CREATE UNIQUE INDEX idx_bookings_active
    ON bookings(template_id, date, user_id)
    WHERE cancelled_at IS NULL;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Seed data
INSERT INTO services (name, default_price) VALUES ('Spinning', 5.0);
INSERT INTO services (name, default_price) VALUES ('Fitness', 5.0);

INSERT INTO settings (key, value) VALUES ('bike_count', '10');
INSERT INTO settings (key, value) VALUES ('center_name', 'Squash Centrum Smizany');
"#;

const V2_CARD_HOLDER_INFO: &str = r#"
ALTER TABLE cards ADD COLUMN first_name TEXT;
ALTER TABLE cards ADD COLUMN last_name TEXT;
ALTER TABLE cards ADD COLUMN company TEXT;
ALTER TABLE cards ADD COLUMN phone TEXT;
UPDATE cards SET allow_debit = 1;
"#;

// Adds a normalized search column so staff can find "Zbyněk" by typing "zbyne".
// Populated by Rust (via backfill_search_text) after the ALTER runs, since
// SQLite can't strip diacritics natively.
const V3_CARD_SEARCH_TEXT: &str = r#"
ALTER TABLE cards ADD COLUMN search_text TEXT NOT NULL DEFAULT '';
CREATE INDEX idx_cards_search_text ON cards(search_text);
"#;

// Monthly pass (Casova karta): records the pass expiry date on the purchase
// transaction row. NULL for every transaction except monthly-pass charges.
// Service is seeded idempotently so re-running migrations is safe.
const V4_MONTHLY_PASS: &str = r#"
ALTER TABLE transactions ADD COLUMN valid_until TEXT;
INSERT OR IGNORE INTO services (name, default_price, active) VALUES ('Monthly pass', 35.0, 1);
"#;

// Spin booking: extends bookings with card/charge columns and adds
// persistent_bookings for recurring class subscriptions.
const V5_SPIN_BOOKING: &str = r#"
ALTER TABLE bookings ADD COLUMN card_id INTEGER REFERENCES cards(id);
ALTER TABLE bookings ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE bookings ADD COLUMN charged_at TEXT;
ALTER TABLE bookings ADD COLUMN charge_transaction_id INTEGER REFERENCES transactions(id);

UPDATE bookings
  SET card_id = (SELECT c.id FROM cards c WHERE c.user_id = bookings.user_id LIMIT 1)
  WHERE card_id IS NULL;

CREATE TABLE IF NOT EXISTS persistent_bookings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    card_id     INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    template_id INTEGER NOT NULL REFERENCES class_templates(id) ON DELETE CASCADE,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    ended_at    TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_persistent_bookings_card_id_template_id_active
    ON persistent_bookings(card_id, template_id)
    WHERE ended_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_bookings_uncharged_future
    ON bookings(date, charged_at)
    WHERE cancelled_at IS NULL AND charged_at IS NULL;
"#;

// Seed 2 instructors and 4 weekly Mon-Thu 18:00 class templates.
// All inserts are conditional so re-running migrations is a no-op.
const V6_SEED_SPIN_CLASSES: &str = r#"
INSERT INTO instructors (name, active)
SELECT 'Stevo', 1 WHERE NOT EXISTS (SELECT 1 FROM instructors WHERE name='Stevo');
INSERT INTO instructors (name, active)
SELECT 'Vlada', 1 WHERE NOT EXISTS (SELECT 1 FROM instructors WHERE name='Vlada');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 0, '18:00', 60, (SELECT id FROM instructors WHERE name='Stevo'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=0 AND start_time='18:00');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 1, '18:00', 60, (SELECT id FROM instructors WHERE name='Vlada'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=1 AND start_time='18:00');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 2, '18:00', 60, (SELECT id FROM instructors WHERE name='Stevo'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=2 AND start_time='18:00');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 3, '18:00', 60, (SELECT id FROM instructors WHERE name='Vlada'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=3 AND start_time='18:00');
"#;

const V7_TRANSACTIONS_SOFT_DELETE: &str = r#"
ALTER TABLE transactions ADD COLUMN deleted_at TEXT;
"#;

const V8_SERVICES_DUAL_LANG_KIND: &str = r#"
-- The CREATE_NEW + INSERT + DROP + RENAME pattern requires foreign-key
-- enforcement to be OFF for the duration of the migration. The migration
-- runner (db::run_migrations) handles that toggle around the transaction.
-- INSERT INTO services_new ... SELECT id, ... preserves the original ids,
-- so transactions.service_id refs continue to resolve after the rename.

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

const V9_TRANSACTIONS_LEGACY_BACKFILL_MARKER: &str = r#"
ALTER TABLE transactions ADD COLUMN legacy_backfilled INTEGER NOT NULL DEFAULT 0;
"#;

const V10_TRANSACTIONS_NOTE_COLUMN: &str = r#"
ALTER TABLE transactions ADD COLUMN note TEXT;
"#;

const V11_TRANSACTIONS_NOTE_CHECK: &str = r#"
-- Defense-in-depth (#28): server already validates note ≤ 200 chars at
-- every entry point. This adds the same constraint at the DB level so a
-- direct sqlite3 write — or a future endpoint that forgets to validate —
-- cannot store an unbounded string.
--
-- SQLite cannot ALTER TABLE to add CHECK constraints on existing columns.
-- Use the CREATE_NEW + INSERT + DROP + RENAME pattern (V8 precedent).
-- Migration runner toggles PRAGMA foreign_keys around the transaction;
-- bookings.charge_transaction_id FK reattaches by name after RENAME.
--
-- Column list mirrors V1 + V4 (valid_until) + V7 (deleted_at) + V9
-- (legacy_backfilled) + V10 (note). Keep types and defaults identical.
-- length() on TEXT counts UTF-8 codepoints, matching the server's
-- chars().count() semantic.

CREATE TABLE transactions_new (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id           INTEGER REFERENCES users(id),
    card_id           INTEGER REFERENCES cards(id),
    staff_id          INTEGER REFERENCES users(id),
    service_id        INTEGER REFERENCES services(id),
    amount            REAL    NOT NULL,
    action            TEXT    NOT NULL,
    created_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    valid_until       TEXT,
    deleted_at        TEXT,
    legacy_backfilled INTEGER NOT NULL DEFAULT 0,
    note              TEXT    CHECK (note IS NULL OR length(note) <= 200)
);

INSERT INTO transactions_new (
    id, user_id, card_id, staff_id, service_id, amount, action, created_at,
    valid_until, deleted_at, legacy_backfilled, note
)
SELECT
    id, user_id, card_id, staff_id, service_id, amount, action, created_at,
    valid_until, deleted_at, legacy_backfilled, note
FROM transactions;

DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;
"#;

// Normalize legacy positive-magnitude + signed-by-action transaction rows to
// the new signed-amount + neutral-action convention used by spinbike_core::
// reports::classify. Pre-rewrite, the MS Access importer wrote action='debit'
// (positive amount) for spends and action='credit'/'activation' (positive
// amount) for top-ups. The classifier only knows 'charge' (negative) /
// 'topup' (positive) / 'visit' (zero), so legacy rows mis-rendered as TopUp
// regardless of whether they were debits or credits. This migration mutates
// every legacy row to the new vocabulary; subsequent runs are no-ops because
// the action-name guards no longer match anything.
//
// Each statement is independently idempotent — re-running this migration
// finds zero matching rows after the first successful pass.
//
// The runner at crate::db::mod (file db/mod.rs) runs every migration inside
// a single tx, so BEGIN/COMMIT are intentionally omitted here.
const V12_NORMALIZE_LEGACY_ACTIONS: &str = r#"
UPDATE transactions SET action='charge', amount = -amount
  WHERE action='debit' AND amount > 0;

UPDATE transactions SET action='visit'
  WHERE action='debit' AND amount = 0 AND valid_until IS NULL;

UPDATE transactions SET action='charge'
  WHERE action='debit' AND amount = 0 AND valid_until IS NOT NULL;

UPDATE transactions SET action='charge'
  WHERE action='credit' AND amount < 0;

UPDATE transactions SET action='topup'
  WHERE action='credit';

UPDATE transactions SET action='topup'
  WHERE action='activation';

-- storno rows (void of a prior transaction) are NOT mutated. The classifier
-- maps action='storno' to EventKind::Other regardless of amount sign, so the
-- ~64 historical refund rows render as Other instead of TopUp without losing
-- the void semantic. See spinbike_core::reports::classify.
"#;

// Drop `cards` as a first-class entity; promote its data into `users`.
//
// The migration runner (db::run_migrations) toggles `PRAGMA foreign_keys = OFF`
// before BEGIN and `ON` after COMMIT. No inline PRAGMA lines are needed here.
//
// Step order matters:
//   1. Recreate users (email nullable, new columns).
//   2. Promote linked-card data into existing user rows.
//   3. Insert unlinked legacy cards as new users (email=NULL).
//   4. Backfill cards.user_id for the freshly-created users.
//   5. Backfill transactions.user_id from cards.
//   6. Recreate transactions (drop card_id, user_id NOT NULL).
//   7. Recreate bookings (drop card_id) + restore indexes.
//   7b. Migrate persistent_bookings card_id → user_id (cards still exists here).
//   8. Drop cards.
const V13_USERS_REPLACE_CARDS: &str = r#"
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
CREATE INDEX        idx_users_search_text ON users(search_text) WHERE search_text IS NOT NULL;

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

-- 5. Backfill transactions.user_id where missing — first via card join, then via orphan fallback.
--    Production data contains some transaction rows whose user_id IS NULL AND
--    card_id either IS NULL OR points to a card row whose user_id remained NULL
--    after step 4 (e.g. legacy import + later card-row deletion). Step 6 needs
--    a non-null user_id for every row, so we insert a synthetic '(deleted)'
--    user and assign every still-NULL row to it. This preserves history rather
--    than dropping rows.
UPDATE transactions
   SET user_id = (SELECT user_id FROM cards WHERE cards.id = transactions.card_id)
 WHERE user_id IS NULL AND card_id IS NOT NULL;

INSERT INTO users (email, name, role)
SELECT NULL, '(deleted)', 'customer'
 WHERE EXISTS (SELECT 1 FROM transactions WHERE user_id IS NULL);

UPDATE transactions
   SET user_id = (SELECT id FROM users WHERE name = '(deleted)' ORDER BY id DESC LIMIT 1)
 WHERE user_id IS NULL;

-- 6. Recreate transactions without card_id (and user_id NOT NULL)
--    Preserves all columns from V11 (valid_until, deleted_at, legacy_backfilled, note).
CREATE TABLE transactions_new (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id           INTEGER NOT NULL REFERENCES users(id),
    staff_id          INTEGER REFERENCES users(id),
    service_id        INTEGER REFERENCES services(id),
    amount            REAL    NOT NULL,
    action            TEXT    NOT NULL,
    valid_until       TEXT,
    deleted_at        TEXT,
    legacy_backfilled INTEGER NOT NULL DEFAULT 0,
    note              TEXT CHECK (note IS NULL OR length(note) <= 200),
    created_at        TEXT    NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO transactions_new
       (id, user_id, staff_id, service_id, amount, action,
        valid_until, deleted_at, legacy_backfilled, note, created_at)
SELECT id, user_id, staff_id, service_id, amount, action,
       valid_until, deleted_at, legacy_backfilled, note, created_at
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
    source                TEXT    NOT NULL DEFAULT 'manual',
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

CREATE INDEX idx_bookings_uncharged_future
    ON bookings(date, charged_at)
    WHERE cancelled_at IS NULL AND charged_at IS NULL;

-- 7b. Migrate persistent_bookings: swap card_id → user_id (via cards join)
--     Must happen BEFORE step 8 (DROP TABLE cards) while cards still exists.
CREATE TABLE persistent_bookings_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    template_id INTEGER NOT NULL REFERENCES class_templates(id) ON DELETE CASCADE,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    ended_at    TEXT
);

INSERT INTO persistent_bookings_new (id, user_id, template_id, created_at, ended_at)
SELECT pb.id, c.user_id, pb.template_id, pb.created_at, pb.ended_at
FROM persistent_bookings pb
JOIN cards c ON c.id = pb.card_id;

DROP TABLE persistent_bookings;
ALTER TABLE persistent_bookings_new RENAME TO persistent_bookings;

CREATE UNIQUE INDEX idx_persistent_bookings_user_id_template_id_active
    ON persistent_bookings(user_id, template_id)
    WHERE ended_at IS NULL;

-- 8. Drop cards
DROP TABLE cards;
"#;

const V14_RENAME_MONTHLY_PASS_LABEL: &str = r#"
-- Issue #50: 'preplatok' means overpayment, not pass. Correct Slovak word
-- for a gym pass is 'permanentka' (feminine), so the adjective also flips:
-- 'Mesačná' not 'Mesačný'. Idempotent: re-runs match zero rows.
UPDATE services
SET name_sk = 'Mesačná permanentka'
WHERE name_sk = 'Mesačný preplatok';
"#;

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;
    use crate::db::{create_memory_pool, run_migrations};
    use sqlx::Connection;

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
        // V4 seeded the pass; V8 dual-language schema renamed the column to
        // name_en/name_sk and tagged the row with kind='monthly_pass'.
        let (name_en, price, active): (String, f64, i64) = sqlx::query_as(
            "SELECT name_en, default_price, active FROM services WHERE kind = 'monthly_pass'",
        )
        .fetch_one(&pool)
        .await
        .expect("Monthly pass service must be seeded by V4");
        assert_eq!(name_en, "Monthly pass");
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
            sqlx::query_scalar("SELECT COUNT(*) FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "Monthly pass must be seeded exactly once");
    }

    #[tokio::test]
    async fn v5_adds_booking_columns_and_persistent_table() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // After V5+V13: bookings retains source, charged_at, charge_transaction_id
        // but card_id is removed by V13. Check the surviving columns.
        let cols: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_table_info('bookings')")
            .fetch_all(&pool)
            .await
            .unwrap();
        let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
        for c in ["source", "charged_at", "charge_transaction_id"] {
            assert!(names.contains(&c), "bookings missing column {c}");
        }
        // V13 drops card_id from bookings.
        assert!(
            !names.contains(&"card_id"),
            "bookings.card_id must be removed by V13; found: {names:?}"
        );

        // persistent_bookings exists with the right unique index (user_id keyed after V13).
        let tbl: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='persistent_bookings'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(tbl.is_some(), "persistent_bookings table missing");

        let idx: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='persistent_bookings'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(
            idx.iter().any(|(n,)| n.contains("user_id_template_id")),
            "unique index on (user_id,template_id) missing; found: {idx:?}"
        );
    }

    #[tokio::test]
    async fn v5_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn v6_seeds_instructors_and_templates() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let stevo_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM instructors WHERE name='Stevo' AND active=1")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(stevo_id.is_some(), "Stevo must be seeded");

        let vlada_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM instructors WHERE name='Vlada' AND active=1")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(vlada_id.is_some(), "Vlada must be seeded");

        // Exactly 4 templates at 18:00 with capacity 19, one per weekday 0..=3.
        let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(
            "SELECT weekday, start_time, capacity, instructor_id
             FROM class_templates
             WHERE start_time = '18:00' AND active = 1
             ORDER BY weekday",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 4, "expected 4 seeded templates");
        for (i, (wd, st, cap, inst)) in rows.iter().enumerate() {
            assert_eq!(*wd, i as i64);
            assert_eq!(st, "18:00");
            assert_eq!(*cap, 19);
            let expected = if *wd == 0 || *wd == 2 {
                stevo_id.unwrap()
            } else {
                vlada_id.unwrap()
            };
            assert_eq!(*inst, expected, "wrong instructor for weekday {wd}");
        }
    }

    #[tokio::test]
    async fn v6_is_idempotent_and_does_not_duplicate() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM class_templates WHERE start_time='18:00' AND active=1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 4);

        let instr_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM instructors WHERE name IN ('Stevo','Vlada')")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(instr_count, 2);
    }

    #[tokio::test]
    async fn v7_adds_deleted_at_to_transactions() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
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
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap(); // second run must not error
    }

    #[tokio::test]
    async fn v8_services_have_dual_lang_and_kind() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Schema: name_sk, name_en, kind, default_price, active (no `name`).
        let cols: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_table_info('services')")
            .fetch_all(&pool)
            .await
            .unwrap();
        let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
        for col in [
            "id",
            "kind",
            "name_sk",
            "name_en",
            "default_price",
            "active",
        ] {
            assert!(
                names.contains(&col),
                "missing column {col} in services, got {names:?}"
            );
        }
        assert!(
            !names.contains(&"name"),
            "old `name` column must be dropped"
        );

        // Existing rows preserved with correct dual-lang
        let rows: Vec<(i64, String, String, String)> =
            sqlx::query_as("SELECT id, kind, name_sk, name_en FROM services ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let by_kind: std::collections::HashMap<&str, &(i64, String, String, String)> =
            rows.iter().map(|r| (r.1.as_str(), r)).collect();
        let pass = by_kind
            .get("monthly_pass")
            .expect("monthly_pass row missing");
        assert_eq!(pass.2, "Mesačná permanentka");
        assert_eq!(pass.3, "Monthly pass");

        // Three new generic rows seeded
        for n_sk in ["Občerstvenie", "Doplnky výživy", "Aktivácia karty"] {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM services WHERE name_sk = ?")
                .bind(n_sk)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 1, "service '{n_sk}' should be seeded once");
        }
    }

    #[tokio::test]
    async fn v8_only_one_monthly_pass_allowed() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let res = sqlx::query(
            "INSERT INTO services (kind, name_sk, name_en, default_price)
             VALUES ('monthly_pass', 'Druhý preplatok', 'Second pass', 35.0)",
        )
        .execute(&pool)
        .await;
        assert!(
            res.is_err(),
            "partial unique index on kind='monthly_pass' must reject duplicates"
        );
    }

    #[tokio::test]
    async fn v8_kind_check_constraint_rejects_unknown() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let res = sqlx::query(
            "INSERT INTO services (kind, name_sk, name_en, default_price)
             VALUES ('foobar', 'X', 'Y', 1.0)",
        )
        .execute(&pool)
        .await;
        assert!(res.is_err(), "kind CHECK constraint must reject 'foobar'");
    }

    #[tokio::test]
    async fn v8_is_idempotent() {
        // Running migrations twice must not fail. Idempotency is enforced by
        // the schema_version check in run_migrations, but exercise it.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();

        for n_sk in ["Občerstvenie", "Doplnky výživy", "Aktivácia karty"] {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM services WHERE name_sk = ?")
                .bind(n_sk)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 1, "service '{n_sk}' must be seeded exactly once");
        }
    }

    #[tokio::test]
    async fn v8_drop_rename_pattern_works_with_fk_child_rows() {
        // Production scenario: services has child rows in transactions referencing
        // services(id). The CREATE/INSERT/DROP/RENAME pattern only works when FK
        // enforcement is OFF for the duration of the migration — the migration
        // runner toggles `PRAGMA foreign_keys` before/after the transaction so
        // V8 succeeds against a populated transactions table.
        //
        // (`PRAGMA defer_foreign_keys = TRUE` inside an open transaction does
        // NOT work for this pattern: SQLite registers the FK violation when
        // DROP TABLE implicitly DELETEs the parent rows, and the subsequent
        // RENAME of the new table to the old name does not clear the pending
        // violation. PRAGMA foreign_keys = OFF (per-connection, before BEGIN)
        // is the only mechanism that works.)
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed a transaction with a real services(id) FK ref.
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('fk-test@x', 'FK Test', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
             VALUES (?, ?, -35.0, 'debit', '2026-01-01 12:00:00')",
        )
        .bind(user_id)
        .bind(svc_id)
        .execute(&pool)
        .await
        .unwrap();

        // Acquire a single connection so PRAGMA + BEGIN target the same one.
        // Mirror what run_migrations does for every migration.
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await
            .unwrap();
        let mut tx = conn.begin().await.unwrap();
        sqlx::query(
            "CREATE TABLE services_test_rebuild (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL DEFAULT 'generic',
                name_sk TEXT NOT NULL,
                name_en TEXT NOT NULL,
                default_price REAL NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            )",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO services_test_rebuild
                SELECT id, kind, name_sk, name_en, default_price, active FROM services",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("DROP TABLE services")
            .execute(&mut *tx)
            .await
            .expect("DROP TABLE services must succeed with foreign_keys = OFF");
        sqlx::query("ALTER TABLE services_test_rebuild RENAME TO services")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit()
            .await
            .expect("commit must succeed — preserved ids restore FK validity");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await
            .unwrap();
        // Release the only connection back to the pool so the validity probe
        // below can acquire it.
        drop(conn);

        // The transaction's service_id ref still resolves after rebuild.
        let still_valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM transactions t
                JOIN services s ON s.id = t.service_id
                WHERE t.user_id = ?
            )",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            still_valid,
            "transactions.service_id ref must still resolve after table rebuild"
        );
    }

    #[tokio::test]
    async fn v9_transactions_have_legacy_backfilled_column() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
        let cols: Vec<(i64, String, String, i64, Option<String>, i64)> =
            sqlx::query_as("PRAGMA table_info(transactions)")
                .fetch_all(&pool)
                .await
                .unwrap();
        let lb = cols
            .iter()
            .find(|c| c.1 == "legacy_backfilled")
            .expect("legacy_backfilled column missing on transactions");
        assert_eq!(lb.2.to_uppercase(), "INTEGER");
        assert_eq!(lb.3, 1, "legacy_backfilled must be NOT NULL");
    }

    #[tokio::test]
    async fn v10_adds_note_column_to_transactions() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let cols: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM pragma_table_info('transactions')")
                .fetch_all(&pool)
                .await
                .unwrap();
        let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
        assert!(
            names.contains(&"note"),
            "transactions.note column missing; found: {names:?}"
        );
    }

    #[tokio::test]
    async fn v10_note_defaults_to_null_for_existing_rows() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Inserting a row without a note column read should yield NULL.
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('note-test@x', 'Note Test', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO transactions (user_id, amount, action) VALUES (?, ?, ?)")
            .bind(user_id)
            .bind(1.0_f64)
            .bind("topup")
            .execute(&pool)
            .await
            .unwrap();
        let note: Option<String> =
            sqlx::query_scalar("SELECT note FROM transactions WHERE user_id = ?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(note.is_none(), "fresh row's note must be NULL");
    }

    // V11 — note CHECK constraint -------------------------------------

    #[tokio::test]
    async fn v11_note_check_accepts_200_chars() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed a user so the transactions.user_id FK is satisfied (per V8/V10
        // test convention). Migrations re-enable foreign_keys at the end.
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('v11-ok-200@x', 'V11 OK 200', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Insert a transaction with a 200-char note (exactly at the bound).
        // Use a Slovak diacritic so the byte count > 200 but char count = 200,
        // matching the server-side validator (uses chars().count(), not len()).
        let note: String = "á".repeat(200);
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, note)
             VALUES (?, ?, 'charge', ?)",
        )
        .bind(user_id)
        .bind(5.0_f64)
        .bind(&note)
        .execute(&pool)
        .await
        .expect("200-char note must be accepted");
    }

    #[tokio::test]
    async fn v11_note_check_rejects_201_chars() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('v11-reject-201@x', 'V11 Reject 201', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let note: String = "á".repeat(201);
        let res = sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, note)
             VALUES (?, ?, 'charge', ?)",
        )
        .bind(user_id)
        .bind(5.0_f64)
        .bind(&note)
        .execute(&pool)
        .await;

        let err = res.expect_err("201-char note must be rejected");
        let msg = err.to_string();
        // Match the SQLite CHECK violation specifically — a generic "FOREIGN
        // KEY constraint failed" must NOT pass this test (that would mean we
        // failed for the wrong reason).
        assert!(
            msg.contains("CHECK"),
            "expected CHECK constraint violation, got: {msg}"
        );
    }

    #[tokio::test]
    async fn v11_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Second run must not error — schema_version check should make V11
        // a no-op on the already-migrated DB.
        run_migrations(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn v11_preserves_existing_rows_across_idempotent_rerun() {
        // Regression fence for the V11 CREATE_NEW + INSERT + DROP + RENAME
        // pattern: even though run_migrations runs the whole chain once and
        // V11 is then schema_version-gated to a no-op, exercise the property
        // that re-running migrations against a populated transactions table
        // does NOT lose rows. Catches a hypothetical future migration that
        // re-rebuilds transactions but forgets to copy data, AND verifies
        // the bookings.charge_transaction_id FK still resolves after a
        // populated re-run cycle.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role)
             VALUES ('preserve@test.local', 'Preserver', 'customer')
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Seed 5 transactions and 5 bookings, each booking pointing at a
        // different transaction.
        for i in 0..5 {
            let tx_id: i64 = sqlx::query_scalar(
                "INSERT INTO transactions (user_id, amount, action)
                 VALUES (?, ?, 'charge') RETURNING id",
            )
            .bind(user_id)
            .bind(1.0_f64 + i as f64)
            .fetch_one(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO bookings (template_id, date, user_id, charge_transaction_id)
                 VALUES (1, ?, ?, ?)",
            )
            .bind(format!("2026-12-{:02}", i + 1))
            .bind(user_id)
            .bind(tx_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Re-run migrations; V11 should remain a no-op via schema_version.
        run_migrations(&pool).await.unwrap();

        let tx_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(tx_count, 5, "transactions row count must survive re-run");

        let bk_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bookings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(bk_count, 5, "bookings row count must survive re-run");

        // FK-via-join must still resolve for all 5.
        let joined: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transactions t
             JOIN bookings b ON b.charge_transaction_id = t.id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            joined, 5,
            "bookings.charge_transaction_id FK must resolve for all 5 rows"
        );
    }

    #[tokio::test]
    async fn v11_drop_rename_pattern_preserves_bookings_fk() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed: a transaction + a booking that references it via charge_transaction_id.
        // After V11 recreates `transactions`, the FK on bookings.charge_transaction_id
        // must continue to resolve (V8 precedent — FK reattaches by table name on RENAME).
        let tx_user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('v11-fk-tx@x', 'V11 FK TX', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let tx_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, amount, action)
             VALUES (?, 5.0, 'charge') RETURNING id",
        )
        .bind(tx_user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        // bookings requires (template_id, date, user_id) NOT NULL FKs.
        // Migrations seed a class_template at id=1 (V6_SEED_SPIN_CLASSES).
        // users requires (email, name, role) — name is NOT NULL.
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, password_hash, role)
             VALUES ('booker@test.local', 'Test Booker', 'x', 'admin')
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO bookings (template_id, date, user_id, charge_transaction_id)
             VALUES (1, '2026-12-01', ?, ?)",
        )
        .bind(user_id)
        .bind(tx_id)
        .execute(&pool)
        .await
        .expect("booking insert must succeed with V11 in place");

        // Verify FK resolves: join must produce a row.
        let joined: i64 = sqlx::query_scalar(
            "SELECT t.id FROM transactions t
             JOIN bookings b ON b.charge_transaction_id = t.id
             WHERE b.charge_transaction_id IS NOT NULL
             LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("transactions ↔ bookings FK must resolve after V11 rebuild");
        assert_eq!(joined, tx_id);
    }

    #[tokio::test]
    async fn v12_normalizes_every_legacy_pattern() {
        use crate::db::{create_memory_pool, run_migrations};
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed one row of every pattern from the spec mutation table.
        // Insert raw legacy-shape rows post-migration, then force V12 to
        // re-run by clearing its schema_version entry.
        // Post-V13 transactions requires user_id NOT NULL — seed a user first.
        let v12_user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('v12-norm@x', 'V12 Norm', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // Each row needs user_id; use individual inserts to avoid repeated-bind complexity.
        for (id, action, amount, valid_until) in [
            (1001i64, "debit", 3.0f64, None::<String>),
            (1002, "debit", 0.0, None),
            (1003, "debit", 0.0, Some("2026-12-31".to_string())),
            (1004, "credit", 2.0, None),
            (1005, "credit", 0.0, None),
            (1006, "credit", -30.0, None),
            (1007, "activation", 30.0, None),
            (1008, "storno", 2.5, None),
            (1009, "storno", 0.0, None),
            // New-convention rows: V12 must leave these unchanged.
            (1010, "charge", -5.0, None),
            (1011, "topup", 7.5, None),
            (1012, "visit", 0.0, None),
        ] {
            sqlx::query(
                "INSERT INTO transactions (id, user_id, action, amount, valid_until)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(v12_user_id)
            .bind(action)
            .bind(amount)
            .bind(valid_until)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Force V12 to re-run by executing the SQL directly. (run_migrations
        // tracks applied migrations by MAX(version); after V13 is applied,
        // simply deleting V12's schema_version row no longer triggers a re-run.)
        let v12_sql = MIGRATIONS.iter().find(|(v, _, _)| *v == 12).unwrap().2;
        for stmt in v12_sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            sqlx::query(stmt).execute(&pool).await.unwrap();
        }

        let rows: Vec<(i64, String, f64)> = sqlx::query_as(
            "SELECT id, action, amount FROM transactions
             WHERE id BETWEEN 1001 AND 1012
             ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let expected: Vec<(i64, &str, f64)> = vec![
            (1001, "charge", -3.0),  // debit > 0 → charge, negated
            (1002, "visit", 0.0),    // debit = 0, no valid_until → visit
            (1003, "charge", 0.0),   // debit = 0, valid_until set → charge
            (1004, "topup", 2.0),    // credit > 0 → topup
            (1005, "topup", 0.0),    // credit = 0 → topup
            (1006, "charge", -30.0), // credit < 0 → charge (already negative)
            (1007, "topup", 30.0),   // activation → topup
            (1008, "storno", 2.5),   // storno > 0 → unchanged (void semantic)
            (1009, "storno", 0.0),   // storno = 0 → unchanged
            (1010, "charge", -5.0),  // already-new charge → unchanged
            (1011, "topup", 7.5),    // already-new topup → unchanged
            (1012, "visit", 0.0),    // already-new visit → unchanged
        ];

        assert_eq!(rows.len(), expected.len(), "all 12 rows must survive");
        for ((id, action, amount), (eid, eaction, eamount)) in rows.iter().zip(expected.iter()) {
            assert_eq!(id, eid, "row id mismatch");
            assert_eq!(action, eaction, "row {id}: action mismatch");
            assert!(
                (amount - eamount).abs() < 1e-9,
                "row {id}: amount {amount} != {eamount}"
            );
        }
    }

    #[tokio::test]
    async fn v12_is_idempotent() {
        use crate::db::{create_memory_pool, run_migrations};
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Post-V13 transactions requires user_id NOT NULL — seed a user first.
        let v12i_user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('v12-idem@x', 'V12 Idem', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (id, user_id, action, amount) VALUES
               (2001, ?, 'debit',  3.0),
               (2002, ?, 'credit', 5.0)",
        )
        .bind(v12i_user_id)
        .bind(v12i_user_id)
        .execute(&pool)
        .await
        .unwrap();

        // First run: mutate. Execute V12 SQL directly (see comment in
        // v12_normalizes_every_legacy_pattern).
        let v12_sql = MIGRATIONS.iter().find(|(v, _, _)| *v == 12).unwrap().2;
        for stmt in v12_sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            sqlx::query(stmt).execute(&pool).await.unwrap();
        }

        let after_first: Vec<(i64, String, f64)> = sqlx::query_as(
            "SELECT id, action, amount FROM transactions
             WHERE id IN (2001, 2002) ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        // Second run: no-op (no rows match the legacy guards anymore).
        for stmt in v12_sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            sqlx::query(stmt).execute(&pool).await.unwrap();
        }

        let after_second: Vec<(i64, String, f64)> = sqlx::query_as(
            "SELECT id, action, amount FROM transactions
             WHERE id IN (2001, 2002) ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(
            after_first, after_second,
            "second V12 run must leave rows unchanged (idempotency)"
        );
        // Sanity: state is the post-backfill shape.
        assert_eq!(after_first[0].1, "charge");
        assert!((after_first[0].2 - (-3.0)).abs() < 1e-9);
        assert_eq!(after_first[1].1, "topup");
        assert!((after_first[1].2 - 5.0).abs() < 1e-9);
    }

    // V13 — users replace cards ----------------------------------------

    /// Helper: execute a multi-statement SQL block the same way run_migrations
    /// does (split on ';', skip blanks, run each statement individually).
    /// Also toggles PRAGMA foreign_keys OFF/ON around the transaction so that
    /// DROP TABLE on a parent table with FK children succeeds.
    async fn apply_sql_block(pool: &sqlx::SqlitePool, sql: &str) {
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await
            .unwrap();
        let mut tx = conn.begin().await.unwrap();
        for stmt in sql.split(';') {
            let trimmed = stmt.trim();
            if trimmed.is_empty() {
                continue;
            }
            sqlx::query(trimmed)
                .execute(&mut *tx)
                .await
                .unwrap_or_else(|e| panic!("SQL statement failed: {e}\n  stmt: {trimmed}"));
        }
        tx.commit().await.unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn migration_13_users_replace_cards_full_round_trip() {
        // Apply migrations 1..=12 using run_migrations, then seed data, then
        // apply V13 manually so we can assert the before→after shape.
        //
        // We can't use run_migrations for V13 after seeding because run_migrations
        // applies ALL pending migrations in one pass; here we need:
        //   1. run_migrations (applies 1..=12)
        //   2. seed legacy data
        //   3. apply V13 only
        //
        // Approach: use a pool with only migrations 1-12 in MIGRATIONS known to
        // the runner — achieved by running run_migrations (which applies up to
        // the current MIGRATIONS slice = 1..=13), then seeding, then deleting
        // schema_version for 13 and re-running.
        //
        // Simpler: use create_memory_pool + run_migrations to apply 1..=12,
        // seed data that requires cards table, then apply the V13 SQL directly
        // via apply_sql_block, then assert.
        //
        // create_memory_pool().await applies run_migrations immediately.
        // So we need a raw pool without migrations, apply 1..=12, seed, apply 13.

        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Bootstrap schema_version (run_migrations expects this table).
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                description TEXT NOT NULL DEFAULT '',
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Apply migrations 1..=12 only.
        for &(v, desc, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v <= 12) {
            apply_sql_block(&pool, sql).await;
            sqlx::query("INSERT INTO schema_version(version, description) VALUES (?, ?)")
                .bind(v)
                .bind(desc)
                .execute(&pool)
                .await
                .unwrap();
        }

        // Seed: 1 staff user, 1 linked card (alice), 1 unlinked named card (bob),
        //       1 unlinked nameless card.
        let staff_id: i64 = sqlx::query_scalar(
            "INSERT INTO users(email,name,role) VALUES('staff@x','Staff','staff') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let alice_user: i64 = sqlx::query_scalar(
            "INSERT INTO users(email,name,role) VALUES('alice@x','Alice Old','customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO cards(barcode,user_id,blocked,credit,allow_debit,
                              first_name,last_name,company,phone,search_text)
             VALUES('CODE1', ?, 0, 12.50, 0, 'Alice', 'Linked', 'Acme', '111', 'alice linked acme')",
        )
        .bind(alice_user)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO cards(barcode,user_id,blocked,credit,allow_debit,
                              first_name,last_name,company,phone,search_text)
             VALUES('CODE2', NULL, 0, -3.00, 1, 'Bob', 'Lonely', NULL, '222', 'bob lonely')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO cards(barcode,user_id,blocked,credit,allow_debit,
                              first_name,last_name,company,phone,search_text)
             VALUES('CODE3', NULL, 1, 0.0, 0, NULL, NULL, NULL, NULL, '')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 4th card: Charlie — linked user with blank first_name/last_name to
        // exercise the COALESCE fallback that preserves users.name.
        let charlie_user: i64 = sqlx::query_scalar(
            "INSERT INTO users(email,name,role) VALUES('charlie@x','Charlie Original','customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO cards(barcode,user_id,blocked,credit,allow_debit,
                              first_name,last_name,company,phone,search_text)
             VALUES('CODE4', ?, 0, 0.0, 0, NULL, NULL, NULL, NULL, '')",
        )
        .bind(charlie_user)
        .execute(&pool)
        .await
        .unwrap();

        // Insert a transaction tied to bob's card (no user_id yet — legacy shape).
        let bob_card_id: i64 = sqlx::query_scalar("SELECT id FROM cards WHERE barcode='CODE2'")
            .fetch_one(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO transactions(card_id, staff_id, amount, action)
             VALUES(?, ?, -1.50, 'charge')",
        )
        .bind(bob_card_id)
        .bind(staff_id)
        .execute(&pool)
        .await
        .unwrap();

        // Seed a persistent_booking for Bob's card (CODE2). Use whatever
        // template V6 seeded rather than assuming a specific id.
        let template_id: i64 = sqlx::query_scalar("SELECT id FROM class_templates LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO persistent_bookings(card_id, template_id) VALUES(?, ?)")
            .bind(bob_card_id)
            .bind(template_id)
            .execute(&pool)
            .await
            .unwrap();

        // Apply V13.
        let v13_sql = MIGRATIONS.iter().find(|(v, _, _)| *v == 13).unwrap().2;
        apply_sql_block(&pool, v13_sql).await;

        // ── Assertions ──────────────────────────────────────────────────

        // Total users: staff + alice + bob + nameless + charlie = 5.
        let users_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(users_total, 5, "staff + alice + bob + nameless + charlie");

        // Alice: credit and card_code promoted from linked card.
        let alice_credit: f64 =
            sqlx::query_scalar("SELECT credit FROM users WHERE email='alice@x'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!((alice_credit - 12.50).abs() < 1e-9, "alice credit mismatch");

        let alice_card: String =
            sqlx::query_scalar("SELECT card_code FROM users WHERE email='alice@x'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(alice_card, "CODE1");

        // Alice: name promoted from card first+last (overrides the original users.name).
        let alice_name: String = sqlx::query_scalar("SELECT name FROM users WHERE email='alice@x'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            alice_name, "Alice Linked",
            "card first+last name overrides existing user.name"
        );

        // Alice: phone COALESCE — alice had NULL in users, card had '111' → result '111'.
        let alice_phone: Option<String> =
            sqlx::query_scalar("SELECT phone FROM users WHERE email='alice@x'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(alice_phone.as_deref(), Some("111"));

        // Alice: company from card.
        let alice_company: Option<String> =
            sqlx::query_scalar("SELECT company FROM users WHERE email='alice@x'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(alice_company.as_deref(), Some("Acme"));

        // Alice: search_text from card.
        let alice_search: Option<String> =
            sqlx::query_scalar("SELECT search_text FROM users WHERE email='alice@x'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(alice_search.as_deref(), Some("alice linked acme"));

        // Bob: email NULL, credit preserved, blocked=0, name assembled from first+last.
        let bob: (Option<String>, f64, i64, String) = sqlx::query_as(
            "SELECT email, credit, blocked, name FROM users WHERE card_code='CODE2'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(bob.0.is_none(), "bob has no email");
        assert!((bob.1 - (-3.00)).abs() < 1e-9, "bob credit mismatch");
        assert_eq!(bob.2, 0, "bob blocked mismatch");
        assert_eq!(bob.3, "Bob Lonely", "bob name mismatch");

        // Bob: allow_debit=1 (promoted from card).
        let bob_allow_debit: i64 =
            sqlx::query_scalar("SELECT allow_debit FROM users WHERE card_code='CODE2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bob_allow_debit, 1);

        // Bob: phone='222' (promoted from card).
        let bob_phone: Option<String> =
            sqlx::query_scalar("SELECT phone FROM users WHERE card_code='CODE2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bob_phone.as_deref(), Some("222"));

        // Nameless card: name falls back to '(no name)'.
        let nameless_name: String =
            sqlx::query_scalar("SELECT name FROM users WHERE card_code='CODE3'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(nameless_name, "(no name)");

        // Charlie: blank card first+last → COALESCE falls back to users.name.
        let charlie_name: String =
            sqlx::query_scalar("SELECT name FROM users WHERE email='charlie@x'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            charlie_name, "Charlie Original",
            "blank card first+last preserves users.name fallback"
        );

        // Bob's user_id (used for both transaction and persistent_booking assertions).
        let bob_user: i64 = sqlx::query_scalar("SELECT id FROM users WHERE card_code='CODE2'")
            .fetch_one(&pool)
            .await
            .unwrap();

        // Bob's transaction now has user_id (not card_id).
        let txn_user: i64 = sqlx::query_scalar("SELECT user_id FROM transactions LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            txn_user, bob_user,
            "transaction.user_id must point to bob's new user row"
        );

        // cards table is gone.
        let cards_table: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='cards'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(cards_table.is_none(), "cards table must be dropped");

        // transactions has no card_id column.
        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('transactions')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            !cols.contains(&"card_id".to_string()),
            "transactions.card_id must be dropped; found cols: {cols:?}"
        );

        // bookings has no card_id column.
        let bcols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('bookings')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            !bcols.contains(&"card_id".to_string()),
            "bookings.card_id must be dropped; found cols: {bcols:?}"
        );

        // persistent_bookings table still exists.
        let pb_table: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='persistent_bookings'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            pb_table.is_some(),
            "persistent_bookings table must survive V13"
        );

        // persistent_bookings has no card_id column.
        let pb_cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('persistent_bookings')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            !pb_cols.contains(&"card_id".to_string()),
            "persistent_bookings.card_id must be dropped after V13; found: {pb_cols:?}"
        );
        assert!(
            pb_cols.contains(&"user_id".to_string()),
            "persistent_bookings must have user_id column after V13; found: {pb_cols:?}"
        );

        // Bob's seeded persistent_booking migrated to his new user_id.
        let pb_user: i64 = sqlx::query_scalar("SELECT user_id FROM persistent_bookings LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            pb_user, bob_user,
            "persistent_bookings.user_id must point to Bob's new user row"
        );

        // Exactly one persistent_booking row survived (no duplication, no loss).
        let pb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM persistent_bookings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            pb_count, 1,
            "persistent_bookings count must be preserved across V13"
        );

        // idx_persistent_bookings_user_id_template_id_active exists.
        let pb_idx: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='index'
             AND name='idx_persistent_bookings_user_id_template_id_active'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            pb_idx.is_some(),
            "idx_persistent_bookings_user_id_template_id_active must exist after V13"
        );

        // idx_bookings_uncharged_future exists.
        let bu_idx: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='index'
             AND name='idx_bookings_uncharged_future'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            bu_idx.is_some(),
            "idx_bookings_uncharged_future must be recreated after V13 bookings rebuild"
        );

        // PRAGMA foreign_key_check must return no rows (no broken FKs).
        let fk_violations: Vec<(String, i64, String, i64)> =
            sqlx::query_as("PRAGMA foreign_key_check")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            fk_violations.is_empty(),
            "PRAGMA foreign_key_check must return no rows after V13; violations: {fk_violations:?}"
        );

        // PRAGMA integrity_check.
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(integrity, "ok", "integrity_check must pass after V13");
    }

    #[tokio::test]
    async fn v14_renames_monthly_pass_label() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // The monthly_pass row now reads the corrected Slovak label.
        let pass_name: String =
            sqlx::query_scalar("SELECT name_sk FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pass_name, "Mesačná permanentka");

        // Other service rows are NOT touched by V14 — kills mutants that
        // broaden the WHERE clause (e.g. `LIKE '%preplatok%'`).
        for n_sk in [
            "Spinning",
            "Fitness",
            "Občerstvenie",
            "Doplnky výživy",
            "Aktivácia karty",
        ] {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM services WHERE name_sk = ?")
                .bind(n_sk)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 1, "service '{n_sk}' must be unchanged after V14");
        }

        // No row still carries the old Slovak label.
        let stale: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM services WHERE name_sk = 'Mesačný preplatok'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stale, 0, "no services row should still carry the old label");

        // Idempotency: running the chain a second time must not error and
        // must not re-mutate the row.
        run_migrations(&pool).await.unwrap();
        let pass_name_again: String =
            sqlx::query_scalar("SELECT name_sk FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pass_name_again, "Mesačná permanentka");
    }
}
