//! Typed error for the DB query layer (#163).
//!
//! Every query function in the `db` submodules used to return
//! `anyhow::Result<T>`. anyhow erases the concrete error type, so a route
//! handler that needed to distinguish a UNIQUE-constraint violation from a
//! real failure had to string-match the error chain
//! (`format!("{e:#}").contains("UNIQUE")` — see the old `routes/users.rs`
//! and `routes/test_fixtures.rs`). That is the textbook library-boundary
//! anti-pattern: anyhow belongs at the APPLICATION boundary, a `thiserror`
//! enum at the LIBRARY boundary. With a typed [`DbError`], callers now
//! `matches!(e, DbError::UniqueViolation)` — the same sqlx-native detector
//! the `create_service` handler already uses.
//!
//! Scope note: the startup/infra functions in [`super`] (`create_pool`,
//! `create_memory_pool`, `run_migrations`) deliberately stay on
//! `anyhow::Result`. They are the application boundary (the issue's "keep
//! anyhow only in bin/main/start_server"), no caller matches on their error
//! kind, and their `.context("Migration v{n} failed")` messages are
//! load-bearing when a migration fails at startup.

use thiserror::Error;

/// A typed error returned by the `db` query layer.
///
/// A bare `?` on any `sqlx` call inside a `db` query function converts the
/// underlying `sqlx::Error` into this type via the [`From`] impl below, which
/// classifies unique-constraint violations into [`DbError::UniqueViolation`]
/// so route handlers can turn them into a friendly 409 without string
/// matching. Every other database failure is wrapped transparently in
/// [`DbError::Sqlx`] and logged at the route boundary before a generic 500.
#[derive(Debug, Error)]
pub enum DbError {
    /// A UNIQUE / PRIMARY KEY constraint was violated (e.g. a duplicate email
    /// or card code). Route handlers map this to a 409 conflict.
    #[error("unique constraint violation")]
    UniqueViolation,

    /// A row that was expected to exist was missing (e.g. updating a user id
    /// that no longer exists). Currently bubbles to a 500 at its single
    /// caller — matching prior behaviour — but is typed so a handler CAN map
    /// it to a 404.
    #[error("record not found")]
    NotFound,

    /// A booking was rejected because the class is already at capacity. The
    /// booking route turns this into a 409 `class_full`.
    ///
    /// The Display text intentionally contains "full": a db-layer test asserts
    /// `.to_string().contains("full")` and the booking route echoes it as the
    /// conflict message.
    #[error("Class is full")]
    ClassFull,

    /// Any other database error (connection, SQL syntax, a non-unique
    /// constraint, ...). Transparent, so its `Display`/`source` is the
    /// underlying `sqlx::Error`.
    #[error(transparent)]
    Sqlx(sqlx::Error),
}

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        // Classify unique-constraint violations up front so callers never have
        // to inspect the sqlx error themselves. `is_unique_violation()` is the
        // sqlx-native detector (robust to SQLite locale / version drift),
        // callable on the `dyn DatabaseError` trait object without importing
        // the trait — the same idiom `routes/admin.rs::create_service` uses.
        match &e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                DbError::UniqueViolation
            }
            _ => DbError::Sqlx(e),
        }
    }
}

/// Result alias for the DB query layer: `Result<T, DbError>`.
pub type Result<T> = std::result::Result<T, DbError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_memory_pool;

    /// A real SQLite UNIQUE violation must classify into `UniqueViolation`, not
    /// the catch-all `Sqlx` variant — this is the whole point of the enum.
    #[tokio::test]
    async fn unique_violation_maps_to_unique_variant() {
        let pool = create_memory_pool().await.unwrap();
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, code TEXT UNIQUE NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t (code) VALUES ('x')")
            .execute(&pool)
            .await
            .unwrap();

        let err: sqlx::Error = sqlx::query("INSERT INTO t (code) VALUES ('x')")
            .execute(&pool)
            .await
            .unwrap_err();

        let db_err = DbError::from(err);
        assert!(
            matches!(db_err, DbError::UniqueViolation),
            "expected UniqueViolation, got {db_err:?}"
        );
    }

    /// A database error that is NOT a unique violation must fall through to the
    /// transparent `Sqlx` variant (so it becomes a 500, not a 409).
    #[tokio::test]
    async fn non_unique_db_error_maps_to_sqlx_variant() {
        let pool = create_memory_pool().await.unwrap();

        let err: sqlx::Error = sqlx::query("SELECT * FROM does_not_exist")
            .execute(&pool)
            .await
            .unwrap_err();

        let db_err = DbError::from(err);
        assert!(
            matches!(db_err, DbError::Sqlx(_)),
            "expected Sqlx, got {db_err:?}"
        );
    }

    /// The booking route matches the variant, but `db::classes` also asserts
    /// `.to_string().contains("full")` — pin the exact Display.
    #[test]
    fn class_full_display_is_stable() {
        assert_eq!(DbError::ClassFull.to_string(), "Class is full");
    }

    #[test]
    fn unique_violation_display_is_stable() {
        assert_eq!(
            DbError::UniqueViolation.to_string(),
            "unique constraint violation"
        );
    }
}
