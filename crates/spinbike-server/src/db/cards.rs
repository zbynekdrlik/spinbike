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
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
}

pub async fn create_card(pool: &SqlitePool, barcode: &str) -> Result<i64> {
    let id =
        sqlx::query_scalar("INSERT INTO cards (barcode, allow_debit) VALUES (?, 1) RETURNING id")
            .bind(barcode)
            .fetch_one(pool)
            .await
            .context("Failed to create card")?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_card_with_info(
    pool: &SqlitePool,
    barcode: &str,
    credit: f64,
    first_name: Option<&str>,
    last_name: Option<&str>,
    company: Option<&str>,
    phone: Option<&str>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO cards (barcode, credit, allow_debit, first_name, last_name, company, phone)
         VALUES (?, ?, 1, ?, ?, ?, ?) RETURNING id",
    )
    .bind(barcode)
    .bind(credit)
    .bind(first_name)
    .bind(last_name)
    .bind(company)
    .bind(phone)
    .fetch_one(pool)
    .await
    .context("Failed to create card with info")?;
    Ok(id)
}

pub async fn update_card_info(
    pool: &SqlitePool,
    card_id: i64,
    first_name: Option<&str>,
    last_name: Option<&str>,
    company: Option<&str>,
    phone: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE cards SET first_name = ?, last_name = ?, company = ?, phone = ? WHERE id = ?",
    )
    .bind(first_name)
    .bind(last_name)
    .bind(company)
    .bind(phone)
    .bind(card_id)
    .execute(pool)
    .await
    .context("Failed to update card info")?;
    Ok(())
}

pub async fn list_all_cards(pool: &SqlitePool) -> Result<Vec<CardRow>> {
    let cards = sqlx::query_as::<_, CardRow>("SELECT * FROM cards ORDER BY barcode")
        .fetch_all(pool)
        .await
        .context("Failed to list cards")?;
    Ok(cards)
}

/// Search cards by partial match across barcode, first_name, last_name, company, phone.
/// Barcode prefix matches sort first; remaining rows sort by last/first name (nulls last).
/// Empty/whitespace query returns an empty Vec. Includes blocked cards so staff can unblock.
/// ASCII case-insensitive (SQLite LIKE default). Slovak diacritics match literally —
/// if that becomes a problem, revisit with the ICU extension.
pub async fn search_cards(pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<CardRow>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let like = format!("%{q}%");
    let prefix = format!("{q}%");
    let cards = sqlx::query_as::<_, CardRow>(
        "SELECT * FROM cards
         WHERE barcode LIKE ?
            OR IFNULL(first_name, '') LIKE ?
            OR IFNULL(last_name, '') LIKE ?
            OR IFNULL(company, '') LIKE ?
            OR IFNULL(phone, '') LIKE ?
         ORDER BY
           CASE WHEN barcode LIKE ? THEN 0 ELSE 1 END,
           last_name IS NULL, last_name ASC,
           first_name IS NULL, first_name ASC,
           barcode ASC
         LIMIT ?",
    )
    .bind(&like)
    .bind(&like)
    .bind(&like)
    .bind(&like)
    .bind(&like)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to search cards")?;
    Ok(cards)
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

/// Round a monetary value to 2 decimal places to mitigate f64 precision issues.
pub fn round_cents(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub async fn update_credit(pool: &SqlitePool, card_id: i64, delta: f64) -> Result<()> {
    let rounded_delta = round_cents(delta);
    sqlx::query("UPDATE cards SET credit = ROUND(credit + ?, 2) WHERE id = ?")
        .bind(rounded_delta)
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

    async fn seed_search_fixtures(pool: &SqlitePool) {
        create_card_with_info(
            pool,
            "70705973",
            0.0,
            Some("Zbyněk"),
            Some("Drlík"),
            Some("NewLevel"),
            Some("+421900111222"),
        )
        .await
        .unwrap();
        create_card_with_info(
            pool,
            "70704440",
            0.0,
            Some("Stevo"),
            Some("Žumerling"),
            Some("Squash Centrum"),
            None,
        )
        .await
        .unwrap();
        create_card_with_info(
            pool,
            "70700001",
            0.0,
            Some("Anna"),
            Some("Nováková"),
            Some("NewLevel"),
            Some("+421900333444"),
        )
        .await
        .unwrap();
        create_card(pool, "99999999").await.unwrap();
    }

    #[tokio::test]
    async fn search_empty_query_returns_empty() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        assert!(search_cards(&pool, "", 10).await.unwrap().is_empty());
        assert!(search_cards(&pool, "   ", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_by_barcode_tail() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        let results = search_cards(&pool, "5973", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].barcode, "70705973");
    }

    #[tokio::test]
    async fn search_by_first_name_partial() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        let results = search_cards(&pool, "Ann", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].barcode, "70700001");
    }

    #[tokio::test]
    async fn search_by_last_name_partial() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        let results = search_cards(&pool, "Drl", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].barcode, "70705973");
    }

    #[tokio::test]
    async fn search_by_company_returns_multiple() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        let results = search_cards(&pool, "NewLevel", 10).await.unwrap();
        assert_eq!(results.len(), 2);
        let barcodes: Vec<_> = results.iter().map(|c| c.barcode.as_str()).collect();
        assert!(barcodes.contains(&"70705973"));
        assert!(barcodes.contains(&"70700001"));
    }

    #[tokio::test]
    async fn search_by_phone_partial() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        let results = search_cards(&pool, "333444", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].barcode, "70700001");
    }

    #[tokio::test]
    async fn search_case_insensitive_ascii() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        let lower = search_cards(&pool, "newlevel", 10).await.unwrap();
        let upper = search_cards(&pool, "NEWLEVEL", 10).await.unwrap();
        assert_eq!(lower.len(), 2);
        assert_eq!(upper.len(), 2);
    }

    #[tokio::test]
    async fn search_limit_honored() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;
        let results = search_cards(&pool, "NewLevel", 1).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn search_barcode_prefix_sorts_first() {
        let pool = setup().await;
        create_card_with_info(&pool, "12345", 0.0, Some("Aaa"), Some("Aaa"), None, None)
            .await
            .unwrap();
        create_card_with_info(
            &pool,
            "99999",
            0.0,
            Some("Aaa"),
            Some("Aaa"),
            Some("12345 Inc"),
            None,
        )
        .await
        .unwrap();
        let results = search_cards(&pool, "12345", 10).await.unwrap();
        assert_eq!(results.len(), 2);
        // Barcode-prefix match must come first, not the company-match card.
        assert_eq!(results[0].barcode, "12345");
        assert_eq!(results[1].barcode, "99999");
    }

    #[tokio::test]
    async fn search_includes_blocked_cards() {
        let pool = setup().await;
        let id = create_card_with_info(
            &pool,
            "BLOCKED-1",
            0.0,
            Some("Bad"),
            Some("Actor"),
            None,
            None,
        )
        .await
        .unwrap();
        set_blocked(&pool, id, true).await.unwrap();
        let results = search_cards(&pool, "Actor", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].blocked, 1);
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
