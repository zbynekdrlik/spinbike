use anyhow::{Context, Result};
use sqlx::SqlitePool;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use spinbike_core::services::CLASS_VISIT_NAMES_EN;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: Option<String>, // nullable since migration #13
    pub name: String,          // NOT NULL DEFAULT '(no name)'
    pub password_hash: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>, // added in migration #13
    pub role: String,
    pub oauth_provider: Option<String>,
    pub oauth_id: Option<String>,
    pub credit: f64,                 // added in migration #13
    pub card_code: Option<String>,   // added in migration #13
    pub blocked: bool,               // added in migration #13 (stored as INTEGER 0/1)
    pub allow_debit: bool,           // added in migration #13
    pub search_text: Option<String>, // added in migration #13
    pub created_at: String,
    pub deleted_at: Option<String>, // added in migration #15
    pub allow_self_entry: bool,     // added in migration #16
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

/// Build the haystack for a user's `search_text` column by concatenating every
/// field a staff member might type into the search box, then normalizing.
pub fn compute_search_text(
    name: Option<&str>,
    company: Option<&str>,
    card_code: Option<&str>,
) -> String {
    let combined = format!(
        "{} {} {}",
        name.unwrap_or(""),
        company.unwrap_or(""),
        card_code.unwrap_or(""),
    );
    normalize_search(&combined)
}

/// Populate `search_text` for users where it's empty. Safe to run on every
/// startup — idempotent, and only touches rows that need it.
pub async fn backfill_search_text(pool: &SqlitePool) -> Result<usize> {
    let rows: Vec<UserRow> = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE search_text IS NULL OR search_text = ''",
    )
    .fetch_all(pool)
    .await
    .context("Failed to scan users for search_text backfill")?;
    let count = rows.len();
    for row in rows {
        let text = compute_search_text(
            Some(&row.name),
            row.company.as_deref(),
            row.card_code.as_deref(),
        );
        sqlx::query("UPDATE users SET search_text = ? WHERE id = ?")
            .bind(&text)
            .bind(row.id)
            .execute(pool)
            .await
            .with_context(|| format!("Failed to backfill search_text for user {}", row.id))?;
    }
    Ok(count)
}

/// Round a monetary value to 2 decimal places to mitigate f64 precision issues.
pub fn round_cents(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[allow(clippy::too_many_arguments)]
pub async fn create_user(
    pool: &SqlitePool,
    email: Option<&str>,
    password_hash: Option<&str>,
    name: &str,
    phone: Option<&str>,
    company: Option<&str>,
    card_code: Option<&str>,
    role: &str,
    initial_credit: Option<f64>,
    oauth_provider: Option<&str>,
    oauth_id: Option<&str>,
) -> sqlx::Result<i64> {
    let search_text = compute_search_text(Some(name), company, card_code);
    let credit = initial_credit.unwrap_or(0.0);
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (email, password_hash, name, phone, company,
                            card_code, role, credit, oauth_provider, oauth_id, search_text)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(email)
    .bind(password_hash)
    .bind(name)
    .bind(phone)
    .bind(company)
    .bind(card_code)
    .bind(role)
    .bind(credit)
    .bind(oauth_provider)
    .bind(oauth_id)
    .bind(&search_text)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn get_user_by_email(pool: &SqlitePool, email: &str) -> Result<Option<UserRow>> {
    let user =
        sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE email = ? AND deleted_at IS NULL")
            .bind(email)
            .fetch_optional(pool)
            .await
            .context("Failed to get user by email")?;
    Ok(user)
}

pub async fn get_user_by_id(pool: &SqlitePool, id: i64) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("Failed to get user by id")?;
    Ok(user)
}

pub async fn get_user_by_oauth(
    pool: &SqlitePool,
    provider: &str,
    oauth_id: &str,
) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE oauth_provider = ? AND oauth_id = ? AND deleted_at IS NULL",
    )
    .bind(provider)
    .bind(oauth_id)
    .fetch_optional(pool)
    .await
    .context("Failed to get user by oauth")?;
    Ok(user)
}

pub async fn list_users(pool: &SqlitePool) -> Result<Vec<UserRow>> {
    let users =
        sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE deleted_at IS NULL ORDER BY id")
            .fetch_all(pool)
            .await
            .context("Failed to list users")?;
    Ok(users)
}

