use anyhow::{Context, Result};
use sqlx::SqlitePool;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

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

/// Fold a string to a diacritic-free, lowercase representation used for
/// searchable matching. "Zbyněk Drlík" → "zbynek drlik". NFD-decomposes, drops
/// combining marks, lowercases. Non-Latin scripts are passed through unchanged.
pub fn normalize_search(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// Build the haystack for a card's `search_text` column by concatenating every
/// field a staff member might type into the search box, then normalizing.
pub fn compute_search_text(
    barcode: &str,
    first_name: Option<&str>,
    last_name: Option<&str>,
    company: Option<&str>,
    phone: Option<&str>,
) -> String {
    let combined = format!(
        "{} {} {} {} {}",
        barcode,
        first_name.unwrap_or(""),
        last_name.unwrap_or(""),
        company.unwrap_or(""),
        phone.unwrap_or(""),
    );
    normalize_search(&combined)
}

/// Populate `search_text` for cards where it's empty. Safe to run on every
/// startup — idempotent, and only touches rows that need it (typically after
/// the V3 migration lands on a DB that already had rows).
pub async fn backfill_search_text(pool: &SqlitePool) -> Result<usize> {
    let rows: Vec<CardRow> = sqlx::query_as::<_, CardRow>(
        "SELECT * FROM cards WHERE search_text IS NULL OR search_text = ''",
    )
    .fetch_all(pool)
    .await
    .context("Failed to scan cards for search_text backfill")?;
    let count = rows.len();
    for row in rows {
        let text = compute_search_text(
            &row.barcode,
            row.first_name.as_deref(),
            row.last_name.as_deref(),
            row.company.as_deref(),
            row.phone.as_deref(),
        );
        sqlx::query("UPDATE cards SET search_text = ? WHERE id = ?")
            .bind(&text)
            .bind(row.id)
            .execute(pool)
            .await
            .with_context(|| format!("Failed to backfill search_text for card {}", row.id))?;
    }
    Ok(count)
}

pub async fn create_card(pool: &SqlitePool, barcode: &str) -> Result<i64> {
    let search_text = compute_search_text(barcode, None, None, None, None);
    let id = sqlx::query_scalar(
        "INSERT INTO cards (barcode, allow_debit, search_text) VALUES (?, 1, ?) RETURNING id",
    )
    .bind(barcode)
    .bind(&search_text)
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
    let search_text = compute_search_text(barcode, first_name, last_name, company, phone);
    let id = sqlx::query_scalar(
        "INSERT INTO cards (barcode, credit, allow_debit, first_name, last_name, company, phone, search_text)
         VALUES (?, ?, 1, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(barcode)
    .bind(credit)
    .bind(first_name)
    .bind(last_name)
    .bind(company)
    .bind(phone)
    .bind(&search_text)
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
    // Look up the barcode so we can recompute search_text.
    let barcode: String = sqlx::query_scalar("SELECT barcode FROM cards WHERE id = ?")
        .bind(card_id)
        .fetch_one(pool)
        .await
        .context("Failed to read barcode for card update")?;
    let search_text = compute_search_text(&barcode, first_name, last_name, company, phone);
    sqlx::query(
        "UPDATE cards SET first_name = ?, last_name = ?, company = ?, phone = ?, search_text = ?
         WHERE id = ?",
    )
    .bind(first_name)
    .bind(last_name)
    .bind(company)
    .bind(phone)
    .bind(&search_text)
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

/// Search cards by partial match. Diacritic- and case-insensitive: the query
/// is folded via `normalize_search` and compared against the pre-computed
/// `search_text` column, so "zbyne" finds "Zbyněk" and "drlik" finds "Drlík".
/// Barcode prefix matches sort first. Empty/whitespace query → empty Vec.
/// Includes blocked cards so staff can find them to unblock.
pub async fn search_cards(pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<CardRow>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let needle = normalize_search(q);
    let like = format!("%{needle}%");
    let prefix = format!("{q}%");
    let cards = sqlx::query_as::<_, CardRow>(
        "SELECT * FROM cards
         WHERE search_text LIKE ?
         ORDER BY
           CASE WHEN barcode LIKE ? THEN 0 ELSE 1 END,
           last_name IS NULL, last_name ASC,
           first_name IS NULL, first_name ASC,
           barcode ASC
         LIMIT ?",
    )
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

/// Return the latest `valid_until` across a card's transactions, or `None` if
/// the card has never had a monthly-pass purchase. Callers compare against
/// today's date to determine whether the pass is active or expired.
pub async fn get_card_pass_valid_until(
    pool: &SqlitePool,
    card_id: i64,
) -> Result<Option<chrono::NaiveDate>> {
    let row: Option<(Option<chrono::NaiveDate>,)> = sqlx::query_as(
        "SELECT MAX(valid_until) FROM transactions
         WHERE card_id = ? AND valid_until IS NOT NULL",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .context("Failed to compute pass valid_until")?;
    Ok(row.and_then(|(d,)| d))
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

    /// Real-world bug: Zbyněk (user's own card) was not found by typing "zbyne".
    /// Diacritic folding must treat ě as e, ž as z, etc.
    #[tokio::test]
    async fn search_folds_slovak_diacritics() {
        let pool = setup().await;
        seed_search_fixtures(&pool).await;

        // ě → e
        let hits = search_cards(&pool, "zbyne", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].barcode, "70705973");

        // í → i
        let hits = search_cards(&pool, "drlik", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].barcode, "70705973");

        // ž → z (last name search on the Žumerling fixture)
        let hits = search_cards(&pool, "zumer", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].barcode, "70704440");

        // Query WITH diacritics must still match stored NFD-folded data.
        let hits = search_cards(&pool, "Žumer", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn normalize_search_folds_lowercase_and_diacritics() {
        assert_eq!(normalize_search("Zbyněk"), "zbynek");
        assert_eq!(normalize_search("Žumerling"), "zumerling");
        assert_eq!(normalize_search("Drlík"), "drlik");
        assert_eq!(normalize_search("Ľuboš"), "lubos");
        // Non-Latin scripts pass through (lowercase applied where the locale has
        // a rule; combining marks filtered).
        assert_eq!(normalize_search("ABC"), "abc");
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

    /// Simulates a DB that predates the V3 migration: cards exist with
    /// empty search_text. Backfill must report how many it fixed AND
    /// actually populate them, else diacritic-folded search silently
    /// returns no results for those rows after upgrade.
    #[tokio::test]
    async fn backfill_populates_empty_search_text_and_reports_count() {
        let pool = setup().await;

        // Insert three rows with empty search_text directly, bypassing the
        // normal create_card path which auto-computes it.
        for (barcode, first, last) in [
            ("LEGACY-1", "Zbyněk", "Drlík"),
            ("LEGACY-2", "Stevo", "Žumerling"),
            ("LEGACY-3", "Anna", "Nováková"),
        ] {
            sqlx::query(
                "INSERT INTO cards (barcode, first_name, last_name, search_text) VALUES (?, ?, ?, '')",
            )
            .bind(barcode)
            .bind(first)
            .bind(last)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Add a row that already has search_text so we can prove backfill
        // skips it (the "WHERE search_text = ''" filter is load-bearing).
        create_card_with_info(&pool, "MODERN-1", 0.0, Some("Eva"), Some("Mod"), None, None)
            .await
            .unwrap();

        let count = backfill_search_text(&pool).await.unwrap();
        assert_eq!(
            count, 3,
            "must report exact count — kills Ok(0) and Ok(1) mutants"
        );

        // Behavior: the three legacy rows now have populated search_text and
        // diacritic-folded search finds them.
        let hits = search_cards(&pool, "zbyne", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].barcode, "LEGACY-1");
        let hits = search_cards(&pool, "zumer", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].barcode, "LEGACY-2");
        let hits = search_cards(&pool, "novak", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].barcode, "LEGACY-3");
    }

    /// A second backfill call on a clean DB must be a no-op — proves the
    /// WHERE filter works and we aren't churning rows on every startup.
    #[tokio::test]
    async fn backfill_is_idempotent() {
        let pool = setup().await;
        create_card_with_info(&pool, "NEW-1", 0.0, Some("A"), Some("B"), None, None)
            .await
            .unwrap();
        let count = backfill_search_text(&pool).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn pass_valid_until_none_when_no_pass_purchased() {
        let pool = setup().await;
        let card_id = create_card(&pool, "NO-PASS").await.unwrap();
        let result = get_card_pass_valid_until(&pool, card_id).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn pass_valid_until_returns_max_across_multiple_passes() {
        use crate::db::transactions::create_transaction_with_valid_until;
        let pool = setup().await;
        let card_id = create_card(&pool, "MULTI-PASS").await.unwrap();
        // Two pass purchases — the later one wins.
        let d1 = chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let d2 = chrono::NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        create_transaction_with_valid_until(
            &pool,
            None,
            Some(card_id),
            None,
            Some(1),
            -35.0,
            "charge",
            Some(d1),
        )
        .await
        .unwrap();
        create_transaction_with_valid_until(
            &pool,
            None,
            Some(card_id),
            None,
            Some(1),
            -35.0,
            "charge",
            Some(d2),
        )
        .await
        .unwrap();

        let result = get_card_pass_valid_until(&pool, card_id).await.unwrap();
        assert_eq!(
            result,
            Some(d2),
            "MAX(valid_until) must win regardless of insert order"
        );
    }

    #[tokio::test]
    async fn pass_valid_until_ignores_non_pass_transactions() {
        use crate::db::transactions::create_transaction;
        let pool = setup().await;
        let card_id = create_card(&pool, "CHARGE-ONLY").await.unwrap();
        create_transaction(&pool, None, Some(card_id), None, Some(1), -5.0, "charge")
            .await
            .unwrap();
        create_transaction(&pool, None, Some(card_id), None, None, 20.0, "topup")
            .await
            .unwrap();
        let result = get_card_pass_valid_until(&pool, card_id).await.unwrap();
        assert_eq!(
            result, None,
            "non-pass transactions must not produce a valid_until"
        );
    }
}
