use anyhow::{Context, Result};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TransactionRow {
    pub id: i64,
    pub user_id: Option<i64>,
    pub card_id: Option<i64>,
    pub staff_id: Option<i64>,
    pub service_id: Option<i64>,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
}

pub async fn create_transaction(
    pool: &SqlitePool,
    user_id: Option<i64>,
    card_id: Option<i64>,
    staff_id: Option<i64>,
    service_id: Option<i64>,
    amount: f64,
    action: &str,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action)
         VALUES (?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(user_id)
    .bind(card_id)
    .bind(staff_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .fetch_one(pool)
    .await
    .context("Failed to create transaction")?;
    Ok(id)
}

pub async fn list_transactions_for_card(
    pool: &SqlitePool,
    card_id: i64,
) -> Result<Vec<TransactionRow>> {
    let txns = sqlx::query_as::<_, TransactionRow>(
        "SELECT * FROM transactions WHERE card_id = ? ORDER BY created_at DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .context("Failed to list transactions for card")?;
    Ok(txns)
}

pub async fn list_transactions_for_user(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<TransactionRow>> {
    let txns = sqlx::query_as::<_, TransactionRow>(
        "SELECT * FROM transactions WHERE user_id = ? ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .context("Failed to list transactions for user")?;
    Ok(txns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::cards::create_card;
    use crate::db::users::create_user;
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn create_and_list_transactions() {
        let pool = setup().await;

        let user_id = create_user(
            &pool,
            "tx@test.com",
            None,
            "Tx",
            None,
            "customer",
            None,
            None,
        )
        .await
        .unwrap();
        let card_id = create_card(&pool, "TX-CARD").await.unwrap();

        create_transaction(
            &pool,
            Some(user_id),
            Some(card_id),
            None,
            Some(1),
            5.0,
            "charge",
        )
        .await
        .unwrap();
        create_transaction(
            &pool,
            Some(user_id),
            Some(card_id),
            None,
            Some(1),
            5.0,
            "charge",
        )
        .await
        .unwrap();

        let by_card = list_transactions_for_card(&pool, card_id).await.unwrap();
        assert_eq!(by_card.len(), 2);

        let by_user = list_transactions_for_user(&pool, user_id).await.unwrap();
        assert_eq!(by_user.len(), 2);
        assert_eq!(by_user[0].action, "charge");
    }
}
