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

#[cfg(test)]
mod tests {
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

        // bookings gained card_id, source, charged_at, charge_transaction_id
        let cols: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_table_info('bookings')")
            .fetch_all(&pool)
            .await
            .unwrap();
        let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
        for c in ["card_id", "source", "charged_at", "charge_transaction_id"] {
            assert!(names.contains(&c), "bookings missing column {c}");
        }

        // persistent_bookings exists with the right unique index
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
            idx.iter().any(|(n,)| n.contains("card_id_template_id")),
            "unique index on (card_id,template_id) missing"
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
        assert_eq!(pass.2, "Mesačný preplatok");
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
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, allow_debit) VALUES ('FK-TEST', 1) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let svc_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, created_at)
             VALUES (?, ?, -35.0, 'debit', '2026-01-01 12:00:00')",
        )
        .bind(card_id)
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
                WHERE t.card_id = ?
            )",
        )
        .bind(card_id)
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
        let card_id: i64 =
            sqlx::query_scalar("INSERT INTO cards (barcode) VALUES ('NOTE-TEST') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("INSERT INTO transactions (card_id, amount, action) VALUES (?, ?, ?)")
            .bind(card_id)
            .bind(1.0_f64)
            .bind("topup")
            .execute(&pool)
            .await
            .unwrap();
        let note: Option<String> =
            sqlx::query_scalar("SELECT note FROM transactions WHERE card_id = ?")
                .bind(card_id)
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

        // Seed a card so the transactions.card_id FK is satisfied (per V8/V10
        // test convention). Migrations re-enable foreign_keys at the end.
        let card_id: i64 =
            sqlx::query_scalar("INSERT INTO cards (barcode) VALUES ('V11-OK-200') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();

        // Insert a transaction with a 200-char note (exactly at the bound).
        // Use a Slovak diacritic so the byte count > 200 but char count = 200,
        // matching the server-side validator (uses chars().count(), not len()).
        let note: String = "á".repeat(200);
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, note)
             VALUES (?, ?, 'charge', ?)",
        )
        .bind(card_id)
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

        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode) VALUES ('V11-REJECT-201') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let note: String = "á".repeat(201);
        let res = sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, note)
             VALUES (?, ?, 'charge', ?)",
        )
        .bind(card_id)
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

        let card_id: i64 =
            sqlx::query_scalar("INSERT INTO cards (barcode) VALUES ('V11-PRESERVE') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, password_hash, role)
             VALUES ('preserve@test.local', 'Preserver', 'x', 'admin')
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Seed 5 transactions and 5 bookings, each booking pointing at a
        // different transaction.
        for i in 0..5 {
            let tx_id: i64 = sqlx::query_scalar(
                "INSERT INTO transactions (card_id, amount, action)
                 VALUES (?, ?, 'charge') RETURNING id",
            )
            .bind(card_id)
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
        let card_id: i64 =
            sqlx::query_scalar("INSERT INTO cards (barcode) VALUES ('V11-FK') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let tx_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (card_id, amount, action)
             VALUES (?, 5.0, 'charge') RETURNING id",
        )
        .bind(card_id)
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
}
