//! Daily day-before spinning reminder (#382 R6, owner ROZHODNUTÉ
//! 2026-10-12: the day before at 18:00 Europe/Bratislava, push with e-mail
//! fallback).
//!
//! For every ACTIVE booking of TOMORROW (not cancelled, class occurrence not
//! cancelled, live account) that was not reminded yet, the customer gets
//! "Zajtra mas spinning o 18:30 (Vlada). Ak nemozes prist, odhlas sa do 9:00,
//! inak sa kredit strhne." — by push when they have ANY stored push
//! subscription, otherwise by e-mail when they have an address (the same
//! "fallback, never a duplicate channel" rule as `jobs::notifications`,
//! #311), otherwise nothing.
//!
//! **Dedupe is per BOOKING**, via `bookings.reminded_at` (V31), stamped only
//! after a successful send — NOT the per-(user, reason) `push_notify_log`
//! cooldown, which would swallow the following week's reminder. So a startup
//! tick, a restart, or a second run never double-sends, and a failed send is
//! retried by the next run.
//!
//! **Startup tick:** [`startup_tick`] runs at boot like every daily job
//! (`daily-job-scheduling.md`), but only sends once the day's 18:00 run hour
//! has passed — a restart at 10:00 must not remind about tomorrow 8 hours
//! early; a restart at 19:00 catches up a run the deploy may have missed.
//! The scheduled run ([`tick`], via `spawn_daily_job` at [`DAILY_RUN_HOUR`])
//! is not gated: it fires at 18:00 by construction.

use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime, Timelike};
use sqlx::SqlitePool;

use crate::db;
use crate::mail::MailHandle;
use crate::push::PushHandle;

/// Wall-clock hour (Europe/Bratislava) of the daily reminder run — the owner's
/// decision (18:00 the day before). Distinct from every other daily job
/// (token_purge 4, pass_renewal 5, notifications 9).
pub const DAILY_RUN_HOUR: u32 = 18;

/// Scheduled run (18:00): remind every not-yet-reminded booking of tomorrow.
pub async fn tick(pool: &SqlitePool, push: &PushHandle, mail: &MailHandle) -> Result<usize> {
    let tomorrow = tomorrow_of(crate::util::today_bratislava())?;
    remind_for_date(pool, push, mail, tomorrow).await
}

/// Boot-time run: same as [`tick`], but only once today's [`DAILY_RUN_HOUR`]
/// has passed (see the module doc).
pub async fn startup_tick(
    pool: &SqlitePool,
    push: &PushHandle,
    mail: &MailHandle,
) -> Result<usize> {
    startup_tick_as_of(pool, push, mail, crate::util::now_bratislava()).await
}

/// [`startup_tick`] with an injected gym-local `now` (testable).
pub async fn startup_tick_as_of(
    pool: &SqlitePool,
    push: &PushHandle,
    mail: &MailHandle,
    now: NaiveDateTime,
) -> Result<usize> {
    if now.hour() < DAILY_RUN_HOUR {
        tracing::debug!(
            now = %now,
            "spinning reminder: startup before {DAILY_RUN_HOUR}:00 — nothing due yet"
        );
        return Ok(0);
    }
    remind_for_date(pool, push, mail, tomorrow_of(now.date())?).await
}

fn tomorrow_of(today: NaiveDate) -> Result<NaiveDate> {
    today
        .succ_opt()
        .ok_or_else(|| anyhow::anyhow!("no calendar day after {today}"))
}

/// The reminder text (Slovak, UNACCENTED — the notification-string
/// convention). The instructor is omitted when the class has none.
pub fn render_reminder(start_time: &str, instructor: Option<&str>) -> (String, String) {
    let who = match instructor {
        Some(name) if !name.trim().is_empty() => format!(" ({})", name.trim()),
        _ => String::new(),
    };
    (
        "Zajtra mas spinning".to_string(),
        format!(
            "Zajtra mas spinning o {start_time}{who}. Ak nemozes prist, odhlas sa do 9:00, \
             inak sa kredit strhne."
        ),
    )
}

/// How a single reminder went out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Push,
    Email,
    /// Nothing reached the customer (send failed, or no channel at all) —
    /// the booking stays un-stamped and is retried by the next run.
    NotDelivered,
}

