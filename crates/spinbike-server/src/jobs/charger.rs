//! T-4h charger: for every uncharged, non-cancelled booking whose
//! start-time is within 4 hours of now, create a transaction (amount 0 if
//! the card has an active monthly pass; else debit the Spinning service
//! price from card credit — negative credit is allowed) and stamp the
//! booking with charged_at + charge_transaction_id.

use anyhow::Result;
use sqlx::SqlitePool;

pub async fn tick(pool: &SqlitePool) -> Result<usize> {
    // Gym-local wall clock (Europe/Bratislava), NOT `chrono::Local` — the
    // charging window is a gym-local concept and must not depend on the server
    // process's OS/TZ configuration (#205). `now_bratislava()` derives it from
    // the named IANA zone, so DST is handled automatically.
    let now_s = crate::util::now_bratislava()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    tick_as_of(pool, &now_s).await
}

pub async fn tick_as_of(pool: &SqlitePool, now_s: &str) -> Result<usize> {
    // Find bookings whose start_time <= now + 4h, not cancelled, not charged.
    let rows: Vec<(i64, i64, String, String, i64)> = sqlx::query_as(
        "SELECT b.id, b.template_id, b.date, t.start_time, b.user_id
         FROM bookings b
         JOIN class_templates t ON t.id = b.template_id
         WHERE b.cancelled_at IS NULL
           AND b.charged_at IS NULL
           AND datetime(b.date || ' ' || t.start_time, '-4 hours') <= datetime(?)",
    )
    .bind(now_s)
    .fetch_all(pool)
    .await?;

    // #329: identify the Spinning service by its stable `kind`, not by its
    // (admin-editable) `name_en` — renaming Spinning via the admin Services
    // tab must not make this lookup miss and the whole tick error out.
    let (service_id, price): (i64, f64) =
        sqlx::query_as("SELECT id, default_price FROM services WHERE kind = ?1 AND active = 1")
            .bind(spinbike_core::services::SPINNING_KIND)
            .fetch_one(pool)
            .await?;
    // Round ONCE, right here where `price` enters the operation, and reuse
    // this SAME rounded value for both the ledger `transactions.amount`
    // INSERT and the `users.credit` UPDATE below — `default_price` carries
    // no rounding guarantee at rest (money-rounding.md / #325/#326/#343).
    let price = crate::db::users::round_cents(price);

    let mut charged = 0usize;
    for (booking_id, _template_id, date, _start, user_id) in rows {
        let mut tx = pool.begin().await?;

        // Double-check nothing else charged it in between.
        let still_open: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM bookings WHERE id = ? AND charged_at IS NULL AND cancelled_at IS NULL",
        )
        .bind(booking_id)
        .fetch_optional(&mut *tx)
        .await?;
        if still_open.is_none() {
            tx.rollback().await?;
            continue;
        }

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
        // involved. The gym-local-day boundary that #205 fixes therefore does
        // not touch this comparison; it only affects the "today"-relative pass
        // checks (door, my_balance, log_visit) which now use
        // `util::today_bratislava()`.
        let has_pass = match &pass_valid_until {
            Some(s) => s.as_str() >= date.as_str(),
            None => false,
        };

        // #365: no active pass covering the booking day, but if the customer
        // has EVER held a monthly pass, auto-renew it at the price of the last
        // one (anchored to the booking day + 1 month) instead of charging the
        // Spinning single-visit price. The fresh pass then covers THIS class (a
        // €0 visit) and the customer's credit goes negative until they settle
        // up. A customer who has NEVER held a pass keeps the Spinning charge
        // (auto_renew_pass returns None). `renewed` gates the credit debit
        // below: the auto-renew path already debited the pass price itself, so
        // only the genuine single-visit charge (case 3) debits `price` here.
        let (amount, renewed) = if has_pass {
            (0.0, false)
        } else {
            // The booking `date` is already a gym-local YYYY-MM-DD calendar
            // date (no tz conversion — bratislava-tz.md), so parsing it to a
            // NaiveDate anchor is pure.
            let anchor = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")?;
            match crate::db::users::auto_renew_pass(&mut tx, user_id, anchor).await? {
                Some(_new_credit) => (0.0, true),
                None => (-price, false),
            }
        };

        // Log the money-relevant decision for every charged booking so a
        // "free visit" dispute can be reconstructed from logs alone.
        tracing::debug!(
            user_id,
            booking_id,
            as_of_date = %date,
            pass_valid_until = ?pass_valid_until,
            has_pass,
            renewed,
            credit,
            amount,
            "charger: pass decision"
        );

        let txn_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, staff_id, service_id, amount, action)
             VALUES (?, NULL, ?, ?, 'visit') RETURNING id",
        )
        .bind(user_id)
        .bind(service_id)
        .bind(amount)
        .fetch_one(&mut *tx)
        .await?;

        // Debit the Spinning price ONLY when we actually charged the single
        // visit — i.e. no active pass AND no prior pass to auto-renew. The
        // auto-renew path (`renewed`) already debited the pass price inside
        // `auto_renew_pass`, so debiting `price` again here would double-charge.
        if !has_pass && !renewed {
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
        charged += 1;
    }
    Ok(charged)
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

    /// Fake "now" of Monday 14:00 (= class_start - 4h, boundary inclusive).
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

    /// Exercises the real-time `tick()` wrapper (not `tick_as_of`). Creates a
    /// short-lead-time template scheduled 30 minutes from now; two bookings on
    /// it must both get charged in a single call, proving `tick` actually
    /// delegates to `tick_as_of(now)` and returns the real count.
    #[tokio::test]
    async fn tick_uses_real_now_and_returns_count() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let now = crate::util::now_bratislava();
        let today = now.date();
        let weekday = today.weekday().num_days_from_monday() as i64;
        // 30 minutes from now — always inside the 4h window but unlikely to
        // collide with V6's 18:00 seed (which, if matched, just yields a
        // distinct template id with no bookings of its own).
        let soon = (now + chrono::Duration::minutes(30))
            .format("%H:%M")
            .to_string();
        let tid = crate::db::classes::create_template(&pool, weekday, &soon, 60, None, 10)
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
            crate::db::classes::create_booking(&pool, tid, &today.to_string(), uid, None, "manual")
                .await
                .unwrap();
        }

        let n = tick(&pool).await.unwrap();
        assert_eq!(n, 2, "tick() must charge all imminent bookings");
    }

    #[tokio::test]
    async fn charger_skips_far_future() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_cid, _bid) = seed_booking(&pool, true, 0.0).await;
        let today = crate::util::today_bratislava();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + Duration::days(days_to_mon);
        // 10:00 is 8 hours before 18:00, outside the 4h window.
        let n = tick_as_of(&pool, &format!("{mon} 10:00:00")).await.unwrap();
        assert_eq!(n, 0);
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

    /// #365: a booking whose customer's monthly pass has EXPIRED (no longer
    /// covers the booking date) but who HAS held a pass before must AUTO-RENEW
    /// the pass at the price of the last one, anchored to the booking day + 1
    /// month, instead of charging the Spinning single-visit price. The class
    /// visit then becomes a €0 pass-covered visit and the customer's credit
    /// goes negative by the pass price (not the Spinning price). A customer who
    /// never held a pass keeps the Spinning charge (covered by
    /// `charger_debits_credit_without_pass`); a VOIDED pass does NOT renew
    /// (covered by `charger_charges_when_pass_is_voided` — the view excludes
    /// it, so auto_renew_pass returns None).
    #[tokio::test]
    async fn charger_auto_renews_expired_pass_instead_of_charging_spinning() {
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
        // A prior pass sold for 35.0 that expired yesterday — it no longer
        // covers the (future) booking date, so has_pass=false, but it is still
        // the user's newest non-voided pass, so auto_renew reads its price.
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

        // The class visit is €0 (covered by the fresh pass), NOT a -5 charge.
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
        assert_eq!(
            amount, 0.0,
            "the class visit must be €0 — covered by the auto-renewed pass, not a Spinning charge"
        );

        // Credit debited by the RENEWED PASS price (35), not Spinning (5): 10 -> -25.
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            (credit - (-25.0)).abs() < 0.01,
            "expected credit -25.0 after auto-renewal at the pass price (10 - 35), got {credit}"
        );

        // A fresh auto-renewal pass row: staff_id NULL, note 'auto-obnova',
        // valid_until anchored to the booking day + 1 month.
        let expected_until = mon
            .checked_add_months(chrono::Months::new(1))
            .unwrap()
            .to_string();
        let renew: (f64, Option<i64>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT amount, staff_id, note, date(valid_until) FROM transactions \
             WHERE user_id = ? AND note = 'auto-obnova'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            (renew.0 - (-35.0)).abs() < 0.001,
            "renewal amount {} != -35",
            renew.0
        );
        assert_eq!(renew.1, None, "auto-renewal must have staff_id NULL");
        assert_eq!(renew.2.as_deref(), Some("auto-obnova"));
        assert_eq!(
            renew.3.as_deref(),
            Some(expected_until.as_str()),
            "charger anchors the renewed pass to the booking day + 1 month"
        );
    }

    /// #372: a monthly pass that expired MORE than 31 days ago must NOT
    /// auto-renew on a class charge either — the charger falls back to the
    /// Spinning single-visit charge, same recency gate as the door path (shared
    /// helper). RED against pre-#372.
    #[tokio::test]
    async fn charger_does_not_auto_renew_pass_expired_over_31_days_ago() {
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
        // A prior pass expired 60 days ago — outside the 31-day recency window.
        sqlx::query(
            "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, valid_until)
             VALUES (?, NULL, ?, -35.0, 'charge', date('now','-60 days'))",
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
            "a >31-day-expired pass must NOT auto-renew in the charger"
        );

        // The class visit is the Spinning single charge, not a EUR0 renewed visit.
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
