use crate::db::error::{DbError, Result};
use sqlx::SqlitePool;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use spinbike_core::services::{CLASS_VISIT_KINDS, class_visit_filter_sql};

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
///
/// Runs unconditionally at every server start, before the listener accepts
/// requests (`bin/server.rs`) — so the per-row loop below is wrapped in ONE
/// transaction (#341) instead of leaving each `UPDATE` to auto-commit (and
/// potentially fsync) on its own; nothing reads `search_text` before this
/// completes either way.
pub async fn backfill_search_text(pool: &SqlitePool) -> Result<usize> {
    let rows: Vec<UserRow> = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE search_text IS NULL OR search_text = ''",
    )
    .fetch_all(pool)
    .await?;
    let count = rows.len();
    if count == 0 {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    for row in rows {
        let text = compute_search_text(
            Some(&row.name),
            row.company.as_deref(),
            row.card_code.as_deref(),
        );
        sqlx::query("UPDATE users SET search_text = ? WHERE id = ?")
            .bind(&text)
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
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
) -> Result<i64> {
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
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE email = ? AND deleted_at IS NULL",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

/// Like [`get_user_by_email`] but does NOT filter out soft-deleted rows.
///
/// The `users.email` UNIQUE constraint counts ALL rows (soft-delete only sets
/// `deleted_at`, it keeps the email), so an email held by a soft-deleted
/// account still reserves the address. The create/update collision check uses
/// this to SURFACE that case (#143) as a resolvable 409 with the archived
/// account's identity, instead of missing it via the `deleted_at IS NULL`
/// filter and then hitting a raw UNIQUE violation → opaque 500.
pub async fn get_user_by_email_including_deleted(
    pool: &SqlitePool,
    email: &str,
) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn get_user_by_id(pool: &SqlitePool, id: i64) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn get_user_by_oauth(
    pool: &SqlitePool,
    provider: &str,
    oauth_id: &str,
) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE oauth_provider = ? AND oauth_id = ? AND deleted_at IS NULL",
    )
    .bind(provider)
    .bind(oauth_id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn list_users(pool: &SqlitePool) -> Result<Vec<UserRow>> {
    let users = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE deleted_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

/// E-mail addresses to alert about an operational problem (the door-health
/// fault alert, #355). Every live (non-deleted, non-blocked) admin-role user
/// with a non-empty e-mail — the owner is a solo operator with the `admin`
/// role, so this reaches them without any separate config knob that could be
/// silently left unset. A placeholder admin address that never accepts mail
/// (e.g. a `.local` seed account) simply fails its own send at the caller and
/// is logged; it never blocks a deliverable recipient.
pub async fn admin_alert_recipients(pool: &SqlitePool) -> Result<Vec<String>> {
    let emails = sqlx::query_scalar::<_, String>(
        "SELECT email FROM users \
         WHERE role = 'admin' \
           AND deleted_at IS NULL \
           AND blocked = 0 \
           AND email IS NOT NULL \
           AND TRIM(email) != '' \
         ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(emails)
}

pub async fn update_user_role(pool: &SqlitePool, user_id: i64, role: &str) -> Result<()> {
    sqlx::query("UPDATE users SET role = ? WHERE id = ?")
        .bind(role)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// User row + its current monthly-pass (id + end date) — populated by a single
/// query LEFT JOINing the canonical `user_active_pass` view (migration V18),
/// which already resolves the newest non-voided pass transaction per user.
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
/// single query. LEFT JOINs the canonical `user_active_pass` view (V18),
/// which already picks the newest non-voided pass transaction per user
/// (ties broken by id DESC) — see that view's own doc comment for the exact
/// predicate.
pub async fn list_all_users_with_pass(
    pool: &SqlitePool,
) -> Result<Vec<(UserRow, Option<(i64, chrono::NaiveDate)>, Option<String>)>> {
    let sql = format!(
        "SELECT u.id, u.email, u.name, u.password_hash, u.phone, u.company,
                u.role, u.oauth_provider, u.oauth_id, u.credit, u.card_code,
                u.blocked, u.allow_debit, u.search_text, u.created_at, u.deleted_at, u.allow_self_entry,
                ap.valid_until AS pass_valid_until,
                ap.pass_tx_id AS pass_tx_id,
                (SELECT MAX(created_at) FROM transactions
                 WHERE user_id = u.id
                   AND deleted_at IS NULL
                   AND {visit_filter}
                ) AS last_visit_at
         FROM users u
         LEFT JOIN user_active_pass ap ON ap.user_id = u.id
         WHERE u.deleted_at IS NULL
         ORDER BY u.name",
        visit_filter = class_visit_filter_sql("service_id"),
    );
    let mut q = sqlx::query_as::<_, UserRowWithPass>(&sql);
    for k in CLASS_VISIT_KINDS {
        q = q.bind(*k);
    }
    let rows: Vec<UserRowWithPass> = q.fetch_all(pool).await?;
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
    // #290: card-code match strength, most specific first. A card_code that
    // genuinely STARTS or ENDS WITH the typed digits ("type the last digits
    // off a card, get that card") must outrank a row that merely CONTAINS
    // those digits somewhere in name/company/card_code — previously only
    // the prefix case was special-cased, so a barcode's TAIL (the common
    // case) fell into the same catch-all bucket as an unrelated card whose
    // id happened to contain the same digits, and ordering fell through to
    // alphabetical name, letting the wrong card win.
    let exact = q.to_string();
    let prefix = format!("{q}%");
    let suffix = format!("%{q}");
    let sql = format!(
        "SELECT u.id, u.email, u.name, u.password_hash, u.phone, u.company,
                u.role, u.oauth_provider, u.oauth_id, u.credit, u.card_code,
                u.blocked, u.allow_debit, u.search_text, u.created_at, u.deleted_at, u.allow_self_entry,
                ap.valid_until AS pass_valid_until,
                ap.pass_tx_id AS pass_tx_id,
                (SELECT MAX(created_at) FROM transactions
                 WHERE user_id = u.id
                   AND deleted_at IS NULL
                   AND {visit_filter}
                ) AS last_visit_at
         FROM users u
         LEFT JOIN user_active_pass ap ON ap.user_id = u.id
         WHERE u.search_text LIKE ?
           AND u.deleted_at IS NULL
         ORDER BY
           CASE
             WHEN u.card_code = ? THEN 0
             WHEN u.card_code LIKE ? OR u.card_code LIKE ? THEN 1
             ELSE 2
           END,
           last_visit_at IS NULL,
           last_visit_at DESC,
           u.name IS NULL, u.name ASC,
           u.card_code ASC
         LIMIT ?",
        visit_filter = class_visit_filter_sql("service_id"),
    );
    let mut qb = sqlx::query_as::<_, UserRowWithPass>(&sql);
    for k in CLASS_VISIT_KINDS {
        qb = qb.bind(*k);
    }
    let rows: Vec<UserRowWithPass> = qb
        .bind(&like)
        .bind(&exact)
        .bind(&prefix)
        .bind(&suffix)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(UserRowWithPass::into_parts).collect())
}

/// Search users by partial match. Diacritic- and case-insensitive: the query
/// is folded via `normalize_search` and compared against the pre-computed
/// `search_text` column, so "zbyne" finds "Zbyněk" and "drlik" finds "Drlík".
/// Card-code exact/prefix/suffix matches sort first (#290 — a barcode TAIL
/// match must outrank a row that merely contains the digits elsewhere, the
/// same fix as `search_users_with_pass`). Empty/whitespace query → empty
/// Vec. Includes blocked users so staff can find them to unblock.
pub async fn search_users(pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<UserRow>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let needle = normalize_search(q);
    let like = format!("%{needle}%");
    let exact = q.to_string();
    let prefix = format!("{q}%");
    let suffix = format!("%{q}");
    let users = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users
         WHERE deleted_at IS NULL AND search_text LIKE ?
         ORDER BY
           CASE
             WHEN card_code = ? THEN 0
             WHEN card_code LIKE ? OR card_code LIKE ? THEN 1
             ELSE 2
           END,
           name IS NULL, name ASC,
           card_code ASC
         LIMIT ?",
    )
    .bind(&like)
    .bind(&exact)
    .bind(&prefix)
    .bind(&suffix)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(users)
}

pub async fn get_user_by_card_code(pool: &SqlitePool, code: &str) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, phone, company, role, oauth_provider,
                oauth_id, credit, card_code, blocked, allow_debit, search_text,
                created_at, deleted_at, allow_self_entry
         FROM users WHERE card_code = ? AND deleted_at IS NULL",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn update_credit(pool: &SqlitePool, user_id: i64, delta: f64) -> Result<()> {
    let rounded_delta = round_cents(delta);
    sqlx::query("UPDATE users SET credit = ROUND(credit + ?, 2) WHERE id = ?")
        .bind(rounded_delta)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_blocked(pool: &SqlitePool, user_id: i64, blocked: bool) -> Result<()> {
    sqlx::query("UPDATE users SET blocked = ? WHERE id = ?")
        .bind(blocked as i64)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_allow_debit(pool: &SqlitePool, user_id: i64, allow: bool) -> Result<()> {
    sqlx::query("UPDATE users SET allow_debit = ? WHERE id = ?")
        .bind(allow as i64)
        .bind(user_id)
        .execute(pool)
        .await?;
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
        .await?;
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
        .await?;
    Ok(())
}

/// Return the expiry date of the user's active monthly pass, or `None` if the
/// user holds no (non-voided) monthly-pass purchase. Resolved through the
/// canonical `user_active_pass` view (migration V18). Callers compare against
/// today's date to determine whether the pass is active or expired.
pub async fn get_user_pass_valid_until(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<chrono::NaiveDate>> {
    // `date(valid_until)` coerces any legacy full-datetime string down to a
    // bare `YYYY-MM-DD` before it is decoded into `chrono::NaiveDate`, matching
    // the charger's own defensive `date(...)` wrap (#179). Every valid_until in
    // prod is already a bare date, but without this a future importer writing a
    // datetime would hard-error the decode here while the charger kept working.
    let row: Option<(chrono::NaiveDate,)> =
        sqlx::query_as("SELECT date(valid_until) FROM user_active_pass WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(d,)| d))
}

/// Return the latest non-voided monthly-pass transaction as (id, valid_until),
/// or None. Resolved through the canonical `user_active_pass` view (V18).
pub async fn get_user_pass_tx(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<(i64, chrono::NaiveDate)>> {
    // `date(valid_until)` coerces any legacy full-datetime string to a bare
    // date before decoding into `chrono::NaiveDate` — same defence as
    // get_user_pass_valid_until above (#179).
    let row: Option<(i64, chrono::NaiveDate)> = sqlx::query_as(
        "SELECT pass_tx_id, date(valid_until) FROM user_active_pass WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// The `transactions.note` stamped on an auto-renewed monthly pass (#365).
/// A human label only — the machine distinguisher from a manual sale is
/// `staff_id IS NULL` (`sell-pass` always records the selling staff's id),
/// NOT this string (per #328, note text is never read for classification).
/// The client app (#357) renders it verbatim, so it must read sensibly in
/// Slovak; kept unaccented like the rest of the DB's Slovak labels.
pub const AUTO_RENEW_NOTE: &str = "auto-obnova";

/// Auto-renew a customer's monthly pass at the price of their most recent
/// (non-voided) pass, anchored to `anchor_day` + 1 calendar month (#365).
///
/// This is the shared money-write called from BOTH visit-charge sites that
/// today bill a single entry when a customer's pass has lapsed — the door
/// route (`routes/door.rs`) and the T-4h class charger (`jobs/charger.rs`).
/// Instead of a one-off single-entry charge, the lapsed customer gets a fresh
/// monthly pass and their credit goes negative until they settle up (owner
/// decision #365, variant b: renew on the next real visit, not via a daily
/// job — so a customer who stops coming never accrues debt).
///
/// Contract:
/// * Returns `Ok(Some(new_credit))` when the user HAS held a monthly pass
///   before: a new pass row is issued (credit debited by the pass price) and
///   the caller should treat the triggering visit as pass-covered (a €0
///   visit, no single-entry charge). `new_credit` is the rounded balance
///   after the debit.
/// * Returns `Ok(None)` — the caller keeps its existing single-entry / class
///   charge unchanged — when ANY of these hold (#372):
///   - the user has NEVER held a monthly pass (no "last price" to renew at);
///   - the last pass expired MORE than 31 days before `anchor_day` (gate 1,
///     recency: only a continuing monthly customer renews, not a years-lapsed
///     one — the prod incident that revived a 2020 pass in 2026);
///   - a paid class-visit exists AFTER that pass's expiry (gate 2: the customer
///     switched from pass-mode to per-visit-mode) — a class-visit `amount<0`,
///     `valid_until IS NULL`, non-voided row recorded as either a door
///     single-entry (`action='charge'`) or a charger Spinning single-visit
///     (`action='visit'`).
///
/// MUST be called inside the caller's open transaction (`&mut *tx`) so the
/// pass issue + the caller's own visit row commit or roll back atomically. Both
/// gates run BEFORE any write, and both call sites insert the triggering visit
/// only AFTER this returns, so the current entry never counts against itself.
///
/// The most recent non-voided pass (regardless of whether it is still valid)
/// is resolved through the canonical `user_active_pass` view (V18) joined back
/// to `transactions.amount` for the price — never hand-rolled pass-status SQL
/// (the same rule door.rs/charger.rs already follow). The price is rounded to
/// cents exactly once and reused for both the ledger `amount` and the credit
/// debit (money-rounding.md / #325/#326). A pass last sold for 0 € renews at
/// 0 € — a deliberate barter flow (#342), not a bug.
pub async fn auto_renew_pass(
    conn: &mut sqlx::SqliteConnection,
    user_id: i64,
    anchor_day: chrono::NaiveDate,
) -> Result<Option<f64>> {
    // Latest non-voided monthly pass + its own price, via the canonical view.
    // `user_active_pass` already picks the newest non-voided pass per user
    // (rn=1, valid_until DESC, id DESC) and filters deleted_at IS NULL, so a
    // voided pass never resolves here — the join hands back that pass's
    // ABS(amount) as "the price of the last one sold".
    let last_pass: Option<(f64, chrono::NaiveDate)> = sqlx::query_as(
        "SELECT t.amount, date(ap.valid_until) \
         FROM user_active_pass ap \
         JOIN transactions t ON t.id = ap.pass_tx_id \
         WHERE ap.user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((last_amount, last_valid_until)) = last_pass else {
        // Never held a monthly pass — nothing to renew at.
        return Ok(None);
    };

    // #372 gate 1 — RECENCY: only auto-renew for a CONTINUING monthly customer,
    // whose last pass expired at most 31 days before this visit (INCLUSIVE). A
    // pass that lapsed longer ago means the customer stopped coming on a pass;
    // reviving a years-dead pass on a door press / class charge was the prod bug
    // this fixes (a 2020 pass renewed in 2026). `anchor_day` is already gym-local
    // (today at the door, the booking date in the charger — bratislava-tz.md)
    // and `valid_until` is a bare gym-local calendar date, so the difference is
    // an apples-to-apples day count with no `chrono::Local` involved.
    const MAX_LAPSE_DAYS: i64 = 31;
    if (anchor_day - last_valid_until).num_days() > MAX_LAPSE_DAYS {
        return Ok(None);
    }

    // #372 gate 2 — NO PAID VISIT SINCE EXPIRY: if the customer paid for even one
    // class visit AFTER the pass expired, they moved from pass-mode to
    // per-visit-mode and must not be auto-renewed. A "paid class-visit" is the
    // codebase-canonical shape (reports.rs / payments.rs): `amount < 0` AND
    // `valid_until IS NULL` on a service with a class-visit `kind` (service-kind.md
    // — never the admin-editable name), recorded EITHER as a door single-entry
    // `action='charge'` (routes/door.rs) OR a charger Spinning single-visit
    // `action='visit'` (jobs/charger.rs) — so BOTH actions must match. `amount<0`
    // excludes EUR0 door/pass-covered `visit` rows; `valid_until IS NULL` excludes
    // pass rows; `deleted_at IS NULL` excludes voided rows; the kind filter
    // excludes bar/generic charges. "After expiry" is a gym-local day comparison
    // done via the UTC instant of gym-local midnight of the day AFTER
    // `valid_until` (`bratislava_day_range_utc`), never `date(created_at)` (UTC, up
    // to ~2h off near midnight — the recurring #205 bug class). The
    // currently-processed visit is not yet written at either call site (door:
    // INSERT after this; charger: INSERT after this), so it cannot count against
    // itself.
    let day_after_expiry = last_valid_until
        .checked_add_days(chrono::Days::new(1))
        .expect("a gym-local calendar date + 1 day is always representable");
    let (cutoff_utc, _) = crate::util::bratislava_day_range_utc(day_after_expiry);
    let cutoff = cutoff_utc.format("%Y-%m-%d %H:%M:%S").to_string();
    let paid_visit_sql = format!(
        "SELECT 1 FROM transactions t \
         WHERE t.user_id = ? \
           AND t.action IN ('charge', 'visit') \
           AND t.amount < 0 \
           AND t.valid_until IS NULL \
           AND t.deleted_at IS NULL \
           AND t.created_at >= ? \
           AND {visit_filter} \
         LIMIT 1",
        visit_filter = class_visit_filter_sql("t.service_id"),
    );
    let mut paid_visit_q = sqlx::query_scalar::<_, i64>(&paid_visit_sql)
        .bind(user_id)
        .bind(&cutoff);
    for k in CLASS_VISIT_KINDS {
        paid_visit_q = paid_visit_q.bind(*k);
    }
    if paid_visit_q.fetch_optional(&mut *conn).await?.is_some() {
        // Switched to per-visit mode — keep the caller's single-entry / class charge.
        return Ok(None);
    }

    // Round ONCE, reuse for both the ledger row and the credit debit.
    let price = round_cents(last_amount.abs());

    // +1 calendar month from the visit day (chrono clamps 31 Jan → 28/29 Feb).
    let valid_until = anchor_day
        .checked_add_months(chrono::Months::new(1))
        .expect("a Bratislava calendar date + 1 month is always representable");

    // Same monthly-pass service the manual sell-pass path uses, resolved by
    // its stable `kind` (never the admin-editable name — service-kind.md).
    let service_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&mut *conn)
        .await?;

    // Debit the pass price. ROUND(...) in SQL is defense-in-depth against any
    // float drift already sitting in the stored credit; the Rust `price` is
    // already rounded above.
    sqlx::query("UPDATE users SET credit = ROUND(credit - ?, 2) WHERE id = ?")
        .bind(price)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;

    let new_credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await?;

    // The new pass row: identical shape to a manual sell-pass EXCEPT
    // staff_id = NULL and the distinguishing note, so history + the client
    // app can tell an automatic renewal from a desk sale (#357).
    sqlx::query(
        "INSERT INTO transactions \
           (user_id, staff_id, service_id, amount, action, valid_until, note) \
         VALUES (?, NULL, ?, ?, 'charge', ?, ?)",
    )
    .bind(user_id)
    .bind(service_id)
    .bind(-price)
    .bind(valid_until)
    .bind(AUTO_RENEW_NOTE)
    .execute(&mut *conn)
    .await?;

    let new_credit = round_cents(new_credit);
    tracing::info!(
        user_id,
        price,
        %valid_until,
        new_credit,
        "pass: auto-renewed expired monthly pass at last price (#365)"
    );
    Ok(Some(new_credit))
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
/// users (still owe money). One scalar subquery for `last_visit_at` plus a
/// LEFT JOIN on the canonical `user_active_pass` view (V18) for the pass
/// columns; at current data scale this is sub-millisecond.
pub async fn list_negative_balance(pool: &SqlitePool) -> Result<Vec<NegativeBalanceUserRow>> {
    let sql = format!(
        "SELECT
            u.id, u.card_code, u.credit, u.blocked, u.name, u.email, u.company,
            (SELECT MAX(t.created_at) FROM transactions t
                WHERE t.user_id = u.id
                  AND t.deleted_at IS NULL
                  AND {visit_filter}
            ) AS last_visit_at,
            ap.valid_until AS pass_valid_until,
            ap.pass_tx_id AS pass_tx_id
         FROM users u
         LEFT JOIN user_active_pass ap ON ap.user_id = u.id
         WHERE u.credit < 0
           AND u.deleted_at IS NULL
         ORDER BY u.credit ASC",
        visit_filter = class_visit_filter_sql("t.service_id"),
    );
    let mut qb = sqlx::query_as::<_, NegativeBalanceUserRow>(&sql);
    for k in CLASS_VISIT_KINDS {
        qb = qb.bind(*k);
    }
    let rows = qb.fetch_all(pool).await?;
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
        .ok_or(DbError::NotFound)?;
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
    .await?;
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
    .await?;
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
    .await?;

    if updated.rows_affected() == 0 {
        // No rows flipped — disambiguate not-found vs already-deleted.
        let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
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
        .await?;
    let deleted_at = row.0.unwrap_or_default();
    Ok(DeleteUserOutcome::Deleted { deleted_at })
}

/// Outcome of a restore (un-soft-delete) attempt.
pub enum RestoreUserOutcome {
    /// A soft-deleted row was reactivated (`deleted_at` cleared).
    Restored,
    /// No user with that id exists.
    NotFound,
    /// The user exists but is already active — nothing to restore (idempotent).
    NotDeleted,
}

/// Un-soft-delete a user by clearing `deleted_at`, bringing back its history
/// and credit (#143 "obnovit ucet"). Atomic NOT-NULL → NULL flip so only a
/// currently soft-deleted row is affected. Restoring is always safe w.r.t. the
/// `email` UNIQUE constraint: while the row was soft-deleted its email was
/// still reserved, so no live row can hold the same address.
pub async fn restore_user(pool: &SqlitePool, id: i64) -> Result<RestoreUserOutcome> {
    let updated =
        sqlx::query("UPDATE users SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(pool)
            .await?;
    if updated.rows_affected() == 1 {
        return Ok(RestoreUserOutcome::Restored);
    }
    // No row flipped — disambiguate not-found vs already-active.
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(if exists.is_none() {
        RestoreUserOutcome::NotFound
    } else {
        RestoreUserOutcome::NotDeleted
    })
}

/// Outcome of a free-email (clear email) attempt.
pub enum ClearEmailOutcome {
    /// The soft-deleted account's email was cleared (set to NULL).
    Cleared,
    /// No user with that id exists.
    NotFound,
    /// Refused: the user is ACTIVE. This path only frees the email of an
    /// ARCHIVED (soft-deleted) account — never a live account's address.
    NotDeleted,
}

/// Clear the `email` of a SOFT-DELETED user (set to NULL), freeing the address
/// for reuse on another account while the old account stays archived (#143
/// "uvolnit email"). SAFETY: the `AND deleted_at IS NOT NULL` guard means an
/// ACTIVE account's email can NEVER be cleared through this function.
/// `search_text` is unaffected — email is not part of it.
pub async fn clear_user_email(pool: &SqlitePool, id: i64) -> Result<ClearEmailOutcome> {
    let updated =
        sqlx::query("UPDATE users SET email = NULL WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(pool)
            .await?;
    if updated.rows_affected() == 1 {
        return Ok(ClearEmailOutcome::Cleared);
    }
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(if exists.is_none() {
        ClearEmailOutcome::NotFound
    } else {
        ClearEmailOutcome::NotDeleted
    })
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

    /// #164: every `UserRow`-decoding query now names its columns explicitly
    /// instead of `SELECT *`. Spot-check that ALL 17 struct fields decode
    /// correctly with real (non-default) values — including the nullable
    /// ones (`company`, `card_code`, `deleted_at`) and the boolean flags
    /// (`blocked`, `allow_debit`, `allow_self_entry`) that a lot of other
    /// tests leave at their default. `get_user_by_id` doesn't filter
    /// `deleted_at`, so this also proves that column decodes post-delete.
    #[tokio::test]
    async fn get_user_by_id_decodes_every_column() {
        let pool = setup().await;

        let id = create_user(
            &pool,
            Some("full@example.com"),
            Some("hash-abc"),
            "Full Fields",
            Some("+999"),
            Some("Acme Inc"),
            Some("CARD-FULL"),
            "staff",
            Some(12.5),
            None,
            None,
        )
        .await
        .unwrap();
        set_blocked(&pool, id, true).await.unwrap();
        set_allow_debit(&pool, id, true).await.unwrap();
        update_user_allow_self_entry(&pool, id, true).await.unwrap();

        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(u.id, id);
        assert_eq!(u.email.as_deref(), Some("full@example.com"));
        assert_eq!(u.name, "Full Fields");
        assert_eq!(u.password_hash.as_deref(), Some("hash-abc"));
        assert_eq!(u.phone.as_deref(), Some("+999"));
        assert_eq!(u.company.as_deref(), Some("Acme Inc"));
        assert_eq!(u.role, "staff");
        assert_eq!(u.oauth_provider, None);
        assert_eq!(u.oauth_id, None);
        assert!((u.credit - 12.5).abs() < f64::EPSILON);
        assert_eq!(u.card_code.as_deref(), Some("CARD-FULL"));
        assert!(u.blocked);
        assert!(u.allow_debit);
        assert!(u.search_text.is_some());
        assert!(!u.created_at.is_empty());
        assert_eq!(u.deleted_at, None);
        assert!(u.allow_self_entry);

        // Soft-delete: deleted_at must decode as Some(_) too (get_user_by_id
        // doesn't filter on it).
        crate::db::users::delete_user(&pool, id).await.unwrap();
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert!(u.deleted_at.is_some());
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

    /// #163: a duplicate insert must surface the typed `DbError::UniqueViolation`
    /// (not an erased error the caller has to string-match) so the route can
    /// return a friendly 409.
    #[tokio::test]
    async fn duplicate_email_surfaces_unique_violation() {
        let pool = setup().await;
        make_user(&pool, Some("dup@test.com"), "First").await;

        let err = create_user(
            &pool,
            Some("dup@test.com"),
            None,
            "Second",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, DbError::UniqueViolation),
            "expected UniqueViolation, got {err:?}"
        );
    }

    /// #163: updating a user id that does not exist returns the typed
    /// `DbError::NotFound`.
    #[tokio::test]
    async fn update_user_info_errors_when_user_missing() {
        let pool = setup().await;
        let err = update_user_info(&pool, 999_999, Some("New"), None, None, None, None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, DbError::NotFound),
            "expected NotFound, got {err:?}"
        );
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

    /// #39 recurrence (PR #290, CI run 31189869886): a card whose
    /// card_code genuinely ENDS WITH the searched digits must outrank an
    /// unrelated card whose id merely CONTAINS those digits somewhere.
    /// Reproduces the exact CI collision — "Jana Testova" (card
    /// `70701001`, a true TAIL match on `1001`) vs "AAA Polluter" (card
    /// `ZZ-99991001999`, contains `1001` in the middle but neither starts
    /// nor ends with it). Before the fix, only PREFIX matches (`1001%`)
    /// were special-cased, so neither card_code qualified and ordering
    /// fell through to plain `name ASC` — "AAA Polluter" sorts before
    /// "Jana Testova" alphabetically and wrongly won.
    #[tokio::test]
    async fn search_users_ranks_barcode_tail_match_above_mere_substring_match() {
        let pool = setup().await;
        create_user(
            &pool,
            None,
            None,
            "AAA Polluter",
            None,
            None,
            Some("ZZ-99991001999"),
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
            "Jana Testova",
            None,
            None,
            Some("70701001"),
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let results = search_users(&pool, "1001", 10).await.unwrap();
        assert_eq!(
            results.len(),
            2,
            "both rows must match the unanchored search"
        );
        assert_eq!(
            results[0].card_code.as_deref(),
            Some("70701001"),
            "the card whose barcode ENDS WITH the query must rank first, not the one that merely contains it"
        );
    }

    /// Same defect, on the function the live `/api/users/search` ROUTE
    /// actually calls (`search_users_with_pass`) — the two db-layer search
    /// functions share the ranking SQL shape, so both need the fix.
    #[tokio::test]
    async fn search_users_with_pass_ranks_barcode_tail_match_above_mere_substring_match() {
        let pool = setup().await;
        create_user(
            &pool,
            None,
            None,
            "AAA Polluter",
            None,
            None,
            Some("ZZ-99991001999"),
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
            "Jana Testova",
            None,
            None,
            Some("70701001"),
            "customer",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let results = search_users_with_pass(&pool, "1001", 10).await.unwrap();
        assert_eq!(
            results.len(),
            2,
            "both rows must match the unanchored search"
        );
        assert_eq!(
            results[0].0.card_code.as_deref(),
            Some("70701001"),
            "the card whose barcode ENDS WITH the query must rank first, not the one that merely contains it"
        );
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
        let pass_svc: i64 =
            sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let d1 = chrono::NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        let d2 = chrono::NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(pass_svc),
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
            Some(pass_svc),
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
        let pass_svc: i64 =
            sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let future = crate::util::today_bratislava() + chrono::Duration::days(10);

        let tx_id = create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(pass_svc),
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

    /// #365: a customer who has NEVER held a monthly pass has no "last price" to
    /// renew at, so `auto_renew_pass` returns None and writes nothing — the
    /// caller keeps its single-entry charge. This is the boundary that keeps
    /// today's behavior for a brand-new customer.
    #[tokio::test]
    async fn auto_renew_pass_returns_none_when_never_held_a_pass() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Never Passed").await;
        sqlx::query("UPDATE users SET credit = 12.0 WHERE id = ?")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();

        let mut conn = pool.acquire().await.unwrap();
        let result = auto_renew_pass(&mut conn, user_id, crate::util::today_bratislava())
            .await
            .unwrap();
        assert_eq!(result, None, "no prior pass → nothing to renew");
        drop(conn);

        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 12.0, "a no-op renewal must not touch credit");
        let tx_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE user_id = ?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            tx_count, 0,
            "no transaction row written when nothing renewed"
        );
    }

    /// #365: an expired prior pass renews at ABS(its amount), anchored to
    /// `anchor_day` + 1 month, debiting credit and writing a distinguishable
    /// (`staff_id NULL`, `note='auto-obnova'`) pass row. Direct helper contract
    /// test complementing the door/charger integration coverage.
    #[tokio::test]
    async fn auto_renew_pass_issues_pass_at_last_price_and_debits() {
        use crate::db::transactions::create_transaction_with_valid_until;
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Renewer").await;
        sqlx::query("UPDATE users SET credit = 10.0 WHERE id = ?")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();
        let pass_svc: i64 =
            sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        // A pass that already expired (10 days ago) — still the last one sold.
        let expired = crate::util::today_bratislava() - chrono::Duration::days(10);
        create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(pass_svc),
            -42.0,
            "charge",
            Some(expired),
            None,
        )
        .await
        .unwrap();

        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let new_credit = auto_renew_pass(&mut conn, user_id, anchor)
            .await
            .unwrap()
            .expect("a prior pass exists → renewal happens");
        drop(conn);
        assert!(
            (new_credit - (-32.0)).abs() < 1e-9,
            "10 - 42 = -32, got {new_credit}"
        );

        // valid_until = 31 Jan + 1 month, clamped to 28 Feb 2026 (non-leap).
        let renewed: (f64, Option<i64>, Option<String>, String) = sqlx::query_as(
            "SELECT amount, staff_id, note, date(valid_until) FROM transactions \
             WHERE user_id = ? AND note = 'auto-obnova'",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!((renewed.0 - (-42.0)).abs() < 1e-9);
        assert_eq!(renewed.1, None);
        assert_eq!(renewed.2.as_deref(), Some(AUTO_RENEW_NOTE));
        assert_eq!(renewed.3, "2026-02-28", "31 Jan + 1 month clamps to 28 Feb");
    }

    // ---- #372: auto-renewal recency + no-paid-visit-since gates ----

    /// Seed the user's most-recent monthly pass at `amount`, expiring on
    /// `valid_until` (a gym-local calendar date). Resolves through
    /// `user_active_pass` exactly like production (uses the shared helper).
    async fn seed_last_pass(
        pool: &SqlitePool,
        user_id: i64,
        amount: f64,
        valid_until: chrono::NaiveDate,
    ) {
        let pass_svc: i64 =
            sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(pool)
                .await
                .unwrap();
        crate::db::transactions::create_transaction_with_valid_until(
            pool,
            Some(user_id),
            None,
            Some(pass_svc),
            amount,
            "charge",
            Some(valid_until),
            None,
        )
        .await
        .unwrap();
    }

    /// Seed a class-visit (Fitness) CHARGE at an explicit `created_at`
    /// (UTC `YYYY-MM-DD HH:MM:SS`), `amount<0`, `valid_until IS NULL`. When
    /// `voided`, stamps `deleted_at` so it must NOT block renewal.
    async fn seed_class_charge(pool: &SqlitePool, user_id: i64, created_at: &str, voided: bool) {
        let fitness: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = ?")
            .bind(CLASS_VISIT_KINDS[0])
            .fetch_one(pool)
            .await
            .unwrap();
        let deleted: Option<&str> = if voided { Some(created_at) } else { None };
        sqlx::query(
            "INSERT INTO transactions \
               (user_id, service_id, amount, action, valid_until, created_at, deleted_at) \
             VALUES (?, ?, -3.0, 'charge', NULL, ?, ?)",
        )
        .bind(user_id)
        .bind(fitness)
        .bind(created_at)
        .bind(deleted)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Run `auto_renew_pass` and return whether it renewed. Asserts the return
    /// value and the written ledger row agree (Some(_) iff exactly one
    /// `auto-obnova` row exists).
    async fn did_renew(pool: &SqlitePool, user_id: i64, anchor: chrono::NaiveDate) -> bool {
        let mut conn = pool.acquire().await.unwrap();
        let result = auto_renew_pass(&mut conn, user_id, anchor).await.unwrap();
        drop(conn);
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transactions WHERE user_id = ? AND note = 'auto-obnova'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            result.is_some(),
            rows == 1,
            "return value and written renewal row must agree"
        );
        result.is_some()
    }

    /// #372(a): a pass that expired > 31 days ago must NOT auto-renew — the
    /// customer is long past the monthly window (the prod bug renewed a
    /// 6-years-dead pass). RED against the pre-#372 helper, which renewed any
    /// prior pass regardless of age.
    #[tokio::test]
    async fn auto_renew_skips_pass_expired_more_than_31_days_ago() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Long Lapsed").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(40)).await;
        assert!(
            !did_renew(&pool, user_id, anchor).await,
            "a pass expired 40 days ago must NOT renew"
        );
    }

    /// #372(b): expired <= 31 days ago BUT a paid class-visit charge happened
    /// after expiry — the customer switched to per-visit mode, so NO renewal.
    /// RED against the pre-#372 helper.
    #[tokio::test]
    async fn auto_renew_skips_when_paid_class_visit_since_expiry() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Switched To Per-Visit").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(10)).await; // vu 2026-02-19
        seed_class_charge(&pool, user_id, "2026-02-25 12:00:00", false).await; // after expiry
        assert!(
            !did_renew(&pool, user_id, anchor).await,
            "a paid class-visit after expiry must block renewal"
        );
    }

    /// #372(b'): the charger records a PAID Spinning single-visit as
    /// `action='visit'` with `amount<0` (jobs/charger.rs), NOT `action='charge'`
    /// — it must ALSO block renewal (the customer paid per-visit). This matches
    /// the codebase-canonical paid-class-visit shape (reports.rs/payments.rs use
    /// `action='visit' OR (action='charge' AND amount<0 AND valid_until NULL)`).
    /// RED against a gate-2 that only matched `action='charge'`.
    #[tokio::test]
    async fn auto_renew_skips_when_paid_charger_visit_since_expiry() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Paid Charger Spinning").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(10)).await;
        let spinning: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = ?")
            .bind(CLASS_VISIT_KINDS[1])
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at) \
             VALUES (?, ?, -5.0, 'visit', NULL, '2026-02-25 12:00:00')",
        )
        .bind(user_id)
        .bind(spinning)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            !did_renew(&pool, user_id, anchor).await,
            "a paid charger Spinning visit (action='visit', amount<0) must block renewal"
        );
    }

    /// #372(c): expired <= 31 days ago, no paid visit in between → renews (the
    /// continuing monthly customer — existing behavior preserved).
    #[tokio::test]
    async fn auto_renew_renews_recent_pass_with_no_paid_visit_since() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Continuing Monthly").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(10)).await;
        assert!(
            did_renew(&pool, user_id, anchor).await,
            "a recent pass with no paid visit since must renew"
        );
    }

    /// #372(d): the recency boundary is INCLUSIVE — a pass that expired EXACTLY
    /// 31 days ago still renews; 32 days does not. The 32-day arm is RED against
    /// the pre-#372 helper.
    #[tokio::test]
    async fn auto_renew_recency_boundary_is_inclusive_at_31_days() {
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let pool = setup().await;

        let u31 = make_user(&pool, None, "Exactly 31").await;
        seed_last_pass(&pool, u31, -30.0, anchor - chrono::Duration::days(31)).await;
        assert!(
            did_renew(&pool, u31, anchor).await,
            "expired exactly 31 days ago must still renew (inclusive boundary)"
        );

        let u32 = make_user(&pool, None, "Exactly 32").await;
        seed_last_pass(&pool, u32, -30.0, anchor - chrono::Duration::days(32)).await;
        assert!(
            !did_renew(&pool, u32, anchor).await,
            "expired 32 days ago must NOT renew"
        );
    }

    /// #372(e): a VOIDED (deleted_at) paid class-visit after expiry does NOT
    /// block renewal — only LIVE paid visits count (matches the view's own
    /// deleted_at discipline).
    #[tokio::test]
    async fn auto_renew_ignores_voided_paid_visit_when_deciding() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Voided Visit").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(10)).await;
        seed_class_charge(&pool, user_id, "2026-02-25 12:00:00", true).await; // voided
        assert!(
            did_renew(&pool, user_id, anchor).await,
            "a voided paid visit must not block renewal"
        );
    }

    /// #372: a class-visit charge BEFORE the pass expiry does not block renewal
    /// — only visits strictly AFTER valid_until count ("medzi vyprsanim a teraz").
    #[tokio::test]
    async fn auto_renew_ignores_paid_visit_before_expiry() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Paid Before Expiry").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(10)).await; // vu 2026-02-19
        seed_class_charge(&pool, user_id, "2026-02-10 12:00:00", false).await; // before expiry
        assert!(
            did_renew(&pool, user_id, anchor).await,
            "a paid visit while the pass was still valid must not block renewal"
        );
    }

    /// #372: a EUR0 door `visit` row after expiry does not block renewal — a
    /// "paid visit" is a class-visit CHARGE with amount<0, not a zero-amount
    /// door visit.
    #[tokio::test]
    async fn auto_renew_ignores_zero_amount_visit_after_expiry() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Zero Visit").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(10)).await;
        let fitness: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = ?")
            .bind(CLASS_VISIT_KINDS[0])
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at) \
             VALUES (?, ?, 0.0, 'visit', '2026-02-25 12:00:00')",
        )
        .bind(user_id)
        .bind(fitness)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            did_renew(&pool, user_id, anchor).await,
            "a EUR0 door visit must not block renewal"
        );
    }

    /// #372: a non-class (generic) paid charge after expiry does not block
    /// renewal — only class-visit kinds (Fitness/Spinning) mean "switched to
    /// per-visit mode"; bar/other items do not.
    #[tokio::test]
    async fn auto_renew_ignores_non_class_charge_after_expiry() {
        let pool = setup().await;
        let user_id = make_user(&pool, None, "Bar Charge").await;
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        seed_last_pass(&pool, user_id, -30.0, anchor - chrono::Duration::days(10)).await;
        let generic: i64 = sqlx::query_scalar(
            "INSERT INTO services (kind, name_sk, name_en, default_price) \
             VALUES ('generic', 'Bar', 'Bar', 2.0) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at) \
             VALUES (?, ?, -2.0, 'charge', NULL, '2026-02-25 12:00:00')",
        )
        .bind(user_id)
        .bind(generic)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            did_renew(&pool, user_id, anchor).await,
            "a non-class charge must not block renewal"
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

        let fitness_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = ?")
            .bind(CLASS_VISIT_KINDS[0])
            .fetch_one(&pool)
            .await
            .unwrap();
        let spinning_id: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = ?")
            .bind(CLASS_VISIT_KINDS[1])
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
        let pass_svc: i64 =
            sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mid_pass_until = chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let mid_pass_tx_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, service_id, amount, action, valid_until, created_at)
             VALUES (?, ?, -25.0, 'charge', ?, '2026-04-01 10:00:00') RETURNING id",
        )
        .bind(mid)
        .bind(pass_svc)
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

    // ─── #143: soft-deleted email conflict resolution ──────────────────────

    #[tokio::test]
    async fn get_user_by_email_including_deleted_finds_soft_deleted_row() {
        let pool = setup().await;
        let id = make_user(&pool, Some("gone@example.com"), "Gone").await;
        delete_user(&pool, id).await.unwrap();

        // The deleted_at-filtered lookup misses the row...
        assert!(
            get_user_by_email(&pool, "gone@example.com")
                .await
                .unwrap()
                .is_none(),
            "filtered lookup must NOT see the soft-deleted row"
        );
        // ...but the including-deleted lookup surfaces it (the #143 case).
        let found = get_user_by_email_including_deleted(&pool, "gone@example.com")
            .await
            .unwrap()
            .expect("including-deleted lookup must find the soft-deleted row");
        assert_eq!(found.id, id);
        assert!(found.deleted_at.is_some());
    }

    #[tokio::test]
    async fn get_user_by_email_including_deleted_finds_live_row_too() {
        let pool = setup().await;
        let id = make_user(&pool, Some("live@example.com"), "Live").await;
        let found = get_user_by_email_including_deleted(&pool, "live@example.com")
            .await
            .unwrap()
            .expect("must find the live row");
        assert_eq!(found.id, id);
        assert!(found.deleted_at.is_none());
    }

    #[tokio::test]
    async fn restore_user_clears_deleted_at_and_keeps_data() {
        let pool = setup().await;
        let id = create_user(
            &pool,
            Some("restore@example.com"),
            None,
            "Restore Me",
            None,
            Some("Acme"),
            Some("CARD-R"),
            "customer",
            Some(7.5),
            None,
            None,
        )
        .await
        .unwrap();
        delete_user(&pool, id).await.unwrap();
        assert!(
            get_user_by_id(&pool, id)
                .await
                .unwrap()
                .unwrap()
                .deleted_at
                .is_some()
        );

        assert!(matches!(
            restore_user(&pool, id).await.unwrap(),
            RestoreUserOutcome::Restored
        ));
        let u = get_user_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(u.deleted_at, None, "deleted_at must be cleared");
        // Data intact: email, credit, card_code, company all preserved.
        assert_eq!(u.email.as_deref(), Some("restore@example.com"));
        assert!((u.credit - 7.5).abs() < f64::EPSILON);
        assert_eq!(u.card_code.as_deref(), Some("CARD-R"));
        assert_eq!(u.company.as_deref(), Some("Acme"));
    }

    #[tokio::test]
    async fn restore_user_reports_not_found_and_not_deleted() {
        let pool = setup().await;
        assert!(matches!(
            restore_user(&pool, 999_999).await.unwrap(),
            RestoreUserOutcome::NotFound
        ));
        let id = make_user(&pool, Some("active@example.com"), "Active").await;
        assert!(matches!(
            restore_user(&pool, id).await.unwrap(),
            RestoreUserOutcome::NotDeleted
        ));
    }

    #[tokio::test]
    async fn clear_user_email_frees_soft_deleted_address() {
        let pool = setup().await;
        let old = make_user(&pool, Some("shared@example.com"), "Old").await;
        delete_user(&pool, old).await.unwrap();

        assert!(matches!(
            clear_user_email(&pool, old).await.unwrap(),
            ClearEmailOutcome::Cleared
        ));
        // The archived row keeps its deleted_at but loses the email...
        let u = get_user_by_id(&pool, old).await.unwrap().unwrap();
        assert_eq!(u.email, None, "email must be cleared");
        assert!(u.deleted_at.is_some(), "row must stay archived");
        // ...so the address is now free for a NEW user.
        let new = make_user(&pool, Some("shared@example.com"), "New").await;
        assert_ne!(new, old);
    }

    #[tokio::test]
    async fn clear_user_email_refuses_active_account() {
        let pool = setup().await;
        let id = make_user(&pool, Some("keep@example.com"), "Keep").await;
        assert!(
            matches!(
                clear_user_email(&pool, id).await.unwrap(),
                ClearEmailOutcome::NotDeleted
            ),
            "must refuse to clear a LIVE account's email"
        );
        // Email is untouched.
        assert_eq!(
            get_user_by_id(&pool, id)
                .await
                .unwrap()
                .unwrap()
                .email
                .as_deref(),
            Some("keep@example.com")
        );

        assert!(matches!(
            clear_user_email(&pool, 999_999).await.unwrap(),
            ClearEmailOutcome::NotFound
        ));
    }
}
