pub mod charger;
pub mod materialiser;
pub mod notifications;
pub mod token_purge;

use sqlx::SqlitePool;
use std::future::Future;

/// Spawn a daily background job aligned to a fixed Bratislava-local
/// wall-clock hour.
///
/// `notifications` (#264) and `token_purge` (#297) each independently grew
/// the same ~20-line sleep-loop in `bin/server.rs` — compute the delay to
/// the next occurrence of the job's own `DAILY_RUN_HOUR` via
/// `util::duration_until_next_bratislava_hour`, sleep, run `tick`, log,
/// loop. `.claude/rules/daily-job-scheduling.md` even told a future author
/// to copy one of those blocks verbatim, which would only grow the
/// divergence. #299 extracts the shared shape here — a future scheduling,
/// logging, or error-handling change (jitter, a metrics counter,
/// panic-catching around `tick()`) now lands once instead of N times.
///
/// `hour` is a PARAMETER, not owned by this function — each job keeps its
/// own `pub const DAILY_RUN_HOUR: u32` in its own module (see
/// `daily-job-scheduling.md` for why: picking a non-colliding hour per job
/// is a per-job decision, not a scheduling-mechanism one).
///
/// `job_name` is used only for log lines — the debug line on sleep and the
/// info/error line after `tick()` — as `"{job_name}: sleeping until the
/// next aligned daily run"` / `"{job_name}: {n}"` / `"{job_name} failed:
/// {e}"`. Pick a name that reads naturally in that shape.
///
/// Does NOT run the job's startup tick — callers still call `<job>::tick`
/// once directly in `main()` BEFORE calling this, so the first observable
/// log line appears right after boot instead of waiting up to 24h for the
/// first aligned hour. This function only owns the recurring sleep-loop.
///
/// `tick` takes an owned `SqlitePool` (cloned once per iteration from the
/// `pool` this function owns), matching neither job's real `tick(&SqlitePool
/// [, &PushHandle]) -> impl Future<...>` signature exactly — every call site
/// wraps its job's `tick` in a small closure: `|pool| async move {
/// token_purge::tick(&pool).await }` for the plain case, or, for a caller
/// with an extra dependency (`notifications::tick`'s `&PushHandle`),
/// `move |pool| { let push = push.clone(); async move {
/// notifications::tick(&pool, &push).await } }` — the helper's own
/// signature never widens for one caller's extra argument.
pub fn spawn_daily_job<F, Fut>(pool: SqlitePool, hour: u32, job_name: &'static str, tick: F)
where
    F: Fn(SqlitePool) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<usize>> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            let delay = crate::util::duration_until_next_bratislava_hour(
                crate::util::now_bratislava(),
                hour,
            );
            tracing::debug!(
                delay_secs = delay.as_secs(),
                hour,
                "{job_name}: sleeping until the next aligned daily run"
            );
            tokio::time::sleep(delay).await;
            match tick(pool.clone()).await {
                Ok(n) if n > 0 => tracing::info!("{job_name}: {n}"),
                Ok(_) => {}
                Err(e) => tracing::error!("{job_name} failed: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    /// Regression coverage for #299: `spawn_daily_job`'s loop body computes
    /// its delay via `crate::util::duration_until_next_bratislava_hour(now,
    /// hour)` — the exact call site both `notifications` (#264) and
    /// `token_purge` (#297) used directly before this helper existed (see
    /// `token_purge.rs`'s own `daily_run_hour_*` tests for the single-job
    /// version of this coverage). The loop itself lives inside a
    /// `tokio::spawn` future and can't be exercised directly, so this pins
    /// the arithmetic at both real job hours instead.
    #[test]
    fn daily_run_hour_before_target_is_later_today_for_both_job_hours() {
        let now = NaiveDate::from_ymd_opt(2026, 8, 8)
            .unwrap()
            .and_hms_opt(1, 15, 0)
            .unwrap();

        // token_purge (hour=4): 01:15 -> 04:00 same day = 2h45m.
        let delay = crate::util::duration_until_next_bratislava_hour(
            now,
            super::token_purge::DAILY_RUN_HOUR,
        );
        assert_eq!(delay.as_secs(), 2 * 3600 + 45 * 60);

        // notifications (hour=9): 01:15 -> 09:00 same day = 7h45m.
        let delay = crate::util::duration_until_next_bratislava_hour(
            now,
            super::notifications::DAILY_RUN_HOUR,
        );
        assert_eq!(delay.as_secs(), 7 * 3600 + 45 * 60);
    }

    #[test]
    fn daily_run_hour_at_or_after_target_rolls_to_tomorrow_for_both_job_hours() {
        // Exactly at token_purge's hour (4) — also before notifications' (9).
        let now = NaiveDate::from_ymd_opt(2026, 8, 8)
            .unwrap()
            .and_hms_opt(4, 0, 0)
            .unwrap();

        let delay = crate::util::duration_until_next_bratislava_hour(
            now,
            super::token_purge::DAILY_RUN_HOUR,
        );
        assert_eq!(
            delay.as_secs(),
            24 * 3600,
            "token_purge must roll to tomorrow"
        );

        let delay = crate::util::duration_until_next_bratislava_hour(
            now,
            super::notifications::DAILY_RUN_HOUR,
        );
        assert_eq!(
            delay.as_secs(),
            5 * 3600,
            "notifications' hour (9) hasn't passed yet — later today"
        );
    }
}
