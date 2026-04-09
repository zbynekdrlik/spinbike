use anyhow::{bail, Context, Result};
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

pub async fn get_booking_count(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
) -> Result<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE template_id = ? AND date = ? AND cancelled_at IS NULL",
    )
    .bind(template_id)
    .bind(date)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Create a booking with capacity enforcement.
pub async fn create_booking(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
    user_id: i64,
    created_by: Option<i64>,
) -> Result<i64> {
    // Fetch template capacity.
    let capacity: i64 = sqlx::query_scalar(
        "SELECT capacity FROM class_templates WHERE id = ?",
    )
    .bind(template_id)
    .fetch_one(pool)
    .await
    .context("Template not found")?;

    // Count active bookings.
    let booked = get_booking_count(pool, template_id, date).await?;

    if booked >= capacity {
        bail!("Class is full ({booked}/{capacity})");
    }

    let id = sqlx::query_scalar(
        "INSERT INTO bookings (template_id, date, user_id, created_by)
         VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(template_id)
    .bind(date)
    .bind(user_id)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .context("Failed to create booking")?;

    Ok(id)
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
        "SELECT * FROM bookings WHERE user_id = ? AND cancelled_at IS NULL ORDER BY date, created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .context("Failed to list user bookings")?;
    Ok(bookings)
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

        let tmpl_id = create_template(&pool, 1, "09:00", 60, None, 10).await.unwrap();
        let booking_id = create_booking(&pool, tmpl_id, "2026-04-14", user_id, None)
            .await
            .unwrap();

        let bookings = list_bookings_for_class(&pool, tmpl_id, "2026-04-14").await.unwrap();
        assert_eq!(bookings.len(), 1);
        assert_eq!(bookings[0].id, booking_id);
        assert_eq!(bookings[0].user_id, user_id);
    }

    #[tokio::test]
    async fn capacity_enforcement() {
        let pool = setup().await;

        // Template with capacity 2.
        let tmpl_id = create_template(&pool, 1, "10:00", 60, None, 2).await.unwrap();
        let u1 = make_user(&pool, "cap1@test.com").await;
        let u2 = make_user(&pool, "cap2@test.com").await;
        let u3 = make_user(&pool, "cap3@test.com").await;

        create_booking(&pool, tmpl_id, "2026-04-14", u1, None).await.unwrap();
        create_booking(&pool, tmpl_id, "2026-04-14", u2, None).await.unwrap();

        // Third booking should fail — class is full.
        let result = create_booking(&pool, tmpl_id, "2026-04-14", u3, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("full"));
    }

    #[tokio::test]
    async fn cancel_booking_frees_spot() {
        let pool = setup().await;

        let tmpl_id = create_template(&pool, 1, "11:00", 60, None, 1).await.unwrap();
        let u1 = make_user(&pool, "cb1@test.com").await;
        let u2 = make_user(&pool, "cb2@test.com").await;

        let b1 = create_booking(&pool, tmpl_id, "2026-04-14", u1, None).await.unwrap();

        // Full — u2 cannot book.
        assert!(create_booking(&pool, tmpl_id, "2026-04-14", u2, None).await.is_err());

        // Cancel u1's booking.
        cancel_booking(&pool, b1).await.unwrap();

        // Now u2 can book.
        create_booking(&pool, tmpl_id, "2026-04-14", u2, None).await.unwrap();
        assert_eq!(get_booking_count(&pool, tmpl_id, "2026-04-14").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn cancel_occurrence_test() {
        let pool = setup().await;

        let tmpl_id = create_template(&pool, 2, "14:00", 60, None, 10).await.unwrap();

        assert!(!is_occurrence_cancelled(&pool, tmpl_id, "2026-04-15").await.unwrap());

        cancel_occurrence(&pool, tmpl_id, "2026-04-15", Some("Holiday"), None)
            .await
            .unwrap();

        assert!(is_occurrence_cancelled(&pool, tmpl_id, "2026-04-15").await.unwrap());
    }

    #[tokio::test]
    async fn duplicate_booking_rejected() {
        let pool = setup().await;
        let user_id = make_user(&pool, "dup@test.com").await;
        let tmpl_id = create_template(&pool, 1, "09:00", 60, None, 10).await.unwrap();

        create_booking(&pool, tmpl_id, "2026-04-14", user_id, None).await.unwrap();

        // Same user, same class, same date — unique index should reject.
        let result = create_booking(&pool, tmpl_id, "2026-04-14", user_id, None).await;
        assert!(result.is_err());
    }
}
