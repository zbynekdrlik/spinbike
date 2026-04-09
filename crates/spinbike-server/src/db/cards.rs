use anyhow::{Context, Result};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CardRow {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: i64,
    pub credit: f64,
    pub allow_debit: i64,
    pub created_at: String,
}

pub async fn create_card(pool: &SqlitePool, barcode: &str) -> Result<i64> {
    let id = sqlx::query_scalar("INSERT INTO cards (barcode) VALUES (?) RETURNING id")
        .bind(barcode)
        .fetch_one(pool)
        .await
        .context("Failed to create card")?;
    Ok(id)
}

pub async fn get_card_by_barcode(pool: &SqlitePool, barcode: &str) -> Result<Option<CardRow>> {
    let card = sqlx::query_as::<_, CardRow>("SELECT * FROM cards WHERE barcode = ?")
        .bind(barcode)
        .fetch_optional(pool)
        .await
        .context("Failed to get card by barcode")?;
    Ok(card)
}

pub async fn get_card_by_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<CardRow>> {
    let cards = sqlx::query_as::<_, CardRow>("SELECT * FROM cards WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(pool)
        .await
        .context("Failed to get cards for user")?;
    Ok(cards)
}

pub async fn link_card_to_user(pool: &SqlitePool, card_id: i64, user_id: i64) -> Result<()> {
    sqlx::query("UPDATE cards SET user_id = ? WHERE id = ?")
        .bind(user_id)
        .bind(card_id)
        .execute(pool)
        .await
        .context("Failed to link card to user")?;
    Ok(())
}

pub async fn update_credit(pool: &SqlitePool, card_id: i64, delta: f64) -> Result<()> {
    sqlx::query("UPDATE cards SET credit = credit + ? WHERE id = ?")
        .bind(delta)
        .bind(card_id)
        .execute(pool)
        .await
        .context("Failed to update credit")?;
    Ok(())
}

pub async fn set_blocked(pool: &SqlitePool, card_id: i64, blocked: bool) -> Result<()> {
    sqlx::query("UPDATE cards SET blocked = ? WHERE id = ?")
        .bind(blocked as i64)
        .bind(card_id)
        .execute(pool)
        .await
        .context("Failed to set blocked status")?;
    Ok(())
}

pub async fn set_allow_debit(pool: &SqlitePool, card_id: i64, allow: bool) -> Result<()> {
    sqlx::query("UPDATE cards SET allow_debit = ? WHERE id = ?")
        .bind(allow as i64)
        .bind(card_id)
        .execute(pool)
        .await
        .context("Failed to set allow_debit")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::users::create_user;
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn create_and_get_card() {
        let pool = setup().await;

        let card_id = create_card(&pool, "CARD-001").await.unwrap();
        let card = get_card_by_barcode(&pool, "CARD-001")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(card.id, card_id);
        assert_eq!(card.barcode, "CARD-001");
        assert!(card.user_id.is_none());
        assert_eq!(card.credit, 0.0);
    }

    #[tokio::test]
    async fn update_credit_add_and_subtract() {
        let pool = setup().await;

        let card_id = create_card(&pool, "CARD-002").await.unwrap();

        update_credit(&pool, card_id, 10.0).await.unwrap();
        let card = get_card_by_barcode(&pool, "CARD-002")
            .await
            .unwrap()
            .unwrap();
        assert!((card.credit - 10.0).abs() < f64::EPSILON);

        update_credit(&pool, card_id, -3.5).await.unwrap();
        let card = get_card_by_barcode(&pool, "CARD-002")
            .await
            .unwrap()
            .unwrap();
        assert!((card.credit - 6.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn link_card_to_user_test() {
        let pool = setup().await;

        let user_id = create_user(&pool, "u@test.com", None, "U", None, "customer", None, None)
            .await
            .unwrap();
        let card_id = create_card(&pool, "CARD-003").await.unwrap();

        link_card_to_user(&pool, card_id, user_id).await.unwrap();

        let cards = get_card_by_user(&pool, user_id).await.unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, card_id);
    }
}