/// Remind every active, not-yet-reminded booking on `date`. Returns how many
/// reminders were delivered. One customer's failure never aborts the rest.
pub async fn remind_for_date(
    pool: &SqlitePool,
    push: &PushHandle,
    mail: &MailHandle,
    date: NaiveDate,
) -> Result<usize> {
    // (booking_id, user_id, start_time, instructor_name, email)
    type DueRow = (i64, i64, String, Option<String>, Option<String>);
    let due: Vec<DueRow> = sqlx::query_as(
        "SELECT b.id, b.user_id, t.start_time, i.name, u.email
           FROM bookings b
           JOIN class_templates t ON t.id = b.template_id
           LEFT JOIN instructors i ON i.id = t.instructor_id
           JOIN users u ON u.id = b.user_id
          WHERE b.date = ?
            AND b.cancelled_at IS NULL
            AND b.reminded_at IS NULL
            AND u.deleted_at IS NULL
            AND u.blocked = 0
            AND NOT EXISTS (SELECT 1 FROM class_cancellations cc
                             WHERE cc.template_id = b.template_id AND cc.date = b.date)
          ORDER BY t.start_time, b.id",
    )
    .bind(date.to_string())
    .fetch_all(pool)
    .await?;

    let candidates = due.len();
    let mut sent = 0usize;
    for (booking_id, user_id, start_time, instructor, email) in due {
        let (title, body) = render_reminder(&start_time, instructor.as_deref());
        match deliver(pool, push, mail, user_id, email.as_deref(), &title, &body).await {
            Ok(Delivery::NotDelivered) => {}
            Ok(channel) => {
                sqlx::query(
                    "UPDATE bookings SET reminded_at = datetime('now')
                     WHERE id = ? AND reminded_at IS NULL",
                )
                .bind(booking_id)
                .execute(pool)
                .await?;
                sent += 1;
                tracing::info!(
                    booking_id,
                    user_id,
                    class_date = %date,
                    start_time = %start_time,
                    ?channel,
                    "spinning reminder: sent"
                );
            }
            Err(e) => {
                tracing::error!(
                    booking_id,
                    user_id,
                    class_date = %date,
                    error = %e,
                    "spinning reminder: delivery errored (retried next run)"
                );
            }
        }
    }
    tracing::info!(
        class_date = %date,
        candidates,
        sent,
        "spinning reminder: run complete"
    );
    Ok(sent)
}

