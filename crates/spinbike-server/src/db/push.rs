//! Push-subscription storage and the anti-spam notify ledger (#264,
//! migrations V23/V24 — see `db::migrations`). Two independent tables:
//!
//! - `push_subscriptions`: one row per browser subscription for a user.
//! - `push_notify_log`: one row per (user, reason) — the daily notification
//!   job's cooldown/episode-cap/re-arm state (`last_notified_at` +
//!   `sent_count`). `jobs::notifications` is the only writer of
//!   `record_notified`/`clear_notified`.

use anyhow::Result;
use sqlx::SqlitePool;

/// The fixed `push_notify_log.reason` vocabulary (enforced by the V23 CHECK
/// constraint too — these constants are the single place the two literal
/// strings are spelled, so a typo can't silently create an untracked reason).
pub const REASON_LOW_CREDIT: &str = "low_credit";
pub const REASON_PASS_EXPIRING: &str = "pass_expiring";

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct PushSubscriptionRow {
    pub id: i64,
    pub user_id: i64,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

/// Store (or refresh) a subscription. `endpoint` is UNIQUE — re-subscribing
/// the same endpoint (a re-click, or the browser silently re-issuing the
/// same subscription) upserts in place rather than creating a duplicate
/// row, and clears any stale failure state from a PRIOR owner/attempt.
pub async fn upsert_subscription(
    pool: &SqlitePool,
    user_id: i64,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(endpoint) DO UPDATE SET
             user_id = excluded.user_id,
             p256dh = excluded.p256dh,
             auth = excluded.auth,
             failure_count = 0,
             last_error_at = NULL",
    )
    .bind(user_id)
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a subscription owned by `user_id` for `endpoint` (the "turn
/// notifications off" path). Scoped to the CALLER's own user_id so one
/// customer can't unsubscribe another's device by guessing an endpoint URL.
pub async fn delete_subscription(pool: &SqlitePool, user_id: i64, endpoint: &str) -> Result<u64> {
    let res = sqlx::query("DELETE FROM push_subscriptions WHERE user_id = ? AND endpoint = ?")
        .bind(user_id)
        .bind(endpoint)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

pub async fn list_subscriptions_for_user(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<PushSubscriptionRow>> {
    let rows = sqlx::query_as::<_, PushSubscriptionRow>(
        "SELECT id, user_id, endpoint, p256dh, auth FROM push_subscriptions WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// True when `user_id` has at least one stored subscription — used by
/// `/api/push/config` to report the client's "on" state without leaking
/// endpoint/key material back to the client.
pub async fn has_subscription(pool: &SqlitePool, user_id: i64) -> Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    Ok(n > 0)
}

/// The push service told us this endpoint is gone (404/410) — delete it
/// outright so dead endpoints don't accumulate forever (per the issue).
pub async fn prune_subscription(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM push_subscriptions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_send_success(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE push_subscriptions
         SET last_success_at = datetime('now'), failure_count = 0
         WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Non-fatal send failure (429/5xx/transport error) — NOT a prune signal on
/// its own (only EndpointNotValid/EndpointNotFound is); just recorded for
/// observability so a subscription that's been failing for a while is
/// visible without any special handling yet.
pub async fn record_send_failure(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE push_subscriptions
         SET last_error_at = datetime('now'), failure_count = failure_count + 1
         WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// One (user, reason) ledger row — the anti-spam episode state (#264
/// follow-up, owner decision 2026-08-08: max `MAX_NOTIFICATIONS_PER_EPISODE`
/// notifications per episode, then silence until re-arm).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct NotifyLogRow {
    /// SQLite `datetime('now')` format, `YYYY-MM-DD HH:MM:SS`.
    pub last_notified_at: String,
    /// How many notifications have been sent THIS episode (since the last
    /// re-arm). Compared against `jobs::notifications::MAX_NOTIFICATIONS_PER_EPISODE`.
    pub sent_count: i64,
}

/// The ledger row for (user, reason), or `None` if never notified this
/// episode / already re-armed.
pub async fn notify_log(
    pool: &SqlitePool,
    user_id: i64,
    reason: &str,
) -> Result<Option<NotifyLogRow>> {
    let row = sqlx::query_as::<_, NotifyLogRow>(
        "SELECT last_notified_at, sent_count FROM push_notify_log WHERE user_id = ? AND reason = ?",
    )
    .bind(user_id)
    .bind(reason)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Stamp (user, reason) as notified NOW and increment `sent_count`. Only
/// called after an actual successful send (see `jobs::notifications`) —
/// never speculatively.
pub async fn record_notified(pool: &SqlitePool, user_id: i64, reason: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO push_notify_log (user_id, reason, last_notified_at, sent_count)
         VALUES (?, ?, datetime('now'), 1)
         ON CONFLICT(user_id, reason) DO UPDATE SET
             last_notified_at = excluded.last_notified_at,
             sent_count = push_notify_log.sent_count + 1",
    )
    .bind(user_id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// Re-arm: drop the ledger row for (user, reason) — the condition is no
/// longer true (credit topped up / pass renewed), so the NEXT time it
/// becomes true both the cooldown clock AND the per-episode `sent_count`
/// start fresh instead of inheriting state from a previous episode.
pub async fn clear_notified(pool: &SqlitePool, user_id: i64, reason: &str) -> Result<()> {
    sqlx::query("DELETE FROM push_notify_log WHERE user_id = ? AND reason = ?")
        .bind(user_id)
        .bind(reason)
        .execute(pool)
        .await?;
    Ok(())
}

/// The user's most recent CREDIT-INCREASING transaction amount (a
/// "top-up"), or `None` if they have never topped up. `action = 'topup' AND
/// amount > 0 AND deleted_at IS NULL` is the SAME filter
/// `routes/users.rs`'s `topped_up` aggregate already uses to identify a
/// real top-up (never a monthly-pass sale — those are `action='charge'`
/// with a NEGATIVE amount; never a charge/visit; never a voided row).
///
/// Ordered `created_at DESC, id DESC` per `.claude/rules/transaction-ordering.md`
/// (#291) — `created_at` has only SECOND precision, so same-second ties
/// need the `id` tiebreaker to deterministically pick the newest row.
pub async fn last_topup_amount(pool: &SqlitePool, user_id: i64) -> Result<Option<f64>> {
    let amount: Option<f64> = sqlx::query_scalar(
        "SELECT amount FROM transactions
         WHERE user_id = ?
           AND action = 'topup'
           AND amount > 0
           AND deleted_at IS NULL
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed_customer(pool: &SqlitePool, email: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO users (email, name, role) VALUES (?, 'Test', 'customer') RETURNING id",
        )
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn upsert_then_list_returns_the_subscription() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "sub@x").await;

        upsert_subscription(&pool, uid, "https://push.example/a", "p256", "auth")
            .await
            .unwrap();

        let rows = list_subscriptions_for_user(&pool, uid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].endpoint, "https://push.example/a");
        assert!(has_subscription(&pool, uid).await.unwrap());
    }

    #[tokio::test]
    async fn upsert_same_endpoint_updates_keys_instead_of_duplicating() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "resub@x").await;

        upsert_subscription(
            &pool,
            uid,
            "https://push.example/dup",
            "old-p256",
            "old-auth",
        )
        .await
        .unwrap();
        upsert_subscription(
            &pool,
            uid,
            "https://push.example/dup",
            "new-p256",
            "new-auth",
        )
        .await
        .unwrap();

        let rows = list_subscriptions_for_user(&pool, uid).await.unwrap();
        assert_eq!(rows.len(), 1, "re-subscribing must upsert, not duplicate");
        assert_eq!(rows[0].p256dh, "new-p256");
        assert_eq!(rows[0].auth, "new-auth");
    }

    #[tokio::test]
    async fn delete_subscription_is_scoped_to_the_owning_user() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid_a = seed_customer(&pool, "a@x").await;
        let uid_b = seed_customer(&pool, "b@x").await;

        upsert_subscription(&pool, uid_a, "https://push.example/scoped", "p", "a")
            .await
            .unwrap();

        // Wrong owner: deletes nothing.
        let removed = delete_subscription(&pool, uid_b, "https://push.example/scoped")
            .await
            .unwrap();
        assert_eq!(removed, 0);
        assert!(has_subscription(&pool, uid_a).await.unwrap());

        // Right owner: deletes it.
        let removed = delete_subscription(&pool, uid_a, "https://push.example/scoped")
            .await
            .unwrap();
        assert_eq!(removed, 1);
        assert!(!has_subscription(&pool, uid_a).await.unwrap());
    }

    #[tokio::test]
    async fn prune_removes_the_row_outright() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "prune@x").await;
        upsert_subscription(&pool, uid, "https://push.example/gone", "p", "a")
            .await
            .unwrap();
        let id = list_subscriptions_for_user(&pool, uid).await.unwrap()[0].id;

        prune_subscription(&pool, id).await.unwrap();

        assert!(!has_subscription(&pool, uid).await.unwrap());
    }

    #[tokio::test]
    async fn record_notified_then_clear_notified_round_trips() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "ledger@x").await;

        assert_eq!(
            notify_log(&pool, uid, REASON_LOW_CREDIT).await.unwrap(),
            None
        );

        record_notified(&pool, uid, REASON_LOW_CREDIT)
            .await
            .unwrap();
        let log = notify_log(&pool, uid, REASON_LOW_CREDIT)
            .await
            .unwrap()
            .expect("row must exist after record_notified");
        assert_eq!(log.sent_count, 1);

        // Re-notifying (upsert) must not create a second row for the same
        // (user, reason) — the PRIMARY KEY(user_id, reason) enforces this —
        // and must INCREMENT sent_count, not reset it.
        record_notified(&pool, uid, REASON_LOW_CREDIT)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM push_notify_log WHERE user_id = ? AND reason = ?",
        )
        .bind(uid)
        .bind(REASON_LOW_CREDIT)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "still one row, not a duplicate");
        let log = notify_log(&pool, uid, REASON_LOW_CREDIT)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(log.sent_count, 2, "second record_notified must increment");

        clear_notified(&pool, uid, REASON_LOW_CREDIT).await.unwrap();
        assert_eq!(
            notify_log(&pool, uid, REASON_LOW_CREDIT).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn the_two_reasons_are_independent_ledger_rows() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "tworeasons@x").await;

        record_notified(&pool, uid, REASON_LOW_CREDIT)
            .await
            .unwrap();
        assert!(
            notify_log(&pool, uid, REASON_PASS_EXPIRING)
                .await
                .unwrap()
                .is_none(),
            "notifying one reason must not affect the other"
        );
    }

    // ── last_topup_amount (#264 follow-up, owner decision 2026-08-08) ──────

    async fn seed_service_id(pool: &SqlitePool, kind: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM services WHERE kind = ?")
            .bind(kind)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn last_topup_amount_is_none_with_no_history() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "notopup@x").await;

        assert_eq!(last_topup_amount(&pool, uid).await.unwrap(), None);
    }

    #[tokio::test]
    async fn last_topup_amount_picks_the_newest_by_created_at_then_id() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "topups@x").await;

        // Two top-ups in the SAME second (same created_at) — the tiebreaker
        // (id DESC) must decide, per transaction-ordering.md (#291).
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, created_at)
             VALUES (?, 10.0, 'topup', '2026-08-08 10:00:00'),
                    (?, 25.0, 'topup', '2026-08-08 10:00:00')",
        )
        .bind(uid)
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(last_topup_amount(&pool, uid).await.unwrap(), Some(25.0));
    }

    #[tokio::test]
    async fn last_topup_amount_excludes_charges_visits_passes_and_voided_rows() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_customer(&pool, "excl@x").await;
        let pass_svc = seed_service_id(&pool, "monthly_pass").await;

        // A charge (negative amount) — not a top-up.
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action) VALUES (?, -5.0, 'charge')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        // A visit (amount 0) — not a top-up.
        sqlx::query("INSERT INTO transactions (user_id, amount, action) VALUES (?, 0.0, 'visit')")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        // A monthly-pass sale (action='charge', negative amount, valid_until
        // set) — the gate must not confuse a pass purchase with a top-up.
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, valid_until)
             VALUES (?, ?, -35.0, 'charge', date('now', '+30 days'))",
        )
        .bind(uid)
        .bind(pass_svc)
        .execute(&pool)
        .await
        .unwrap();
        // A voided top-up — must not count as a real top-up.
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, deleted_at)
             VALUES (?, 50.0, 'topup', datetime('now'))",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(
            last_topup_amount(&pool, uid).await.unwrap(),
            None,
            "none of charge/visit/pass-sale/voided rows count as a top-up"
        );

        // Now a genuine, non-voided top-up.
        sqlx::query("INSERT INTO transactions (user_id, amount, action) VALUES (?, 20.0, 'topup')")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(last_topup_amount(&pool, uid).await.unwrap(), Some(20.0));
    }
}
