//! Magic-link login tokens (#108).
//!
//! Passwordless customer onboarding + recovery. A token is 32 random bytes
//! encoded base64url (no padding). The RAW token travels ONLY inside the
//! emailed link; the DB stores ONLY its SHA-256 hex (`token_hash`), so a DB
//! read never yields a usable token. Redemption is single-use and race-safe:
//! one atomic `UPDATE ... SET used_at = datetime('now') WHERE used_at IS NULL
//! AND expires_at > datetime('now') ... RETURNING user_id` marks and returns
//! the row in a single statement, so two concurrent redemptions of the same
//! token can never both succeed.
//!
//! SECURITY: never log the raw token. Log the hash if anything is logged.

use crate::db::error::Result;
use base64::Engine as _;
use rand::Rng;
use sqlx::SqlitePool;

/// Invite (onboarding) token lifetime: 14 days.
pub const INVITE_TTL_SECS: i64 = 14 * 24 * 60 * 60;
/// Login-link (recovery) token lifetime: 24 hours.
pub const LOGIN_TTL_SECS: i64 = 24 * 60 * 60;

/// Login-code (in-PWA numeric) token lifetime: 10 minutes (#227).
pub const CODE_TTL_SECS: i64 = 10 * 60;

/// The three token purposes. Kept as `&str` constants so callers and the SQL
/// CHECK constraint (migration V17, widened by V21) stay in sync.
pub const PURPOSE_INVITE: &str = "invite";
pub const PURPOSE_LOGIN: &str = "login";
/// Third purpose (#227): a short 6-digit numeric code the user types inside the
/// installed PWA. Added to the CHECK by migration V21.
pub const PURPOSE_CODE: &str = "code";

/// A 6-digit code is invalidated after this many failed verify attempts — the
/// mandatory low-entropy brute-force guard (6 digits is guessable) (#227).
pub const MAX_CODE_ATTEMPTS: i64 = 5;

/// Generate a fresh raw token: 32 cryptographically-random bytes encoded as
/// URL-safe base64 without padding (43 chars, safe in a query string). The
/// return value is the ONLY place the raw token exists — store its hash, put
/// this in the link, then drop it.
pub fn generate_raw_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// SHA-256 hex of the raw token — exactly what is stored in `token_hash`.
pub fn hash_token(raw: &str) -> String {
    crate::db::sha256_hex(raw)
}

/// Generate a fresh 6-digit login code (#227): a cryptographically-random value
/// in `000000..=999999`, always rendered as exactly 6 digits (zero-padded, so
/// `42` becomes `"000042"`). The return value is the ONLY place the raw code
/// exists — store its per-user hash, email this to the user, then drop it.
pub fn generate_code() -> String {
    let n: u32 = rand::rng().random_range(0..1_000_000);
    format!("{n:06}")
}

/// Per-user SHA-256 hex of a login code (#227). The code is salted with its
/// `user_id` (`"{user_id}:{code}"`) so two users issued the SAME 6-digit value
/// never collide on the `token_hash` UNIQUE index, and a stored code hash is
/// bound to its own account (it can only be redeemed for the user it was issued
/// to). Never log the raw code.
pub fn hash_code(user_id: i64, code: &str) -> String {
    crate::db::sha256_hex(&format!("{user_id}:{code}"))
}

/// Create a token for `user_id` with the given `purpose` and TTL, store its
/// hash, and return the RAW token (for the link). `expires_at` is computed in
/// SQL via `datetime('now', ?)` so it uses the exact same clock/format the
/// redemption comparison (`expires_at > datetime('now')`) reads back.
pub async fn create_token(
    pool: &SqlitePool,
    user_id: i64,
    purpose: &str,
    ttl_secs: i64,
) -> Result<String> {
    let raw = generate_raw_token();
    let hash = hash_token(&raw);
    let interval = format!("{ttl_secs:+} seconds");
    sqlx::query(
        "INSERT INTO login_tokens (user_id, token_hash, purpose, expires_at)
         VALUES (?, ?, ?, datetime('now', ?))",
    )
    .bind(user_id)
    .bind(&hash)
    .bind(purpose)
    .bind(&interval)
    .execute(pool)
    .await?;
    Ok(raw)
}

