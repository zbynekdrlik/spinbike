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
    // Find bookings whose start_time <= now + 4h, not cancelled, not charged, and linked to a card.
    let rows: Vec<(i64, i64, String, String, i64)> = sqlx::query_as(
        "SELECT b.id, b.template_id, b.date, t.start_time, b.card_id
         FROM bookings b
         JOIN class_templates t ON t.id = b.template_id
         WHERE b.cancelled_at IS NULL
           AND b.charged_at IS NULL
           AND b.card_id IS NOT NULL
           AND datetime(b.date || ' ' || t.start_time, '-4 hours') <= datetime(?)",
    )
    .bind(now_s)
    .fetch_all(pool)
    .await?;

    let (service_id, price): (i64, f64) = sqlx::query_as(
        "SELECT id, default_price FROM services WHERE name = 'Spinning' AND active = 1",
    )
    .fetch_one(pool)
    .await?;

    let mut charged = 0usize;
    for (booking_id, _template_id, date, _start, card_id) in rows {
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

        // Load card state and latest pass valid_until (may be NULL).
        let (user_id, _credit, pass_valid_until): (Option<i64>, f64, Option<String>) =
            sqlx::query_as(
                "SELECT c.user_id, c.credit,
                    (SELECT MAX(valid_until) FROM transactions
                     WHERE card_id = c.id AND valid_until IS NOT NULL)
             FROM cards c WHERE c.id = ?",
            )
            .bind(card_id)
            .fetch_one(&mut *tx)
            .await?;

        let has_pass = match &pass_valid_until {
            Some(s) => s.as_str() >= date.as_str(),
            None => false,
        };
        let amount = if has_pass { 0.0 } else { -price };

        let txn_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action)
             VALUES (?, ?, NULL, ?, ?, 'visit') RETURNING id",
        )
        .bind(user_id)
        .bind(card_id)
        .bind(service_id)
        .bind(amount)
        .fetch_one(&mut *tx)
        .await?;

        if !has_pass {
            sqlx::query("UPDATE cards SET credit = ROUND(credit - ?, 2) WHERE id = ?")
                .bind(price)
                .bind(card_id)
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

    /// Returns (card_id, booking_id) for a card booked for the nearest Monday
    /// at 18:00 (V6-seeded template). If `pass` is true, a pass transaction is
    /// inserted with valid_until 30 days in the future.
    async fn seed_booking(pool: &SqlitePool, pass: bool, credit: f64) -> (i64, i64) {
        let uid: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
                .fetch_one(pool)
                .await
                .unwrap();
        let cid: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B', ?, ?) RETURNING id",
        )
        .bind(uid)
        .bind(credit)
        .fetch_one(pool)
        .await
        .unwrap();
        if pass {
            let svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE name='Monthly pass'")
                .fetch_one(pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO transactions (user_id, card_id, service_id, amount, action, valid_until)
                 VALUES (?, ?, ?, -35.0, 'charge', date('now','+30 days'))",
            )
            .bind(uid)
            .bind(cid)
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

        let bid = crate::db::classes::create_booking(
            pool,
            tid,
            &mon.to_string(),
            uid,
            Some(cid),
            None,
            "manual",
        )
        .await
        .unwrap();
        (cid, bid)
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
        let (cid, bid) = seed_booking(&pool, true, 0.0).await;
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
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
            .bind(cid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 0.0, "pass should not touch credit");
    }

    #[tokio::test]
    async fn charger_debits_credit_without_pass() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, bid) = seed_booking(&pool, false, 10.0).await;
        tick_as_of(&pool, &now_at_14()).await.unwrap();
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
            .bind(cid)
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
        let (cid, _) = seed_booking(&pool, false, 2.0).await;
        tick_as_of(&pool, &now_at_14()).await.unwrap();
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM cards WHERE id = ?")
            .bind(cid)
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
            let uid: i64 =
                sqlx::query_scalar("INSERT INTO users (email, name) VALUES (?, 'u') RETURNING id")
                    .bind(format!("u{i}@x"))
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let cid: i64 = sqlx::query_scalar(
                "INSERT INTO cards (barcode, user_id, credit) VALUES (?, ?, 10.0) RETURNING id",
            )
            .bind(format!("B{i}"))
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
            crate::db::classes::create_booking(
                &pool,
                tid,
                &today.to_string(),
                uid,
                Some(cid),
                None,
                "manual",
            )
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
}