pub async fn update_user_role(pool: &SqlitePool, user_id: i64, role: &str) -> Result<()> {
    sqlx::query("UPDATE users SET role = ? WHERE id = ?")
        .bind(role)
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to update user role")?;
    Ok(())
}

/// User row + its current monthly-pass (id + end date) — populated by a single
/// query with correlated subquery to pull the row with MAX(valid_until) per user.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRowWithPass {
    pub id: i64,
    pub email: Option<String>,
    pub name: String,
    pub password_hash: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub role: String,
    pub oauth_provider: Option<String>,
    pub oauth_id: Option<String>,
    pub credit: f64,
    pub card_code: Option<String>,
    pub blocked: bool,
    pub allow_debit: bool,
    pub search_text: Option<String>,
    pub created_at: String,
    pub deleted_at: Option<String>,
    pub allow_self_entry: bool,
    pub pass_valid_until: Option<chrono::NaiveDate>,
    pub pass_tx_id: Option<i64>,
    pub last_visit_at: Option<String>,
}

impl UserRowWithPass {
    /// Decompose into the user portion, the pass (id + date), and the last visit timestamp.
    pub fn into_parts(self) -> (UserRow, Option<(i64, chrono::NaiveDate)>, Option<String>) {
        let pass = match (self.pass_tx_id, self.pass_valid_until) {
            (Some(id), Some(date)) => Some((id, date)),
            _ => None,
        };
        let last_visit_at = self.last_visit_at;
        (
            UserRow {
                id: self.id,
                email: self.email,
                name: self.name,
                password_hash: self.password_hash,
                phone: self.phone,
                company: self.company,
                role: self.role,
                oauth_provider: self.oauth_provider,
                oauth_id: self.oauth_id,
                credit: self.credit,
                card_code: self.card_code,
                blocked: self.blocked,
                allow_debit: self.allow_debit,
                search_text: self.search_text,
                created_at: self.created_at,
                deleted_at: self.deleted_at,
                allow_self_entry: self.allow_self_entry,
            },
            pass,
            last_visit_at,
        )
    }
}

/// Return all users with their current monthly-pass (tx id + end date) in a
/// single query. Uses correlated subqueries to pick the newest non-voided pass
/// transaction per user (ties broken by id DESC).
pub async fn list_all_users_with_pass(
    pool: &SqlitePool,
) -> Result<Vec<(UserRow, Option<(i64, chrono::NaiveDate)>, Option<String>)>> {
    let rows: Vec<UserRowWithPass> = sqlx::query_as::<_, UserRowWithPass>(
        "SELECT u.id, u.email, u.name, u.password_hash, u.phone, u.company,
                u.role, u.oauth_provider, u.oauth_id, u.credit, u.card_code,
                u.blocked, u.allow_debit, u.search_text, u.created_at, u.deleted_at, u.allow_self_entry,
                (SELECT MAX(valid_until) FROM transactions
                 WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
                ) AS pass_valid_until,
                (SELECT id FROM transactions
                 WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
                 ORDER BY valid_until DESC, id DESC LIMIT 1
                ) AS pass_tx_id,
                (SELECT MAX(created_at) FROM transactions
                 WHERE user_id = u.id
                   AND deleted_at IS NULL
                   AND service_id IN (SELECT id FROM services WHERE name_en IN (?, ?))
                ) AS last_visit_at
         FROM users u
         WHERE u.deleted_at IS NULL
         ORDER BY u.name",
    )
    .bind(CLASS_VISIT_NAMES_EN[0])
    .bind(CLASS_VISIT_NAMES_EN[1])
    .fetch_all(pool)
    .await
    .context("Failed to list users with pass")?;
    Ok(rows.into_iter().map(UserRowWithPass::into_parts).collect())
}