/// Atomically redeem a raw token: mark it used and return its `user_id` iff it
/// is (a) known, (b) not yet used, (c) not expired, and (d) has a purpose in
/// `allowed_purposes`. Returns `None` for any failing token (invalid / expired
/// / already used / wrong purpose) — the caller maps all of these to a single
/// uniform rejection so nothing is leaked. Single `UPDATE ... RETURNING` keeps
/// mark-and-fetch atomic against concurrent redemption.
pub async fn redeem(
    pool: &SqlitePool,
    raw: &str,
    allowed_purposes: &[&str],
) -> Result<Option<i64>> {
    if allowed_purposes.is_empty() {
        return Ok(None);
    }
    let hash = hash_token(raw);
    let placeholders = std::iter::repeat_n("?", allowed_purposes.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "UPDATE login_tokens SET used_at = datetime('now') \
         WHERE token_hash = ? \
           AND used_at IS NULL \
           AND expires_at > datetime('now') \
           AND purpose IN ({placeholders}) \
         RETURNING user_id"
    );
    let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(hash);
    for p in allowed_purposes {
        q = q.bind(*p);
    }
    let user_id = q.fetch_optional(pool).await?;
    Ok(user_id)
}

/// Create a fresh 6-digit login code for `user_id`, store its per-user hash with
/// `purpose='code'` and a 10-minute TTL, and return the RAW code (for the
/// email). Every PRIOR code row for this user is DELETED first: requesting a new
/// code invalidates all earlier unused codes (#227), and deleting (rather than
/// marking used) also frees the `token_hash` UNIQUE slot so a rare
/// same-user/same-value re-issue can never trip the unique constraint on insert.
/// Invite/login tokens are untouched. Returns the raw code — the only place it
/// ever exists in cleartext.
pub async fn create_code(pool: &SqlitePool, user_id: i64) -> Result<String> {
    // A new code supersedes any earlier one for this user.
    sqlx::query("DELETE FROM login_tokens WHERE user_id = ? AND purpose = ?")
        .bind(user_id)
        .bind(PURPOSE_CODE)
        .execute(pool)
        .await?;

    let code = generate_code();
    let hash = hash_code(user_id, &code);
    let interval = format!("{CODE_TTL_SECS:+} seconds");
    sqlx::query(
        "INSERT INTO login_tokens (user_id, token_hash, purpose, expires_at)
         VALUES (?, ?, ?, datetime('now', ?))",
    )
    .bind(user_id)
    .bind(&hash)
    .bind(PURPOSE_CODE)
    .bind(&interval)
    .execute(pool)
    .await?;
    Ok(code)
}

