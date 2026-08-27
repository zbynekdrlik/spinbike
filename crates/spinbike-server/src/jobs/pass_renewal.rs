//! Daily contiguous monthly-pass auto-renewal (#374).
//!
//! Replaces the removed visit-triggered mechanism (#365/#372). For every user
//! who has the per-user `auto_renew_pass` flag ON, is not deleted/blocked, and
//! whose newest (non-voided) monthly pass has EXPIRED, this job issues a fresh
//! monthly pass at the price of their last one — driven by the END of the
//! previous month (this daily run), never by their next visit. The renewal is
//! CONTIGUOUS (the new pass continues from where the old one ended) within a
//! small tolerance; a bigger gap starts fresh from today. See
//! `.claude/rules/pass-auto-renewal.md` and `db::users::renew_expired_pass` /
//! `renewal_valid_until` for the money-write + continuity semantics.
//!
//! Idempotent: only expired-pass users are selected, and a renewal moves the
//! user's pass `valid_until` to `>= today`, so a second run the same day (or a
//! restart-startup tick) renews nobody twice — max one renewal per user per
//! run, never a chain of months. A push notification fires after each renewal
//! via the shared push infra (no anti-spam ledger, no e-mail fallback — the
//! once-per-renewal event is its own throttle; a user with no subscription
//! simply gets nothing, without error).

use anyhow::Result;
use sqlx::SqlitePool;

use crate::push::PushHandle;

/// Wall-clock hour (Europe/Bratislava, 0..=23) the daily renewal is aligned to
/// (`.claude/rules/daily-job-scheduling.md`; scheduled via
/// `jobs::spawn_daily_job`). 05:00: off-peak, distinct from the other daily
/// jobs (token_purge = 4, notifications = 9), and BEFORE the 09:00 notification
/// job so a renewed pass suppresses that user's redundant "pass expiring"
/// reminder the same morning.
pub const DAILY_RUN_HOUR: u32 = 5;

/// Run one renewal pass. Returns the number of passes renewed.
pub async fn tick(pool: &SqlitePool, push: &PushHandle) -> Result<usize> {
    tick_as_of(pool, push, crate::util::today_bratislava()).await
}

/// `tick` with an explicit gym-local `today` (testable). `today` is derived
/// from `crate::util::today_bratislava()` in production — never `chrono::Local`
/// (`.claude/rules/bratislava-tz.md`).
async fn tick_as_of(
    pool: &SqlitePool,
    push: &PushHandle,
    today: chrono::NaiveDate,
) -> Result<usize> {
    let today_s = today.format("%Y-%m-%d").to_string();

    // Candidates: flag ON, live account, newest non-voided pass already expired.
    // Resolved through the canonical `user_active_pass` view (V18) — the JOIN
    // excludes users who never held a pass, and the `date(ap.valid_until) < ?`
    // filter excludes users whose pass still covers today (idempotency). The
    // per-user money-write (`renew_expired_pass`) re-guards both defensively.
    let candidates: Vec<i64> = sqlx::query_scalar(
        "SELECT u.id \
         FROM users u \
         JOIN user_active_pass ap ON ap.user_id = u.id \
         WHERE u.auto_renew_pass = 1 \
           AND u.deleted_at IS NULL \
           AND u.blocked = 0 \
           AND date(ap.valid_until) < ? \
         ORDER BY u.id",
    )
    .bind(&today_s)
    .fetch_all(pool)
    .await?;

    let mut renewed = 0usize;
    for user_id in candidates {
        match crate::db::users::renew_expired_pass(pool, user_id, today).await {
            Ok(Some(renewal)) => {
                renewed += 1;
                // Best-effort push notification — a delivery failure must never
                // roll back or hide the renewal that already committed.
                let (title, body) =
                    render_renewal_notification(renewal.new_valid_until, renewal.new_credit);
                if let Err(e) = crate::jobs::notifications::send_to_subscriptions_for_user(
                    pool, push, user_id, &title, &body,
                )
                .await
                {
                    tracing::error!(
                        user_id,
                        error = %e,
                        "pass_renewal: push notification failed after renewal (renewal kept)"
                    );
                }
            }
            Ok(None) => {
                // Raced/covered between the query and the write — nothing to do.
            }
            Err(e) => {
                // One user's failure must not abort the whole run.
                tracing::error!(
                    user_id,
                    error = %e,
                    "pass_renewal: renewal failed for user, skipping"
                );
            }
        }
    }

    tracing::info!(renewed, "pass_renewal: daily tick complete");
    Ok(renewed)
}

