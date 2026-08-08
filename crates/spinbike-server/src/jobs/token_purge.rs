//! Periodic housekeeping (#119): delete `login_tokens` rows that can never
//! redeem again (used, or expired) so the table doesn't grow unbounded.
//! Purely a maintenance job — `redeem` already rejects both classes of row,
//! so deleting them changes no auth behavior.

use anyhow::Result;
use sqlx::SqlitePool;

/// Wall-clock hour (Europe/Bratislava, 0..=23) the daily purge is aligned to
/// (#297 — same pattern #264 established for `jobs::notifications::
/// DAILY_RUN_HOUR`; see `util::duration_until_next_bratislava_hour`, which
/// `bin/server.rs` uses to schedule this). 04:00: well outside any plausible
/// gym operating hours, and offset from the 09:00 push-notification job so
/// the two daily jobs never compete for the DB pool at the same instant.
/// Purely a housekeeping choice — this job has no customer-visible effect at
/// any hour, so any off-peak time would do.
pub const DAILY_RUN_HOUR: u32 = 4;

/// Run one purge pass. Returns the number of rows removed.
///
/// `usize` here (not the DB layer's native `u64`) to match the return type
/// of the sibling job modules (`charger::tick`, `materialiser::sweep`).
pub async fn tick(pool: &SqlitePool) -> Result<usize> {
    let removed = crate::db::login_tokens::purge_expired_and_used(pool).await?;
    Ok(removed as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        create_memory_pool,
        login_tokens::{PURPOSE_INVITE, create_token},
        run_migrations,
    };
    use chrono::NaiveDate;

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

    /// Regression test for #297: `bin/server.rs`'s purge spawn-loop schedules
    /// itself by calling `util::duration_until_next_bratislava_hour(now,
    /// DAILY_RUN_HOUR)` — this pins that exact production call to the real
    /// `DAILY_RUN_HOUR` constant, not just the generic helper (already
    /// covered separately in `util.rs`'s own tests). Before the target hour
    /// today, the delay must land LATER TODAY, not roll to tomorrow — which
    /// is exactly the bug class (#264/#297) a `tokio::time::interval` cannot
    /// express: a restart-pinned interval has no notion of "later today".
    #[test]
    fn daily_run_hour_before_target_is_later_today() {
        let now = NaiveDate::from_ymd_opt(2026, 8, 8)
            .unwrap()
            .and_hms_opt(1, 15, 0)
            .unwrap();
        let delay = crate::util::duration_until_next_bratislava_hour(now, DAILY_RUN_HOUR);
        // 01:15 -> 04:00 same day = 2h45m.
        assert_eq!(delay.as_secs(), 2 * 3600 + 45 * 60);
    }

    /// At/after the target hour, the delay must roll to TOMORROW at
    /// `DAILY_RUN_HOUR` — never zero (which would busy-loop the purge) and
    /// never today's hour again (which already passed).
    #[test]
    fn daily_run_hour_at_or_after_target_rolls_to_tomorrow() {
        let now = NaiveDate::from_ymd_opt(2026, 8, 8)
            .unwrap()
            .and_hms_opt(4, 0, 0)
            .unwrap();
        let delay = crate::util::duration_until_next_bratislava_hour(now, DAILY_RUN_HOUR);
        assert_eq!(delay.as_secs(), 24 * 3600);
    }
}
