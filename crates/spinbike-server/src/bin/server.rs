use std::path::PathBuf;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

use spinbike_server::db;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("spinbike=info".parse()?))
        .init();

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()?;

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "spinbike.db".to_string());

    let test_mode =
        spinbike_server::is_test_mode_from_env(std::env::var("SPINBIKE_TEST_MODE").ok().as_deref());
    let jwt_secret =
        spinbike_server::resolve_jwt_secret(std::env::var("JWT_SECRET").ok().as_deref(), test_mode)
            .map_err(|e| anyhow::anyhow!(e))?;

    let pool = db::create_pool(&PathBuf::from(&db_path)).await?;
    db::run_migrations(&pool).await?;

    // Independent PushHandle for the background job loop below — mirrors how
    // `charger`/`materialiser`/`token_purge` operate on their own `pool.clone()`
    // rather than through `AppState` (built separately, inside `start_server`).
    // Both instantiations read the same `VAPID_PRIVATE_KEY` env var, so they
    // agree on Enabled/Disabled deterministically.
    let push = spinbike_server::push::PushHandle::spawn();

    // Populate search_text for any users that pre-date the migration.
    let backfilled = db::users::backfill_search_text(&pool).await?;
    if backfilled > 0 {
        tracing::info!("backfilled search_text for {backfilled} users");
    }

    // Run persistent-booking materialiser once at startup so the DB reflects
    // the full 14-day window before the first request arrives.
    match spinbike_server::jobs::materialiser::sweep(&pool).await {
        Ok(n) if n > 0 => tracing::info!("materialised {n} persistent bookings at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup materialiser sweep failed: {e}"),
    }

    // Run charger once at startup to cover bookings that became eligible while
    // the server was down — otherwise a restart inside the 4-hour window skips
    // them until the next scheduled tick.
    match spinbike_server::jobs::charger::tick(&pool).await {
        Ok(n) if n > 0 => tracing::info!("charged {n} bookings at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup charger tick failed: {e}"),
    }

    // Run the login_tokens purge once at startup too — cheap, and gives an
    // observable log line right after a deploy instead of waiting a full day.
    match spinbike_server::jobs::token_purge::tick(&pool).await {
        Ok(n) if n > 0 => tracing::info!("login_tokens purge removed {n} rows at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup login_tokens purge failed: {e}"),
    }

    // Run the push-notification evaluation once at startup too (#264) — same
    // reasoning as token_purge above: an observable log line right after a
    // deploy, and covers the window while the server was down.
    match spinbike_server::jobs::notifications::tick(&pool, &push).await {
        Ok(n) if n > 0 => tracing::info!("push: sent {n} notifications at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup push notifications tick failed: {e}"),
    }

    // Charger: every 60s. `Delay` skips back-to-back catch-up ticks if a tick
    // runs long, preventing the same bookings from being reprocessed rapidly.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await; // first tick fires immediately; ignore.
            loop {
                interval.tick().await;
                if let Err(e) = spinbike_server::jobs::charger::tick(&pool).await {
                    tracing::error!("charger tick failed: {e}");
                }
            }
        });
    }

    // Materialiser: every 60 minutes.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(e) = spinbike_server::jobs::materialiser::sweep(&pool).await {
                    tracing::error!("materialiser sweep failed: {e}");
                }
            }
        });
    }

    // login_tokens purge: daily, aligned to a fixed Bratislava-local
    // wall-clock hour (#297 — same fix #264 applied to the sibling
    // `notifications` job below) rather than an uptime-relative
    // `tokio::time::interval`, which pins the purge to whatever moment the
    // server process last restarted (e.g. 03:00 after an overnight deploy)
    // forever. Pure housekeeping (#119) with no customer-visible effect, but
    // the pattern should stay consistent with the other daily job. Startup
    // already ran the job once above, so this loop's first sleep waits for
    // the NEXT occurrence of the aligned hour. `spawn_daily_job` (#299) owns
    // the sleep-loop mechanics — see its doc comment / `.claude/rules/
    // daily-job-scheduling.md` for the DST-safety rationale.
    spinbike_server::jobs::spawn_daily_job(
        pool.clone(),
        spinbike_server::jobs::token_purge::DAILY_RUN_HOUR,
        "login_tokens purge",
        "rows removed",
        |pool| async move { spinbike_server::jobs::token_purge::tick(&pool).await },
    );

    // Push notifications: daily, aligned to a fixed Bratislava-local
    // wall-clock hour (#264 review finding) rather than an uptime-relative
    // interval — the latter would pin customer-visible notifications to
    // whatever moment the server process last restarted (e.g. 03:00 after
    // an overnight deploy) forever. Startup already ran the job once above,
    // so this loop's first sleep waits for the NEXT occurrence of the
    // aligned hour. `notifications::tick` takes an extra `&PushHandle` that
    // `spawn_daily_job`'s signature doesn't carry, so it's captured by the
    // closure instead of widening the helper for one caller (#299).
    {
        let push = push.clone();
        spinbike_server::jobs::spawn_daily_job(
            pool.clone(),
            spinbike_server::jobs::notifications::DAILY_RUN_HOUR,
            "push notifications",
            "sent",
            move |pool| {
                let push = push.clone();
                async move { spinbike_server::jobs::notifications::tick(&pool, &push).await }
            },
        );
    }

    spinbike_server::start_server(pool, port, jwt_secret).await?;

    Ok(())
}
