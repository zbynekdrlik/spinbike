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
//! one-off single-entry customer must NOT get it. It fires ONLY when ALL
//! THREE hold: `credit <= LOW_CREDIT_THRESHOLD_EUR` AND the user's most
//! recent top-up was `>= MIN_LAST_TOPUP_EUR` AND the user does NOT hold an
//! active monthly pass as of `today` (see #306 below). A user with no
//! top-up history at all never gets this reminder. The expiring-pass
//! reminder is UNAFFECTED by this gate — it fires for any user with a pass
//! ending within the window, regardless of top-up size.
//!
//! **Active-pass suppression (#306, owner-confirmed root cause, 2026-08-08):
//! while a customer holds an active monthly pass, their `credit` is 0 BY
//! DESIGN** (every visit during the pass period is booked as
//! `action='visit'`, amount 0 — nothing is ever deducted from credit while
//! the pass covers it). `low_credit`'s original condition looked only at
//! `credit`/`last_topup` and never checked pass status, so a pass holder
//! whose most recent top-up happened to clear the 20 EUR gate got a
//! nonsensical "Dochadza ti kredit" push while their pass was still fully
//! valid. The fix reads the SAME canonical `user_active_pass` view (V18)
//! the `pass_expiring` loop below already reads, via
//! `db::users::get_user_pass_valid_until`, and suppresses `low_credit`
//! whenever `pass_valid_until >= today` (inclusive — a pass expiring
//! TODAY still covers the whole of today, so the customer must not be
//! nagged about credit until tomorrow). This makes the sequence coherent:
//! `pass_expiring` fires 3 days before the pass ends; the moment it
//! actually expires, `low_credit` naturally unlocks (its own condition
//! was true all along, just gated) and can remind the customer to top up.
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
use crate::mail::MailHandle;
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

/// After this many CONSECUTIVE send failures (429/5xx/malformed-payload —
/// any non-Sent, non-Gone outcome), prune the subscription outright rather
/// than retrying it forever (#264 review finding — only 404/410 pruned
/// before this). The job runs daily, so 10 is roughly 10 days of grace for
/// a genuinely transient issue to self-resolve before giving up.
pub const MAX_CONSECUTIVE_FAILURES: i64 = 10;

/// Wall-clock hour (Europe/Bratislava, 0..=23) the daily job is aligned to
/// (#264 review finding — see `util::duration_until_next_bratislava_hour`,
/// which `bin/server.rs` uses to schedule this). Mid-morning: late enough
/// that a customer is normally awake, early enough to act on it before the
/// gym's own opening hours.
pub const DAILY_RUN_HOUR: u32 = 9;

/// Bundles the three things every per-user evaluation needs regardless of
/// reason — purely to keep `evaluate_reason`'s arg count under clippy's
/// `too_many_arguments` limit (8 positional args tripped it); no behavior
/// implication, just fewer parameters to pass around.
struct Ctx<'a> {
    pool: &'a SqlitePool,
    push: &'a PushHandle,
    /// #311: e-mail FALLBACK channel — used only when a customer has no
    /// stored push subscription at all (see `evaluate_reason`).
    mail: &'a MailHandle,
    today: NaiveDate,
}

/// Run one evaluation pass. Returns the number of notifications actually
/// sent (summed across both reasons, across both channels).
pub async fn tick(pool: &SqlitePool, push: &PushHandle, mail: &MailHandle) -> Result<usize> {
    let today = crate::util::today_bratislava();
    tick_as_of(pool, push, mail, today).await
}

