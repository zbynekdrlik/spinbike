//! Daily job (#264): PWA push reminders for low credit / an expiring
//! monthly pass. Runs once a day (unlike `charger`'s 60s / `materialiser`'s
//! 60min) — these are reminders, not alerts.
//!
//! Trigger data comes from `users.credit` and the canonical
//! `user_active_pass` view (migration V18) — NEVER `transactions` directly,
//! the same convention `charger`/`my_balance` already use.
//!
//! **Anti-spam (mandatory, per the issue):** a per-(user, reason) cooldown
//! via `db::push`'s `push_notify_log` ledger, combined with re-arm-on-clear.
//! Each tick, per (user, reason):
//! - condition FALSE now -> DELETE the ledger row (re-arm immediately, so a
//!   customer who tops up and later drops low again is notified right
//!   away, not stuck waiting out a stale clock).
//! - condition TRUE now -> send only if no ledger row exists yet, or the
//!   existing row's `last_notified_at` is >= `COOLDOWN_DAYS` old; otherwise
//!   skip silently (still condition-true, just inside the cooldown).
//! The ledger is stamped ONLY after an actual successful send — a customer
//! with the condition true but no stored subscription is re-evaluated
//! (cheaply) every day until they subscribe, never falsely marked notified.

use anyhow::Result;
use chrono::{Days, NaiveDate};
use sqlx::SqlitePool;

use crate::db;
use crate::push::{PushHandle, SendOutcome};

/// Below this many EUR of credit, a customer can no longer afford their
/// next Spinning class (= the live prod `services.default_price` for
/// Spinning at the time this was written — #264's own issue comment). A
/// named constant, easy for the CEO to have changed later.
pub const LOW_CREDIT_THRESHOLD_EUR: f64 = 3.3;

/// A monthly pass counts as "expiring soon" starting this many days before
/// its `valid_until` (inclusive of both ends: today, and today+N).
pub const PASS_EXPIRING_DAYS: i64 = 3;

/// Minimum days between two notifications for the SAME (user, reason) —
/// the issue's own suggested floor.
pub const COOLDOWN_DAYS: i64 = 7;

/// Run one evaluation pass. Returns the number of notifications actually
/// sent (summed across both reasons).
pub async fn tick(pool: &SqlitePool, push: &PushHandle) -> Result<usize> {
    let today = crate::util::today_bratislava();
    tick_as_of(pool, push, today).await
}

