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

use anyhow::{Context, Result};
use base64::Engine as _;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

/// Invite (onboarding) token lifetime: 14 days.
pub const INVITE_TTL_SECS: i64 = 14 * 24 * 60 * 60;
/// Login-link (recovery) token lifetime: 24 hours.
pub const LOGIN_TTL_SECS: i64 = 24 * 60 * 60;

/// The two token purposes. Kept as `&str` constants so callers and the SQL
/// CHECK constraint (migration V17) stay in sync.
pub const PURPOSE_INVITE: &str = "invite";
pub const PURPOSE_LOGIN: &str = "login";

/// Generate a fresh raw token: 32 cryptographically-random bytes encoded as
/// URL-safe base64 without padding (43 chars, safe in a query string). The
/// return value is the ONLY place the raw token exists — store its hash, put
/// this in the link, then drop it.
pub fn generate_raw_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// SHA-256 hex of the raw token — exactly what is stored in `token_hash`.
pub fn hash_token(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    hex::encode(digest)
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
    .await
    .context("Failed to insert login token")?;
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
    let user_id = q
        .fetch_optional(pool)
        .await
        .context("Failed to redeem login token")?;
    Ok(user_id)
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
    .await
    .context("Failed to purge login tokens")?;
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
}
