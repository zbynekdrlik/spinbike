pub mod classes;
pub mod error;
pub mod login_tokens;
pub mod migrations;
pub mod persistent_bookings;
pub mod reports;
pub mod settings;
pub mod transactions;
pub mod users;

use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Connection, Row, SqlitePool};
use tracing::info;

use migrations::MIGRATIONS;

pub use error::DbError;

/// Create a persistent SQLite pool with WAL mode and foreign keys.
///
/// `busy_timeout` is set to 30 seconds via `after_connect`, which runs the
/// PRAGMA on every new pool connection AFTER sqlx finishes its own setup
/// (journal_mode, foreign_keys, etc.). This is the single source of truth
/// for the timeout value — `SqliteConnectOptions::busy_timeout()` is NOT
/// also set, to avoid a PRAGMA-ordering edge case where WAL setup could
/// race ahead of the timeout (#45 — observed twice as `database is locked`
/// under E2E concurrency despite SqliteConnectOptions::busy_timeout=30s).
///
/// Why 30 seconds: SQLite is single-writer, and even short admin ops
/// (a 100-row UPDATE batch in the legacy backfill, a schema migration)
/// can briefly hold the writer lock. 30 s is well over any realistic
/// short-write window so concurrent staff API calls wait instead of
/// returning SQLITE_BUSY (500 to the caller). The unit test below
/// asserts the value is actually applied, so cargo-mutants catches any
/// drift in either the integer or the PRAGMA string.
pub async fn create_pool(db_path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("PRAGMA busy_timeout = 30000;")
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .context("Failed to connect to SQLite")?;

    Ok(pool)
}

/// Create an in-memory SQLite pool for testing.
pub async fn create_memory_pool() -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(":memory:")
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .context("Failed to create in-memory SQLite pool")?;

    Ok(pool)
}

/// SHA-256 hex digest of a migration's SQL body — the tamper-detection
/// fingerprint recorded in `schema_version.checksum` (V19, #170). Same hash
/// idiom as `db::login_tokens::hash_token`.
fn migration_checksum(sql: &str) -> String {
    hex::encode(Sha256::digest(sql.as_bytes()))
}

