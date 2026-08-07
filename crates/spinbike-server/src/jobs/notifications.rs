//! Daily job (#264): PWA push reminders for low credit / an expiring
//! monthly pass. Runs once a day (unlike `charger`'s 60s / `materialiser`'s
//! 60min) — these are reminders, not alerts.
//!
//! Trigger data comes from `users.credit`, `transactions` (last top-up —
//! see `db::push::last_topup_amount`), and the canonical `user_active_pass`
//! view (migration V18) — NEVER raw `transactions` for pass status, the
//! same convention `charger`/`my_balance` already use.
//!
//! **Low-credit gate (owner decision, 2026-08-07 — see #264's own
//! comments): the low-credit reminder is customer-tier-scoped.** A
//! one-off single-entry customer must NOT get it. It fires ONLY when BOTH
//! hold: `credit <= LOW_CREDIT_THRESHOLD_EUR` AND the user's most recent
//! top-up was `>= MIN_LAST_TOPUP_EUR`. A user with no top-up history at
//! all never gets this reminder. The expiring-pass reminder is UNAFFECTED
//! by this gate — it fires for any user with a pass ending within the
//! window, regardless of top-up size.
//!
//! **Anti-spam (owner decision, 2026-08-08 — mandatory per the issue,
//! refined with an episode cap):** per (user, reason), tracked in
//! `db::push`'s `push_notify_log` ledger (`last_notified_at` +
//! `sent_count`). Each tick, per (user, reason):
//!
//! - condition FALSE now: DELETE the ledger row (re-arm immediately — both
//!   the cooldown clock AND the episode counter restart from zero, so a
//!   customer who tops up and later drops low again is notified right
//!   away, not stuck waiting out a stale clock or a used-up episode).
//! - condition TRUE now: skip if `sent_count >= MAX_NOTIFICATIONS_PER_EPISODE`
//!   (silence for the rest of THIS episode — never nag a customer who
//!   simply never comes back). Otherwise send only if no ledger row exists
//!   yet, or the existing row's `last_notified_at` is
//!   `>= NOTIFY_COOLDOWN_DAYS` old.
//!
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
pub const LOW_CREDIT_THRESHOLD_EUR: f64 = 3.30;

/// The low-credit reminder is scoped to REGULAR customers, not one-off
/// single-entry visitors (owner decision, 2026-08-07): it only fires when
/// the user's most recent top-up was AT LEAST this much. `>=`, not `>` — a
/// round 20 EUR payment is a normal customer top-up; a one-off single
/// entry is roughly the price of one class, an order of magnitude smaller.
pub const MIN_LAST_TOPUP_EUR: f64 = 20.0;

/// A monthly pass counts as "expiring soon" starting this many days before
/// its `valid_until` (inclusive of both ends: today, and today+N).
pub const PASS_EXPIRING_DAYS: i64 = 3;

/// Minimum days between two notifications for the SAME (user, reason) —
/// the issue's own suggested floor, confirmed by the owner 2026-08-08.
pub const NOTIFY_COOLDOWN_DAYS: i64 = 7;

/// Cap on notifications per (user, reason) EPISODE (owner decision,
/// 2026-08-08) — after this many sends since the last re-arm, stay silent
/// until the condition clears, rather than nagging weekly forever a
/// customer who never comes back.
pub const MAX_NOTIFICATIONS_PER_EPISODE: i64 = 2;

/// Bundles the three things every per-user evaluation needs regardless of
/// reason — purely to keep `evaluate_reason`'s arg count under clippy's
/// `too_many_arguments` limit (8 positional args tripped it); no behavior
/// implication, just fewer parameters to pass around.
struct Ctx<'a> {
    pool: &'a SqlitePool,
    push: &'a PushHandle,
    today: NaiveDate,
}

/// Run one evaluation pass. Returns the number of notifications actually
/// sent (summed across both reasons).
pub async fn tick(pool: &SqlitePool, push: &PushHandle) -> Result<usize> {
    let today = crate::util::today_bratislava();
    tick_as_of(pool, push, today).await
}

