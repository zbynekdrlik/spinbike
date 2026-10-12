//! Binding charger (#382 R1): every uncharged, non-cancelled booking becomes
//! binding at 09:00 Europe/Bratislava ON its class day and is charged from
//! then on (a booking created after 09:00 that day is charged on the next
//! 60-second tick). Replaces the old "4 hours before the start" rule. The
//! charge is a transaction row (amount 0 if the customer's active monthly
//! pass covers the class day; else the Spinning price debited from credit —
//! negative credit is allowed) and the booking is stamped with `charged_at`
//! + `charge_transaction_id`. Bookings on a class occurrence the staff
//! cancelled (`class_cancellations`) are never charged — the class did not
//! happen.

use anyhow::Result;
use sqlx::SqlitePool;

pub async fn tick(pool: &SqlitePool) -> Result<usize> {
    // Gym-local wall clock (Europe/Bratislava), NOT `chrono::Local` — the
    // 09:00 cutoff is a gym-local concept and must not depend on the server
    // process's OS/TZ configuration (#205). `now_bratislava()` derives it from
    // the named IANA zone, so DST is handled automatically.
    let now_s = crate::util::now_bratislava()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    tick_as_of(pool, &now_s).await
}

/// The Spinning service id and its price, rounded ONCE right here where the
/// price enters the operation and reused for both the ledger
/// `transactions.amount` INSERT and the `users.credit` UPDATE —
/// `default_price` carries no rounding guarantee at rest (money-rounding.md
/// / #325/#326/#343). #329: identified by its stable `kind`, never by the
/// admin-editable `name_en`.
async fn spinning_price(pool: &SqlitePool) -> Result<(i64, f64)> {
    let (service_id, price): (i64, f64) =
        sqlx::query_as("SELECT id, default_price FROM services WHERE kind = ?1 AND active = 1")
            .bind(spinbike_core::services::SPINNING_KIND)
            .fetch_one(pool)
            .await?;
    Ok((service_id, crate::db::users::round_cents(price)))
}

/// One charger pass as of the gym-local `now_s` (`YYYY-MM-DD HH:MM:SS`).
pub async fn tick_as_of(pool: &SqlitePool, now_s: &str) -> Result<usize> {
    // Due = the class day's binding cutoff (`date` + 09:00:00, the same
    // constant `booking_rules::charge_cutoff` uses) is at or before now, the
    // booking is still open, and its class occurrence was not cancelled.
    let due: Vec<i64> = sqlx::query_scalar(
        "SELECT b.id
         FROM bookings b
         WHERE b.cancelled_at IS NULL
           AND b.charged_at IS NULL
           AND datetime(b.date || ' ' || ?1) <= datetime(?2)
           AND NOT EXISTS (SELECT 1 FROM class_cancellations cc
                            WHERE cc.template_id = b.template_id AND cc.date = b.date)
         ORDER BY b.date, b.id",
    )
    .bind(crate::booking_rules::CHARGE_CUTOFF_TIME)
    .bind(now_s)
    .fetch_all(pool)
    .await?;
    if due.is_empty() {
        return Ok(0);
    }

    let (service_id, price) = spinning_price(pool).await?;
    let mut charged = 0usize;
    for booking_id in due {
        if charge_booking(pool, booking_id, service_id, price).await? {
            charged += 1;
        }
    }
    tracing::info!(charged, as_of = %now_s, "charger: binding charges written");
    Ok(charged)
}

/// Charge ONE booking right now, regardless of the clock — the test-only
/// `/api/test/charge-booking` fixture uses it so an E2E spec can produce a
/// charged booking without running the whole charger against a shared DB.
/// Returns whether it was charged (false when already charged/cancelled).
pub async fn charge_one(pool: &SqlitePool, booking_id: i64) -> Result<bool> {
    let (service_id, price) = spinning_price(pool).await?;
    charge_booking(pool, booking_id, service_id, price).await
}