/// Build the (title, body) of the post-renewal push. Slovak, UNACCENTED (the
/// DB/notification-string convention), date as `DD.MM.YYYY`, credit to cents.
fn render_renewal_notification(
    new_valid_until: chrono::NaiveDate,
    new_credit: f64,
) -> (String, String) {
    let title = "Permanentka predlzena".to_string();
    let body = format!(
        "Vasa permanentka bola predlzena do {}. Aktualny kredit: {:.2} EUR",
        new_valid_until.format("%d.%m.%Y"),
        new_credit
    );
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};
    use crate::push::{PushHandle, TEST_AUTH_B64, TEST_P256DH_B64, TEST_VAPID_PRIVATE_KEY_B64};
    use chrono::NaiveDate;
    use httpmock::MockServer;

    fn disabled_push() -> PushHandle {
        // No VAPID key → disabled; the renewal path never reaches an actual
        // send in tests without a seeded subscription, so this is enough for
        // every behavior test that doesn't assert the push itself.
        PushHandle::from_base64_private_key("")
    }

    async fn seed_user(pool: &SqlitePool, credit: f64, flag: bool) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO users (email, name, credit, auto_renew_pass) \
             VALUES (?, 'U', ?, ?) RETURNING id",
        )
        .bind(format!("u{credit}@x"))
        .bind(credit)
        .bind(if flag { 1 } else { 0 })
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// Seed a monthly-pass 'charge' row (respects the V20 invariant trigger:
    /// action='charge' + a monthly_pass service whenever valid_until is set).
    async fn seed_pass(pool: &SqlitePool, user_id: i64, amount: f64, valid_until: NaiveDate) {
        let svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
            .fetch_one(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, valid_until) \
             VALUES (?, NULL, ?, ?, 'charge', ?)",
        )
        .bind(user_id)
        .bind(svc)
        .bind(amount)
        .bind(valid_until)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn credit_of(pool: &SqlitePool, user_id: i64) -> f64 {
        sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn renewal_rows(pool: &SqlitePool, user_id: i64) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM transactions \
             WHERE user_id = ? AND note = 'auto-obnova' AND valid_until IS NOT NULL",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 27).unwrap()
    }

    /// (b) A flagged user whose pass expired within the tolerance gets a
    /// CONTIGUOUS renewal: the new pass starts the day after the old ended, at
    /// the last price, credit debited (into negative if needed).
    #[tokio::test]
    async fn renews_flagged_user_contiguously() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        // Expired 2 days ago (<= 3-day tolerance) → contiguous.
        let last_vu = today() - chrono::Duration::days(2); // 2026-08-25
        seed_pass(&pool, uid, -35.0, last_vu).await;

        let n = tick_as_of(&pool, &disabled_push(), today()).await.unwrap();
        assert_eq!(n, 1);

        assert!(
            (credit_of(&pool, uid).await - 65.0).abs() < 1e-9,
            "100 - 35"
        );
        assert_eq!(renewal_rows(&pool, uid).await, 1);

        // Contiguous: valid_until = (last_vu + 1 day) + 1 month = 2026-09-26.
        let vu: String = sqlx::query_scalar(
            "SELECT date(valid_until) FROM transactions WHERE user_id = ? AND note = 'auto-obnova'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(vu, "2026-09-26", "contiguous: 2026-08-25 +1d +1month");
    }

    /// (c) A user WITHOUT the flag is never renewed, even with an expired pass.
    #[tokio::test]
    async fn does_not_renew_without_flag() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, false).await;
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(2)).await;

        let n = tick_as_of(&pool, &disabled_push(), today()).await.unwrap();
        assert_eq!(n, 0);
        assert_eq!(renewal_rows(&pool, uid).await, 0);
        assert!((credit_of(&pool, uid).await - 100.0).abs() < 1e-9);
    }

    /// (d) Idempotent: a second run the same day renews nobody again (the first
    /// renewal moved valid_until to >= today, so the user is no longer expired).
    #[tokio::test]
    async fn is_idempotent_on_a_second_run_same_day() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(2)).await;

        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            1
        );
        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            0,
            "a second run the same day must renew nobody"
        );
        assert_eq!(renewal_rows(&pool, uid).await, 1);
        assert!(
            (credit_of(&pool, uid).await - 65.0).abs() < 1e-9,
            "no double debit"
        );
    }

    /// (e) A big gap (well past the tolerance) renews FROM TODAY — no back-dated
    /// months, so valid_until = today + 1 month, not old_vu + months.
    #[tokio::test]
    async fn big_gap_renews_from_today() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        // Expired 90 days ago — way outside the 3-day tolerance.
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(90)).await;

        let n = tick_as_of(&pool, &disabled_push(), today()).await.unwrap();
        assert_eq!(n, 1);

        let vu: String = sqlx::query_scalar(
            "SELECT date(valid_until) FROM transactions WHERE user_id = ? AND note = 'auto-obnova'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        // today (2026-08-27) + 1 month = 2026-09-27.
        assert_eq!(vu, "2026-09-27", "big gap: fresh from today + 1 month");
    }

    /// (f) The debit goes into the negative — no credit gate (owner decision).
    #[tokio::test]
    async fn debits_into_negative() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 0.0, true).await;
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(1)).await;

        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            1
        );
        assert!(
            (credit_of(&pool, uid).await - (-35.0)).abs() < 1e-9,
            "0 - 35 = -35 (into negative)"
        );
    }

    /// Skip a soft-deleted user (never debit a deleted account).
    #[tokio::test]
    async fn skips_deleted_user() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(2)).await;
        sqlx::query("UPDATE users SET deleted_at = datetime('now') WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            0
        );
        assert_eq!(renewal_rows(&pool, uid).await, 0);
    }

    /// Skip a blocked user.
    #[tokio::test]
    async fn skips_blocked_user() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(2)).await;
        sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            0
        );
        assert_eq!(renewal_rows(&pool, uid).await, 0);
    }

    /// A flagged user with NO pass history is skipped (no price, no continuity).
    #[tokio::test]
    async fn skips_user_with_no_pass_history() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;

        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            0
        );
        assert_eq!(renewal_rows(&pool, uid).await, 0);
        assert!((credit_of(&pool, uid).await - 100.0).abs() < 1e-9);
    }

    /// A flagged user whose pass is STILL live today is not renewed (idempotency
    /// / the query's expired-only filter).
    #[tokio::test]
    async fn skips_user_with_live_pass() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        // Still valid (expires in 5 days).
        seed_pass(&pool, uid, -35.0, today() + chrono::Duration::days(5)).await;

        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            0
        );
        assert_eq!(renewal_rows(&pool, uid).await, 0);
    }

    /// A 0 € barter pass renews at 0 € — deliberate (#342): credit unchanged, a
    /// 0 € renewal row is still written.
    #[tokio::test]
    async fn zero_price_pass_renews_at_zero() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 7.5, true).await;
        seed_pass(&pool, uid, 0.0, today() - chrono::Duration::days(1)).await;

        assert_eq!(
            tick_as_of(&pool, &disabled_push(), today()).await.unwrap(),
            1
        );
        assert!(
            (credit_of(&pool, uid).await - 7.5).abs() < 1e-9,
            "a 0 EUR renewal must not change credit"
        );
        let zero_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transactions \
             WHERE user_id = ? AND note = 'auto-obnova' AND amount = 0.0 AND valid_until IS NOT NULL",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(zero_rows, 1, "a 0 EUR renewal row must still be issued");
    }

    /// The post-renewal push CONTENT: exact Slovak text, DD.MM.YYYY date, credit
    /// to cents, and NO diacritics (the DB/notification-string convention).
    #[test]
    fn renders_the_renewal_notification_text() {
        let (title, body) =
            render_renewal_notification(NaiveDate::from_ymd_opt(2026, 9, 26).unwrap(), -12.5);
        assert_eq!(title, "Permanentka predlzena");
        assert_eq!(
            body,
            "Vasa permanentka bola predlzena do 26.09.2026. Aktualny kredit: -12.50 EUR"
        );
        assert!(
            body.is_ascii(),
            "notification strings must be unaccented ASCII, got {body:?}"
        );
    }

    /// Wiring: a renewed user WITH a push subscription actually gets a push
    /// send (the job calls the shared delivery). Uses a local mock push service
    /// returning 201 so the encrypted RFC-8291 POST succeeds end-to-end.
    #[tokio::test]
    async fn fires_a_push_to_a_subscribed_user_after_renewal() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(1)).await;

        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/wpush/renew");
                then.status(201);
            })
            .await;
        crate::db::push::upsert_subscription(
            &pool,
            uid,
            &server.url("/wpush/renew"),
            TEST_P256DH_B64,
            TEST_AUTH_B64,
        )
        .await
        .unwrap();

        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        let n = tick_as_of(&pool, &push, today()).await.unwrap();
        assert_eq!(n, 1, "the renewal still happens");
        assert_eq!(
            mock.calls_async().await,
            1,
            "the subscribed user must receive exactly one renewal push"
        );
    }

    /// A user with NO subscription is renewed but simply gets no push, and the
    /// tick does not error (owner decision: no subscription → nothing).
    #[tokio::test]
    async fn renews_without_error_when_no_subscription() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let uid = seed_user(&pool, 100.0, true).await;
        seed_pass(&pool, uid, -35.0, today() - chrono::Duration::days(1)).await;

        // Enabled handle, but no subscription seeded → send is never attempted.
        let push = PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64);
        assert_eq!(tick_as_of(&pool, &push, today()).await.unwrap(), 1);
    }

    /// #205 bug class (`.claude/rules/bratislava-tz.md`): the production `tick`
    /// derives "today" from the shared `crate::util::today_bratislava()` helper,
    /// never `chrono::Local` (OS/TZ-dependent). Source-level invariant test —
    /// the runtime output can't differ in this already-Bratislava environment.
    #[test]
    fn tick_computes_today_via_shared_bratislava_helper() {
        let src = include_str!("pass_renewal.rs");
        let production_src = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("file always has content before its own test module marker");
        assert!(
            !production_src.contains("Local::now()"),
            "must not use chrono::Local::now() (OS-TZ dependent)"
        );
        assert!(
            production_src.contains("crate::util::today_bratislava()"),
            "must derive today via the shared Bratislava helper"
        );
    }
}