/// `today` is injected so tests are deterministic — mirrors
/// `charger::tick`/`tick_as_of`.
pub async fn tick_as_of(pool: &SqlitePool, push: &PushHandle, today: NaiveDate) -> Result<usize> {
    let ctx = Ctx { pool, push, today };
    let mut sent = 0usize;

    // ---- low_credit ----
    // The last-top-up gate (#264, owner decision 2026-08-07) needs a
    // per-user lookup against `transactions` — done here in the LOOP via
    // `db::push::last_topup_amount` rather than a single joined query, to
    // keep the SQL simple and reuse the exact same helper the unit tests
    // exercise directly. Fitness-center scale (tens to low hundreds of
    // customers), so an N+1 here is negligible.
    type CreditRow = (i64, f64);
    let credit_rows: Vec<CreditRow> = sqlx::query_as(
        "SELECT id, credit FROM users
         WHERE role = 'customer' AND blocked = 0 AND deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    for (user_id, credit) in credit_rows {
        let last_topup = db::push::last_topup_amount(pool, user_id).await?;
        let condition = credit <= LOW_CREDIT_THRESHOLD_EUR
            && last_topup.is_some_and(|t| t >= MIN_LAST_TOPUP_EUR);
        let title = "Dochadza ti kredit";
        let body = format!("Tvoj zostatok je {credit:.2} EUR. Doplat si kredit na recepcii.");
        if evaluate_reason(
            &ctx,
            user_id,
            db::push::REASON_LOW_CREDIT,
            condition,
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
    // UNAFFECTED by the low-credit top-up gate above (owner decision).
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
            &ctx,
            user_id,
            db::push::REASON_PASS_EXPIRING,
            condition,
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

/// One reason's per-user evaluation: re-arm on clear, episode cap, cooldown
/// check, send to every stored subscription, prune gone endpoints, and
/// stamp the ledger only after an ACTUAL successful send. Returns `true`
/// iff at least one subscription was successfully notified.
async fn evaluate_reason(
    ctx: &Ctx<'_>,
    user_id: i64,
    reason: &str,
    condition: bool,
    title: &str,
    body: &str,
) -> Result<bool> {
    let pool = ctx.pool;
    let push = ctx.push;
    let today = ctx.today;

    if !condition {
        db::push::clear_notified(pool, user_id, reason).await?;
        return Ok(false);
    }

    if let Some(log) = db::push::notify_log(pool, user_id, reason).await? {
        if log.sent_count >= MAX_NOTIFICATIONS_PER_EPISODE {
            // Episode cap reached — stay silent until the condition clears
            // and re-arms (owner decision, 2026-08-08).
            return Ok(false);
        }
        let last_date = log
            .last_notified_at
            .split(' ')
            .next()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
        if let Some(last_date) = last_date
            && (today - last_date).num_days() < NOTIFY_COOLDOWN_DAYS
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

    /// A qualifying top-up (>= MIN_LAST_TOPUP_EUR) — most low-credit tests
    /// need this to even reach the condition (#264 owner decision
    /// 2026-08-07: no top-up history = no low-credit reminder, regardless
    /// of how low credit is).
    async fn seed_topup(pool: &SqlitePool, user_id: i64, amount: f64) {
        sqlx::query("INSERT INTO transactions (user_id, amount, action) VALUES (?, ?, 'topup')")
            .bind(user_id)
            .bind(amount)
            .execute(pool)
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

    /// The REAL current UTC date, not a hardcoded constant. `evaluate_reason`
    /// compares the injected `today` against `last_notified_at`/`sent_count`
    /// rows stamped by `record_notified` with REAL `datetime('now')` — a
    /// fixed historical `today` would silently drift out of sync with real
    /// wall-clock time as the calendar moves on (the exact bug this
    /// replaced: a hardcoded 2026-08-07 broke the very next day). Tests that
    /// need date ARITHMETIC (day+2, day+8, ...) work correctly regardless of
    /// which real date this resolves to, since they only compare offsets
    /// from it — never a specific historical value.
    fn test_today() -> NaiveDate {
        chrono::Utc::now().date_naive()
    }

    // ── low-credit top-up gate (#264 owner decision, 2026-08-07) ───────────

    #[tokio::test]
    async fn low_credit_with_no_topup_history_never_notifies() {
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

        let uid = seed_customer(&pool, "notopup@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 0, "no top-up history -> never a low-credit push");
    }

    #[tokio::test]
    async fn low_credit_with_last_topup_below_the_gate_does_not_notify() {
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

        // Last top-up 10 EUR (below the 20 EUR gate) + credit 1 EUR.
        let uid = seed_customer(&pool, "single-entry@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 10.0).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 0, "last top-up below MIN_LAST_TOPUP_EUR -> no push");
    }

    #[tokio::test]
    async fn low_credit_with_last_topup_at_the_gate_notifies() {
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

        // Last top-up EXACTLY 20 EUR (the boundary is >=, not >) + credit 1 EUR.
        let uid = seed_customer(&pool, "regular@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 1, "last top-up == MIN_LAST_TOPUP_EUR (>=) -> pushes");
    }

    #[tokio::test]
    async fn low_credit_uses_the_most_recent_topup_not_an_older_larger_one() {
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

        // An OLD large top-up, then a RECENT small one — must gate on the
        // most recent, not the largest or a sum/average (owner decision:
        // "nie sucet, nie priemer").
        let uid = seed_customer(&pool, "recency@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, created_at)
             VALUES (?, 100.0, 'topup', '2026-01-01 10:00:00')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (user_id, amount, action, created_at)
             VALUES (?, 5.0, 'topup', '2026-08-01 10:00:00')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(
            sent, 0,
            "most recent top-up (5 EUR) is below the gate, even though an older one was 100 EUR"
        );
    }

    // ── anti-spam: cooldown, episode cap, re-arm ────────────────────────────

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
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(
            sent, 1,
            "condition true, no prior notification -> must send"
        );
        let log = db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
            .await
            .unwrap()
            .expect("a successful send must stamp the ledger");
        assert_eq!(log.sent_count, 1);

        // Same day, condition still true, but inside the 7-day cooldown ->
        // no second send.
        let sent_again = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent_again, 0, "must respect the cooldown");
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn low_credit_full_anti_spam_sequence_cooldown_episode_cap_then_rearm() {
        // The exact four-part sequence the owner's cadence decision
        // (2026-08-08) requires:
        //   1. second tick same day -> nothing (cooldown)
        //   2. tick 8 days later, still true -> second notification
        //   3. tick 8 days after THAT, still true -> nothing (episode cap 2)
        //   4. condition clears then re-triggers -> sent immediately,
        //      counter restarted from zero
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

        let uid = seed_customer(&pool, "sequence@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let day0 = test_today();

        // Tick 1 (day 0): first notification, sent_count -> 1.
        let sent = tick_as_of(&pool, &push, day0).await.unwrap();
        assert_eq!(sent, 1, "first tick must notify");

        // Tick 2 (day 0, same day): cooldown -> nothing.
        let sent = tick_as_of(&pool, &push, day0).await.unwrap();
        assert_eq!(sent, 0, "1. second tick same day -> nothing (cooldown)");
        assert_eq!(mock.calls_async().await, 1);

        // Tick 3 (day 8): cooldown expired -> second notification, count -> 2.
        let day8 = day0 + Days::new(8);
        let sent = tick_as_of(&pool, &push, day8).await.unwrap();
        assert_eq!(sent, 1, "2. tick 8 days later -> second notification");
        assert_eq!(mock.calls_async().await, 2);
        let log = db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(log.sent_count, 2);

        // Tick 4 (day 16): cooldown long expired, but episode cap (2)
        // already reached -> nothing, ever, until re-arm.
        let day16 = day8 + Days::new(8);
        let sent = tick_as_of(&pool, &push, day16).await.unwrap();
        assert_eq!(
            sent, 0,
            "3. tick 8 days after that -> nothing (episode cap 2)"
        );
        assert_eq!(mock.calls_async().await, 2, "no third send at the cap");

        // Condition clears (customer tops up above the low-credit
        // threshold) then re-triggers -> must notify immediately, counter
        // restarted from zero.
        sqlx::query("UPDATE users SET credit = 100.0 WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        let sent = tick_as_of(&pool, &push, day16).await.unwrap();
        assert_eq!(sent, 0, "condition cleared -> re-arm, no send");
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none(),
            "re-arm must drop the ledger row entirely"
        );

        sqlx::query("UPDATE users SET credit = 1.0 WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        let sent = tick_as_of(&pool, &push, day16).await.unwrap();
        assert_eq!(
            sent, 1,
            "4. condition re-triggers -> sent immediately, counter from zero"
        );
        let log = db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(log.sent_count, 1, "episode counter restarted from zero");
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
        seed_topup(&pool, uid, 20.0).await;

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
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
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
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 0, "a Gone endpoint is never counted as a real send");
        assert!(
            !db::push::has_subscription(&pool, uid).await.unwrap(),
            "410 must prune the subscription"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
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
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, test_today()).await.unwrap();
        assert_eq!(sent, 0);
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
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
            db::push::notify_log(&pool, uid_soon, db::push::REASON_PASS_EXPIRING)
                .await
                .unwrap()
                .is_some()
        );
        for uid in [uid_far, uid_expired, uid_none] {
            assert!(
                db::push::notify_log(&pool, uid, db::push::REASON_PASS_EXPIRING)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn pass_expiring_is_not_gated_by_the_low_credit_topup_rule() {
        // Owner decision: the top-up gate applies ONLY to low_credit. A
        // pass-expiring user with no top-up history at all must STILL be
        // notified.
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

        // Plenty of credit (never triggers low_credit) and NO top-up
        // history at all — only the pass matters here.
        let uid = seed_customer(&pool, "passonly@x", 100.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/passonly")).await;
        seed_pass(
            &pool,
            uid,
            &(today + Days::new(1)).format("%Y-%m-%d").to_string(),
        )
        .await;

        let sent = tick_as_of(&pool, &push, today).await.unwrap();
        assert_eq!(
            sent, 1,
            "pass-expiring must fire regardless of top-up history"
        );
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
        seed_topup(&pool, uid, 20.0).await;
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
