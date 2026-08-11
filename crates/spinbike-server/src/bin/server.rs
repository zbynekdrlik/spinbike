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

    // Independent MailHandle for the same background job loop (#311 — the
    // e-mail fallback channel). Same reasoning as `push` above: its own
    // instantiation rather than reaching into `AppState` (built separately
    // inside `start_server`), reading the same `SMTP_*` env so both agree
    // on Enabled/Disabled deterministically.
    let mail = spinbike_server::mail::MailHandle::spawn();

    // Five independent startup passes — none consumes another's output, so
    // run them concurrently instead of one after another (#341): previously
    // the listener only opened after the SUM of all five, instead of after
    // the slowest. Each job's outcome is still checked/logged individually,
    // exactly as before — a join! that merged the 5 outcomes into one log
    // line, or let one job's failure swallow another's, would be worse than
    // the serial version it replaces. backfill_search_text's `?`-propagates
    // behavior is preserved too: its Result is still checked with `?`,
    // right after every job has run, instead of before the other 4 even
    // start (a narrow, deliberate behavior change — the other 4 are
    // independent of backfill's success, and each already logs its own
    // failure non-fatally, so a broken DB fails them too either way).
    let (
        backfill_result,
        materialiser_result,
        charger_result,
        token_purge_result,
        notifications_result,
    ) = tokio::join!(
        db::users::backfill_search_text(&pool),
        spinbike_server::jobs::materialiser::sweep(&pool),
        spinbike_server::jobs::charger::tick(&pool),
        spinbike_server::jobs::token_purge::tick(&pool),
        spinbike_server::jobs::notifications::tick(&pool, &push, &mail),
    );

    let backfilled = backfill_result?;
    if backfilled > 0 {
        tracing::info!("backfilled search_text for {backfilled} users");
    }

    match materialiser_result {
        Ok(s) if s.created > 0 => {
            tracing::info!("materialised {} persistent bookings at startup", s.created)
        }
        Ok(_) => {}
        Err(e) => tracing::error!("startup materialiser sweep failed: {e}"),
    }

    match charger_result {
        Ok(n) if n > 0 => tracing::info!("charged {n} bookings at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup charger tick failed: {e}"),
    }

    match token_purge_result {
        Ok(n) if n > 0 => tracing::info!("login_tokens purge removed {n} rows at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup login_tokens purge failed: {e}"),
    }

    match notifications_result {
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
    // aligned hour. `notifications::tick` takes extra `&PushHandle`/
    // `&MailHandle` (#311 added the latter, the e-mail fallback channel)
    // that `spawn_daily_job`'s signature doesn't carry, so both are
    // captured by the closure instead of widening the helper for one
    // caller (#299).
    {
        let push = push.clone();
        let mail = mail.clone();
        spinbike_server::jobs::spawn_daily_job(
            pool.clone(),
            spinbike_server::jobs::notifications::DAILY_RUN_HOUR,
            "push notifications",
            "sent",
            move |pool| {
                let push = push.clone();
                let mail = mail.clone();
                async move { spinbike_server::jobs::notifications::tick(&pool, &push, &mail).await }
            },
        );
    }

    spinbike_server::start_server(pool, port, jwt_secret).await?;

    Ok(())
}
