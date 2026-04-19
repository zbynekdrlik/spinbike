use anyhow::{Context, Result};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassTemplateRow {
    pub id: i64,
    pub weekday: i64,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub capacity: i64,
    pub active: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BookingRow {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub user_id: i64,
    pub created_by: Option<i64>,
    pub source: String,
    pub created_at: String,
    pub cancelled_at: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CancellationRow {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub reason: Option<String>,
    pub cancelled_by: Option<i64>,
    pub created_at: String,
}

pub async fn create_template(
    pool: &SqlitePool,
    weekday: i64,
    start_time: &str,
    duration_minutes: i64,
    instructor_id: Option<i64>,
    capacity: i64,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity)
         VALUES (?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(weekday)
    .bind(start_time)
    .bind(duration_minutes)
    .bind(instructor_id)
    .bind(capacity)
    .fetch_one(pool)
    .await
    .context("Failed to create template")?;
    Ok(id)
}

pub async fn list_active_templates(pool: &SqlitePool) -> Result<Vec<ClassTemplateRow>> {
    let templates = sqlx::query_as::<_, ClassTemplateRow>(
        "SELECT * FROM class_templates WHERE active = 1 ORDER BY weekday, start_time",
    )
    .fetch_all(pool)
    .await
    .context("Failed to list active templates")?;
    Ok(templates)
}

pub async fn list_all_templates(pool: &SqlitePool) -> Result<Vec<ClassTemplateRow>> {
    let templates = sqlx::query_as::<_, ClassTemplateRow>(
        "SELECT * FROM class_templates ORDER BY weekday, start_time",
    )
    .fetch_all(pool)
    .await
    .context("Failed to list all templates")?;
    Ok(templates)
}

pub async fn cancel_occurrence(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
    reason: Option<&str>,
    cancelled_by: Option<i64>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO class_cancellations (template_id, date, reason, cancelled_by)
         VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(template_id)
    .bind(date)
    .bind(reason)
    .bind(cancelled_by)
    .fetch_one(pool)
    .await
    .context("Failed to cancel occurrence")?;
    Ok(id)
}

pub async fn is_occurrence_cancelled(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
) -> Result<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM class_cancellations WHERE template_id = ? AND date = ?",
    )
    .bind(template_id)
    .bind(date)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn get_booking_count(pool: &SqlitePool, template_id: i64, date: &str) -> Result<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE template_id = ? AND date = ? AND cancelled_at IS NULL",
    )
    .bind(template_id)
    .bind(date)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Create a booking with atomic capacity enforcement via a single INSERT with subquery.
pub async fn create_booking(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
    user_id: i64,
    card_id: Option<i64>,
    created_by: Option<i64>,
    source: &str,
) -> Result<i64> {
    let result = sqlx::query_scalar::<_, i64>(
        "INSERT INTO bookings (template_id, date, user_id, card_id, created_by, source)
         SELECT ?1, ?2, ?3, ?4, ?5, ?6
         WHERE (SELECT COUNT(*) FROM bookings
                WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL)
               < (SELECT capacity FROM class_templates WHERE id = ?1)
         RETURNING id",
    )
    .bind(template_id)
    .bind(date)
    .bind(user_id)
    .bind(card_id)
    .bind(created_by)
    .bind(source)
    .fetch_optional(pool)
    .await?;
    match result {
        Some(id) => Ok(id),
        None => anyhow::bail!("Class is full"),
    }
}

pub async fn cancel_booking(pool: &SqlitePool, booking_id: i64) -> Result<()> {
    sqlx::query("UPDATE bookings SET cancelled_at = datetime('now') WHERE id = ?")
        .bind(booking_id)
        .execute(pool)
        .await
        .context("Failed to cancel booking")?;
    Ok(())
}

pub async fn list_bookings_for_class(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
) -> Result<Vec<BookingRow>> {
    let bookings = sqlx::query_as::<_, BookingRow>(
        "SELECT * FROM bookings WHERE template_id = ? AND date = ? AND cancelled_at IS NULL ORDER BY created_at",
    )
    .bind(template_id)
    .bind(date)
    .fetch_all(pool)
    .await
    .context("Failed to list bookings for class")?;
    Ok(bookings)
}

