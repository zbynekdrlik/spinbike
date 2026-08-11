//! Persistent-booking materialiser: ensures a concrete booking row exists for
//! every future occurrence of an active persistent subscription within the
//! next 14 days. Skips occurrences where the class is full. Idempotent.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{Datelike, Duration};
use sqlx::SqlitePool;

use crate::db::error::DbError;

pub const WINDOW_DAYS: i64 = 14;

/// One (persistent booking, occurrence date) pair the sweep needs to check
/// and possibly materialise.
struct Candidate {
    persistent_id: i64,
    user_id: i64,
    template_id: i64,
    date: String,
}

/// `?, ?, ..., ?` — `n` placeholders, comma-joined, for a dynamic `IN (...)`.
fn placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(", ")
}

pub async fn sweep(pool: &SqlitePool) -> Result<usize> {
    let persistents = crate::db::persistent_bookings::list_active_all(pool).await?;
    let templates = sqlx::query_as::<_, crate::db::classes::ClassTemplateRow>(
        "SELECT id, weekday, start_time, duration_minutes, instructor_id, capacity, active
         FROM class_templates WHERE active = 1",
    )
    .fetch_all(pool)
    .await?;

    let today = crate::util::today_bratislava();

    // Build every (template, date, user) candidate this sweep might need to
    // materialise. Unchanged filter logic from before #341 — only the
    // per-candidate DB checks below are batched.
    let mut candidates: Vec<Candidate> = Vec::new();
    for p in &persistents {
        let Some(tpl) = templates.iter().find(|t| t.id == p.template_id) else {
            // The subscription points at a template that has been deactivated
            // or deleted. Log so staff can investigate; skip materialising.
            tracing::warn!(
                "persistent booking id={} references inactive/missing template {}",
                p.id,
                p.template_id
            );
            continue;
        };

        for offset in 0..=WINDOW_DAYS {
            let d = today + Duration::days(offset);
            if d.weekday().num_days_from_monday() as i64 != tpl.weekday {
                continue;
            }
            candidates.push(Candidate {
                persistent_id: p.id,
                user_id: p.user_id,
                template_id: tpl.id,
                date: d.to_string(),
            });
        }
    }

    let total = candidates.len();
    if candidates.is_empty() {
        return Ok(0);
    }

    // Batch-prefetch what the old code queried once PER candidate (#341):
    // cancelled occurrences, existing bookings, and current booked counts —
    // for the whole DISTINCT (template_id, date) set the candidates above
    // reference — then join in memory instead of issuing up to 3 round
    // trips per candidate. `template_id IN (...) AND date IN (...)` can
    // return rows for pairs that were never actually candidates (e.g. a
    // different template cancelled on a date this template also has a
    // candidate for); that's fine — every read below is joined back against
    // the exact `(template_id, date[, user_id])` key, so a pair that isn't a
    // real match for THIS template+date never affects it.
    let mut template_ids: Vec<i64> = candidates.iter().map(|c| c.template_id).collect();
    template_ids.sort_unstable();
    template_ids.dedup();
    let mut dates: Vec<String> = candidates.iter().map(|c| c.date.clone()).collect();
    dates.sort_unstable();
    dates.dedup();
    let tpl_ph = placeholders(template_ids.len());
    let date_ph = placeholders(dates.len());

    let cancelled: HashSet<(i64, String)> = {
        let sql = format!(
            "SELECT template_id, date FROM class_cancellations
             WHERE template_id IN ({tpl_ph}) AND date IN ({date_ph})"
        );
        let mut q = sqlx::query_as::<_, (i64, String)>(&sql);
        for t in &template_ids {
            q = q.bind(t);
        }
        for d in &dates {
            q = q.bind(d);
        }
        q.fetch_all(pool).await?.into_iter().collect()
    };

    let existing: HashSet<(i64, String, i64)> = {
        let sql = format!(
            "SELECT template_id, date, user_id FROM bookings
             WHERE template_id IN ({tpl_ph}) AND date IN ({date_ph}) AND cancelled_at IS NULL"
        );
        let mut q = sqlx::query_as::<_, (i64, String, i64)>(&sql);
        for t in &template_ids {
            q = q.bind(t);
        }
        for d in &dates {
            q = q.bind(d);
        }
        q.fetch_all(pool).await?.into_iter().collect()
    };

    let booked_counts: HashMap<(i64, String), i64> = {
        let sql = format!(
            "SELECT template_id, date, COUNT(*) AS cnt FROM bookings
             WHERE template_id IN ({tpl_ph}) AND date IN ({date_ph}) AND cancelled_at IS NULL
             GROUP BY template_id, date"
        );
        let mut q = sqlx::query_as::<_, (i64, String, i64)>(&sql);
        for t in &template_ids {
            q = q.bind(t);
        }
        for d in &dates {
            q = q.bind(d);
        }
        q.fetch_all(pool)
            .await?
            .into_iter()
            .map(|(t, d, c)| ((t, d), c))
            .collect()
    };

    let capacity_by_template: HashMap<i64, i64> =
        templates.iter().map(|t| (t.id, t.capacity)).collect();

    let mut created = 0usize;
    let mut skipped_full = 0usize;
    let mut errored = 0usize;

    for c in candidates {
        let key = (c.template_id, c.date.clone());
        if cancelled.contains(&key) {
            continue;
        }
        if existing.contains(&(c.template_id, c.date.clone(), c.user_id)) {
            continue;
        }
        // Snapshot-based pre-check: avoids attempting an INSERT we already
        // know will fail. This snapshot was taken once, up front, for the
        // whole tick — it can go stale relative to bookings created by an
        // external request, OR by an earlier candidate processed earlier in
        // THIS SAME sweep (the count below is never incremented as the loop
        // creates bookings). `create_booking`'s own atomic check below is
        // the real authority; see the ClassFull arm.
        let booked = booked_counts.get(&key).copied().unwrap_or(0);
        let capacity = capacity_by_template
            .get(&c.template_id)
            .copied()
            .unwrap_or(0);
        if booked >= capacity {
            continue;
        }

        // `ClassFull` here is an EXPECTED, per-item outcome, not a
        // sweep-level failure (#345): the pre-check above is advisory, this
        // atomic check is authoritative, and losing the race just means
        // someone else (a normal booking request, or another candidate
        // earlier in this same tick) took the slot first. Log it, count it,
        // and keep processing the rest of the candidates — one full class
        // must never suppress materialisation for every other subscriber.
        // Any OTHER error is unexpected (pool/IO); it's also counted and
        // logged rather than aborting, so a single bad row can't cost every
        // other subscriber their materialisation either.
        match crate::db::classes::create_booking(
            pool,
            c.template_id,
            &c.date,
            c.user_id,
            None,
            "persistent",
        )
        .await
        {
            Ok(_) => created += 1,
            Err(DbError::ClassFull) => {
                skipped_full += 1;
                tracing::warn!(
                    "materialiser: persistent booking id={} user={} template={} date={} \
                     skipped — class became full before insert",
                    c.persistent_id,
                    c.user_id,
                    c.template_id,
                    c.date
                );
            }
            Err(e) => {
                errored += 1;
                tracing::error!(
                    "materialiser: failed to materialise persistent booking id={} user={} \
                     template={} date={}: {e}",
                    c.persistent_id,
                    c.user_id,
                    c.template_id,
                    c.date
                );
            }
        }
    }

    tracing::info!(
        "materialiser sweep: candidates={total} created={created} \
         skipped_full={skipped_full} errored={errored}"
    );

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed(pool: &SqlitePool, weekday: i64) -> (i64, i64) {
        let uid: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
                .fetch_one(pool)
                .await
                .unwrap();
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday = ? AND start_time='18:00'",
        )
        .bind(weekday)
        .fetch_one(pool)
        .await
        .unwrap();
        (uid, tid)
    }

    /// Regression test for #327: `sweep()` must derive "today" from the
    /// shared, tz-database-driven `crate::util::today_bratislava()` helper,
    /// never from `chrono::Local::now()` (which reads the server process's
    /// OS/TZ config — UTC on the prod systemd unit, drifting from real
    /// Bratislava local time around midnight; the #205/#222 bug class).
    ///
    /// This stays a source-level guard rather than a behavioral one because
    /// it pins the CALL SITE itself: a behavioral test could only observe a
    /// divergence during the ~1-2h/day window where Europe/Bratislava and
    /// UTC disagree on the calendar date, which would make the test flaky
    /// by time-of-day. Since #336, CI additionally runs this whole suite
    /// under `TZ=UTC` in the `test-tz-utc` job, so a regression here is now
    /// ALSO caught behaviorally on every push — this guard just makes the
    /// failure immediate and self-explanatory instead of a midnight-only
    /// surprise.
    #[test]
    fn sweep_computes_today_via_shared_bratislava_helper() {
        let src = include_str!("materialiser.rs");
        let production_src = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("file always has content before its own test module marker");
        assert!(
            !production_src.contains("Local::now()"),
            "sweep() must not call chrono::Local::now() directly (OS-TZ dependent — #327)"
        );
        assert!(
            production_src.contains("crate::util::today_bratislava()"),
            "sweep() must derive today via the shared crate::util::today_bratislava() helper (#327)"
        );
    }

    #[tokio::test]
    async fn sweep_materialises_future_bookings() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, uid, tid)
            .await
            .unwrap();

        let made = sweep(&pool).await.unwrap();
        assert!(made >= 1, "at least one Monday in next 14 days");

        let sources: Vec<(String,)> =
            sqlx::query_as("SELECT source FROM bookings WHERE user_id = ?")
                .bind(uid)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(sources.iter().all(|(s,)| s == "persistent"));
    }

    #[tokio::test]
    async fn sweep_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, uid, tid)
            .await
            .unwrap();

        let first = sweep(&pool).await.unwrap();
        let second = sweep(&pool).await.unwrap();
        assert_eq!(second, 0, "second sweep should create nothing");
        assert!(first > 0);
    }

    #[tokio::test]
    async fn sweep_skips_full_classes() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Pick a Monday strictly in the future (avoid "today is Monday" flake).
        let today = crate::util::today_bratislava();
        let m = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let offset = if m == 0 { 7 } else { m };
        let next_mon = today + Duration::days(offset);
        let date_s = next_mon.to_string();

        for n in 0..19 {
            let uid: i64 =
                sqlx::query_scalar("INSERT INTO users (email, name) VALUES (?, 'u') RETURNING id")
                    .bind(format!("u{n}@x"))
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            crate::db::classes::create_booking(&pool, tid, &date_s, uid, None, "manual")
                .await
                .unwrap();
        }

        let (uid, _) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, uid, tid)
            .await
            .unwrap();

        let _ = sweep(&pool).await.unwrap();
        let got: Option<i64> =
            sqlx::query_scalar("SELECT id FROM bookings WHERE user_id=? AND date=?")
                .bind(uid)
                .bind(&date_s)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(got.is_none(), "must skip full class");
    }

    /// #341: `sweep()`'s cancellation check is now one batched query over
    /// `template_id IN (...) AND date IN (...)`, joined back in memory by
    /// the exact `(template_id, date)` pair. This pins that pairing: two
    /// templates on the SAME weekday (so they share a candidate date),
    /// only ONE of them cancelled on that date — the other template's
    /// booking on the identical date must still be created. A naive
    /// "date IN cancelled_dates" check with no template pairing would
    /// wrongly skip BOTH.
    #[tokio::test]
    async fn sweep_batched_cancellation_check_is_scoped_per_template_not_just_per_date() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let today = crate::util::today_bratislava();
        let weekday = today.weekday().num_days_from_monday() as i64;
        let today_s = today.to_string();

        let tid1 = crate::db::classes::create_template(&pool, weekday, "06:00", 45, None, 5)
            .await
            .unwrap();
        let tid2 = crate::db::classes::create_template(&pool, weekday, "07:00", 45, None, 5)
            .await
            .unwrap();

        // Cancel template 1's occurrence today; template 2 is untouched.
        crate::db::classes::cancel_occurrence(&pool, tid1, &today_s, Some("test"), None)
            .await
            .unwrap();

        let user_a: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('a@x','a') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let user_b: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('b@x','b') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        crate::db::persistent_bookings::create(&pool, user_a, tid1)
            .await
            .unwrap();
        crate::db::persistent_bookings::create(&pool, user_b, tid2)
            .await
            .unwrap();

        let _ = sweep(&pool).await.unwrap();

        let a_today: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM bookings WHERE template_id=? AND date=? AND user_id=?",
        )
        .bind(tid1)
        .bind(&today_s)
        .bind(user_a)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            a_today.is_none(),
            "template 1 is cancelled today — user A must NOT get a booking today"
        );

        let b_today: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM bookings WHERE template_id=? AND date=? AND user_id=?",
        )
        .bind(tid2)
        .bind(&today_s)
        .bind(user_b)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            b_today.is_some(),
            "template 2 was never cancelled — user B's booking today must still be created, \
             proving the batched cancellation check doesn't cross-contaminate between \
             templates that merely share a candidate DATE"
        );
    }

    /// #341: the batched existing-booking check is likewise scoped by the
    /// full `(template_id, date, user_id)` triple, not just `(date,
    /// user_id)`. A user with an unrelated MANUAL booking on template 1
    /// today must still get their PERSISTENT booking materialised on a
    /// different template (2) for the identical date.
    #[tokio::test]
    async fn sweep_batched_existing_check_is_scoped_per_template_not_just_per_user_and_date() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let today = crate::util::today_bratislava();
        let weekday = today.weekday().num_days_from_monday() as i64;
        let today_s = today.to_string();

        let tid1 = crate::db::classes::create_template(&pool, weekday, "06:00", 45, None, 5)
            .await
            .unwrap();
        let tid2 = crate::db::classes::create_template(&pool, weekday, "07:00", 45, None, 5)
            .await
            .unwrap();

        let user_x: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('x@x','x') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();

        // Manual booking on template 1 today — no persistent subscription
        // for template 1 at all, so sweep() should never touch it again.
        crate::db::classes::create_booking(&pool, tid1, &today_s, user_x, None, "manual")
            .await
            .unwrap();
        // A persistent subscription on the DIFFERENT template 2.
        crate::db::persistent_bookings::create(&pool, user_x, tid2)
            .await
            .unwrap();

        let _ = sweep(&pool).await.unwrap();

        let tid2_booking: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM bookings WHERE template_id=? AND date=? AND user_id=?",
        )
        .bind(tid2)
        .bind(&today_s)
        .bind(user_x)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            tid2_booking.is_some(),
            "user's existing booking on template 1 must not block their persistent \
             booking on template 2 for the same date — the batched existing-booking \
             check must be scoped per template, not just per (date, user)"
        );

        let tid1_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM bookings WHERE template_id=? AND date=?")
                .bind(tid1)
                .bind(&today_s)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            tid1_count, 1,
            "sweep() must not create a second booking on template 1 (no persistent there)"
        );
    }

    /// Regression test for #345: a `ClassFull` from `create_booking()` must
    /// not abort the whole sweep.
    ///
    /// Two persistent subscriptions (A, B) share a capacity-1 template —
    /// the batched booked-count snapshot #341 introduced is taken ONCE, up
    /// front, for the whole tick, so BOTH candidates read it as "not full"
    /// even though only one INSERT can actually win the atomic capacity
    /// check. The loser gets a genuine `DbError::ClassFull` from
    /// `create_booking()` — exactly the race the issue describes (there, an
    /// external `/api/classes` request racing the snapshot; here, an
    /// earlier candidate in the very same tick, which is deterministic and
    /// needs no real concurrency to reproduce). A third, unrelated
    /// persistent subscription (C) on a DIFFERENT template proves the
    /// sweep kept going instead of aborting on the loser's error.
    #[tokio::test]
    async fn sweep_continues_past_a_classfull_race_instead_of_aborting_the_whole_tick() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let today = crate::util::today_bratislava();
        let weekday = today.weekday().num_days_from_monday() as i64;

        // Capacity 1 — two persistents due on it guarantee exactly one
        // ClassFull race per matching date.
        let tid_full = crate::db::classes::create_template(&pool, weekday, "06:00", 45, None, 1)
            .await
            .unwrap();
        // Capacity 5, unrelated — proves the sweep didn't abort.
        let tid_other = crate::db::classes::create_template(&pool, weekday, "07:00", 45, None, 5)
            .await
            .unwrap();

        let user_a: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('a@x','a') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let user_b: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('b@x','b') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let user_c: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('c@x','c') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();

        crate::db::persistent_bookings::create(&pool, user_a, tid_full)
            .await
            .unwrap();
        crate::db::persistent_bookings::create(&pool, user_b, tid_full)
            .await
            .unwrap();
        crate::db::persistent_bookings::create(&pool, user_c, tid_other)
            .await
            .unwrap();

        // Today's weekday recurs at offsets 0, 7 and 14 inside the 15-day
        // (0..=WINDOW_DAYS) window — 3 distinct matching dates. Per date on
        // tid_full: exactly one of {A, B} wins the atomic INSERT, the other
        // gets ClassFull. tid_other has no capacity contention at all.
        let made = sweep(&pool)
            .await
            .expect("a ClassFull race must not abort the whole sweep (#345)");
        assert_eq!(
            made, 6,
            "3 winners on the capacity-1 template (one per matching date) + \
             3 bookings for user C on the unrelated template"
        );

        let full_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM bookings WHERE template_id=? AND cancelled_at IS NULL",
        )
        .bind(tid_full)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            full_count, 3,
            "capacity-1 template must end up with exactly one booking per matching date \
             (capacity respected, and not zero — proving the sweep didn't abort)"
        );

        let other_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM bookings WHERE template_id=? AND user_id=? \
             AND cancelled_at IS NULL",
        )
        .bind(tid_other)
        .bind(user_c)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            other_count, 3,
            "user C's bookings on the unrelated template must all be created — a \
             ClassFull race on the OTHER template must never suppress materialisation \
             for everyone else this tick (#345)"
        );
    }
}
