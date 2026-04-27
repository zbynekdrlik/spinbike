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
}
