//! Periodic housekeeping (#119): delete `login_tokens` rows that can never
//! redeem again (used, or expired) so the table doesn't grow unbounded.
//! Purely a maintenance job — `redeem` already rejects both classes of row,
//! so deleting them changes no auth behavior.

use anyhow::Result;
use sqlx::SqlitePool;

/// Run one purge pass. Returns the number of rows removed.
pub async fn tick(pool: &SqlitePool) -> Result<u64> {
    Ok(crate::db::login_tokens::purge_expired_and_used(pool).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        create_memory_pool,
        login_tokens::{PURPOSE_INVITE, create_token},
        run_migrations,
    };

    #[tokio::test]
    async fn tick_delegates_to_purge_and_returns_the_removed_count() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES ('t@x', 'T', 'customer') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // One expired (negative TTL), one live.
        create_token(&pool, uid, PURPOSE_INVITE, -10).await.unwrap();
        create_token(&pool, uid, PURPOSE_INVITE, 1_209_600)
            .await
            .unwrap();

        let n = tick(&pool).await.unwrap();
        assert_eq!(n, 1, "tick() must purge exactly the expired row");

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 1, "the live token must remain");
    }
}
