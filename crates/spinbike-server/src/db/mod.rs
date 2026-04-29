pub mod backfill;
pub mod cards;
pub mod classes;
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
/// `busy_timeout` is set explicitly to 30 seconds. SQLite is single-writer,
/// and even short admin operations (a 100-row UPDATE batch in the legacy
/// backfill, a schema migration, etc.) can briefly hold the writer lock.
/// 30 s is well over any realistic short-write window so concurrent staff
/// API calls wait for the lock instead of returning SQLITE_BUSY (500 to
/// the caller). sqlx's default is 5 s, which the first prod backfill run
/// hit because debug-build batches held the lock too long.
pub async fn create_pool(db_path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(30))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
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

        // Execute the migration SQL (may contain multiple statements).
        // SQLite does not support transactional DDL well with multiple statements
        // via sqlx::query, so we execute each statement individually inside a tx.
        let mut tx = conn.begin().await?;

        for statement in sql.split(';') {
            let trimmed = statement.trim();
            if trimmed.is_empty() {
                continue;
            }
            sqlx::query(trimmed)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("Migration v{version} failed on: {trimmed}"))?;
        }

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

        let expected = vec![
            "bookings",
            "cards",
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
        assert_eq!(version, 10);
    }
}
