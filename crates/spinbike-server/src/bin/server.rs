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

    // Populate search_text for any cards that pre-date the V3 migration.
    let backfilled = db::cards::backfill_search_text(&pool).await?;
    if backfilled > 0 {
        tracing::info!("backfilled search_text for {backfilled} cards");
    }

    // Run persistent-booking materialiser once at startup so the DB reflects
    // the full 14-day window before the first request arrives.
    match spinbike_server::jobs::materialiser::sweep(&pool).await {
        Ok(n) if n > 0 => tracing::info!("materialised {n} persistent bookings at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup materialiser sweep failed: {e}"),
    }

    // Charger: every 60s.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
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
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(e) = spinbike_server::jobs::materialiser::sweep(&pool).await {
                    tracing::error!("materialiser sweep failed: {e}");
                }
            }
        });
    }

    spinbike_server::start_server(pool, port, jwt_secret).await?;

    Ok(())
}
