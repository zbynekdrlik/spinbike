//! T-4h charger — full impl in Task 7.
use anyhow::Result;
use sqlx::SqlitePool;

pub async fn tick(_pool: &SqlitePool) -> Result<usize> {
    Ok(0)
}
