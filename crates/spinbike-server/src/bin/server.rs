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

    let jwt_secret = match std::env::var("JWT_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => {
            tracing::warn!(
                "JWT_SECRET not set — using insecure default. DO NOT use in production!"
            );
            "dev-secret-change-in-production".to_string()
        }
    };

    let pool = db::create_pool(&PathBuf::from(&db_path)).await?;
    db::run_migrations(&pool).await?;

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

    // login_tokens purge: daily. Pure housekeeping (#119) — no need to run
    // more often than that at fitness-center scale.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await; // first tick fires immediately; ignore (startup already ran it above).
            loop {
                interval.tick().await;
                match spinbike_server::jobs::token_purge::tick(&pool).await {
                    Ok(n) if n > 0 => tracing::info!("login_tokens purge removed {n} rows"),
                    Ok(_) => {}
                    Err(e) => tracing::error!("login_tokens purge failed: {e}"),
                }
            }
        });
    }

    spinbike_server::start_server(pool, port, jwt_secret).await?;

    Ok(())
}