/// Write the binding charge for one booking in ONE transaction. `price` is
/// the already-rounded Spinning price. Returns `false` (nothing written)
/// when the booking is no longer open — charged or cancelled in between.
async fn charge_booking(
    pool: &SqlitePool,
    booking_id: i64,
    service_id: i64,
    price: f64,
) -> Result<bool> {
    let mut tx = pool.begin().await?;

    // Double-check nothing else charged/cancelled it in between.
    let open: Option<(i64, String)> = sqlx::query_as(
        "SELECT user_id, date FROM bookings
         WHERE id = ? AND charged_at IS NULL AND cancelled_at IS NULL",
    )
    .bind(booking_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id, date)) = open else {
        tx.rollback().await?;
        tracing::debug!(booking_id, "charger: skip — booking no longer open");
        return Ok(false);
    };

    // Load user state and the expiry of the user's active monthly pass, if
    // any. The pass is resolved through the canonical `user_active_pass`
    // view (migration V18) — the SINGLE definition of "active pass" shared
    // with my_balance, the user lists and get_user_pass_*. Crucially the
    // view filters `deleted_at IS NULL`, so a VOIDED pass no longer reads as
    // active here (the money defect fixed by #159: previously a voided pass
    // gave a free visit with no credit debit). `date(valid_until)` coerces
    // any legacy datetime string to YYYY-MM-DD so the comparison against the
    // (already YYYY-MM-DD) booking date is a consistent calendar-date
    // compare, never a raw datetime string compare.
    let (credit, pass_valid_until): (f64, Option<String>) = sqlx::query_as(
        "SELECT u.credit,
                (SELECT date(ap.valid_until) FROM user_active_pass ap
                 WHERE ap.user_id = u.id)
         FROM users u WHERE u.id = ?",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    // A pass covers a booking when it is still valid ON the booking's date
    // (valid_until is inclusive of the last day). Both sides are bare
    // `YYYY-MM-DD` calendar dates already fixed in gym-local terms — the
    // pass's `valid_until` and the class booking's own `date` — so this is
    // an apples-to-apples calendar-date compare with NO "today"/`date('now')`
    // involved.
    let has_pass = match &pass_valid_until {
        Some(s) => s.as_str() >= date.as_str(),
        None => false,
    };

    // An active pass covering the booking day → €0 visit; otherwise the
    // Spinning single-visit charge. #374 REMOVED visit-triggered
    // auto-renewal (was #365): a class charge never renews a pass anymore.
    let amount = if has_pass { 0.0 } else { -price };

    let txn_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, staff_id, service_id, amount, action)
         VALUES (?, NULL, ?, ?, 'visit') RETURNING id",
    )
    .bind(user_id)
    .bind(service_id)
    .bind(amount)
    .fetch_one(&mut *tx)
    .await?;

    // Debit the Spinning price only when no active pass covered the booking.
    if !has_pass {
        sqlx::query("UPDATE users SET credit = ROUND(credit - ?, 2) WHERE id = ?")
            .bind(price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query(
        "UPDATE bookings SET charged_at = datetime('now'), charge_transaction_id = ? WHERE id = ?",
    )
    .bind(txn_id)
    .bind(booking_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Log the money-relevant decision for every charged booking so a "free
    // visit" or "why was I charged" dispute can be reconstructed from logs
    // alone.
    tracing::info!(
        user_id,
        booking_id,
        class_date = %date,
        pass_valid_until = ?pass_valid_until,
        has_pass,
        credit_before = credit,
        amount,
        transaction_id = txn_id,
        "charger: booking charged"
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};
    use chrono::{Datelike, Duration};

    /// Returns (user_id, booking_id) for a user booked for the nearest Monday
    /// at 18:00 (V6-seeded template). If `pass` is true, a pass transaction is
    /// inserted with valid_until 30 days in the future.
    async fn seed_booking(pool: &SqlitePool, pass: bool, credit: f64) -> (i64, i64) {
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, credit) VALUES ('u@x','u',?) RETURNING id",
        )
        .bind(credit)
        .fetch_one(pool)
        .await
        .unwrap();
        if pass {
            let svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
                .fetch_one(pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO transactions (user_id, service_id, amount, action, valid_until)
                 VALUES (?, ?, -35.0, 'charge', date('now','+30 days'))",
            )
            .bind(uid)
            .bind(svc)
            .execute(pool)
            .await
            .unwrap();
        }
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'",
        )
        .fetch_one(pool)
        .await
        .unwrap();

        let today = crate::util::today_bratislava();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + Duration::days(days_to_mon);

        let bid =
            crate::db::classes::create_booking(pool, tid, &mon.to_string(), uid, None, "manual")
                .await
                .unwrap();
        (uid, bid)
    }

    /// The Monday `seed_booking` books (today when today is a Monday).
    fn next_monday() -> chrono::NaiveDate {
        let today = crate::util::today_bratislava();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        today + Duration::days(days_to_mon)
    }

    /// Fake "now" of Monday 14:00 (after the class day's 09:00 binding
    /// cutoff, #382 — previously the 4h-before boundary).
    fn now_at_14() -> String {
        let today = crate::util::today_bratislava();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + Duration::days(days_to_mon);
        format!("{mon} 14:00:00")
    }

    #[tokio::test]
    async fn charger_free_when_pass_active() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, bid) = seed_booking(&pool, true, 0.0).await;
        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 1);
        let (charged_at, txn_id): (Option<String>, Option<i64>) =
            sqlx::query_as("SELECT charged_at, charge_transaction_id FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(charged_at.is_some());
        let amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
            .bind(txn_id.unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(amount, 0.0);
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 0.0, "pass should not touch credit");
    }

    #[tokio::test]
    async fn charger_debits_credit_without_pass() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, bid) = seed_booking(&pool, false, 10.0).await;
        tick_as_of(&pool, &now_at_14()).await.unwrap();
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 5.0);
        // Amount on the transaction must be NEGATIVE (debit), not positive.
        let txn_id: i64 =
            sqlx::query_scalar("SELECT charge_transaction_id FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        let amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
            .bind(txn_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(amount < 0.0, "charge amount must be negative (debit)");
        assert_eq!(amount, -5.0);
    }

    #[tokio::test]
    async fn charger_allows_negative_credit() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, _) = seed_booking(&pool, false, 2.0).await;
        tick_as_of(&pool, &now_at_14()).await.unwrap();
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, -3.0);
    }

    #[tokio::test]
    async fn charger_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_cid, _bid) = seed_booking(&pool, true, 0.0).await;
        let a = tick_as_of(&pool, &now_at_14()).await.unwrap();
        let b = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(a, 1);
        assert_eq!(b, 0);
    }

    #[tokio::test]
    async fn charger_is_idempotent_on_credit_debit_path() {
        // The pass-active idempotency test above exercises the `amount = 0`
        // branch, which doesn't touch `cards.credit`. This one pins the
        // credit-debit branch: a second tick must NOT re-debit, and the
        // transactions table must contain exactly one charge row.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, _bid) = seed_booking(&pool, false, 10.0).await;

        let a = tick_as_of(&pool, &now_at_14()).await.unwrap();
        let b = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(a, 1);
        assert_eq!(b, 0);

        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 5.0, "credit must be debited only once (10 -> 5)");

        let visit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transactions WHERE user_id = ? AND action = 'visit'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(visit_count, 1, "exactly one visit transaction expected");
    }

    #[tokio::test]
    async fn charger_skips_cancelled() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_cid, bid) = seed_booking(&pool, true, 0.0).await;
        sqlx::query("UPDATE bookings SET cancelled_at = datetime('now') WHERE id = ?")
            .bind(bid)
            .execute(&pool)
            .await
            .unwrap();
        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 0);
    }

    /// Exercises the real-time `tick()` wrapper (not `tick_as_of`). Two
    /// bookings on YESTERDAY's occurrence of a fresh template are past the
    /// class day's 09:00 binding cutoff whatever the real time of day is
    /// (#382 — the old "class in 30 minutes" setup depended on the 4h rule
    /// and would be time-of-day dependent under the 09:00 rule), so a single
    /// `tick()` must charge both, proving it delegates to `tick_as_of(now)`
    /// and returns the real count.
    #[tokio::test]
    async fn tick_uses_real_now_and_returns_count() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let yesterday = crate::util::today_bratislava() - Duration::days(1);
        let weekday = yesterday.weekday().num_days_from_monday() as i64;
        let tid = crate::db::classes::create_template(&pool, weekday, "07:15", 60, None, 10)
            .await
            .unwrap();

        for i in 0..2 {
            let uid: i64 = sqlx::query_scalar(
                "INSERT INTO users (email, name, credit) VALUES (?, 'u', 10.0) RETURNING id",
            )
            .bind(format!("u{i}@x"))
            .fetch_one(&pool)
            .await
            .unwrap();
            crate::db::classes::create_booking(
                &pool,
                tid,
                &yesterday.to_string(),
                uid,
                None,
                "manual",
            )
            .await
            .unwrap();
        }

        let n = tick(&pool).await.unwrap();
        assert_eq!(n, 2, "tick() must charge every booking past its cutoff");
    }

    #[tokio::test]
    async fn charger_skips_far_future() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_cid, _bid) = seed_booking(&pool, true, 0.0).await;
        let today = crate::util::today_bratislava();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + Duration::days(days_to_mon);
        // #382: the binding charge happens at 09:00 ON the class day, so the
        // day before (outside the old 4h window too) is never charged.
        let day_before = mon - Duration::days(1);
        let n = tick_as_of(&pool, &format!("{day_before} 10:00:00"))
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    /// #382 R1 RED: the binding charge fires at 09:00 Europe/Bratislava on
    /// the class day — not 4 hours before the start. Before the fix a Monday
    /// 18:00 booking was only charged from 14:00, so 09:00 charged nothing.
    #[tokio::test]
    async fn charger_charges_at_0900_on_the_class_day() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, bid) = seed_booking(&pool, false, 10.0).await;
        let n = tick_as_of(&pool, &format!("{} 09:00:00", next_monday()))
            .await
            .unwrap();
        assert_eq!(n, 1, "09:00 on the class day is the binding charge moment");
        let charged_at: Option<String> =
            sqlx::query_scalar("SELECT charged_at FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(charged_at.is_some());
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 5.0);
    }

    /// #382 R1: one second before 09:00 on the class day the customer may
    /// still cancel, so nothing is charged yet.
    #[tokio::test]
    async fn charger_does_not_charge_before_0900_on_the_class_day() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_uid, _bid) = seed_booking(&pool, false, 10.0).await;
        let n = tick_as_of(&pool, &format!("{} 08:59:59", next_monday()))
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    /// #382 R1: a booking created on the class day AFTER 09:00 is charged on
    /// the very next tick (the cutoff already passed when it was created).
    #[tokio::test]
    async fn charger_charges_a_booking_created_after_0900_on_the_next_tick() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let mon = next_monday();
        // First tick at 10:00 with nothing booked yet.
        assert_eq!(
            tick_as_of(&pool, &format!("{mon} 10:00:00")).await.unwrap(),
            0
        );
        // The customer books at 10:00:30; the next tick (one minute later)
        // must charge it immediately.
        let (_uid, bid) = seed_booking(&pool, false, 10.0).await;
        assert_eq!(
            tick_as_of(&pool, &format!("{mon} 10:01:00")).await.unwrap(),
            1
        );
        let charged_at: Option<String> =
            sqlx::query_scalar("SELECT charged_at FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(charged_at.is_some());
    }

    /// #382 RED: a booking on a class occurrence the staff CANCELLED
    /// (`class_cancellations`) must never be charged — the class did not
    /// happen. Before the fix the charger ignored `class_cancellations`
    /// entirely and billed every booked customer of a cancelled class.
    #[tokio::test]
    async fn charger_skips_a_cancelled_class_occurrence() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, bid) = seed_booking(&pool, false, 10.0).await;
        let tid: i64 = sqlx::query_scalar("SELECT template_id FROM bookings WHERE id = ?")
            .bind(bid)
            .fetch_one(&pool)
            .await
            .unwrap();
        crate::db::classes::cancel_occurrence(
            &pool,
            tid,
            &next_monday().to_string(),
            Some("instructor ill"),
            None,
        )
        .await
        .unwrap();

        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 0, "a cancelled class occurrence must not be charged");
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 10.0);
    }

    /// Regression test for #159 (real money defect): the charger's OLD
    /// predicate (`valid_until IS NOT NULL`, no `deleted_at` filter) still
    /// treated a VOIDED monthly pass as active, so a visit against a voided
    /// pass wrote amount=0 and skipped the credit debit — a free visit the
    /// customer should have paid for. After the fix, the charger resolves the
    /// pass through the canonical `user_active_pass` view (migration V18),
    /// which excludes voided rows, so a voided pass must be CHARGED like any
    /// other uncovered visit.
    #[tokio::test]
    async fn charger_charges_when_pass_is_voided() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, credit) VALUES ('u@x','u',?) RETURNING id",
        )
        .bind(10.0)
        .fetch_one(&pool)
        .await
        .unwrap();
        let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let pass_tx_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, service_id, amount, action, valid_until)
             VALUES (?, ?, -35.0, 'charge', date('now','+30 days')) RETURNING id",
        )
        .bind(uid)
        .bind(pass_svc)
        .fetch_one(&pool)
        .await
        .unwrap();

        // Void the pass — sets deleted_at, KEEPS valid_until (the real void
        // path: db::transactions::soft_delete, same as the staff-facing
        // void_transaction handler).
        crate::db::transactions::soft_delete(&pool, pass_tx_id)
            .await
            .unwrap();

        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let today = crate::util::today_bratislava();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + Duration::days(days_to_mon);
        let bid =
            crate::db::classes::create_booking(&pool, tid, &mon.to_string(), uid, None, "manual")
                .await
                .unwrap();

        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 1);

        let (charged_at, txn_id): (Option<String>, Option<i64>) =
            sqlx::query_as("SELECT charged_at, charge_transaction_id FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(charged_at.is_some());

        let amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
            .bind(txn_id.unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            amount < 0.0,
            "a VOIDED pass must NOT exempt the visit charge — amount must be a debit, got {amount}"
        );
        assert_eq!(amount, -5.0, "Spinning default_price is 5.0");

        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            credit, 5.0,
            "credit must be debited when the pass is voided (10.0 - 5.0 price); a voided pass \
             must never produce a free visit"
        );
    }

    /// #343 RED: `charger.rs` must round `default_price` to cents exactly
    /// ONCE, right after it enters the operation, and reuse that SAME
    /// rounded value for BOTH the ledger `transactions.amount` INSERT and
    /// the `users.credit` debit — matching the #325/#326 pattern the rest
    /// of the codebase already follows (money-rounding.md). Before the fix,
    /// `tick_as_of` inserts the RAW `-price` into the ledger while wrapping
    /// the credit UPDATE in SQL `ROUND(credit - ?, 2)`, so a `default_price`
    /// carrying sub-cent float drift (e.g. left over from before admin.rs's
    /// own write-boundary fix, or any f64 JSON round-trip) produces a
    /// ledger row permanently out of sync with the rounded credit delta.
    /// This directly seeds an unrounded `default_price` via SQL (bypassing
    /// the admin.rs boundary entirely, simulating pre-existing drift) to
    /// prove the charger itself rounds regardless of what's already sitting
    /// in the `services` row.
    #[tokio::test]
    async fn charger_rounds_ledger_amount_same_as_credit_debit() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Simulate a Spinning price carrying float/precision drift.
        let unrounded_price = 12.301_f64;
        sqlx::query("UPDATE services SET default_price = ? WHERE kind = 'group_class'")
            .bind(unrounded_price)
            .execute(&pool)
            .await
            .unwrap();

        let (uid, bid) = seed_booking(&pool, false, 100.0).await;
        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 1);

        let txn_id: i64 =
            sqlx::query_scalar("SELECT charge_transaction_id FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        let amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
            .bind(txn_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();

        let expected_rounded = crate::db::users::round_cents(unrounded_price);
        assert!(
            (amount - (-expected_rounded)).abs() < 1e-9,
            "ledger amount must be rounded to cents like the credit debit, not the raw \
             unrounded default_price ({unrounded_price}): got {amount}, expected {}",
            -expected_rounded
        );
        let expected_credit = crate::db::users::round_cents(100.0 - expected_rounded);
        assert!(
            (credit - expected_credit).abs() < 1e-9,
            "credit must debit by the SAME rounded amount as the ledger row: got {credit}, \
             expected {expected_credit}"
        );
    }

    /// #329 RED: the charger must find "the Spinning service" by its stable
    /// `services.kind` handle, NOT by matching the mutable `name_en` display
    /// string. Renaming the Spinning row via the admin Services tab (a normal
    /// admin action — `PUT /api/admin/services/{id}` only guards
    /// name_sk/name_en non-empty, nothing stops a rename) must not break the
    /// 4-hour auto-charger. Before the fix, `tick_as_of` looks the price up
    /// with `WHERE name_en = 'Spinning'`, so a renamed row is invisible to
    /// it and `fetch_one` returns `RowNotFound` — the whole tick errors out
    /// and every booking in the batch silently stops getting charged.
    #[tokio::test]
    async fn charger_uses_spinning_kind_not_name_en() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Simulate an admin renaming Spinning via the Services tab — same
        // effect as `PUT /api/admin/services/{id}` with a new name_en, which
        // the route allows unconditionally (no special-casing for this row).
        sqlx::query("UPDATE services SET name_en = 'Renamed Class' WHERE name_sk = 'Spinning'")
            .execute(&pool)
            .await
            .unwrap();

        let (uid, bid) = seed_booking(&pool, false, 10.0).await;
        let n = tick_as_of(&pool, &now_at_14())
            .await
            .expect("charger must still find the renamed Spinning row by kind, not name_en");
        assert_eq!(n, 1, "the booking must still get charged after the rename");

        let (charged_at, txn_id): (Option<String>, Option<i64>) =
            sqlx::query_as("SELECT charged_at, charge_transaction_id FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(charged_at.is_some());
        let amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
            .bind(txn_id.unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(amount, -5.0, "must still charge the Spinning default_price");
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 5.0);
    }

    /// #374 (behavior change from #365): visit-triggered auto-renewal is REMOVED
    /// from the charger too. A customer whose pass expired (yesterday here) — so
    /// it no longer covers the booking date — gets the plain Spinning
    /// single-visit charge, and NO fresh pass row is written. Continuous renewal
    /// is the daily `jobs::pass_renewal` job's job now, gated on the per-user
    /// `auto_renew_pass` flag — never a class charge. A never-held-pass customer
    /// keeps the Spinning charge (covered by `charger_debits_credit_without_pass`).
    /// RED against pre-#374 (which auto-renewed here).
    #[tokio::test]
    async fn charger_charges_spinning_for_expired_pass_no_renewal() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, credit) VALUES ('u@x','u',10.0) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind='monthly_pass'")
            .fetch_one(&pool)
            .await
            .unwrap();
        // A prior pass sold for 35.0 that expired yesterday — still the newest
        // non-voided pass, but the charger no longer renews it (#374).
        sqlx::query(
            "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, valid_until)
             VALUES (?, NULL, ?, -35.0, 'charge', date('now','-1 day'))",
        )
        .bind(uid)
        .bind(pass_svc)
        .execute(&pool)
        .await
        .unwrap();

        // Book the nearest Monday 18:00 (same as seed_booking).
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let today = crate::util::today_bratislava();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + Duration::days(days_to_mon);
        let bid =
            crate::db::classes::create_booking(&pool, tid, &mon.to_string(), uid, None, "manual")
                .await
                .unwrap();

        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 1);

        // No auto-renewal row.
        let renew_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transactions WHERE user_id = ? AND note = 'auto-obnova'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            renew_count, 0,
            "a class charge must NOT auto-renew any pass (#374)"
        );

        // The class visit is the Spinning single charge (amount<0), not a €0
        // renewed visit.
        let txn_id: i64 =
            sqlx::query_scalar("SELECT charge_transaction_id FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        let amount: f64 = sqlx::query_scalar("SELECT amount FROM transactions WHERE id = ?")
            .bind(txn_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            amount < 0.0,
            "must charge the Spinning single-visit price, got {amount}"
        );

        // Credit dropped by the Spinning price (read from seed), not 35 (which
        // would leave -25).
        let sp_price: f64 =
            sqlx::query_scalar("SELECT default_price FROM services WHERE kind='group_class'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            (credit - (10.0 - sp_price)).abs() < 0.01,
            "expected Spinning charge (10 - {sp_price}), got {credit}"
        );
    }
}