/// Run all pending migrations inside transactions with schema_version tracking.
pub async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // Ensure schema_version table exists.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .context("Failed to create schema_version table")?;

    let current_version: i64 =
        sqlx::query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_version")
            .fetch_one(pool)
            .await
            .context("Failed to read schema version")?
            .get("v");

    for &(version, description, sql) in MIGRATIONS {
        if version <= current_version {
            continue;
        }

        info!(version, description, "applying migration");

        // Acquire a single connection for the duration of this migration so
        // that the foreign-key PRAGMA toggles affect the same connection that
        // runs the migration SQL. (PRAGMA scope is per-connection.)
        let mut conn = pool.acquire().await?;

        // Disable FK enforcement around table-rebuild migrations (V8's
        // CREATE_NEW + INSERT + DROP + RENAME pattern would otherwise fail at
        // commit when transactions has child rows). PRAGMA foreign_keys can
        // only be changed when no transaction is open, so we set it BEFORE
        // BEGIN and restore AFTER COMMIT.
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await
            .context("Failed to disable foreign_keys for migration")?;

        // Execute the migration SQL block. sqlx::raw_sql lets SQLite parse
        // multi-statement SQL itself, so '--' line comments and ';' inside
        // comments / string literals don't trip up a hand-rolled splitter.
        // Issue #73: a previous naive split-on-';' broke V14 because a
        // comment line contained two semicolons.
        let mut tx = conn.begin().await?;

        sqlx::raw_sql(sql)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Migration v{version} failed"))?;

        sqlx::query("INSERT INTO schema_version (version, description) VALUES (?, ?)")
            .bind(version)
            .bind(description)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await
            .context("Failed to re-enable foreign_keys after migration")?;

        info!(version, "migration applied");
    }

    // Verify + backfill checksums for every migration in MIGRATIONS (#170).
    // By this point every migration has been applied (the loop above never
    // returns early) — including V19 itself, whose ALTER TABLE added the
    // `checksum` column mid-loop, so the INSERT above deliberately never set
    // it for the migration(s) applied in this run.
    //
    // A NULL checksum means "not yet fingerprinted" — either a row that
    // predates this feature (v1..v18 on the very first boot after this
    // ships) or one just applied above in this same run — and gets
    // backfilled from the CURRENT const, establishing the baseline going
    // forward. A non-NULL checksum that no longer matches the const means
    // the migration's SQL was edited after being applied: refuse to boot
    // rather than run against a schema that no longer matches what shipped.
    for &(version, description, sql) in MIGRATIONS {
        let expected = migration_checksum(sql);
        let stored: Option<String> =
            sqlx::query_scalar("SELECT checksum FROM schema_version WHERE version = ?")
                .bind(version)
                .fetch_one(pool)
                .await
                .with_context(|| format!("Failed to read checksum for migration v{version}"))?;

        match stored {
            None => {
                sqlx::query("UPDATE schema_version SET checksum = ? WHERE version = ?")
                    .bind(&expected)
                    .bind(version)
                    .execute(pool)
                    .await
                    .with_context(|| {
                        format!("Failed to backfill checksum for migration v{version}")
                    })?;
            }
            Some(actual) if actual != expected => {
                anyhow::bail!(
                    "migration {version} ({description}) has been modified after being applied — checksum mismatch"
                );
            }
            Some(_) => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn test_migrations_run_on_fresh_db() {
        let pool = setup().await;

        // Verify all expected tables exist.
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        // V13 dropped the `cards` table and promoted its columns into `users`.
        // V17 added `login_tokens` (magic-link tokens) — sorts after
        // `instructors`, before `persistent_bookings`.
        let expected = vec![
            "bookings",
            "class_cancellations",
            "class_templates",
            "instructors",
            "login_tokens",
            "persistent_bookings",
            "schema_version",
            "services",
            "settings",
            "transactions",
            "users",
        ];
        assert_eq!(tables, expected);

        // Verify seed data.
        // V1 seeded Spinning + Fitness; V4 seeded Monthly pass; V8 seeded
        // Občerstvenie + Doplnky výživy + Aktivácia karty.
        let svc_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM services")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(svc_count, 6);

        let setting: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'bike_count'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(setting, "10");
    }

    #[tokio::test]
    async fn test_migrations_are_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Running again should not error.
        run_migrations(&pool).await.unwrap();

        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_version")
            .fetch_one(&pool)
            .await
            .unwrap();
        // Pin against the registered MIGRATIONS array rather than a magic number
        // so future migrations don't require touching this assertion.
        let expected = MIGRATIONS.last().expect("at least one migration").0;
        assert_eq!(version, expected);
    }

    /// Pins the after_connect PRAGMA — both the integer (30000) and the
    /// SQL string. cargo-mutants would otherwise be free to mutate either
    /// without any test catching it. If this test fails, busy_timeout is
    /// not being applied to pool connections; investigate before merging.
    #[tokio::test]
    async fn busy_timeout_is_30_seconds_per_connection() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await.unwrap();

        let timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout;")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(timeout, 30000, "busy_timeout should be 30000 ms (30 s)");
    }

    /// Issue #73 regression: the migration runner used to do `sql.split(';')`
    /// which treated semicolons inside `--` comments as statement terminators
    /// and broke any migration whose comments mentioned `;`. After switching
    /// to `sqlx::raw_sql`, SQLite parses statement boundaries itself and
    /// comments are correctly ignored.
    #[tokio::test]
    async fn raw_sql_block_tolerates_semicolons_inside_comments() {
        // create_memory_pool uses max_connections=1, so all calls go through
        // the same connection (and the same in-memory database).
        let pool = create_memory_pool().await.unwrap();

        let sql = r#"
            -- First sentence; second sentence; third sentence.
            -- Another line with one ; semicolon inside.
            CREATE TABLE issue_73 (id INTEGER PRIMARY KEY);
            INSERT INTO issue_73 (id) VALUES (1);
            INSERT INTO issue_73 (id) VALUES (2);
        "#;

        sqlx::raw_sql(sql).execute(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM issue_73")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    /// #170: on a fresh DB, every applied migration ends up with a non-null
    /// checksum in schema_version, matching `migration_checksum` of its
    /// current SQL const. Covers both the mid-loop path (V19 itself, whose
    /// INSERT deliberately leaves checksum NULL) and the post-loop backfill
    /// path (v1..v18) — both must converge on the same fingerprinted state.
    #[tokio::test]
    async fn fresh_db_backfills_checksum_for_every_migration() {
        let pool = setup().await;

        for &(version, description, sql) in MIGRATIONS {
            let expected = migration_checksum(sql);
            let stored: Option<String> =
                sqlx::query_scalar("SELECT checksum FROM schema_version WHERE version = ?")
                    .bind(version)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                stored,
                Some(expected),
                "migration v{version} ({description}) missing/wrong checksum"
            );
        }
    }

    /// #170: a schema_version row with a NULL checksum (simulating a
    /// pre-upgrade row written before this feature existed) gets backfilled
    /// on the next `run_migrations` call — no error, no special-casing.
    #[tokio::test]
    async fn null_checksum_row_gets_backfilled_on_rerun() {
        let pool = setup().await;

        // Simulate "before this feature shipped": wipe v1's checksum back to
        // NULL, as if it had been applied by an older binary.
        sqlx::query("UPDATE schema_version SET checksum = NULL WHERE version = 1")
            .execute(&pool)
            .await
            .unwrap();

        let wiped: Option<String> =
            sqlx::query_scalar("SELECT checksum FROM schema_version WHERE version = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(wiped, None, "test setup: checksum should be NULL now");

        // Re-running migrations must backfill it, not error.
        run_migrations(&pool).await.unwrap();

        let expected = migration_checksum(MIGRATIONS[0].2);
        let backfilled: Option<String> =
            sqlx::query_scalar("SELECT checksum FROM schema_version WHERE version = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(backfilled, Some(expected));
    }

    /// #170 core regression guard: if an already-applied migration's stored
    /// checksum no longer matches its current SQL const (i.e. someone edited
    /// the const after it shipped), `run_migrations` MUST fail loudly — never
    /// silently continue and boot against a schema that no longer matches
    /// what was recorded as applied.
    #[tokio::test]
    async fn tampered_migration_checksum_fails_loudly() {
        let pool = setup().await;

        // Simulate tampering: an already-applied migration (v1) now has a
        // stored checksum that does not match `migration_checksum` of its
        // current const — as if the const's SQL had been edited post-hoc.
        sqlx::query("UPDATE schema_version SET checksum = 'deadbeef' WHERE version = 1")
            .execute(&pool)
            .await
            .unwrap();

        let result = run_migrations(&pool).await;
        let err = result.expect_err("tampered checksum must fail run_migrations, not succeed");
        let msg = format!("{err:?}").to_lowercase();
        assert!(
            msg.contains("checksum") && msg.contains("modified"),
            "expected a checksum-mismatch error, got: {msg}"
        );
    }
}