/// Search users with their monthly-pass (tx id + end date) — single query to avoid N+1.
pub async fn search_users_with_pass(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<(UserRow, Option<(i64, chrono::NaiveDate)>, Option<String>)>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let normalized = normalize_search(q);
    let like = format!("%{normalized}%");
    let prefix = format!("{q}%");
    let rows: Vec<UserRowWithPass> = sqlx::query_as::<_, UserRowWithPass>(
        "SELECT u.id, u.email, u.name, u.password_hash, u.phone, u.company,
                u.role, u.oauth_provider, u.oauth_id, u.credit, u.card_code,
                u.blocked, u.allow_debit, u.search_text, u.created_at, u.deleted_at, u.allow_self_entry,
                (SELECT MAX(valid_until) FROM transactions
                 WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
                ) AS pass_valid_until,
                (SELECT id FROM transactions
                 WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
                 ORDER BY valid_until DESC, id DESC LIMIT 1
                ) AS pass_tx_id,
                (SELECT MAX(created_at) FROM transactions
                 WHERE user_id = u.id
                   AND deleted_at IS NULL
                   AND service_id IN (SELECT id FROM services WHERE name_en IN (?, ?))
                ) AS last_visit_at
         FROM users u
         WHERE u.search_text LIKE ?
           AND u.deleted_at IS NULL
         ORDER BY
           CASE WHEN u.card_code LIKE ? THEN 0 ELSE 1 END,
           last_visit_at IS NULL,
           last_visit_at DESC,
           u.name IS NULL, u.name ASC,
           u.card_code ASC
         LIMIT ?",
    )
    .bind(CLASS_VISIT_NAMES_EN[0])
    .bind(CLASS_VISIT_NAMES_EN[1])
    .bind(&like)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to search users with pass")?;
    Ok(rows.into_iter().map(UserRowWithPass::into_parts).collect())
}

/// Search users by partial match. Diacritic- and case-insensitive: the query
/// is folded via `normalize_search` and compared against the pre-computed
/// `search_text` column, so "zbyne" finds "Zbyněk" and "drlik" finds "Drlík".
/// Card-code prefix matches sort first. Empty/whitespace query → empty Vec.
/// Includes blocked users so staff can find them to unblock.
pub async fn search_users(pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<UserRow>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let needle = normalize_search(q);
    let like = format!("%{needle}%");
    let prefix = format!("{q}%");
    let users = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users
         WHERE deleted_at IS NULL AND search_text LIKE ?
         ORDER BY
           CASE WHEN card_code LIKE ? THEN 0 ELSE 1 END,
           name IS NULL, name ASC,
           card_code ASC
         LIMIT ?",
    )
    .bind(&like)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to search users")?;
    Ok(users)
}

pub async fn get_user_by_card_code(pool: &SqlitePool, code: &str) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT * FROM users WHERE card_code = ? AND deleted_at IS NULL",
    )
    .bind(code)
    .fetch_optional(pool)
    .await
    .context("Failed to get user by card_code")?;
    Ok(user)
}

pub async fn update_credit(pool: &SqlitePool, user_id: i64, delta: f64) -> Result<()> {
    let rounded_delta = round_cents(delta);
    sqlx::query("UPDATE users SET credit = ROUND(credit + ?, 2) WHERE id = ?")
        .bind(rounded_delta)
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to update credit")?;
    Ok(())
}

pub async fn set_blocked(pool: &SqlitePool, user_id: i64, blocked: bool) -> Result<()> {
    sqlx::query("UPDATE users SET blocked = ? WHERE id = ?")
        .bind(blocked as i64)
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to set blocked status")?;
    Ok(())
}

pub async fn set_allow_debit(pool: &SqlitePool, user_id: i64, allow: bool) -> Result<()> {
    sqlx::query("UPDATE users SET allow_debit = ? WHERE id = ?")
        .bind(allow as i64)
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to set allow_debit")?;
    Ok(())
}

/// Set the per-user opt-in flag for self-service door entry.
/// Admin-only — caller must enforce role at the route layer.
pub async fn update_user_allow_self_entry(
    pool: &SqlitePool,
    user_id: i64,
    allow: bool,
) -> Result<()> {
    sqlx::query("UPDATE users SET allow_self_entry = ? WHERE id = ?")
        .bind(if allow { 1 } else { 0 })
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to update allow_self_entry")?;
    Ok(())
}

/// Set the password hash for a user. Caller must enforce role authorization
/// (admin can set any user's password; customer can set OWN password; staff
/// cannot reset other users' passwords).
pub async fn update_user_password_hash(
    pool: &SqlitePool,
    user_id: i64,
    password_hash: &str,
) -> Result<()> {
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(password_hash)
        .bind(user_id)
        .execute(pool)
        .await
        .context("Failed to update password_hash")?;
    Ok(())
}

