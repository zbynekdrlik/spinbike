use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct PersistentBookingRow {
    pub id: i64,
    pub user_id: i64,
    pub template_id: i64,
    pub created_at: String,
    pub ended_at: Option<String>,
}

/// Create a persistent booking subscription, or return the existing active
/// subscription's id if one already exists for (user, template).
pub async fn create(pool: &SqlitePool, user_id: i64, template_id: i64) -> Result<i64> {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM persistent_bookings
         WHERE user_id = ? AND template_id = ? AND ended_at IS NULL",
    )
    .bind(user_id)
    .bind(template_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO persistent_bookings (user_id, template_id) VALUES (?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(template_id)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// End the active subscription for (user, template). Returns rows affected.
pub async fn end(pool: &SqlitePool, user_id: i64, template_id: i64) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE persistent_bookings SET ended_at = datetime('now')
         WHERE user_id = ? AND template_id = ? AND ended_at IS NULL",
    )
    .bind(user_id)
    .bind(template_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn list_for_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<PersistentBookingRow>> {
    let rows = sqlx::query_as::<_, PersistentBookingRow>(
        "SELECT id, user_id, template_id, created_at, ended_at
         FROM persistent_bookings WHERE user_id = ? AND ended_at IS NULL
         ORDER BY template_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_active_all(pool: &SqlitePool) -> Result<Vec<PersistentBookingRow>> {
    let rows = sqlx::query_as::<_, PersistentBookingRow>(
        "SELECT id, user_id, template_id, created_at, ended_at
         FROM persistent_bookings WHERE ended_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed(pool: &SqlitePool) -> (i64, i64) {
        let uid: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
                .fetch_one(pool)
                .await
                .unwrap();
        // V6 already seeded Mon 18:00 — reuse that template.
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        (uid, tid)
    }

    #[tokio::test]
    async fn create_then_list_returns_row() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let _ = create(&pool, uid, tid).await.unwrap();
        let rows = list_for_user(&pool, uid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].template_id, tid);
    }

    #[tokio::test]
    async fn end_marks_row_and_removes_from_active_list() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let _ = create(&pool, uid, tid).await.unwrap();
        let affected = end(&pool, uid, tid).await.unwrap();
        assert_eq!(affected, 1);
        let rows = list_for_user(&pool, uid).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn create_is_idempotent_when_active_row_exists() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let first = create(&pool, uid, tid).await.unwrap();
        let second = create(&pool, uid, tid).await.unwrap();
        assert_eq!(first, second, "second call must return the same id");
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM persistent_bookings WHERE user_id=? AND template_id=? AND ended_at IS NULL"
        ).bind(uid).bind(tid).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn list_active_all_returns_everything_unended() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let _ = create(&pool, uid, tid).await.unwrap();
        let all = list_active_all(&pool).await.unwrap();
        assert_eq!(all.len(), 1);
    }
}
