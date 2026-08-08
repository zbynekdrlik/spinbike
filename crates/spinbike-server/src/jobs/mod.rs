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
/// `job_name` is used for ALL log lines — the debug line on sleep and the
/// error line after `tick()` — as `"{job_name}: sleeping until the next
/// aligned daily run"` / `"{job_name} failed: {e}"`. Pick a name that reads
/// naturally in that shape.
///
/// `unit` is used ONLY in the success line, as `"{job_name}: {n} {unit}"`
/// (e.g. `job_name = "login_tokens purge"`, `unit = "rows removed"` →
/// `"login_tokens purge: 5 rows removed"`) — a separate parameter rather
/// than folding a noun into `job_name` itself, because `job_name` is reused
/// verbatim in the sleep/error lines where a success-only phrase like
/// "rows removed" would read as broken grammar (a review on #299 caught
/// this: collapsing both jobs' bespoke `"{n} rows removed"` / `"sent {n}
/// notifications"` wording into a single job-identity string lost what `n`
/// actually counts).
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
/// signature never widens for one caller's extra argument (this is what
/// `unit` is NOT — it is a plain `&'static str`, not a dependency, so it
/// stays a direct parameter).
pub fn spawn_daily_job<F, Fut>(
    pool: SqlitePool,
    hour: u32,
    job_name: &'static str,
    unit: &'static str,
    tick: F,
) where
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
                Ok(n) if n > 0 => tracing::info!("{job_name}: {n} {unit}"),
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

    /// Captures `tracing` output emitted while `f` runs, via an in-memory
    /// `MakeWriter` — `tracing-subscriber`'s `fmt` layer is already a
    /// runtime dependency (`bin/server.rs` uses it to configure the real
    /// process subscriber), so this needs no new dev-dependency.
    /// `tracing::subscriber::set_default` scopes the subscriber to the
    /// current OS thread; `#[tokio::test]`'s default runtime is
    /// single-threaded, so a `tokio::spawn`ed task polled from inside `f`
    /// still runs on the same thread and is captured too.
    async fn capture_tracing_output<F, Fut>(f: F) -> String
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct BufWriter(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for BufWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let buf = BufWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .finish();

        let _guard = tracing::subscriber::set_default(subscriber);
        f().await;

        String::from_utf8(buf.0.lock().unwrap().clone()).expect("log output must be valid utf8")
    }

    /// Spawns `spawn_daily_job(pool, hour, job_name, unit, tick)` against a
    /// fresh in-memory pool with a paused virtual clock, then drives the
    /// clock past the computed delay and yields enough times for the woken
    /// task to actually run `tick` once. Shared by all three
    /// `spawn_daily_job_*` tests below — each independently reimplemented
    /// this exact pool/pause/spawn/yield/advance/yield dance in an earlier
    /// draft of this PR, and each independently had to work around the SAME
    /// two timing gotchas before landing on the sequence below (a review on
    /// #299 flagged the duplication and both gotchas are cheap insurance
    /// against silently reintroducing either):
    ///
    /// - `tokio::time::pause()` must run AFTER `create_memory_pool()`, not
    ///   before / not via `#[tokio::test(start_paused = true)]` — pausing
    ///   first let tokio's auto-advance-when-idle race ahead of the pool's
    ///   own connect-acquire deadline and fail with "pool timed out while
    ///   waiting for an open connection" (fixed in `7e5719f`).
    /// - a bare `tokio::time::advance` before ANY yield never wakes the
    ///   spawned task's `sleep`, because a freshly `tokio::spawn`ed task
    ///   hasn't been polled yet and so hasn't registered that timer —
    ///   `yield_now()` first lets the executor poll it up to its first
    ///   `.await` (fixed in `8865732`).
    async fn spawn_and_drive_one_tick<F, Fut>(
        hour: u32,
        job_name: &'static str,
        unit: &'static str,
        tick: F,
    ) where
        F: Fn(SqlitePool) -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<usize>> + Send + 'static,
    {
        let pool = crate::db::create_memory_pool().await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        tokio::time::pause();

        let approx_delay =
            crate::util::duration_until_next_bratislava_hour(crate::util::now_bratislava(), hour);

        super::spawn_daily_job(pool, hour, job_name, unit, tick);

        tokio::task::yield_now().await;
        tokio::time::advance(approx_delay + std::time::Duration::from_secs(2)).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    /// Regression coverage for #299: cargo-mutants surfaced 5 surviving
    /// mutants, all on the same line — `Ok(n) if n > 0 => tracing::info!`
    /// mutated to `true`, `false`, `n < 0`, `n == 0`, `n >= 0` were all
    /// MISSED, because `spawn_daily_job_runs_tick_after_the_computed_delay`
    /// below always returns `Ok(0)` from `tick`, so the guard's outcome was
    /// never actually distinguished by any assertion.
    ///
    /// This test (positive count, expect the info log) and the next one
    /// (zero count, expect NO info log) together kill all 5: for `n=5`,
    /// `n>0`/`n>=0` agree with the original (log fires) while
    /// `false`/`n<0`/`n==0` disagree (no log); for `n=0`, `n>0`/`n<0` agree
    /// with the original (no log) while `true`/`n==0`/`n>=0` disagree (log
    /// fires) — so between the two cases every mutant produces at least one
    /// observably different log outcome. Each test also asserts `tick`
    /// itself ran exactly once, so a broken spawn/sleep/call path can't
    /// hide behind a vacuously-true "log absent" assertion.
    #[tokio::test]
    async fn spawn_daily_job_logs_info_when_tick_returns_a_positive_count() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in_job = calls.clone();

        let output = capture_tracing_output(move || {
            spawn_and_drive_one_tick(17, "capture job pos", "widgets", move |_pool| {
                let calls = calls_in_job.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(5)
                }
            })
        })
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 1, "tick must have run once");
        assert!(
            output.contains("capture job pos: 5 widgets"),
            "expected the positive-count info log, got: {output:?}"
        );
    }

    /// See `spawn_daily_job_logs_info_when_tick_returns_a_positive_count`
    /// above — the zero-count half of the same mutation-gap coverage.
    #[tokio::test]
    async fn spawn_daily_job_does_not_log_info_when_tick_returns_zero() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in_job = calls.clone();

        let output = capture_tracing_output(move || {
            spawn_and_drive_one_tick(18, "capture job zero", "widgets", move |_pool| {
                let calls = calls_in_job.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(0)
                }
            })
        })
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 1, "tick must have run once");
        assert!(
            !output.contains("capture job zero:"),
            "did not expect an info log for a zero count, got: {output:?}"
        );
    }

    /// Regression coverage for #299: proves `spawn_daily_job` actually
    /// RUNS its `tick` closure after sleeping — the two
    /// `daily_run_hour_*` tests near the top of this module only pin the
    /// delay arithmetic, which leaves the function's own body (spawn the
    /// loop, sleep, call `tick`) unexercised. A mutation run caught
    /// exactly this gap: replacing the whole function body with `()` was
    /// MISSED (no test failed) until this test was added.
    ///
    /// Uses `spawn_and_drive_one_tick`'s paused virtual clock — see its own
    /// doc comment for why plain `#[tokio::test(start_paused = true)]`
    /// does NOT work here (it was tried and reverted in `7e5719f`).
    #[tokio::test]
    async fn spawn_daily_job_runs_tick_after_the_computed_delay() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_in_job = calls.clone();

        // Arbitrary hour/unit, unrelated to either real job's
        // DAILY_RUN_HOUR — this test only cares that `spawn_daily_job`
        // eventually calls `tick`, not which hour or unit.
        spawn_and_drive_one_tick(17, "test job", "widgets", move |_pool| {
            let calls = calls_in_job.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(0)
            }
        })
        .await;

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "spawn_daily_job must call tick once after its sleep elapses"
        );
    }
}
