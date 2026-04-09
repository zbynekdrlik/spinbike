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

    let jwt_secret =
        std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-in-production".to_string());

    let pool = db::create_pool(&PathBuf::from(&db_path)).await?;
    db::run_migrations(&pool).await?;

    spinbike_server::start_server(pool, port, jwt_secret).await?;

    Ok(())
}