/// Verify a login code for `user_id` and, on success, atomically redeem it
/// (single-use). Semantics (#227):
/// - Considers only the user's NEWEST unused, unexpired `code` row.
/// - Correct code → mark it used and return `Some(user_id)`.
/// - Wrong code → increment its `attempts`; on the `MAX_CODE_ATTEMPTS`-th failed
///   attempt the code row is INVALIDATED (marked used) so it can never be
///   guessed further.
/// - No live code / expired / already used / already exhausted → `Ok(None)`.
///
/// Every failure mode collapses to `Ok(None)`; the caller maps each `None` to a
/// single uniform rejection, so nothing distinguishes "wrong code" from
/// "expired" from "no code" to a client. Wrapped in a transaction so the
/// find-then-update is one atomic unit against concurrent verifies (SQLite
/// serialises writers).
pub async fn verify_code(pool: &SqlitePool, user_id: i64, code: &str) -> Result<Option<i64>> {
    let candidate = hash_code(user_id, code);
    let mut tx = pool.begin().await?;

    // Newest still-live code row for this user.
    let row: Option<(i64, String, i64)> = sqlx::query_as(
        "SELECT id, token_hash, attempts FROM login_tokens
         WHERE user_id = ? AND purpose = ?
           AND used_at IS NULL
           AND expires_at > datetime('now')
         ORDER BY id DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(PURPOSE_CODE)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((row_id, stored_hash, attempts)) = row else {
        // No live code to verify against. Commit the (empty) tx and reject.
        tx.commit().await?;
        return Ok(None);
    };

    if stored_hash == candidate {
        // Correct → single-use redeem. The `used_at IS NULL` guard keeps the
        // mark race-safe even though we already hold the row inside the tx.
        sqlx::query(
            "UPDATE login_tokens SET used_at = datetime('now') WHERE id = ? AND used_at IS NULL",
        )
        .bind(row_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Some(user_id));
    }

    // Wrong → count the failed attempt; invalidate the code at the cap.
    let new_attempts = attempts + 1;
    if new_attempts >= MAX_CODE_ATTEMPTS {
        sqlx::query("UPDATE login_tokens SET attempts = ?, used_at = datetime('now') WHERE id = ?")
            .bind(new_attempts)
            .bind(row_id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("UPDATE login_tokens SET attempts = ? WHERE id = ?")
            .bind(new_attempts)
            .bind(row_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(None)
}

/// Delete rows that can no longer redeem: already used, or expired.
/// `redeem`'s validity check is `used_at IS NULL AND expires_at >
/// datetime('now')` — this predicate is the exact logical negation
/// (`used_at IS NOT NULL OR expires_at <= datetime('now')`), so it is
/// mutually exclusive with "still redeemable": purging never removes a row
/// `redeem` would still accept, and never leaves behind a row `redeem` would
/// reject. Pure housekeeping; it only stops the table from growing
/// unbounded. Returns the number of rows removed.
pub async fn purge_expired_and_used(pool: &SqlitePool) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM login_tokens WHERE used_at IS NOT NULL OR expires_at <= datetime('now')",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed_customer(pool: &SqlitePool, email: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES (?, 'T', 'customer') RETURNING id",
        )
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[test]
    fn generated_tokens_are_urlsafe_and_unique() {
        let a = generate_raw_token();
        let b = generate_raw_token();
        assert_ne!(a, b, "two tokens must differ");
        // 32 bytes → 43 base64url chars, no padding.
        assert_eq!(a.len(), 43, "unexpected token length: {a:?}");
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token must be URL-safe (no '+', '/', '='): {a:?}"
        );
    }

    #[test]
    fn ttl_constants_are_exactly_14_days_and_24_hours() {
        // Pin the business requirement (invite = 14 days, login-link = 24 hours)
        // to literal seconds so any arithmetic drift in the constant definitions
        // is caught. Literals (not products) so the test itself has nothing to
        // mutate.
        assert_eq!(INVITE_TTL_SECS, 1_209_600, "invite TTL must be 14 days");
        assert_eq!(LOGIN_TTL_SECS, 86_400, "login-link TTL must be 24 hours");
    }

    #[test]
    fn hash_is_deterministic_sha256_hex() {
        // Known SHA-256("abc") vector — pins the algorithm + hex encoding.
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_token("abc"), hash_token("abc"));
        assert_ne!(hash_token("abc"), hash_token("abd"));
    }

    #[tokio::test]
    async fn create_then_redeem_returns_user_id_once() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "redeem@x").await;

        let raw = create_token(&pool, uid, PURPOSE_INVITE, INVITE_TTL_SECS)
            .await
            .unwrap();
        // Only the hash is stored — the raw token is never in the DB.
        let stored_raw: Option<i64> =
            sqlx::query_scalar("SELECT id FROM login_tokens WHERE token_hash = ?")
                .bind(&raw)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(
            stored_raw.is_none(),
            "raw token must NOT be stored verbatim"
        );

        let redeemed = redeem(&pool, &raw, &[PURPOSE_INVITE, PURPOSE_LOGIN])
            .await
            .unwrap();
        assert_eq!(redeemed, Some(uid), "valid token must return its user_id");
    }

    #[tokio::test]
    async fn reused_token_is_rejected() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "reuse@x").await;

        let raw = create_token(&pool, uid, PURPOSE_LOGIN, LOGIN_TTL_SECS)
            .await
            .unwrap();
        let first = redeem(&pool, &raw, &[PURPOSE_LOGIN]).await.unwrap();
        assert_eq!(first, Some(uid));
        let second = redeem(&pool, &raw, &[PURPOSE_LOGIN]).await.unwrap();
        assert_eq!(second, None, "a token must be single-use");
    }

    #[tokio::test]
    async fn expired_token_is_rejected() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "expired@x").await;

        // Negative TTL → expires_at is already in the past.
        let raw = create_token(&pool, uid, PURPOSE_INVITE, -10).await.unwrap();
        let redeemed = redeem(&pool, &raw, &[PURPOSE_INVITE, PURPOSE_LOGIN])
            .await
            .unwrap();
        assert_eq!(redeemed, None, "expired token must be rejected");
        // And it must NOT be marked used (it never validated).
        let used: Option<String> =
            sqlx::query_scalar("SELECT used_at FROM login_tokens WHERE token_hash = ?")
                .bind(hash_token(&raw))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(used.is_none(), "rejected token must stay unused");
    }

    #[tokio::test]
    async fn unknown_token_is_rejected() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let redeemed = redeem(&pool, "never-issued", &[PURPOSE_INVITE, PURPOSE_LOGIN])
            .await
            .unwrap();
        assert_eq!(redeemed, None);
    }

    #[tokio::test]
    async fn wrong_purpose_is_rejected_by_scoping() {
        // An invite token must NOT redeem when only 'login' is allowed — the
        // purpose-scoping mechanism. (The token-login endpoint allows BOTH
        // purposes, so this is a defense-in-depth mechanism test, not the
        // endpoint's behavior.)
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "purpose@x").await;

        let raw = create_token(&pool, uid, PURPOSE_INVITE, INVITE_TTL_SECS)
            .await
            .unwrap();
        let scoped_out = redeem(&pool, &raw, &[PURPOSE_LOGIN]).await.unwrap();
        assert_eq!(
            scoped_out, None,
            "invite token must not redeem as login-only"
        );
        // Still unused → it can still redeem under the correct scope.
        let scoped_in = redeem(&pool, &raw, &[PURPOSE_INVITE]).await.unwrap();
        assert_eq!(scoped_in, Some(uid));
    }

    #[tokio::test]
    async fn redeem_with_empty_allowed_purposes_returns_none() {
        // The empty-slice guard prevents building an invalid `purpose IN ()`
        // clause — an empty allow-list can redeem nothing.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "empty-scope@x").await;

        let raw = create_token(&pool, uid, PURPOSE_INVITE, INVITE_TTL_SECS)
            .await
            .unwrap();
        let redeemed = redeem(&pool, &raw, &[]).await.unwrap();
        assert_eq!(redeemed, None, "empty allow-list must redeem nothing");
        // The token must remain unused (the empty-list path never marked it).
        let used: Option<String> =
            sqlx::query_scalar("SELECT used_at FROM login_tokens WHERE token_hash = ?")
                .bind(hash_token(&raw))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            used.is_none(),
            "token must stay unused after empty-scope redeem"
        );
    }

    #[tokio::test]
    async fn purge_removes_used_and_expired_but_keeps_live_token() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "purge@x").await;

        // Used: create + redeem so used_at gets stamped.
        let used_raw = create_token(&pool, uid, PURPOSE_LOGIN, LOGIN_TTL_SECS)
            .await
            .unwrap();
        redeem(&pool, &used_raw, &[PURPOSE_LOGIN]).await.unwrap();

        // Expired: negative TTL puts expires_at in the past, never redeemed.
        create_token(&pool, uid, PURPOSE_INVITE, -10).await.unwrap();

        // Live: unused, future expiry — must survive the purge.
        let live_raw = create_token(&pool, uid, PURPOSE_INVITE, INVITE_TTL_SECS)
            .await
            .unwrap();

        let removed = purge_expired_and_used(&pool).await.unwrap();
        assert_eq!(removed, 2, "used + expired rows must be removed");

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 1, "only the live token should remain");

        // The purge must not have touched redeem behavior: the live token
        // still redeems successfully afterwards.
        let redeemed = redeem(&pool, &live_raw, &[PURPOSE_INVITE]).await.unwrap();
        assert_eq!(
            redeemed,
            Some(uid),
            "live token must still redeem after a purge"
        );
    }

    #[tokio::test]
    async fn purge_is_a_noop_when_nothing_qualifies() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "purge-noop@x").await;
        create_token(&pool, uid, PURPOSE_INVITE, INVITE_TTL_SECS)
            .await
            .unwrap();

        let removed = purge_expired_and_used(&pool).await.unwrap();
        assert_eq!(removed, 0, "a live-only table must purge nothing");
    }

    // ── login codes (#227) ────────────────────────────────────────────────

    #[test]
    fn generated_code_is_exactly_six_digits() {
        for _ in 0..200 {
            let c = generate_code();
            assert_eq!(c.len(), 6, "code must be 6 chars: {c:?}");
            assert!(
                c.chars().all(|ch| ch.is_ascii_digit()),
                "code must be all digits: {c:?}"
            );
            let n: u32 = c.parse().expect("code must parse as a number");
            assert!(n < 1_000_000, "code must be < 1_000_000: {n}");
        }
    }

    #[test]
    fn code_ttl_is_ten_minutes() {
        assert_eq!(CODE_TTL_SECS, 600, "login-code TTL must be 10 minutes");
    }

    #[test]
    fn hash_code_is_per_user_and_deterministic() {
        // Deterministic for the same (user, code).
        assert_eq!(hash_code(7, "123456"), hash_code(7, "123456"));
        // Salted by user_id: the same code hashes DIFFERENTLY for two users, so
        // two users issued the same value never collide on token_hash.
        assert_ne!(hash_code(7, "123456"), hash_code(8, "123456"));
        // Different code → different hash.
        assert_ne!(hash_code(7, "123456"), hash_code(7, "123457"));
    }

    #[tokio::test]
    async fn create_code_returns_six_digits_and_stores_only_the_hash() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-create@x").await;

        let code = create_code(&pool, uid).await.unwrap();
        assert_eq!(code.len(), 6);

        // The raw code is NEVER stored; only its per-user hash is.
        let raw_present: Option<i64> =
            sqlx::query_scalar("SELECT id FROM login_tokens WHERE token_hash = ?")
                .bind(&code)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(raw_present.is_none(), "raw code must not be stored");
        let hash_present: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM login_tokens WHERE token_hash = ? AND purpose = 'code'",
        )
        .bind(hash_code(uid, &code))
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(hash_present.is_some(), "per-user hash must be stored");
    }

    #[tokio::test]
    async fn create_code_invalidates_the_previous_code() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-supersede@x").await;

        let first = create_code(&pool, uid).await.unwrap();
        let second = create_code(&pool, uid).await.unwrap();

        // Only one code row exists (the old one was deleted, not accumulated).
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM login_tokens WHERE user_id = ? AND purpose = 'code'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "a new code must supersede (delete) the prior one");

        // The OLD code no longer verifies; the NEW one does.
        assert_eq!(verify_code(&pool, uid, &first).await.unwrap(), None);
        assert_eq!(verify_code(&pool, uid, &second).await.unwrap(), Some(uid));
    }

    #[tokio::test]
    async fn verify_code_happy_path_redeems_once() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-happy@x").await;

        let code = create_code(&pool, uid).await.unwrap();
        assert_eq!(
            verify_code(&pool, uid, &code).await.unwrap(),
            Some(uid),
            "correct code must return the user id"
        );
        // Single-use: a second verify with the same code fails.
        assert_eq!(
            verify_code(&pool, uid, &code).await.unwrap(),
            None,
            "a code is single-use"
        );
    }

    #[tokio::test]
    async fn verify_code_wrong_code_counts_an_attempt() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-wrong@x").await;

        let code = create_code(&pool, uid).await.unwrap();
        let wrong = wrong_code(&code);
        assert_eq!(verify_code(&pool, uid, &wrong).await.unwrap(), None);
        let attempts: i64 = sqlx::query_scalar(
            "SELECT attempts FROM login_tokens WHERE user_id = ? AND purpose = 'code'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(attempts, 1, "a wrong verify must increment attempts");
        // The still-live code still accepts the correct value afterwards.
        assert_eq!(verify_code(&pool, uid, &code).await.unwrap(), Some(uid));
    }

    #[tokio::test]
    async fn verify_code_correct_on_the_fifth_attempt_still_works() {
        // Four wrong attempts leave the code live; a correct 5th entry succeeds.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-4wrong@x").await;

        let code = create_code(&pool, uid).await.unwrap();
        let wrong = wrong_code(&code);
        for _ in 0..4 {
            assert_eq!(verify_code(&pool, uid, &wrong).await.unwrap(), None);
        }
        assert_eq!(
            verify_code(&pool, uid, &code).await.unwrap(),
            Some(uid),
            "the correct code must still work after 4 failed attempts"
        );
    }

    #[tokio::test]
    async fn verify_code_five_wrong_attempts_invalidate_the_code() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-5wrong@x").await;

        let code = create_code(&pool, uid).await.unwrap();
        let wrong = wrong_code(&code);
        for _ in 0..5 {
            assert_eq!(verify_code(&pool, uid, &wrong).await.unwrap(), None);
        }
        // After MAX_CODE_ATTEMPTS wrong tries the code is invalidated — even the
        // CORRECT value no longer logs in.
        assert_eq!(
            verify_code(&pool, uid, &code).await.unwrap(),
            None,
            "5 wrong attempts must invalidate the code"
        );
        let used: Option<String> = sqlx::query_scalar(
            "SELECT used_at FROM login_tokens WHERE user_id = ? AND purpose = 'code'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(used.is_some(), "an exhausted code must be marked used");
    }

    #[tokio::test]
    async fn verify_code_expired_returns_none() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-expired@x").await;

        // Insert an already-expired code row directly.
        sqlx::query(
            "INSERT INTO login_tokens (user_id, token_hash, purpose, expires_at)
             VALUES (?, ?, 'code', datetime('now', '-1 minute'))",
        )
        .bind(uid)
        .bind(hash_code(uid, "123456"))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_code(&pool, uid, "123456").await.unwrap(),
            None,
            "an expired code must not verify"
        );
    }

    #[tokio::test]
    async fn verify_code_with_no_code_returns_none() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "code-none@x").await;
        assert_eq!(verify_code(&pool, uid, "123456").await.unwrap(), None);
    }

    #[tokio::test]
    async fn verify_code_is_scoped_to_the_issuing_user() {
        // A code created for user A must never verify for user B (per-user salt).
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let a = seed_customer(&pool, "code-a@x").await;
        let b = seed_customer(&pool, "code-b@x").await;

        let code = create_code(&pool, a).await.unwrap();
        assert_eq!(
            verify_code(&pool, b, &code).await.unwrap(),
            None,
            "a code must not verify for a different user"
        );
        // And it still works for its own user.
        assert_eq!(verify_code(&pool, a, &code).await.unwrap(), Some(a));
    }

    /// Return a 6-digit code guaranteed to DIFFER from `code` (flip the last
    /// digit), so "wrong code" tests never accidentally match the real one.
    fn wrong_code(code: &str) -> String {
        let last: u32 = code[5..6].parse().unwrap();
        format!("{}{}", &code[..5], (last + 1) % 10)
    }
}
