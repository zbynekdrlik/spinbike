pub mod classes;
pub mod login_tokens;
pub mod migrations;
pub mod persistent_bookings;
pub mod reports;
pub mod settings;
pub mod transactions;
pub mod users;

use std::path::Path;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Connection, Row, SqlitePool};
use tracing::info;

use migrations::MIGRATIONS;

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
        let expected = vec![
            "bookings",
            "class_cancellations",
            "class_templates",
            "instructors",
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
}