pub async fn list_user_bookings(pool: &SqlitePool, user_id: i64) -> Result<Vec<BookingRow>> {
    let bookings = sqlx::query_as::<_, BookingRow>(
        "SELECT * FROM bookings WHERE user_id = ? AND cancelled_at IS NULL AND date >= date('now') ORDER BY date, created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .context("Failed to list user bookings")?;
    Ok(bookings)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpcomingRow {
    pub template_id: i64,
    pub date: String,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub booked: i64,
    pub state: String, // "free" | "booked" | "auto" | "full" | "past" | "cancelled"
    pub booking_id: Option<i64>,
}

pub async fn list_upcoming_for_card(
    pool: &SqlitePool,
    card_id: i64,
    from: &str,
    to: &str,
) -> Result<Vec<UpcomingRow>> {
    use chrono::{Datelike, Duration, NaiveDate};
    let from_d = NaiveDate::parse_from_str(from, "%Y-%m-%d")?;
    let to_d = NaiveDate::parse_from_str(to, "%Y-%m-%d")?;

    let templates: Vec<ClassTemplateRow> = sqlx::query_as(
        "SELECT id, weekday, start_time, duration_minutes, instructor_id, capacity, active
         FROM class_templates WHERE active = 1",
    )
    .fetch_all(pool)
    .await?;

    let now = chrono::Local::now().naive_local();
    let mut out = Vec::new();
    let mut d = from_d;
    while d <= to_d {
        for t in &templates {
            if d.weekday().num_days_from_monday() as i64 != t.weekday {
                continue;
            }
            let date_s = d.to_string();

            let cancelled: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM class_cancellations WHERE template_id = ? AND date = ?",
            )
            .bind(t.id)
            .bind(&date_s)
            .fetch_optional(pool)
            .await?;

            let booked: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM bookings WHERE template_id = ? AND date = ? AND cancelled_at IS NULL"
            ).bind(t.id).bind(&date_s).fetch_one(pool).await?;

            let my_row: Option<(i64, String)> = sqlx::query_as(
                "SELECT id, source FROM bookings
                 WHERE template_id = ? AND date = ? AND card_id = ? AND cancelled_at IS NULL",
            )
            .bind(t.id)
            .bind(&date_s)
            .bind(card_id)
            .fetch_optional(pool)
            .await?;

            let start_dt = format!("{date_s} {}", t.start_time);
            let start_parsed =
                chrono::NaiveDateTime::parse_from_str(&start_dt, "%Y-%m-%d %H:%M").ok();
            let is_past = matches!(start_parsed, Some(s) if s <= now);

            let state = if cancelled.is_some() {
                "cancelled"
            } else if is_past {
                "past"
            } else if let Some((_, src)) = &my_row {
                if src == "persistent" {
                    "auto"
                } else {
                    "booked"
                }
            } else if booked >= t.capacity {
                "full"
            } else {
                "free"
            };

            let instructor_name: Option<String> = if let Some(iid) = t.instructor_id {
                sqlx::query_scalar("SELECT name FROM instructors WHERE id = ?")
                    .bind(iid)
                    .fetch_optional(pool)
                    .await?
            } else {
                None
            };

            out.push(UpcomingRow {
                template_id: t.id,
                date: date_s,
                start_time: t.start_time.clone(),
                duration_minutes: t.duration_minutes,
                instructor_id: t.instructor_id,
                instructor_name,
                capacity: t.capacity,
                booked,
                state: state.to_string(),
                booking_id: my_row.map(|(id, _)| id),
            });
        }
        d += Duration::days(1);
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(a.start_time.cmp(&b.start_time)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::users::create_user;
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn make_user(pool: &SqlitePool, email: &str) -> i64 {
        create_user(pool, email, None, "Test", None, "customer", None, None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn create_template_and_booking() {
        let pool = setup().await;
        let user_id = make_user(&pool, "t1@test.com").await;

        let tmpl_id = create_template(&pool, 1, "09:00", 60, None, 10)
            .await
            .unwrap();
        let booking_id =
            create_booking(&pool, tmpl_id, "2026-04-14", user_id, None, None, "manual")
                .await
                .unwrap();

        let bookings = list_bookings_for_class(&pool, tmpl_id, "2026-04-14")
            .await
            .unwrap();
        assert_eq!(bookings.len(), 1);
        assert_eq!(bookings[0].id, booking_id);
        assert_eq!(bookings[0].user_id, user_id);
    }

    #[tokio::test]
    async fn capacity_enforcement() {
        let pool = setup().await;

        // Template with capacity 2.
        let tmpl_id = create_template(&pool, 1, "10:00", 60, None, 2)
            .await
            .unwrap();
        let u1 = make_user(&pool, "cap1@test.com").await;
        let u2 = make_user(&pool, "cap2@test.com").await;
        let u3 = make_user(&pool, "cap3@test.com").await;

        create_booking(&pool, tmpl_id, "2026-04-14", u1, None, None, "manual")
            .await
            .unwrap();
        create_booking(&pool, tmpl_id, "2026-04-14", u2, None, None, "manual")
            .await
            .unwrap();

        // Third booking should fail — class is full.
        let result = create_booking(&pool, tmpl_id, "2026-04-14", u3, None, None, "manual").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("full"));
    }

    #[tokio::test]
    async fn cancel_booking_frees_spot() {
        let pool = setup().await;

        let tmpl_id = create_template(&pool, 1, "11:00", 60, None, 1)
            .await
            .unwrap();
        let u1 = make_user(&pool, "cb1@test.com").await;
        let u2 = make_user(&pool, "cb2@test.com").await;

        let b1 = create_booking(&pool, tmpl_id, "2026-04-14", u1, None, None, "manual")
            .await
            .unwrap();

        // Full — u2 cannot book.
        assert!(
            create_booking(&pool, tmpl_id, "2026-04-14", u2, None, None, "manual")
                .await
                .is_err()
        );

        // Cancel u1's booking.
        cancel_booking(&pool, b1).await.unwrap();

        // Now u2 can book.
        create_booking(&pool, tmpl_id, "2026-04-14", u2, None, None, "manual")
            .await
            .unwrap();
        assert_eq!(
            get_booking_count(&pool, tmpl_id, "2026-04-14")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn cancel_occurrence_test() {
        let pool = setup().await;

        let tmpl_id = create_template(&pool, 2, "14:00", 60, None, 10)
            .await
            .unwrap();

        assert!(
            !is_occurrence_cancelled(&pool, tmpl_id, "2026-04-15")
                .await
                .unwrap()
        );

        cancel_occurrence(&pool, tmpl_id, "2026-04-15", Some("Holiday"), None)
            .await
            .unwrap();

        assert!(
            is_occurrence_cancelled(&pool, tmpl_id, "2026-04-15")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn create_booking_records_card_id_and_source() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let user_id: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B1', ?, 0) RETURNING id",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let template_id = create_template(&pool, 0, "18:00", 60, None, 19)
            .await
            .unwrap();

        let id = create_booking(
            &pool,
            template_id,
            "2026-04-20",
            user_id,
            Some(card_id),
            None,
            "persistent",
        )
        .await
        .unwrap();

        let (got_card, got_source): (i64, String) =
            sqlx::query_as("SELECT card_id, source FROM bookings WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(got_card, card_id);
        assert_eq!(got_source, "persistent");
    }

    #[tokio::test]
    async fn duplicate_booking_rejected() {
        let pool = setup().await;
        let user_id = make_user(&pool, "dup@test.com").await;
        let tmpl_id = create_template(&pool, 1, "09:00", 60, None, 10)
            .await
            .unwrap();

        create_booking(&pool, tmpl_id, "2026-04-14", user_id, None, None, "manual")
            .await
            .unwrap();

        // Same user, same class, same date — unique index should reject.
        let result =
            create_booking(&pool, tmpl_id, "2026-04-14", user_id, None, None, "manual").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_upcoming_for_card_joins_booking_state() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let user_id: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B', ?, 0) RETURNING id",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        use chrono::{Datelike, Duration};
        let today = chrono::Local::now().date_naive();
        // Find the next Monday (weekday 0) strictly in the future — avoids timing flakes if today *is* Monday.
        let days_to_mon = {
            let m = (7 - today.weekday().num_days_from_monday() as i64) % 7;
            if m == 0 { 7 } else { m }
        };
        let mon = today + Duration::days(days_to_mon);
        // V6 already seeded the Monday 18:00 template; use it.
        let template_id: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // No booking yet: state == "free".
        let rows = list_upcoming_for_card(
            &pool,
            card_id,
            &today.to_string(),
            &(today + Duration::days(14)).to_string(),
        )
        .await
        .unwrap();
        let monday_row = rows.iter().find(|r| r.date == mon.to_string()).unwrap();
        assert_eq!(monday_row.state, "free");
        assert!(monday_row.booking_id.is_none());

        // Book manual: state == "booked".
        let bid = create_booking(
            &pool,
            template_id,
            &mon.to_string(),
            user_id,
            Some(card_id),
            None,
            "manual",
        )
        .await
        .unwrap();
        let rows = list_upcoming_for_card(
            &pool,
            card_id,
            &today.to_string(),
            &(today + Duration::days(14)).to_string(),
        )
        .await
        .unwrap();
        let monday_row = rows.iter().find(|r| r.date == mon.to_string()).unwrap();
        assert_eq!(monday_row.state, "booked");
        assert_eq!(monday_row.booking_id, Some(bid));
    }
}
