//! Persistent-booking materialiser: ensures a concrete booking row exists for
//! every future occurrence of an active persistent subscription within the
//! next 14 days. Skips occurrences where the class is full. Idempotent.

use anyhow::Result;
use chrono::{Datelike, Duration, Local};
use sqlx::SqlitePool;

pub const WINDOW_DAYS: i64 = 14;

pub async fn sweep(pool: &SqlitePool) -> Result<usize> {
    let persistents = crate::db::persistent_bookings::list_active_all(pool).await?;
    let templates = sqlx::query_as::<_, crate::db::classes::ClassTemplateRow>(
        "SELECT id, weekday, start_time, duration_minutes, instructor_id, capacity, active
         FROM class_templates WHERE active = 1",
    )
    .fetch_all(pool)
    .await?;

    let today = Local::now().date_naive();
    let mut created = 0usize;

    for p in &persistents {
        let Some(tpl) = templates.iter().find(|t| t.id == p.template_id) else {
            continue;
        };

        for offset in 0..=WINDOW_DAYS {
            let d = today + Duration::days(offset);
            if d.weekday().num_days_from_monday() as i64 != tpl.weekday {
                continue;
            }
            let date_s = d.to_string();

            // Skip cancelled classes.
            let cancelled: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM class_cancellations WHERE template_id = ? AND date = ?",
            )
            .bind(tpl.id)
            .bind(&date_s)
            .fetch_optional(pool)
            .await?;
            if cancelled.is_some() {
                continue;
            }

            // Skip if a booking already exists for this card (manual or persistent).
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM bookings
                 WHERE template_id = ? AND date = ? AND card_id = ? AND cancelled_at IS NULL",
            )
            .bind(tpl.id)
            .bind(&date_s)
            .bind(p.card_id)
            .fetch_optional(pool)
            .await?;
            if existing.is_some() {
                continue;
            }

            // Lookup the user_id linked to the card (legacy cards may have NULL).
            let user_id: Option<i64> = sqlx::query_scalar("SELECT user_id FROM cards WHERE id = ?")
                .bind(p.card_id)
                .fetch_one(pool)
                .await?;
            let Some(uid) = user_id else { continue };

            // Check capacity up front so we don't rely on string-matching the
            // create_booking error to detect a full class.
            let booked: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM bookings
                 WHERE template_id = ? AND date = ? AND cancelled_at IS NULL",
            )
            .bind(tpl.id)
            .bind(&date_s)
            .fetch_one(pool)
            .await?;
            if booked >= tpl.capacity {
                continue;
            }

            crate::db::classes::create_booking(
                pool,
                tpl.id,
                &date_s,
                uid,
                Some(p.card_id),
                None,
                "persistent",
            )
            .await?;
            created += 1;
        }
    }
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
        let cid: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B', ?, 0) RETURNING id",
        )
        .bind(uid)
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
        (cid, tid)
    }

    #[tokio::test]
    async fn sweep_materialises_future_bookings() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, tid) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, cid, tid)
            .await
            .unwrap();

        let made = sweep(&pool).await.unwrap();
        assert!(made >= 1, "at least one Monday in next 14 days");

        let sources: Vec<(String,)> =
            sqlx::query_as("SELECT source FROM bookings WHERE card_id = ?")
                .bind(cid)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(sources.iter().all(|(s,)| s == "persistent"));
    }

    #[tokio::test]
    async fn sweep_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, tid) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, cid, tid)
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
        let today = Local::now().date_naive();
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
            let cid: i64 = sqlx::query_scalar(
                "INSERT INTO cards (barcode, user_id, credit) VALUES (?, ?, 0) RETURNING id",
            )
            .bind(format!("B{n}"))
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
            crate::db::classes::create_booking(&pool, tid, &date_s, uid, Some(cid), None, "manual")
                .await
                .unwrap();
        }

        let (cid, _) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, cid, tid)
            .await
            .unwrap();

        let _ = sweep(&pool).await.unwrap();
        let got: Option<i64> =
            sqlx::query_scalar("SELECT id FROM bookings WHERE card_id=? AND date=?")
                .bind(cid)
                .bind(&date_s)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(got.is_none(), "must skip full class");
    }
}
