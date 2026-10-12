//! Instructor queries (#381): name lookup for the public schedule and the
//! admin delete with its "still teaches an active class" guard.
//!
//! The list / create / rename / (de)activate handlers predate this module and
//! still run their one-statement SQL inline in `routes/admin.rs`; the delete
//! lives here because it is a multi-statement transaction with a typed
//! outcome the route maps onto 204 / 404 / 409.

use std::collections::HashMap;

use crate::db::error::Result;
use sqlx::SqlitePool;

/// `instructors.id -> name` for every instructor (active or not) — the
/// schedule resolves each class template's `instructor_id` against it with
/// one query per request instead of one per occurrence.
pub async fn names_by_id(pool: &SqlitePool) -> Result<HashMap<i64, String>> {
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM instructors")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Outcome of [`delete_instructor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteInstructorOutcome {
    /// The row is gone.
    Deleted,
    /// No instructor with that id.
    NotFound,
    /// Refused — the instructor still teaches `active_templates` ACTIVE
    /// class template(s). Nothing was changed.
    InUse { active_templates: i64 },
}

/// Permanently delete an instructor (owner decision on #381: deletion, not
/// deactivation).
///
/// Refused while any ACTIVE class template references the instructor — the
/// admin must reassign or deactivate that class first. INACTIVE templates
/// that still point at the instructor are detached (`instructor_id = NULL`)
/// in the same transaction: `class_templates.instructor_id REFERENCES
/// instructors(id)` is enforced (`foreign_keys(true)` on every pool), so
/// leaving them would turn the DELETE into an FK error.
pub async fn delete_instructor(pool: &SqlitePool, id: i64) -> Result<DeleteInstructorOutcome> {
    let mut tx = pool.begin().await?;

    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM instructors WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_none() {
        return Ok(DeleteInstructorOutcome::NotFound);
    }

    let active_templates: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM class_templates WHERE instructor_id = ? AND active = 1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if active_templates > 0 {
        // Dropping `tx` rolls back; nothing was written anyway.
        return Ok(DeleteInstructorOutcome::InUse { active_templates });
    }

    let detached = sqlx::query(
        "UPDATE class_templates SET instructor_id = NULL WHERE instructor_id = ? AND active = 0",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query("DELETE FROM instructors WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    tracing::info!(
        instructor_id = id,
        detached_inactive_templates = detached,
        "instructor deleted"
    );
    Ok(DeleteInstructorOutcome::Deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn new_instructor(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO instructors (name) VALUES (?) RETURNING id")
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn instructor_exists(pool: &SqlitePool, id: i64) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM instructors WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
            == 1
    }

    #[tokio::test]
    async fn names_by_id_returns_every_instructor() {
        let pool = setup().await;
        let iid = new_instructor(&pool, "Judita").await;
        let names = names_by_id(&pool).await.unwrap();
        assert_eq!(names.get(&1).map(String::as_str), Some("Stopa"));
        assert_eq!(names.get(&2).map(String::as_str), Some("Vlada"));
        assert_eq!(names.get(&iid).map(String::as_str), Some("Judita"));
        assert_eq!(names.len(), 3);
    }

    #[tokio::test]
    async fn unreferenced_instructor_is_deleted() {
        let pool = setup().await;
        let iid = new_instructor(&pool, "Temp").await;
        assert_eq!(
            delete_instructor(&pool, iid).await.unwrap(),
            DeleteInstructorOutcome::Deleted
        );
        assert!(!instructor_exists(&pool, iid).await);
    }

    #[tokio::test]
    async fn instructor_of_an_active_template_is_refused_and_kept() {
        let pool = setup().await;
        // V6 seed: instructor 1 teaches the active Mon + Wed templates.
        assert_eq!(
            delete_instructor(&pool, 1).await.unwrap(),
            DeleteInstructorOutcome::InUse {
                active_templates: 2
            }
        );
        assert!(instructor_exists(&pool, 1).await);
    }

    #[tokio::test]
    async fn inactive_template_is_detached_then_instructor_deleted() {
        let pool = setup().await;
        let iid = new_instructor(&pool, "Retired").await;
        let tid: i64 = sqlx::query_scalar(
            "INSERT INTO class_templates (weekday, start_time, instructor_id, capacity, active)
             VALUES (4, '07:00', ?, 10, 0) RETURNING id",
        )
        .bind(iid)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            delete_instructor(&pool, iid).await.unwrap(),
            DeleteInstructorOutcome::Deleted
        );
        assert!(!instructor_exists(&pool, iid).await);
        let detached: Option<i64> =
            sqlx::query_scalar("SELECT instructor_id FROM class_templates WHERE id = ?")
                .bind(tid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            detached, None,
            "the inactive template must survive, detached"
        );
    }

    #[tokio::test]
    async fn missing_instructor_is_not_found() {
        let pool = setup().await;
        assert_eq!(
            delete_instructor(&pool, 999_999).await.unwrap(),
            DeleteInstructorOutcome::NotFound
        );
    }
}
