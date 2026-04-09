use anyhow::{Context, Result};
use sqlx::SqlitePool;

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .context("Failed to get setting")?;
    Ok(value)
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .context("Failed to set setting")?;
    Ok(())
}

pub async fn get_bike_count(pool: &SqlitePool) -> Result<i64> {
    let val = get_setting(pool, "bike_count")
        .await?
        .unwrap_or_else(|| "10".to_string());
    Ok(val.parse()?)
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

    #[tokio::test]
    async fn settings_seeded() {
        let pool = setup().await;

        let bike_count = get_bike_count(&pool).await.unwrap();
        assert_eq!(bike_count, 10);

        let name = get_setting(&pool, "center_name").await.unwrap().unwrap();
        assert_eq!(name, "Squash Centrum Smizany");
    }

    #[tokio::test]
    async fn upsert_setting() {
        let pool = setup().await;

        // Update existing.
        set_setting(&pool, "bike_count", "15").await.unwrap();
        assert_eq!(get_bike_count(&pool).await.unwrap(), 15);

        // Insert new.
        set_setting(&pool, "new_key", "hello").await.unwrap();
        assert_eq!(get_setting(&pool, "new_key").await.unwrap().unwrap(), "hello");
    }
}