/// `today` is injected so tests are deterministic — mirrors
/// `charger::tick`/`tick_as_of`.
pub async fn tick_as_of(
    pool: &SqlitePool,
    push: &PushHandle,
    mail: &MailHandle,
    today: NaiveDate,
) -> Result<usize> {
    let ctx = Ctx {
        pool,
        push,
        mail,
        today,
    };
    let mut sent = 0usize;

    // ---- low_credit ----
    // The last-top-up gate (#264, owner decision 2026-08-07) needs a
    // per-user lookup against `transactions` — done here in the LOOP via
    // `db::push::last_topup_amount` rather than a single joined query, to
    // keep the SQL simple and reuse the exact same helper the unit tests
    // exercise directly. Fitness-center scale (tens to low hundreds of
    // customers), so an N+1 here is negligible.
    // #311: `email` is fetched here too (not a separate per-user query) —
    // reuses the ROW this loop already fetches instead of adding an N+1
    // lookup, and is threaded into `evaluate_reason` as the e-mail
    // FALLBACK address (used only when the customer has no push
    // subscription at all).
    type CreditRow = (i64, f64, Option<String>);
    let credit_rows: Vec<CreditRow> = sqlx::query_as(
        "SELECT id, credit, email FROM users
         WHERE role = 'customer' AND blocked = 0 AND deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    for (user_id, credit, email) in credit_rows {
        // A single row's DB error must NOT abort the whole batch (#264
        // review finding) — log and move on to the next customer instead
        // of propagating with `?`, which would silently skip every
        // REMAINING user for the rest of this tick.
        let last_topup = match db::push::last_topup_amount(pool, user_id).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    user_id,
                    error = %e,
                    "push: low_credit — failed to read last top-up, skipping this user"
                );
                continue;
            }
        };
        // #306: an active monthly pass suppresses low_credit — while it
        // covers the customer, credit is 0 by design (see module doc) and
        // "Dochadza ti kredit" is nonsensical. Same canonical
        // `user_active_pass` view (V18) `pass_expiring` reads below.
        // `>=` today (inclusive) — a pass expiring TODAY still covers the
        // whole of today.
        let pass_valid_until = match db::users::get_user_pass_valid_until(pool, user_id).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    user_id,
                    error = %e,
                    "push: low_credit — failed to read pass status, skipping this user"
                );
                continue;
            }
        };
        let has_active_pass = pass_valid_until.is_some_and(|vu| vu >= today);
        let condition = credit <= LOW_CREDIT_THRESHOLD_EUR
            && last_topup.is_some_and(|t| t >= MIN_LAST_TOPUP_EUR)
            && !has_active_pass;
        tracing::debug!(
            user_id,
            credit = %format!("{credit:.2}"),
            last_topup = %last_topup.map(|t| format!("{t:.2}")).unwrap_or_else(|| "none".to_string()),
            has_active_pass,
            condition,
            "push: low_credit evaluation"
        );
        let title = "Dochadza ti kredit";
        let body = format!("Tvoj zostatok je {credit:.2} EUR. Doplat si kredit na recepcii.");
        match evaluate_reason(
            &ctx,
            user_id,
            db::push::REASON_LOW_CREDIT,
            condition,
            title,
            &body,
            email.as_deref(),
        )
        .await
        {
            Ok(true) => {
                tracing::info!(user_id, reason = "low_credit", "push: notified");
                sent += 1;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!(
                    user_id,
                    reason = "low_credit",
                    error = %e,
                    "push: evaluation failed, skipping this user"
                );
            }
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

    // #311: `u.email` fetched alongside — same reasoning as `credit_rows`
    // above (reuse the existing row, no extra N+1 query).
    type PassRow = (i64, Option<String>, Option<String>);
    let pass_rows: Vec<PassRow> = sqlx::query_as(
        "SELECT u.id, date(ap.valid_until), u.email
         FROM users u
         LEFT JOIN user_active_pass ap ON ap.user_id = u.id
         WHERE u.role = 'customer' AND u.blocked = 0 AND u.deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    for (user_id, valid_until, email) in pass_rows {
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

        tracing::debug!(
            user_id,
            valid_until = %valid_until.as_deref().unwrap_or("none"),
            condition,
            "push: pass_expiring evaluation"
        );

        // Same "log and continue, never abort the batch" discipline as the
        // low_credit loop above (#264 review finding).
        match evaluate_reason(
            &ctx,
            user_id,
            db::push::REASON_PASS_EXPIRING,
            condition,
            &title,
            &body,
            email.as_deref(),
        )
        .await
        {
            Ok(true) => {
                tracing::info!(user_id, reason = "pass_expiring", "push: notified");
                sent += 1;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!(
                    user_id,
                    reason = "pass_expiring",
                    error = %e,
                    "push: evaluation failed, skipping this user"
                );
            }
        }
    }

    tracing::info!(sent, "push: daily tick complete");
    Ok(sent)
}

/// One reason's per-user evaluation: re-arm on clear, episode cap, cooldown
/// check, send to every stored subscription, prune gone endpoints, and
/// stamp the ledger only after an ACTUAL successful send. Returns `true`
/// iff the customer was actually notified — by push OR, as a #311
/// FALLBACK, by e-mail.
///
/// #311 (owner decision, variant (a) — 2026-08-10): e-mail is a FALLBACK
/// ONLY, never a duplicate channel. The choice is made on whether the
/// customer has ANY stored push subscription at all (`subs.is_empty()`),
/// not on whether THIS TICK's push delivery actually succeeded — a
/// customer with a subscription that is merely failing transiently still
/// gets push-only treatment; the existing `MAX_CONSECUTIVE_FAILURES`
/// pruning already handles the case where it's genuinely dead, at which
/// point the customer naturally falls onto the e-mail path on a later
/// tick. `email` is `None` for a legacy card-migrated account
/// (`users.email IS NULL`) — that combination (no subscription, no email)
/// skips silently: no error, no ledger write, same as "no subscription"
/// always behaved before this ticket.
async fn evaluate_reason(
    ctx: &Ctx<'_>,
    user_id: i64,
    reason: &str,
    condition: bool,
    title: &str,
    body: &str,
    email: Option<&str>,
) -> Result<bool> {
    let pool = ctx.pool;
    let push = ctx.push;
    let mail = ctx.mail;
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
        // #311: no push subscription at all -> fall back to e-mail, IF the
        // customer has one on file. `filter` + `.trim()` also excludes an
        // empty/whitespace-only address (defensive — every write path that
        // sets `users.email` already trims (`routes/auth.rs`,
        // `routes/users.rs`), but `evaluate_reason` has no visibility into
        // that upstream guarantee, and `Mailbox::parse()` may reject a
        // padded address outright). `addr` itself is the TRIMMED value, so
        // a stray future untrimmed write can't turn into a
        // permanently-failing, never-retried-successfully address.
        let Some(addr) = email.map(str::trim).filter(|e| !e.is_empty()) else {
            // Neither push nor e-mail — leave the ledger untouched so this
            // is re-evaluated (cheaply) every day until the customer
            // subscribes or an e-mail is added, rather than being falsely
            // marked as already notified.
            return Ok(false);
        };
        let html = format!("<p>{body}</p>");
        return match mail.send(addr, title, body, &html).await {
            Ok(()) => {
                db::push::record_notified(pool, user_id, reason).await?;
                Ok(true)
            }
            Err(e) => {
                // Same discipline as a failed push send: NEVER stamp the
                // ledger on failure, so the next tick retries instead of
                // silently eating the customer's notification for
                // NOTIFY_COOLDOWN_DAYS.
                tracing::warn!(
                    user_id,
                    reason = %reason,
                    to = %addr,
                    error = %e,
                    "push: email fallback send failed, ledger not stamped (retried next tick)"
                );
                Ok(false)
            }
        };
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
                let failure_count = db::push::record_send_failure(pool, sub.id).await?;
                if failure_count >= MAX_CONSECUTIVE_FAILURES {
                    // A malformed p256dh/auth yields SendOutcome::Failed
                    // forever, but pruning was previously wired only to
                    // Gone (404/410) — a permanently-broken subscription
                    // would be retried indefinitely (#264 review finding).
                    tracing::warn!(
                        user_id,
                        subscription_id = sub.id,
                        failure_count,
                        "push: pruning subscription after repeated send failures"
                    );
                    db::push::prune_subscription(pool, sub.id).await?;
                }
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
    use crate::mail::MailHandle;
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

    /// #311: a legacy card-migrated account with NO email on file at all —
    /// the owner's decision says this must skip silently (neither push nor
    /// email), never error.
    async fn seed_customer_no_email(pool: &SqlitePool, credit: f64) -> i64 {
        users::create_user(
            pool,
            None,
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
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "notopup@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
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
        let mail = crate::mail::MailHandle::disabled();

        // Last top-up 10 EUR (below the 20 EUR gate) + credit 1 EUR.
        let uid = seed_customer(&pool, "single-entry@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 10.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
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
        let mail = crate::mail::MailHandle::disabled();

        // Last top-up EXACTLY 20 EUR (the boundary is >=, not >) + credit 1 EUR.
        let uid = seed_customer(&pool, "regular@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
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
        let mail = crate::mail::MailHandle::disabled();

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

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(
            sent, 0,
            "most recent top-up (5 EUR) is below the gate, even though an older one was 100 EUR"
        );
    }

    #[tokio::test]
    async fn credit_exactly_at_the_low_credit_threshold_notifies() {
        // Boundary is <=, not < — credit exactly AT the threshold must
        // still notify.
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
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "atthreshold@x", LOW_CREDIT_THRESHOLD_EUR).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, MIN_LAST_TOPUP_EUR).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(sent, 1, "credit == threshold (<=) must still notify");
    }

    #[tokio::test]
    async fn credit_just_above_the_low_credit_threshold_does_not_notify() {
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
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "abovethreshold@x", LOW_CREDIT_THRESHOLD_EUR + 0.01).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, MIN_LAST_TOPUP_EUR).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(sent, 0, "credit just above the threshold must not notify");
    }

    // ── #306: an active monthly pass suppresses low_credit ─────────────────

    #[tokio::test]
    async fn low_credit_suppressed_while_pass_is_active() {
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
        let mail = crate::mail::MailHandle::disabled();
        let today = test_today();

        // Prod shape (#306): credit is 0 BY DESIGN while a pass covers every
        // visit, and a qualifying top-up (>= MIN_LAST_TOPUP_EUR) would
        // otherwise satisfy the low_credit condition — but the pass is
        // active another 10 days, so it must NOT notify.
        let uid = seed_customer(&pool, "pass-active@x", 0.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 35.0).await;
        seed_pass(
            &pool,
            uid,
            &(today + Days::new(10)).format("%Y-%m-%d").to_string(),
        )
        .await;

        let sent = tick_as_of(&pool, &push, &mail, today).await.unwrap();
        assert_eq!(
            sent, 0,
            "an active monthly pass (valid another 10 days) must suppress low_credit"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn low_credit_re_enabled_once_the_pass_has_expired() {
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
        let mail = crate::mail::MailHandle::disabled();
        let today = test_today();

        // Same customer shape as above, but the pass expired YESTERDAY —
        // the suppression must lift and low_credit must fire normally.
        let uid = seed_customer(&pool, "pass-expired@x", 0.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 35.0).await;
        seed_pass(
            &pool,
            uid,
            &(today - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string(),
        )
        .await;

        let sent = tick_as_of(&pool, &push, &mail, today).await.unwrap();
        assert_eq!(
            sent, 1,
            "low_credit must unlock the day after the pass has expired"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn low_credit_still_suppressed_on_the_pass_expiry_day_itself() {
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
        let mail = crate::mail::MailHandle::disabled();
        let today = test_today();

        // Boundary: valid_until == today — the pass still covers the whole
        // of today, so low_credit must stay suppressed. pass_expiring is a
        // SEPARATE, independent reason and legitimately fires today too
        // (today is inside its own inclusive [today, today+3] window) —
        // total `sent` is 1, but it must be pass_expiring, never low_credit.
        let uid = seed_customer(&pool, "pass-today@x", 0.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 35.0).await;
        seed_pass(&pool, uid, &today.format("%Y-%m-%d").to_string()).await;

        let sent = tick_as_of(&pool, &push, &mail, today).await.unwrap();
        assert_eq!(
            sent, 1,
            "pass_expiring itself still legitimately fires on the expiry day"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none(),
            "low_credit must stay suppressed while the pass covers today"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_PASS_EXPIRING)
                .await
                .unwrap()
                .is_some(),
            "pass_expiring is independent of the low_credit suppression"
        );
    }

    #[tokio::test]
    async fn tick_delegates_to_tick_as_of_with_real_today() {
        // `tick` (the public, non-`_as_of` entry point `server.rs` actually
        // calls) is otherwise never exercised directly by any other test
        // here — all of them call `tick_as_of` with an injected date.
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
        let mail = crate::mail::MailHandle::disabled();

        // A user with no qualifying condition at all: tick() must return
        // 0 — proves it isn't hardcoded to always report a send.
        seed_customer(&pool, "realtick-clean@x", 100.0).await;
        let sent = tick(&pool, &push, &mail).await.unwrap();
        assert_eq!(sent, 0, "tick() must report 0 when nothing qualifies");

        // Now a genuinely qualifying user: tick() must return 1 — proves
        // it isn't hardcoded to always report nothing either.
        let uid = seed_customer(&pool, "realtick@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick(&pool, &push, &mail).await.unwrap();
        assert_eq!(sent, 1, "tick() must actually evaluate and send");
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
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "low@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
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
        let sent_again = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(sent_again, 0, "must respect the cooldown");
        assert_eq!(mock.calls_async().await, 1);
    }

    /// Exact boundary: the owner's decision says "last_notified_at is
    /// `>= NOTIFY_COOLDOWN_DAYS` old" — 7 days exactly must resend, 6 days
    /// must NOT (still inside the cooldown).
    #[tokio::test]
    async fn cooldown_boundary_exactly_seven_days_resends_six_days_does_not() {
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
        let mail = crate::mail::MailHandle::disabled();
        let today = test_today();

        // Six days ago -> still inside the cooldown, no resend.
        let uid_six = seed_customer(&pool, "sixdays@x", 1.0).await;
        seed_subscription(&pool, uid_six, &server.url("/wpush/six")).await;
        seed_topup(&pool, uid_six, 20.0).await;
        sqlx::query(
            "INSERT INTO push_notify_log (user_id, reason, last_notified_at, sent_count)
             VALUES (?, 'low_credit', datetime('now', '-6 days'), 1)",
        )
        .bind(uid_six)
        .execute(&pool)
        .await
        .unwrap();

        // Seven days ago exactly -> cooldown satisfied, must resend.
        let uid_seven = seed_customer(&pool, "sevendays@x", 1.0).await;
        seed_subscription(&pool, uid_seven, &server.url("/wpush/seven")).await;
        seed_topup(&pool, uid_seven, 20.0).await;
        sqlx::query(
            "INSERT INTO push_notify_log (user_id, reason, last_notified_at, sent_count)
             VALUES (?, 'low_credit', datetime('now', '-7 days'), 1)",
        )
        .bind(uid_seven)
        .execute(&pool)
        .await
        .unwrap();

        let sent = tick_as_of(&pool, &push, &mail, today).await.unwrap();
        assert_eq!(sent, 1, "only the 7-day-old row must resend");
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
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "sequence@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let day0 = test_today();

        // Tick 1 (day 0): first notification, sent_count -> 1.
        let sent = tick_as_of(&pool, &push, &mail, day0).await.unwrap();
        assert_eq!(sent, 1, "first tick must notify");

        // Tick 2 (day 0, same day): cooldown -> nothing.
        let sent = tick_as_of(&pool, &push, &mail, day0).await.unwrap();
        assert_eq!(sent, 0, "1. second tick same day -> nothing (cooldown)");
        assert_eq!(mock.calls_async().await, 1);

        // Tick 3 (day 8): cooldown expired -> second notification, count -> 2.
        let day8 = day0 + Days::new(8);
        let sent = tick_as_of(&pool, &push, &mail, day8).await.unwrap();
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
        let sent = tick_as_of(&pool, &push, &mail, day16).await.unwrap();
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
        let sent = tick_as_of(&pool, &push, &mail, day16).await.unwrap();
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
        let sent = tick_as_of(&pool, &push, &mail, day16).await.unwrap();
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
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "reup@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(sent, 1);

        // Customer tops up — condition clears.
        sqlx::query("UPDATE users SET credit = 100.0 WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();

        let sent_after_topup = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
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
        let sent_again = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
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
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "gone@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
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

    /// A subscription that keeps failing (429/5xx/malformed data — never a
    /// 404/410 `Gone`) must eventually be pruned too, not retried forever
    /// (#264 review finding). A failed send never stamps the notify
    /// ledger, so the SAME (user, reason) is re-evaluated — and the
    /// subscription re-attempted — on every tick with no cooldown in the
    /// way, letting repeated `tick_as_of` calls simulate consecutive
    /// failures directly.
    #[tokio::test]
    async fn repeated_failures_eventually_prune_the_subscription() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(500);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "failing@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/a")).await;
        seed_topup(&pool, uid, 20.0).await;

        for _ in 0..(MAX_CONSECUTIVE_FAILURES - 1) {
            tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        }
        assert!(
            db::push::has_subscription(&pool, uid).await.unwrap(),
            "must not prune before reaching the threshold"
        );

        tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert!(
            !db::push::has_subscription(&pool, uid).await.unwrap(),
            "must prune once failure_count reaches MAX_CONSECUTIVE_FAILURES"
        );
    }

    /// No push subscription, and the e-mail FALLBACK channel itself is
    /// unconfigured/failing (`MailHandle::disabled()`, e.g. SMTP_* env
    /// missing on this deployment) — still nothing sent, no error, no
    /// ledger write. Predates #311 (the customer used to just have no
    /// subscription); still exercises a real code path after #311's
    /// e-mail fallback landed, since a disabled mail transport always
    /// errors on `send()` — see `email_fallback_send_failure_does_not_stamp_the_ledger`
    /// below for the same shape spelled out explicitly as its own case.
    #[tokio::test]
    async fn no_subscription_no_send_no_ledger_write() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let mail = crate::mail::MailHandle::disabled();

        let uid = seed_customer(&pool, "nosub@x", 1.0).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(sent, 0);
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none()
        );
    }

    // ── #311: e-mail fallback (owner decision, variant (a) — 2026-08-10) ───

    /// Case 1: no push subscription at all, but the customer HAS an email
    /// on file -> exactly one e-mail is sent, zero pushes.
    #[tokio::test]
    async fn no_push_subscription_with_email_sends_exactly_one_email() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let mail = MailHandle::capture();

        let uid = seed_customer(&pool, "fallback@x", 1.0).await;
        seed_topup(&pool, uid, 20.0).await;
        // Deliberately NO seed_subscription() call — no push subscription.

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(
            sent, 1,
            "no push subscription + has email -> exactly 1 send"
        );

        let captured = mail
            .last_captured()
            .expect("the low_credit e-mail must have been sent via MailHandle");
        assert_eq!(captured.to, "fallback@x");
        assert!(
            captured.subject.to_lowercase().contains("kredit"),
            "subject should mention the low-credit reason, got {:?}",
            captured.subject
        );

        let log = db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
            .await
            .unwrap()
            .expect("a successful e-mail send must stamp the ledger exactly like a push send");
        assert_eq!(log.sent_count, 1);
    }

    /// Case 2: no push subscription AND `users.email IS NULL` (the typical
    /// legacy card-migrated account) -> nothing sent, no error, no ledger
    /// write. Uses a WORKING mail transport (`capture()`) so a false pass
    /// (mail unavailable, not "no address") is ruled out.
    #[tokio::test]
    async fn no_push_subscription_and_no_email_sends_nothing() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let mail = MailHandle::capture();

        let uid = seed_customer_no_email(&pool, 1.0).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(
            sent, 0,
            "no push subscription and no email on file -> nothing sent"
        );
        assert!(
            mail.last_captured().is_none(),
            "a working mail transport must never be dialed for a NULL email"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none()
        );
    }

    /// Case 3 (the variant-(a) guard): a customer with a WORKING push
    /// subscription AND an email on file must get exactly the push — never
    /// both, never the email instead.
    #[tokio::test]
    async fn push_subscription_present_never_falls_back_to_email() {
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
        let mail = MailHandle::capture();

        let uid = seed_customer(&pool, "hasboth@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/hasboth")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(sent, 1, "exactly one notification, the push");
        assert!(
            mail.last_captured().is_none(),
            "a customer with a working push subscription must never also get an email"
        );
    }

    /// Same guard as above, but for a subscription that EXISTS and is
    /// FAILING this particular tick (a 500 from the push service) — the
    /// channel choice is made on subscription EXISTENCE, never on whether
    /// THIS TICK's delivery actually succeeded (`.claude/rules/
    /// push-notifications.md`'s #311 section). Without this test, a future
    /// regression that falls back to email whenever a push SEND fails
    /// (rather than only when there's no subscription at all) would pass
    /// `push_subscription_present_never_falls_back_to_email` above
    /// unchanged (that test's mock always returns 201) and silently start
    /// double-notifying customers with a flaky-but-present subscription.
    #[tokio::test]
    async fn push_subscription_failing_this_tick_still_never_falls_back_to_email() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(500);
            })
            .await;
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let mail = MailHandle::capture();

        let uid = seed_customer(&pool, "flaky@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/flaky")).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &mail, test_today()).await.unwrap();
        assert_eq!(
            sent, 0,
            "the push send failed this tick, and a subscription still exists -> no email fallback"
        );
        assert!(
            mail.last_captured().is_none(),
            "a subscription that exists (even failing) must never trigger the email fallback"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none(),
            "a failed push send must not stamp the ledger either"
        );
    }

    /// Case 4: no push subscription, has an email, but the SMTP send
    /// itself FAILS -> the ledger is NOT stamped, so the very next tick
    /// (same day, no cooldown in the way since nothing was ever recorded)
    /// retries and can succeed once the transport recovers.
    #[tokio::test]
    async fn email_fallback_send_failure_does_not_stamp_the_ledger() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let failing_mail = MailHandle::disabled();

        let uid = seed_customer(&pool, "retry@x", 1.0).await;
        seed_topup(&pool, uid, 20.0).await;

        let sent = tick_as_of(&pool, &push, &failing_mail, test_today())
            .await
            .unwrap();
        assert_eq!(sent, 0, "a failed SMTP send must not count as sent");
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none(),
            "a failed send must NEVER stamp the ledger"
        );

        // The transport "recovers" (a working handle this time) — since the
        // ledger was never stamped, the SAME day retries immediately, with
        // no cooldown blocking it.
        let working_mail = MailHandle::capture();
        let sent_retry = tick_as_of(&pool, &push, &working_mail, test_today())
            .await
            .unwrap();
        assert_eq!(sent_retry, 1, "next tick must retry and succeed");
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_some()
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
        let mail = crate::mail::MailHandle::disabled();
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

        let sent = tick_as_of(&pool, &push, &mail, today).await.unwrap();
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
        let mail = crate::mail::MailHandle::disabled();
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

        let sent = tick_as_of(&pool, &push, &mail, today).await.unwrap();
        assert_eq!(
            sent, 1,
            "pass-expiring must fire regardless of top-up history"
        );
    }

    #[tokio::test]
    async fn low_credit_and_pass_expiring_can_never_co_fire_for_the_same_user() {
        // #306 FIX to this pre-existing test: before #306, a customer with
        // low credit AND a pass expiring soon got BOTH pushes (sent == 2)
        // — that WAS the bug (see the module doc's #306 section). Any user
        // for whom pass_expiring's condition is true necessarily holds an
        // active pass (its own window is `valid_until` between today and
        // today+PASS_EXPIRING_DAYS, i.e. `>= today`) — which is now
        // EXACTLY the condition that suppresses low_credit. So the two
        // reasons are structurally mutually exclusive for one user: this
        // test now locks that invariant instead of the old buggy "both
        // fire" expectation.
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
        let mail = crate::mail::MailHandle::disabled();
        let today = test_today();

        // Would otherwise qualify for low_credit (credit low, top-up
        // clears the gate) AND has a pass expiring soon.
        let uid = seed_customer(&pool, "both@x", 1.0).await;
        seed_subscription(&pool, uid, &server.url("/wpush/both")).await;
        seed_topup(&pool, uid, 20.0).await;
        seed_pass(
            &pool,
            uid,
            &(today + Days::new(1)).format("%Y-%m-%d").to_string(),
        )
        .await;

        let sent = tick_as_of(&pool, &push, &mail, today).await.unwrap();
        assert_eq!(
            sent, 1,
            "only pass_expiring must fire — the active pass suppresses low_credit"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_LOW_CREDIT)
                .await
                .unwrap()
                .is_none(),
            "low_credit must stay suppressed while the pass is active"
        );
        assert!(
            db::push::notify_log(&pool, uid, db::push::REASON_PASS_EXPIRING)
                .await
                .unwrap()
                .is_some(),
            "pass_expiring must still fire on its own"
        );
    }
}