/// `today` is injected so tests are deterministic — mirrors
/// `charger::tick`/`tick_as_of`.
pub async fn tick_as_of(pool: &SqlitePool, push: &PushHandle, today: NaiveDate) -> Result<usize> {
    let mut sent = 0usize;

    // ---- low_credit ----
    type CreditRow = (i64, f64);
    let credit_rows: Vec<CreditRow> = sqlx::query_as(
        "SELECT id, credit FROM users
         WHERE role = 'customer' AND blocked = 0 AND deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    for (user_id, credit) in credit_rows {
        let condition = credit < LOW_CREDIT_THRESHOLD_EUR;
        let title = "Dochadza ti kredit";
        let body = format!("Tvoj zostatok je {credit:.2} EUR. Doplat si kredit na recepcii.");
        if evaluate_reason(
            pool,
            push,
            user_id,
            db::push::REASON_LOW_CREDIT,
            condition,
            today,
            title,
            &body,
        )
        .await?
        {
            sent += 1;
        }
    }

    // ---- pass_expiring ----
    // `user_active_pass` (V18) returns the LATEST non-voided monthly-pass
    // charge per user regardless of whether it's already in the past — same
    // gotcha `my_balance.rs` handles by comparing `date(valid_until)`
    // against "today" explicitly. Here the window is `[today, today+N]`
    // inclusive, which naturally excludes an already-expired pass (its
    // valid_until is < today) without a separate "still active" check.
    let cutoff_s = (today + Days::new(PASS_EXPIRING_DAYS as u64))
        .format("%Y-%m-%d")
        .to_string();
    let today_s = today.format("%Y-%m-%d").to_string();

    type PassRow = (i64, Option<String>);
    let pass_rows: Vec<PassRow> = sqlx::query_as(
        "SELECT u.id, date(ap.valid_until)
         FROM users u
         LEFT JOIN user_active_pass ap ON ap.user_id = u.id
         WHERE u.role = 'customer' AND u.blocked = 0 AND u.deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    for (user_id, valid_until) in pass_rows {
        let condition = matches!(
            &valid_until,
            Some(vu) if vu.as_str() >= today_s.as_str() && vu.as_str() <= cutoff_s.as_str()
        );

        // Only needed when condition is true, but cheap to compute either
        // way and keeps the call to evaluate_reason uniform with low_credit.
        let (title, body) = match &valid_until {
            Some(vu) => {
                let days_left = NaiveDate::parse_from_str(vu, "%Y-%m-%d")
                    .map(|d| (d - today).num_days())
                    .unwrap_or(0);
                (
                    format!("Permanentka ti konci o {days_left} dni"),
                    format!("Tvoja permanentka plati do {vu}. Predlz si ju na recepcii."),
                )
            }
            None => (String::new(), String::new()),
        };

        if evaluate_reason(
            pool,
            push,
            user_id,
            db::push::REASON_PASS_EXPIRING,
            condition,
            today,
            &title,
            &body,
        )
        .await?
        {
            sent += 1;
        }
    }

    Ok(sent)
}

/// One reason's per-user evaluation: re-arm on clear, cooldown check, send
/// to every stored subscription, prune gone endpoints, and stamp the
/// ledger only after an ACTUAL successful send. Returns `true` iff at
/// least one subscription was successfully notified.
async fn evaluate_reason(
    pool: &SqlitePool,
    push: &PushHandle,
    user_id: i64,
    reason: &str,
    condition: bool,
    today: NaiveDate,
    title: &str,
    body: &str,
) -> Result<bool> {
    if !condition {
        db::push::clear_notified(pool, user_id, reason).await?;
        return Ok(false);
    }

    if let Some(last) = db::push::last_notified_at(pool, user_id, reason).await? {
        let last_date = last
            .split(' ')
            .next()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
        if let Some(last_date) = last_date
            && (today - last_date).num_days() < COOLDOWN_DAYS
        {
            return Ok(false);
        }
    }

    let subs = db::push::list_subscriptions_for_user(pool, user_id).await?;
    if subs.is_empty() {
        // Nothing to notify — leave the ledger untouched so this is
        // re-evaluated (cheaply) every day until the customer subscribes,
        // rather than being falsely marked as already notified.
        return Ok(false);
    }

    let mut any_sent = false;
    for sub in subs {
        match push
            .send(&sub.endpoint, &sub.p256dh, &sub.auth, title, body)
            .await
        {
            SendOutcome::Sent => {
                db::push::record_send_success(pool, sub.id).await?;
                any_sent = true;
            }
            SendOutcome::Gone => {
                db::push::prune_subscription(pool, sub.id).await?;
            }
            SendOutcome::RateLimited | SendOutcome::Retryable | SendOutcome::Failed => {
                db::push::record_send_failure(pool, sub.id).await?;
            }
        }
    }

    if any_sent {
        db::push::record_notified(pool, user_id, reason).await?;
    }

    Ok(any_sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations, users};
    use crate::push::{PushHandle, TEST_AUTH_B64, TEST_P256DH_B64, TEST_VAPID_PRIVATE_KEY_B64};
    use httpmock::MockServer;

    async fn seed_customer(pool: &SqlitePool, email: &str, credit: f64) -> i64 {
        users::create_user(
            pool,
            Some(email),
            None,
            "Test",
            None,
            None,
            None,
            "customer",
            Some(credit),
            None,
            None,
        )
        .await
        .unwrap()
    }

    async fn seed_subscription(pool: &SqlitePool, user_id: i64, endpoint: &str) {
        db::push::upsert_subscription(pool, user_id, endpoint, TEST_P256DH_B64, TEST_AUTH_B64)
            .await
            .unwrap();
    }

    /// Insert a monthly-pass charge (the invariant trigger requires
    /// action='charge' + the kind='monthly_pass' service whenever
    /// valid_until is set — same pattern as `jobs::charger`'s tests).
    async fn seed_pass(pool: &SqlitePool, user_id: i64, valid_until: &str) {
        let svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
            .fetch_one(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, valid_until)
             VALUES (?, ?, -35.0, 'charge', ?)",
        )
        .bind(user_id)
        .bind(svc)
        .bind(valid_until)
        .execute(pool)
        .await
        .unwrap();
    }

    fn test_today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 7).unwrap()
    }

    #[tokio::test]
    async fn low_credit_fires_once_then_respects_cooldown() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(201);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);

        let uid = seed_customer(&pool, "low@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(
            sent, 1,
            "condition true, no prior notification -> must send"
        );
        assert!(
            db::push::last_notified_at(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_some(),
            "a successful send must stamp the ledger"
        );

        // Same day, condition still true, but inside the 7-day cooldown ->
        // no second send.
        let sent_again = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent_again, 0, "must respect the cooldown");
        assert_eq!(mock.hits_async().await, 1);
    }

    #[tokio::test]
    async fn low_credit_resends_after_cooldown_expires() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(201);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);

        let uid = seed_customer(&pool, "stale@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;

        // Simulate an old notification, 8 days ago (past the 7-day floor).
        sqlx::query(
            "INSERT INTO push_notify_log (user_id, reason, last_notified_at)
             VALUES (?, 'low_credit', datetime('now', '-8 days'))",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 1, "cooldown expired -> must resend");
        assert_eq!(mock.hits_async().await, 1);
    }

    #[tokio::test]
    async fn low_credit_re_arms_when_condition_clears() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(201);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);

        let uid = seed_customer(&pool, "reup@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 1);

        // Customer tops up — condition clears.
        sqlx::query("UPDATE users SET credit = 100.0 WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();

        let sent_after_topup = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent_after_topup, 0);
        assert!(
            db::push::last_notified_at(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none(),
            "the ledger row must be cleared the moment the condition clears"
        );

        // Drops low again the SAME day — must fire immediately (re-armed,
        // not stuck waiting out the old cooldown clock).
        sqlx::query("UPDATE users SET credit = 1.0 WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        let sent_again = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent_again, 1, "must re-notify immediately after re-arm");
    }

    #[tokio::test]
    async fn gone_404_or_410_prunes_the_subscription_and_never_stamps_the_ledger() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(410);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);

        let uid = seed_customer(&pool, "gone@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 0, "a Gone endpoint is never counted as a real send");
        assert!(
            !db::push::has_subscription(&pool, uid).await.unwrap(),
            "410 must prune the subscription"
        );
        assert!(
            db::push::last_notified_at(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none(),
            "a pruned/failed send must NOT stamp the ledger — retry tomorrow once resubscribed"
        );
    }

    #[tokio::test]
    async fn no_subscription_no_send_no_ledger_write() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);

        let uid = seed_customer(&pool, "nosub@x", 1.0).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 0);
        assert!(
            db::push::last_notified_at(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn pass_expiring_within_window_fires_outside_window_does_not() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(201);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let today = test_today();

        // Within the 3-day window (today + 2).
        let uid_soon = seed_customer(&pool, "soon@x", 100.0).await;
        seed_subscription(&pool, uid_soon, &server.url("/wpush/soon")).await;
        seed_pass(
            &pool,
            uid_soon,
            &(today + Days::new(2)).format("%Y-%m-%d").to_string(),
        )
        .await;

        // Outside the window (today + 10) — far from expiring.
        let uid_far = seed_customer(&pool, "far@x", 100.0).await;
        seed_subscription(&pool, uid_far, &server.url("/wpush/far")).await;
        seed_pass(
            &pool,
            uid_far,
            &(today + Days::new(10)).format("%Y-%m-%d").to_string(),
        )
        .await;

        // Already expired (yesterday) — no active pass at all anymore.
        let uid_expired = seed_customer(&pool, "expired@x", 100.0).await;
        seed_subscription(&pool, uid_expired, &server.url("/wpush/expired")).await;
        seed_pass(
            &pool,
            uid_expired,
            &(today - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string(),
        )
        .await;

        // No pass at all.
        let uid_none = seed_customer(&pool, "none@x", 100.0).await;
        seed_subscription(&pool, uid_none, &server.url("/wpush/none")).await;

        let sent = tick_as_of(&pool, &push, today).await.unwrap();
        assert_eq!(sent, 1, "only the within-window customer must be notified");
        assert!(
            db::push::last_notified_at(&pool, uid_soon, db::push::REASON_PASS_EXPIRING)
                .await
                .unwrap()
                .is_some()
        );
        for uid in [uid_far, uid_expired, uid_none] {
            assert!(
                db::push::last_notified_at(&pool, uid, db::push::REASON_PASS_EXPIRING)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn low_credit_and_pass_expiring_are_independent_for_the_same_user() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(201);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let today = test_today();

        // Low credit AND a pass expiring soon at once — must send BOTH,
        // and count both toward `sent`.
        let uid = seed_customer(&pool, "both@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/both")).await;
        seed_pass(
            &pool,
            uid,
            &(today + Days::new(1)).format("%Y-%m-%d").to_string(),
        )
        .await;

        let sent = tick_as_of(&pool, &push, today).await.unwrap();
        assert_eq!(sent, 2, "both reasons must fire independently");
    }
}