/// Return the latest `valid_until` across a user's transactions, or `None` if
/// the user has never had a monthly-pass purchase. Callers compare against
/// today's date to determine whether the pass is active or expired.
pub async fn get_user_pass_valid_until(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<chrono::NaiveDate>> {
    let row: Option<(Option<chrono::NaiveDate>,)> = sqlx::query_as(
        "SELECT MAX(valid_until) FROM transactions
         WHERE user_id = ? AND valid_until IS NOT NULL AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .context("Failed to compute pass valid_until")?;
    Ok(row.and_then(|(d,)| d))
}

/// Return the latest non-voided pass transaction as (id, valid_until), or None.
pub async fn get_user_pass_tx(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<(i64, chrono::NaiveDate)>> {
    let row: Option<(i64, chrono::NaiveDate)> = sqlx::query_as(
        "SELECT id, valid_until FROM transactions
         WHERE user_id = ? AND valid_until IS NOT NULL AND deleted_at IS NULL
         ORDER BY valid_until DESC, id DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch latest pass transaction")?;
    Ok(row)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NegativeBalanceUserRow {
    pub id: i64,
    pub card_code: Option<String>,
    pub credit: f64,
    pub blocked: bool,
    pub name: String,
    pub email: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub pass_valid_until: Option<chrono::NaiveDate>,
    pub pass_tx_id: Option<i64>,
}

/// Users with `credit < 0`, sorted most-negative-first. Includes blocked
/// users (still owe money). Three scalar subqueries on `transactions`
/// run for each negative-credit user; at current data scale this is
/// sub-millisecond.
pub async fn list_negative_balance(pool: &SqlitePool) -> Result<Vec<NegativeBalanceUserRow>> {
    let rows = sqlx::query_as::<_, NegativeBalanceUserRow>(
        "SELECT
            u.id, u.card_code, u.credit, u.blocked, u.name, u.email, u.company,
            (SELECT MAX(t.created_at) FROM transactions t
                WHERE t.user_id = u.id
                  AND t.deleted_at IS NULL
                  AND t.service_id IN (SELECT id FROM services WHERE name_en IN (?, ?))
            ) AS last_visit_at,
            (SELECT MAX(valid_until) FROM transactions
                WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
                WHERE user_id = u.id AND valid_until IS NOT NULL AND deleted_at IS NULL
                ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id
         FROM users u
         WHERE u.credit < 0
           AND u.deleted_at IS NULL
         ORDER BY u.credit ASC",
    )
    .bind(CLASS_VISIT_NAMES_EN[0])
    .bind(CLASS_VISIT_NAMES_EN[1])
    .fetch_all(pool)
    .await
    .context("Failed to list users with negative balance")?;
    Ok(rows)
}

pub async fn update_user_info(
    pool: &SqlitePool,
    user_id: i64,
    name: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
    company: Option<&str>,
    card_code: Option<&str>,
) -> Result<()> {
    // Read the current row so we can compute search_text correctly under partial updates.
    let current = get_user_by_id(pool, user_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("User {} not found", user_id))?;
    let effective_name = name.unwrap_or(&current.name);
    let effective_company = company.or(current.company.as_deref());
    let effective_code = card_code.or(current.card_code.as_deref());
    let search_text = compute_search_text(Some(effective_name), effective_company, effective_code);
    sqlx::query(
        "UPDATE users
            SET name      = COALESCE(?, name),
                email     = COALESCE(?, email),
                phone     = COALESCE(?, phone),
                company   = COALESCE(?, company),
                card_code = COALESCE(?, card_code),
                search_text = ?
          WHERE id = ?",
    )
    .bind(name)
    .bind(email)
    .bind(phone)
    .bind(company)
    .bind(card_code)
    .bind(&search_text)
    .bind(user_id)
    .execute(pool)
    .await
    .context("Failed to update user info")?;
    Ok(())
}

/// Row returned by `users_by_last_movement`.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct UserByMovementRow {
    pub id: i64,
    pub name: String,
    pub card_code: Option<String>,
    pub last_movement_at: Option<String>,
    pub allow_self_entry: bool,
}

/// List users (excluding soft-deleted) with their most recent non-voided
/// transaction's created_at, sorted oldest-movement-first. Users with no
/// transactions appear first (last_movement_at IS NULL).
pub async fn users_by_last_movement(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<UserByMovementRow>> {
    let rows = sqlx::query_as::<_, UserByMovementRow>(
        "SELECT
            u.id,
            u.name,
            u.card_code,
            u.allow_self_entry,
            MAX(t.created_at) AS last_movement_at
           FROM users u
           LEFT JOIN transactions t
             ON t.user_id = u.id AND t.deleted_at IS NULL
          WHERE u.deleted_at IS NULL
          GROUP BY u.id
          ORDER BY last_movement_at IS NULL DESC,
                   last_movement_at ASC,
                   u.id ASC
          LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .context("Failed to list users by last movement")?;
    Ok(rows)
}

/// Outcome of a soft-delete attempt.
pub enum DeleteUserOutcome {
    Deleted { deleted_at: String },
    NotFound,
    AlreadyDeleted,
}

/// Soft-delete a user by setting `deleted_at` to now. Idempotent semantics:
/// returns `AlreadyDeleted` if the user already has `deleted_at`. Transactions
/// for that user are NOT touched.
pub async fn delete_user(pool: &SqlitePool, id: i64) -> Result<DeleteUserOutcome> {
    // Atomic: only one concurrent caller can flip NULL → datetime('now').
    let updated = sqlx::query(
        "UPDATE users SET deleted_at = datetime('now')
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await
    .context("Failed to soft-delete user")?;

    if updated.rows_affected() == 0 {
        // No rows flipped — disambiguate not-found vs already-deleted.
        let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("Failed to check user existence after no-op delete")?;
        return Ok(if exists.is_none() {
            DeleteUserOutcome::NotFound
        } else {
            DeleteUserOutcome::AlreadyDeleted
        });
    }

    // Read back the timestamp we just wrote.
    let row: (Option<String>,) = sqlx::query_as("SELECT deleted_at FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .context("Failed to read deleted_at after soft-delete")?;
    let deleted_at = row.0.unwrap_or_default();
    Ok(DeleteUserOutcome::Deleted { deleted_at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn make_user(pool: &SqlitePool, email: Option<&str>, name: &str) -> i64 {
        create_user(
            pool, email, None, name, None, None, None, "customer", None, None, None,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn create_and_get_user() {
        let pool = setup().await;

        let id = create_user(
            &pool,
            Some("alice@example.com"),
            Some("hash123"),
            "Alice",
            Some("+1234"),
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let user = get_user_by_email(&pool, "alice@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.id, id);
        assert_eq!(user.name, "Alice");
        assert_eq!(user.role, "customer");
        assert_eq!(user.phone.as_deref(), Some("+1234"));

        let user2 = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(user2.email.as_deref(), Some("alice@example.com"));
    }

    #[tokio::test]
    async fn duplicate_email_fails() {
        let pool = setup().await;

        create_user(
            &pool,
            Some("bob@example.com"),
            None,
            "Bob",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let result = create_user(
            &pool,
            Some("bob@example.com"),
            None,
            "Bob2",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn search_empty_query_returns_empty() {
        let pool = setup().await;
        make_user(&pool, Some("a@test.com"), "Alice Aaa").await;
        assert!(search_users(&pool, "", 10).await.unwrap().is_empty());
        assert!(search_users(&pool, "   ", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_by_name_partial() {
        let pool = setup().await;
        make_user(&pool, Some("a@test.com"), "Alice Aaa").await;
        make_user(&pool, Some("b@test.com"), "Bob Bbb").await;
        let results = search_users(&pool, "Ali", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Alice Aaa");
    }

    #[tokio::test]
    async fn search_folds_slovak_diacritics() {
        let pool = setup().await;
        make_user(&pool, None, "Zbyněk Drlík").await;
        make_user(&pool, None, "Stevo Žumerling").await;

        let hits = search_users(&pool, "zbyne", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Zbyněk Drlík");

        let hits = search_users(&pool, "zumer", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Stevo Žumerling");
    }

    #[tokio::test]
    async fn search_case_insensitive_ascii() {
        let pool = setup().await;
        create_user(
            &pool,
            None,
            None,
            "Anna Company",
            None,
            Some("NewLevel"),
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        create_user(
            &pool,
            None,
            None,
            "Eva Company",
            None,
            Some("NewLevel"),
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let lower = search_users(&pool, "newlevel", 10).await.unwrap();
        let upper = search_users(&pool, "NEWLEVEL", 10).await.unwrap();
        assert_eq!(lower.len(), 2);
        assert_eq!(upper.len(), 2);
    }

    #[tokio::test]
    async fn search_limit_honored() {
        let pool = setup().await;
        create_user(
            &pool,
            None,
            None,
            "Name One",
            None,
            Some("NewLevel"),
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        create_user(
            &pool,
            None,
            None,
            "Name Two",
            None,
            Some("NewLevel"),
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let results = search_users(&pool, "NewLevel", 1).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn search_includes_blocked_users() {
        let pool = setup().await;
        let id = create_user(
            &pool,
            None,
            None,
            "Bad Actor",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        set_blocked(&pool, id, true).await.unwrap();
        let results = search_users(&pool, "Bad Actor", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].blocked);
    }

    #[tokio::test]
    async fn update_credit_add_and_subtract() {
        let pool = setup().await;
        let id = make_user(&pool, Some("c@test.com"), "Credit User").await;

        update_credit(&pool, id, 10.0).await.unwrap();
        let user = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!((user.credit - 10.0).abs() < f64::EPSILON);

        update_credit(&pool, id, -3.5).await.unwrap();
        let user = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!((user.credit - 6.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn get_user_by_card_code_test() {
        let pool = setup().await;
        create_user(
            &pool,
            None,
            None,
            "Card User",
            None,
            None,
            Some("CARD-ABC"),
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let found = get_user_by_card_code(&pool, "CARD-ABC").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Card User");

        let not_found = get_user_by_card_code(&pool, "MISSING").await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn pass_valid_until_none_when_no_pass_purchased() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "No Pass").await;
        let result = get_user_pass_valid_until(&pool, user_id).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn pass_valid_until_returns_max_across_multiple_passes() {
        use crate::db::transactions::create_transaction_with_valid_until;
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Multi Pass").await;
        let d1 = chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let d2 = chrono::NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(1),
            -35.0,
            "charge",
            Some(d1),
            None,
        )
        .await
        .unwrap();
        create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(1),
            -35.0,
            "charge",
            Some(d2),
            None,
        )
        .await
        .unwrap();

        let result = get_user_pass_valid_until(&pool, user_id).await.unwrap();
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
        let user_id = make_user(&pool, None, "Charge Only").await;
        create_transaction(&pool, Some(user_id), None, Some(1), -5.0, "charge", None)
            .await
            .unwrap();
        create_transaction(&pool, Some(user_id), None, None, 20.0, "topup", None)
            .await
            .unwrap();
        let result = get_user_pass_valid_until(&pool, user_id).await.unwrap();
        assert_eq!(
            result, None,
            "non-pass transactions must not produce a valid_until"
        );
    }

    #[tokio::test]
    async fn pass_validity_ignores_soft_deleted_pass() {
        use crate::db::transactions::create_transaction_with_valid_until;
        let pool = setup().await;
        let user_id = make_user(&pool, None, "PV Test").await;
        let future = chrono::Local::now().date_naive() + chrono::Duration::days(10);

        let tx_id = create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(1),
            -35.0,
            "charge",
            Some(future),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            get_user_pass_valid_until(&pool, user_id).await.unwrap(),
            Some(future)
        );

        crate::db::transactions::soft_delete(&pool, tx_id)
            .await
            .unwrap();

        assert_eq!(
            get_user_pass_valid_until(&pool, user_id).await.unwrap(),
            None,
            "soft-deleted pass sale must not count as active pass"
        );
    }

    #[tokio::test]
    async fn list_negative_balance_returns_only_negatives_sorted() {
        let pool = setup().await;

        let pos = make_user(&pool, None, "Positive User").await;
        let mid = make_user(&pool, None, "Mid User").await;
        let deep = make_user(&pool, None, "Deep User").await;

        update_credit(&pool, pos, 5.0).await.unwrap();
        update_credit(&pool, mid, -3.5).await.unwrap();
        update_credit(&pool, deep, -10.0).await.unwrap();

        let fitness_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?")
            .bind(CLASS_VISIT_NAMES_EN[0])
            .fetch_one(&pool)
            .await
            .unwrap();
        let spinning_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?")
            .bind(CLASS_VISIT_NAMES_EN[1])
            .fetch_one(&pool)
            .await
            .unwrap();

        // `mid` got a free visit for Fitness.
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
             VALUES (?, ?, 0.0, 'visit', '2026-04-22 12:00:00')",
        )
        .bind(mid)
        .bind(fitness_id)
        .execute(&pool)
        .await
        .unwrap();
        // `mid` later paid for a Spinning entry from credit.
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
             VALUES (?, ?, -3.30, 'charge', '2026-04-25 18:00:00')",
        )
        .bind(mid)
        .bind(spinning_id)
        .execute(&pool)
        .await
        .unwrap();
        // `deep` topped up €5 (last payment). No visits.
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, created_at)
             VALUES (?, 5.0, 'topup', '2026-03-05 09:00:00')",
        )
        .bind(deep)
        .execute(&pool)
        .await
        .unwrap();

        // `mid` also has an active monthly pass.
        let mid_pass_until = chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let mid_pass_tx_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, amount, action, valid_until, created_at)
             VALUES (?, -25.0, 'charge', ?, '2026-04-01 10:00:00') RETURNING id",
        )
        .bind(mid)
        .bind(mid_pass_until)
        .fetch_one(&pool)
        .await
        .unwrap();

        let rows = list_negative_balance(&pool).await.unwrap();
        assert_eq!(rows.len(), 2, "positive user must be excluded");
        assert_eq!(rows[0].id, deep);
        assert!((rows[0].credit - (-10.0)).abs() < f64::EPSILON);
        assert_eq!(rows[0].last_visit_at, None);
        assert_eq!(rows[0].pass_tx_id, None, "deep has no pass");
        assert_eq!(rows[0].pass_valid_until, None);
        assert_eq!(rows[1].id, mid);
        assert!((rows[1].credit - (-3.5)).abs() < f64::EPSILON);
        // The later 'charge' row counts as a visit.
        assert_eq!(
            rows[1].last_visit_at.as_deref(),
            Some("2026-04-25 18:00:00"),
        );
        assert_eq!(rows[1].pass_tx_id, Some(mid_pass_tx_id));
        assert_eq!(rows[1].pass_valid_until, Some(mid_pass_until));
    }

    #[tokio::test]
    async fn backfill_populates_empty_search_text_and_reports_count() {
        let pool = setup().await;

        // Insert three rows with empty search_text directly.
        for name in ["Zbyněk Drlík", "Stevo Žumerling", "Anna Nováková"] {
            sqlx::query("INSERT INTO users (name, search_text) VALUES (?, '')")
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }

        // Add a row that already has search_text so we can prove backfill skips it.
        create_user(
            &pool,
            None,
            None,
            "Eva Modern",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let count = backfill_search_text(&pool).await.unwrap();
        assert_eq!(
            count, 3,
            "must report exact count — kills Ok(0) and Ok(1) mutants"
        );

        let hits = search_users(&pool, "zbyne", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Zbyněk Drlík");
    }

    #[tokio::test]
    async fn backfill_is_idempotent() {
        let pool = setup().await;
        create_user(
            &pool,
            None,
            None,
            "Already Has Text",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let count = backfill_search_text(&pool).await.unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn normalize_search_folds_lowercase_and_diacritics() {
        assert_eq!(normalize_search("Zbyněk"), "zbynek");
        assert_eq!(normalize_search("Žumerling"), "zumerling");
        assert_eq!(normalize_search("Drlík"), "drlik");
        assert_eq!(normalize_search("Ľuboš"), "lubos");
        assert_eq!(normalize_search("ABC"), "abc");
    }

    #[tokio::test]
    async fn update_user_info_name_only_preserves_other_fields() {
        let pool = setup().await;
        let id = create_user(
            &pool,
            Some("a@b"),
            None,
            "Alice",
            Some("111"),
            Some("Acme"),
            Some("CODE1"),
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        update_user_info(&pool, id, Some("Alice Renamed"), None, None, None, None)
            .await
            .unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(u.name, "Alice Renamed");
        assert_eq!(u.email.as_deref(), Some("a@b"));
        assert_eq!(u.phone.as_deref(), Some("111"));
        assert_eq!(u.company.as_deref(), Some("Acme"));
        assert_eq!(u.card_code.as_deref(), Some("CODE1"));
    }

    #[tokio::test]
    async fn update_user_info_email_only_preserves_other_fields() {
        let pool = setup().await;
        let id = create_user(
            &pool,
            Some("a@b"),
            None,
            "Alice",
            Some("111"),
            Some("Acme"),
            Some("CODE1"),
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        update_user_info(&pool, id, None, Some("new@b"), None, None, None)
            .await
            .unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(u.email.as_deref(), Some("new@b"));
        assert_eq!(u.name, "Alice");
        assert_eq!(u.phone.as_deref(), Some("111"));
        assert_eq!(u.company.as_deref(), Some("Acme"));
        assert_eq!(u.card_code.as_deref(), Some("CODE1"));
    }

    #[tokio::test]
    async fn update_user_info_recomputes_search_text_under_partial_update() {
        let pool = setup().await;
        let id = create_user(
            &pool,
            Some("a@b"),
            None,
            "Alice",
            None,
            Some("Acme"),
            Some("CODE1"),
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        update_user_info(&pool, id, Some("Alice Renamed"), None, None, None, None)
            .await
            .unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        let expected = normalize_search("Alice Renamed Acme CODE1");
        assert_eq!(u.search_text.as_deref(), Some(expected.as_str()));
    }

    // ── mutant #1: replace set_allow_debit → Ok(()) ───────────────────────
    // If set_allow_debit is a no-op, the flag never changes and the assertions
    // fail because allow_debit stays at its default (false).
    #[tokio::test]
    async fn set_allow_debit_round_trips() {
        let pool = setup().await;
        let id = create_user(
            &pool,
            Some("ad@x.com"),
            None,
            "AD",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        set_allow_debit(&pool, id, true).await.unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!(u.allow_debit, "set_allow_debit(true) must persist");

        set_allow_debit(&pool, id, false).await.unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!(!u.allow_debit, "set_allow_debit(false) must persist");
    }

    /// get_user_by_oauth must return Some for an active user and None after
    /// soft-delete (kills mutant: replace return with Ok(None)).
    #[tokio::test]
    async fn get_user_by_oauth_respects_soft_delete() {
        let pool = setup().await;
        // Seed a user with oauth fields directly — create_user doesn't accept
        // oauth params, so insert via SQL.
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO users(email, name, role, oauth_provider, oauth_id)
             VALUES('oa@x.com', 'OAuth User', 'customer', 'google', 'sub-123')
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let found = get_user_by_oauth(&pool, "google", "sub-123").await.unwrap();
        assert!(found.is_some(), "active oauth user must be returned");
        assert_eq!(found.unwrap().id, id);

        // Soft-delete and confirm the lookup now returns None.
        sqlx::query("UPDATE users SET deleted_at = datetime('now') WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let after = get_user_by_oauth(&pool, "google", "sub-123").await.unwrap();
        assert!(after.is_none(), "soft-deleted oauth user must be hidden");
    }

    // ── allow_self_entry default and round-trip ────────────────────────────
    // mutant kill: replace update_user_allow_self_entry → Ok(()) would leave
    // the flag unchanged and both assertions would fail.
    #[tokio::test]
    async fn allow_self_entry_default_is_false() {
        let pool = setup().await;
        let id = make_user(&pool, Some("ase@x.com"), "ASE").await;
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!(
            !u.allow_self_entry,
            "allow_self_entry must default to false after migration"
        );
    }

    #[tokio::test]
    async fn update_user_allow_self_entry_round_trips() {
        let pool = setup().await;
        let id = make_user(&pool, Some("ase2@x.com"), "ASE2").await;

        update_user_allow_self_entry(&pool, id, true).await.unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!(
            u.allow_self_entry,
            "update_user_allow_self_entry(true) must persist"
        );

        update_user_allow_self_entry(&pool, id, false)
            .await
            .unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!(
            !u.allow_self_entry,
            "update_user_allow_self_entry(false) must persist"
        );
    }
}