/// Push when the customer has ANY stored subscription; otherwise the e-mail
/// fallback when they have an address; otherwise nothing.
async fn deliver(
    pool: &SqlitePool,
    push: &PushHandle,
    mail: &MailHandle,
    user_id: i64,
    email: Option<&str>,
    title: &str,
    body: &str,
) -> Result<Delivery> {
    let subs = db::push::list_subscriptions_for_user(pool, user_id).await?;
    if !subs.is_empty() {
        let any_sent = crate::jobs::notifications::send_to_subscriptions(
            pool, push, user_id, subs, title, body,
        )
        .await?;
        if !any_sent {
            tracing::warn!(
                user_id,
                "spinning reminder: push failed on every subscription, not stamped"
            );
            return Ok(Delivery::NotDelivered);
        }
        return Ok(Delivery::Push);
    }
    let Some(addr) = email.map(str::trim).filter(|e| !e.is_empty()) else {
        tracing::debug!(
            user_id,
            "spinning reminder: no push subscription and no e-mail — skipped"
        );
        return Ok(Delivery::NotDelivered);
    };
    let html = format!("<p>{body}</p>");
    match mail.send(addr, title, body, &html).await {
        Ok(()) => Ok(Delivery::Email),
        Err(e) => {
            tracing::warn!(
                user_id,
                to = %addr,
                error = %e,
                "spinning reminder: e-mail fallback failed, not stamped"
            );
            Ok(Delivery::NotDelivered)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};
    use crate::push::{TEST_AUTH_B64, TEST_P256DH_B64, TEST_VAPID_PRIVATE_KEY_B64};
    use httpmock::MockServer;

    const DAY: &str = "2026-10-13";

    fn day() -> NaiveDate {
        NaiveDate::parse_from_str(DAY, "%Y-%m-%d").unwrap()
    }

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn user(pool: &SqlitePool, email: Option<&str>) -> i64 {
        sqlx::query_scalar("INSERT INTO users (email, name) VALUES (?, 'R') RETURNING id")
            .bind(email)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// The V6/V30 Tuesday 18:30 class taught by Vlada.
    async fn tuesday_template(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT id FROM class_templates WHERE weekday = 1")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn book(pool: &SqlitePool, uid: i64) -> i64 {
        let tid = tuesday_template(pool).await;
        crate::db::classes::create_booking(pool, tid, DAY, uid, None, "persistent")
            .await
            .unwrap()
    }

    async fn reminded(pool: &SqlitePool, bid: i64) -> bool {
        sqlx::query_scalar::<_, Option<String>>("SELECT reminded_at FROM bookings WHERE id = ?")
            .bind(bid)
            .fetch_one(pool)
            .await
            .unwrap()
            .is_some()
    }

    fn push() -> PushHandle {
        PushHandle::from_base64_private_key(TEST_VAPID_PRIVATE_KEY_B64)
    }

    #[test]
    fn reminder_text_is_unaccented_slovak_with_time_and_instructor() {
        let (title, body) = render_reminder("18:30", Some("Vlada"));
        assert_eq!(title, "Zajtra mas spinning");
        assert_eq!(
            body,
            "Zajtra mas spinning o 18:30 (Vlada). Ak nemozes prist, odhlas sa do 9:00, \
             inak sa kredit strhne."
        );
        let (_, no_instr) = render_reminder("18:00", None);
        assert!(no_instr.starts_with("Zajtra mas spinning o 18:00. Ak"));
        assert!(body.is_ascii(), "unaccented");
    }

    #[test]
    fn run_hour_is_18_and_distinct_from_the_other_daily_jobs() {
        assert_eq!(DAILY_RUN_HOUR, 18);
        for other in [
            crate::jobs::notifications::DAILY_RUN_HOUR,
            crate::jobs::pass_renewal::DAILY_RUN_HOUR,
            crate::jobs::token_purge::DAILY_RUN_HOUR,
        ] {
            assert_ne!(DAILY_RUN_HOUR, other);
        }
    }

    /// One push per booking, stamped; a second run (e.g. a startup tick
    /// after a restart) sends nothing more.
    #[tokio::test]
    async fn sends_one_push_per_booking_and_never_twice() {
        let pool = setup().await;
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(201);
            })
            .await;
        let uid = user(&pool, Some("p@x")).await;
        db::push::upsert_subscription(
            &pool,
            uid,
            &server.url("/wpush/r"),
            TEST_P256DH_B64,
            TEST_AUTH_B64,
        )
        .await
        .unwrap();
        let bid = book(&pool, uid).await;
        let mail = MailHandle::capture();

        let sent = remind_for_date(&pool, &push(), &mail, day()).await.unwrap();
        assert_eq!(sent, 1);
        assert!(reminded(&pool, bid).await);
        assert_eq!(mock.calls_async().await, 1);
        assert!(mail.last_captured().is_none(), "push only, no e-mail too");

        let again = startup_tick_as_of(&pool, &push(), &mail, at("2026-10-12 19:00:00"))
            .await
            .unwrap();
        assert_eq!(again, 0, "dedupe per booking");
        assert_eq!(mock.calls_async().await, 1);
    }

    /// No push subscription → exactly one e-mail with the same text.
    #[tokio::test]
    async fn falls_back_to_email_without_a_push_subscription() {
        let pool = setup().await;
        let uid = user(&pool, Some("mail@x")).await;
        let bid = book(&pool, uid).await;
        let mail = MailHandle::capture();

        let sent = remind_for_date(&pool, &push(), &mail, day()).await.unwrap();
        assert_eq!(sent, 1);
        assert!(reminded(&pool, bid).await);
        let m = mail.last_captured().expect("e-mail fallback");
        assert_eq!(m.to, "mail@x");
        assert_eq!(m.subject, "Zajtra mas spinning");
        assert!(m.text.contains("o 18:30 (Vlada)"), "got {:?}", m.text);
    }

    /// Neither channel → nothing sent, NOT stamped (retried next run); a
    /// failing push is not stamped either.
    #[tokio::test]
    async fn an_undelivered_reminder_is_not_stamped() {
        let pool = setup().await;
        let no_channel = user(&pool, None).await;
        let b1 = book(&pool, no_channel).await;

        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST);
                then.status(500);
            })
            .await;
        let failing = user(&pool, Some("f@x")).await;
        db::push::upsert_subscription(
            &pool,
            failing,
            &server.url("/wpush/f"),
            TEST_P256DH_B64,
            TEST_AUTH_B64,
        )
        .await
        .unwrap();
        let b2 = book(&pool, failing).await;

        let mail = MailHandle::capture();
        let sent = remind_for_date(&pool, &push(), &mail, day()).await.unwrap();
        assert_eq!(sent, 0);
        assert!(!reminded(&pool, b1).await);
        assert!(!reminded(&pool, b2).await);
        assert!(
            mail.last_captured().is_none(),
            "a push customer never gets the e-mail"
        );
    }

    /// Cancelled bookings, cancelled class occurrences, other days and
    /// blocked accounts get nothing.
    #[tokio::test]
    async fn skips_cancelled_bookings_cancelled_classes_and_other_days() {
        let pool = setup().await;
        let mail = MailHandle::capture();
        let tid = tuesday_template(&pool).await;

        let cancelled_booking_user = user(&pool, Some("c1@x")).await;
        let bid = book(&pool, cancelled_booking_user).await;
        crate::db::classes::cancel_booking(
            &pool,
            bid,
            Some(cancelled_booking_user),
            crate::db::classes::CancelKind::Customer,
        )
        .await
        .unwrap();

        let blocked = user(&pool, Some("b@x")).await;
        sqlx::query("UPDATE users SET blocked = 1 WHERE id = ?")
            .bind(blocked)
            .execute(&pool)
            .await
            .unwrap();
        book(&pool, blocked).await;

        let next_week = user(&pool, Some("n@x")).await;
        crate::db::classes::create_booking(&pool, tid, "2026-10-20", next_week, None, "manual")
            .await
            .unwrap();

        assert_eq!(
            remind_for_date(&pool, &push(), &mail, day()).await.unwrap(),
            0
        );

        // A live booking on a CANCELLED occurrence: no reminder either.
        let live = user(&pool, Some("l@x")).await;
        book(&pool, live).await;
        crate::db::classes::cancel_occurrence(&pool, tid, DAY, Some("x"), None)
            .await
            .unwrap();
        assert_eq!(
            remind_for_date(&pool, &push(), &mail, day()).await.unwrap(),
            0
        );
        assert!(mail.last_captured().is_none());
    }

    /// The boot-time run sends nothing before 18:00 (no 8-hours-early
    /// reminder after a morning deploy) and catches up after 18:00.
    #[tokio::test]
    async fn startup_tick_only_sends_after_the_run_hour() {
        let pool = setup().await;
        let uid = user(&pool, Some("s@x")).await;
        let bid = book(&pool, uid).await;
        let mail = MailHandle::capture();

        let early = startup_tick_as_of(&pool, &push(), &mail, at("2026-10-12 17:59:59"))
            .await
            .unwrap();
        assert_eq!(early, 0);
        assert!(!reminded(&pool, bid).await);

        let late = startup_tick_as_of(&pool, &push(), &mail, at("2026-10-12 18:00:00"))
            .await
            .unwrap();
        assert_eq!(late, 1);
        assert!(reminded(&pool, bid).await);
    }

    /// Source invariant (#205 bug class): the job derives its clock via the
    /// shared Bratislava helpers, never `chrono::Local`.
    #[test]
    fn computes_dates_via_shared_bratislava_helpers() {
        let src = include_str!("spinning_reminder.rs");
        let production_src = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("file has content before its test module");
        assert!(!production_src.contains("Local::now()"));
        assert!(production_src.contains("crate::util::today_bratislava()"));
        assert!(production_src.contains("crate::util::now_bratislava()"));
    }
}
