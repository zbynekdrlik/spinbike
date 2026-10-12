//! "Is this customer a spinner?" (#382 R7) — drives the "Moje spinningy"
//! card at the top of the customer home; gym-only customers see no spinning.

use crate::db::error::Result;
use sqlx::SqlitePool;

/// How far back a spinning visit/booking still makes a customer a
/// "spinner" (owner ROZHODNUTÉ on #382: 365 days, not 90 — spinning pauses
/// over summer, so a 90-day window would drop every regular after the break).
pub const SPINNER_LOOKBACK_DAYS: i64 = 365;

/// Is `user_id` a spinner as of the gym-local `today`? True when the user has
/// an ACTIVE recurring subscription, OR a non-cancelled spinning booking
/// dated within the last [`SPINNER_LOOKBACK_DAYS`] days (or in the future),
/// OR a non-voided Spinning transaction (identified by `services.kind`,
/// never the admin-editable name — `service-kind.md`) created within that
/// window. The transaction bound is the UTC instant of the window's first
/// gym-local midnight (`bratislava_day_range_utc`), because
/// `transactions.created_at` is a UTC instant column.
pub async fn is_spinner(pool: &SqlitePool, user_id: i64, today: chrono::NaiveDate) -> Result<bool> {
    let first_day = today - chrono::Duration::days(SPINNER_LOOKBACK_DAYS);
    let since_utc = crate::util::bratislava_day_range_utc(first_day)
        .0
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let spinner: i64 = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM persistent_bookings
                         WHERE user_id = ?1 AND ended_at IS NULL)
             OR EXISTS (SELECT 1 FROM bookings
                         WHERE user_id = ?1 AND cancelled_at IS NULL AND date >= ?2)
             OR EXISTS (SELECT 1 FROM transactions t
                          JOIN services s ON s.id = t.service_id
                         WHERE t.user_id = ?1 AND t.deleted_at IS NULL
                           AND s.kind = ?3 AND t.created_at >= ?4)",
    )
    .bind(user_id)
    .bind(first_day.to_string())
    .bind(spinbike_core::services::SPINNING_KIND)
    .bind(since_utc)
    .fetch_one(pool)
    .await?;
    Ok(spinner != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::booking_cancel::{CancelKind, cancel_booking};
    use crate::db::classes::{create_booking, create_template};
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn make_user(pool: &SqlitePool, email: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO users (email, name) VALUES (?, 'S') RETURNING id")
            .bind(email)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn visit(pool: &SqlitePool, uid: i64, kind: &str, created_at: &str, voided: bool) {
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at, deleted_at)
             VALUES (?, (SELECT id FROM services WHERE kind = ?), 0.0, 'visit', ?,
                     CASE WHEN ? THEN datetime('now') END)",
        )
        .bind(uid)
        .bind(kind)
        .bind(created_at)
        .bind(voided)
        .execute(pool)
        .await
        .unwrap();
    }

    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
    }

    /// The exact 365-day boundary. Window start = 2025-10-12 00:00
    /// Europe/Bratislava (CEST) = 2025-10-11 22:00:00 UTC.
    #[tokio::test]
    async fn spinning_visit_counts_from_exactly_365_days_back() {
        let pool = setup().await;
        let outside = make_user(&pool, "out@test.com").await;
        visit(&pool, outside, "group_class", "2025-10-11 21:59:59", false).await;
        assert!(
            !is_spinner(&pool, outside, today()).await.unwrap(),
            "one second before the 365-day window"
        );
        let inside = make_user(&pool, "in@test.com").await;
        visit(&pool, inside, "group_class", "2025-10-11 22:00:00", false).await;
        assert!(
            is_spinner(&pool, inside, today()).await.unwrap(),
            "the first instant of the 365-day window"
        );
    }

    /// Gym-only visits and voided spinning visits never make a spinner.
    #[tokio::test]
    async fn gym_visits_and_voided_spinning_do_not_count() {
        let pool = setup().await;
        let uid = make_user(&pool, "gym@test.com").await;
        assert!(
            !is_spinner(&pool, uid, today()).await.unwrap(),
            "nothing yet"
        );
        visit(&pool, uid, "single_entry", "2026-10-01 10:00:00", false).await;
        assert!(!is_spinner(&pool, uid, today()).await.unwrap(), "gym only");
        visit(&pool, uid, "group_class", "2026-09-01 10:00:00", true).await;
        assert!(!is_spinner(&pool, uid, today()).await.unwrap(), "voided");
    }

    /// A non-cancelled booking counts, a cancelled one does not; an active
    /// recurring subscription counts on its own.
    #[tokio::test]
    async fn bookings_and_recurring_slots_count() {
        let pool = setup().await;
        let uid = make_user(&pool, "bk@test.com").await;
        let tid = create_template(&pool, 4, "18:00", 60, None, 10)
            .await
            .unwrap();
        let bid = create_booking(&pool, tid, "2026-10-23", uid, None, "manual")
            .await
            .unwrap();
        assert!(is_spinner(&pool, uid, today()).await.unwrap(), "booking");
        cancel_booking(&pool, bid, Some(uid), CancelKind::Customer)
            .await
            .unwrap();
        assert!(!is_spinner(&pool, uid, today()).await.unwrap(), "cancelled");

        let recurring = make_user(&pool, "rc@test.com").await;
        crate::db::persistent_bookings::create(&pool, recurring, tid)
            .await
            .unwrap();
        assert!(
            is_spinner(&pool, recurring, today()).await.unwrap(),
            "recurring"
        );
    }
}
