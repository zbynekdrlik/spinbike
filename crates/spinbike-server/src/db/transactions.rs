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
    // ISO-8601 date (YYYY-MM-DD). Set only for monthly-pass charges.
    pub valid_until: Option<chrono::NaiveDate>,
    // Joined from services — None when the transaction wasn't tied to a service.
    #[sqlx(default)]
    pub service_name: Option<String>,
    pub deleted_at: Option<String>,
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

#[allow(clippy::too_many_arguments)]
pub async fn create_transaction_with_valid_until(
    pool: &SqlitePool,
    user_id: Option<i64>,
    card_id: Option<i64>,
    staff_id: Option<i64>,
    service_id: Option<i64>,
    amount: f64,
    action: &str,
    valid_until: Option<chrono::NaiveDate>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, valid_until)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(user_id)
    .bind(card_id)
    .bind(staff_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .bind(valid_until)
    .fetch_one(pool)
    .await
    .context("Failed to create transaction with valid_until")?;
    Ok(id)
}

pub async fn list_transactions_for_card(
    pool: &SqlitePool,
    card_id: i64,
) -> Result<Vec<TransactionRow>> {
    let txns = sqlx::query_as::<_, TransactionRow>(
        "SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
                t.amount, t.action, t.created_at, t.valid_until,
                s.name AS service_name, t.deleted_at
         FROM transactions t
         LEFT JOIN services s ON s.id = t.service_id
         WHERE t.card_id = ?
         ORDER BY t.created_at DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .context("Failed to list transactions for card")?;
    Ok(txns)
}

/// Paginated variant of [`list_transactions_for_card`].
///
/// * `limit`  — number of rows to return; defaults to 10, capped at 500.
/// * `before` — ISO 8601 datetime cursor; when present, only rows with
///   `created_at < before` are returned.
///
/// Soft-deleted rows are included (staff need to see voided entries).
pub async fn list_transactions_for_card_paginated(
    pool: &SqlitePool,
    card_id: i64,
    limit: Option<usize>,
    before: Option<&str>,
) -> Result<Vec<TransactionRow>> {
    let effective_limit = limit.unwrap_or(10).min(500) as i64;

    let txns = match before {
        Some(cursor) => sqlx::query_as::<_, TransactionRow>(
            "SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
                        t.amount, t.action, t.created_at, t.valid_until,
                        s.name AS service_name, t.deleted_at
                 FROM transactions t
                 LEFT JOIN services s ON s.id = t.service_id
                 WHERE t.card_id = ? AND t.created_at < ?
                 ORDER BY t.created_at DESC
                 LIMIT ?",
        )
        .bind(card_id)
        .bind(cursor)
        .bind(effective_limit)
        .fetch_all(pool)
        .await
        .context("Failed to list paginated transactions for card (with cursor)")?,
        None => sqlx::query_as::<_, TransactionRow>(
            "SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
                        t.amount, t.action, t.created_at, t.valid_until,
                        s.name AS service_name, t.deleted_at
                 FROM transactions t
                 LEFT JOIN services s ON s.id = t.service_id
                 WHERE t.card_id = ?
                 ORDER BY t.created_at DESC
                 LIMIT ?",
        )
        .bind(card_id)
        .bind(effective_limit)
        .fetch_all(pool)
        .await
        .context("Failed to list paginated transactions for card")?,
    };
    Ok(txns)
}

pub async fn list_transactions_for_user(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<TransactionRow>> {
    let txns = sqlx::query_as::<_, TransactionRow>(
        "SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
                t.amount, t.action, t.created_at, t.valid_until,
                s.name AS service_name, t.deleted_at
         FROM transactions t
         LEFT JOIN services s ON s.id = t.service_id
         WHERE t.user_id = ?
         ORDER BY t.created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .context("Failed to list transactions for user")?;
    Ok(txns)
}

/// Mark a transaction as voided. Sets `deleted_at` to the current datetime
/// if the row exists and is not already voided. No-op otherwise.
pub async fn soft_delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE transactions SET deleted_at = datetime('now') \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await
    .context("Failed to soft-delete transaction")?;
    Ok(())
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

    #[tokio::test]
    async fn transaction_stores_and_retrieves_valid_until() {
        let pool = setup().await;
        let card_id = create_card(&pool, "VU-1").await.unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        create_transaction_with_valid_until(
            &pool,
            None,
            Some(card_id),
            None,
            Some(1),
            -35.0,
            "charge",
            Some(date),
        )
        .await
        .unwrap();

        let rows = list_transactions_for_card(&pool, card_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].valid_until, Some(date));
    }

    #[tokio::test]
    async fn transaction_without_valid_until_reads_back_as_none() {
        let pool = setup().await;
        let card_id = create_card(&pool, "VU-2").await.unwrap();
        create_transaction(&pool, None, Some(card_id), None, None, 10.0, "topup")
            .await
            .unwrap();
        let rows = list_transactions_for_card(&pool, card_id).await.unwrap();
        assert_eq!(rows[0].valid_until, None);
    }

    #[tokio::test]
    async fn soft_delete_sets_deleted_at() {
        let pool = setup().await;
        let card_id = create_card(&pool, "SD-1").await.unwrap();
        let tx_id = create_transaction(&pool, None, Some(card_id), None, None, 5.0, "topup")
            .await
            .unwrap();

        soft_delete(&pool, tx_id).await.unwrap();

        let deleted_at: Option<String> =
            sqlx::query_scalar("SELECT deleted_at FROM transactions WHERE id = ?")
                .bind(tx_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(deleted_at.is_some(), "deleted_at must be set");
    }

    #[tokio::test]
    async fn soft_delete_is_idempotent_on_missing_row() {
        let pool = setup().await;
        // Non-existent id must not error — no-op.
        soft_delete(&pool, 99999).await.unwrap();
    }

    #[tokio::test]
    async fn list_transactions_returns_deleted_at_flag() {
        let pool = setup().await;
        let card_id = create_card(&pool, "SD-LIST").await.unwrap();
        let tx_id = create_transaction(&pool, None, Some(card_id), None, None, 5.0, "topup")
            .await
            .unwrap();
        soft_delete(&pool, tx_id).await.unwrap();

        let rows = list_transactions_for_card(&pool, card_id).await.unwrap();
        assert_eq!(
            rows.len(),
            1,
            "soft-deleted rows must still appear in history"
        );
        assert!(
            rows[0].deleted_at.is_some(),
            "voided row must expose deleted_at"
        );
    }

    /// Insert `n` transactions for `card_id` with created_at spaced one second apart
    /// starting from `2026-01-01T00:00:00`. Returns timestamps oldest→newest.
    async fn insert_n_transactions(pool: &SqlitePool, card_id: i64, n: usize) -> Vec<String> {
        let mut timestamps = Vec::with_capacity(n);
        for i in 0..n {
            let ts = format!("2026-01-01T00:{:02}:{:02}", i / 60, i % 60);
            sqlx::query(
                "INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(card_id)
            .bind(1.0_f64)
            .bind("charge")
            .bind(&ts)
            .execute(pool)
            .await
            .unwrap();
            timestamps.push(ts);
        }
        timestamps
    }

    #[tokio::test]
    async fn paginated_default_returns_at_most_10_newest_first() {
        let pool = setup().await;
        let card_id = create_card(&pool, "PAG-1").await.unwrap();
        let timestamps = insert_n_transactions(&pool, card_id, 15).await;

        let rows = list_transactions_for_card_paginated(&pool, card_id, None, None)
            .await
            .unwrap();

        assert_eq!(rows.len(), 10, "default limit must return exactly 10 rows");

        // newest row first: timestamps are "2026-01-01T00:00:00" .. "2026-01-01T00:00:14"
        // The 10 newest are indices 14 down to 5.
        assert_eq!(
            rows[0].created_at, timestamps[14],
            "first row must be the newest"
        );
        assert!(
            rows[0].created_at > rows[1].created_at,
            "rows must be newest-first"
        );
    }

    #[tokio::test]
    async fn paginated_before_cursor_returns_only_older_rows() {
        let pool = setup().await;
        let card_id = create_card(&pool, "PAG-2").await.unwrap();
        let timestamps = insert_n_transactions(&pool, card_id, 5).await;

        // cursor is the 3rd timestamp (index 2, "2026-01-01T00:00:02").
        // Only rows with created_at < cursor must be returned: indices 0 and 1.
        let cursor = &timestamps[2];
        let rows =
            list_transactions_for_card_paginated(&pool, card_id, None, Some(cursor.as_str()))
                .await
                .unwrap();

        assert_eq!(
            rows.len(),
            2,
            "only rows older than cursor must be returned"
        );
        for row in &rows {
            assert!(
                row.created_at < *cursor,
                "every row must be strictly older than the cursor"
            );
        }
    }

    #[tokio::test]
    async fn paginated_explicit_limit_and_cap() {
        let pool = setup().await;
        let card_id = create_card(&pool, "PAG-3").await.unwrap();
        insert_n_transactions(&pool, card_id, 10).await;

        // explicit limit=3 returns 3 rows
        let rows = list_transactions_for_card_paginated(&pool, card_id, Some(3), None)
            .await
            .unwrap();
        assert_eq!(rows.len(), 3, "explicit limit=3 must return exactly 3 rows");

        // limit=1000 is capped at 500; with only 10 rows we get 10 (cap does not inflate)
        let rows_capped = list_transactions_for_card_paginated(&pool, card_id, Some(1000), None)
            .await
            .unwrap();
        // 10 rows exist; effective limit is min(1000,500)=500 → all 10 returned
        assert_eq!(
            rows_capped.len(),
            10,
            "limit=1000 capped to 500; all 10 existing rows returned"
        );
    }
}
