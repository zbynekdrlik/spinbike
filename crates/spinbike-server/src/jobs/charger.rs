//! T-4h charger: for every uncharged, non-cancelled booking whose
//! start-time is within 4 hours of now, create a transaction (amount 0 if
//! the card has an active monthly pass; else debit the Spinning service
//! price from card credit — negative credit is allowed) and stamp the
//! booking with charged_at + charge_transaction_id.

use anyhow::Result;
use sqlx::SqlitePool;

pub async fn tick(pool: &SqlitePool) -> Result<usize> {
    let now_s = chrono::Local::now()
        .naive_local()
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

    let (service_id, price): (i64, f64) =
        sqlx::query_as("SELECT id, default_price FROM services WHERE name_en = ?1 AND active = 1")
            .bind(spinbike_core::services::SPINNING_NAME_EN)
            .fetch_one(pool)
            .await?;

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

        // Load user state and latest pass valid_until (may be NULL).
        // `date(valid_until)` coerces legacy datetime strings to YYYY-MM-DD so
        // the lexicographic string comparison below stays correct even if an
        // importer ever stores a time component.
        let (_credit, pass_valid_until): (f64, Option<String>) = sqlx::query_as(
            "SELECT u.credit,
                    (SELECT MAX(date(valid_until)) FROM transactions
                     WHERE user_id = u.id AND valid_until IS NOT NULL)
             FROM users u WHERE u.id = ?",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        let has_pass = match &pass_valid_until {
            Some(s) => s.as_str() >= date.as_str(),
            None => false,
        };
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
        charged += 1;
    }
    Ok(charged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};
    use chrono::{Datelike, Duration, Local};

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

        let today = Local::now().date_naive();
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
        let today = Local::now().date_naive();
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

        let now = chrono::Local::now();
        let today = now.date_naive();
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
        let today = Local::now().date_naive();
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
        let today = Local::now().date_naive();
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
}
