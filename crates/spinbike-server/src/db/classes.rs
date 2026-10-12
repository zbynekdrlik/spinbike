use crate::db::error::{DbError, Result};
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
    ?;
    Ok(id)
}

pub async fn list_active_templates(pool: &SqlitePool) -> Result<Vec<ClassTemplateRow>> {
    let templates = sqlx::query_as::<_, ClassTemplateRow>(
        "SELECT id, weekday, start_time, duration_minutes, instructor_id, capacity, active
         FROM class_templates WHERE active = 1 ORDER BY weekday, start_time",
    )
    .fetch_all(pool)
    .await?;
    Ok(templates)
}

pub async fn list_all_templates(pool: &SqlitePool) -> Result<Vec<ClassTemplateRow>> {
    let templates = sqlx::query_as::<_, ClassTemplateRow>(
        "SELECT id, weekday, start_time, duration_minutes, instructor_id, capacity, active
         FROM class_templates ORDER BY weekday, start_time",
    )
    .fetch_all(pool)
    .await?;
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
    .await?;
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
    created_by: Option<i64>,
    source: &str,
) -> Result<i64> {
    let result = sqlx::query_scalar::<_, i64>(
        "INSERT INTO bookings (template_id, date, user_id, created_by, source)
         SELECT ?1, ?2, ?3, ?4, ?5
         WHERE (SELECT COUNT(*) FROM bookings
                WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL)
               < (SELECT capacity FROM class_templates WHERE id = ?1)
         RETURNING id",
    )
    .bind(template_id)
    .bind(date)
    .bind(user_id)
    .bind(created_by)
    .bind(source)
    .fetch_optional(pool)
    .await?;
    match result {
        Some(id) => Ok(id),
        None => Err(DbError::ClassFull),
    }
}

/// In which capacity a booking is cancelled — persisted as
/// `bookings.cancel_kind` (V31, #382). It decides two things: whether a
/// charged booking is refunded (only [`CancelKind::Staff`]), and whether the
/// materialiser may ever re-create that (user, template, date) — never after
/// a `Customer`/`Staff` cancellation, but yes after an `Unsubscribe` (the
/// system ending a subscription the customer may later re-enable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelKind {
    /// The customer cancelled their own booking (before the 09:00 deadline).
    Customer,
    /// Staff/admin cancelled it — at any time, refunding a charged booking.
    Staff,
    /// The system ended a recurring subscription (future, uncharged only).
    Unsubscribe,
}

impl CancelKind {
    /// The `bookings.cancel_kind` value (CHECK-constrained by V31).
    pub const fn as_str(self) -> &'static str {
        match self {
            CancelKind::Customer => "customer",
            CancelKind::Staff => "staff",
            CancelKind::Unsubscribe => "unsubscribe",
        }
    }
}

/// Display note on a staff refund row. Display ONLY — the refund is linked
/// and recognised by `bookings.refund_transaction_id`, never by this text.
pub const REFUND_NOTE: &str = "Vratenie spinningu (zrusena rezervacia)";

/// Result of [`cancel_booking`] for a booking that was still active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CancelOutcome {
    /// The (rounded) amount credited back, when a staff cancel reversed a
    /// non-zero charge.
    pub refunded: Option<f64>,
    /// The separate refund `transactions` row, linked from the booking.
    pub refund_transaction_id: Option<i64>,
}

/// Cancel one ACTIVE booking (#382 R2/R3). Returns `Ok(None)` when the
/// booking does not exist or is already cancelled (a raced double cancel).
///
/// The deadline policy (customer only until 09:00 on the class day) is the
/// CALLER's job (`booking_rules::customer_may_cancel`); this function only
/// records the cancellation and, for [`CancelKind::Staff`], reverses a
/// charge: when the booking was charged a NON-ZERO amount (a pass holder's
/// 0 EUR visit needs nothing) it inserts a separate `storno` ledger row with
/// the positive amount, the cancelling staff id and the Spinning service,
/// credits the customer, and links the row via
/// `bookings.refund_transaction_id`. The refund is rounded ONCE and reused
/// for the credit UPDATE and the ledger INSERT (`money-rounding.md`). A
/// charge row someone already voided is not refunded again. Everything runs
/// in ONE transaction.
pub async fn cancel_booking(
    pool: &SqlitePool,
    booking_id: i64,
    cancelled_by: Option<i64>,
    kind: CancelKind,
) -> Result<Option<CancelOutcome>> {
    let mut tx = pool.begin().await?;

    let row: Option<(i64, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT user_id, charge_transaction_id, charged_at
         FROM bookings WHERE id = ? AND cancelled_at IS NULL",
    )
    .bind(booking_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id, charge_txn_id, charged_at)) = row else {
        tx.rollback().await?;
        return Ok(None);
    };

    let mut outcome = CancelOutcome {
        refunded: None,
        refund_transaction_id: None,
    };

    if kind == CancelKind::Staff
        && charged_at.is_some()
        && let Some(charge_id) = charge_txn_id
    {
        let charge: Option<(f64, Option<i64>)> = sqlx::query_as(
            "SELECT amount, service_id FROM transactions WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(charge_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((amount, service_id)) = charge
            && amount < 0.0
        {
            let refund = crate::db::users::round_cents(-amount);
            let refund_id: i64 = sqlx::query_scalar(
                "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, note)
                 VALUES (?, ?, ?, ?, 'storno', ?) RETURNING id",
            )
            .bind(user_id)
            .bind(cancelled_by)
            .bind(service_id)
            .bind(refund)
            .bind(REFUND_NOTE)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query("UPDATE users SET credit = ROUND(credit + ?, 2) WHERE id = ?")
                .bind(refund)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            outcome.refunded = Some(refund);
            outcome.refund_transaction_id = Some(refund_id);
        }
    }

    sqlx::query(
        "UPDATE bookings
            SET cancelled_at = datetime('now'), cancelled_by = ?, cancel_kind = ?,
                refund_transaction_id = ?
          WHERE id = ?",
    )
    .bind(cancelled_by)
    .bind(kind.as_str())
    .bind(outcome.refund_transaction_id)
    .bind(booking_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    tracing::info!(
        booking_id,
        user_id,
        cancelled_by = ?cancelled_by,
        kind = kind.as_str(),
        refunded = ?outcome.refunded,
        refund_transaction_id = ?outcome.refund_transaction_id,
        "booking cancelled"
    );
    Ok(Some(outcome))
}

/// How far back a spinning visit/booking still makes a customer a
/// "spinner" (#382 R7, owner ROZHODNUTÉ: 365 days, not 90 — spinning pauses
/// over summer, so a 90-day window would drop every regular after the break).
pub const SPINNER_LOOKBACK_DAYS: i64 = 365;

/// #382 R7: is `user_id` a spinner as of the gym-local `today`? True when
/// the user has an ACTIVE recurring subscription, OR a non-cancelled
/// spinning booking dated within the last [`SPINNER_LOOKBACK_DAYS`] days
/// (or in the future), OR a non-voided Spinning transaction (identified by
/// `services.kind`, never the admin-editable name — `service-kind.md`)
/// created within that window. The transaction bound is the UTC instant of
/// the window's first gym-local midnight (`bratislava_day_range_utc`), since
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

pub async fn list_bookings_for_class(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
) -> Result<Vec<BookingRow>> {
    let bookings = sqlx::query_as::<_, BookingRow>(
        "SELECT id, template_id, date, user_id, created_by, source, created_at, cancelled_at
         FROM bookings WHERE template_id = ? AND date = ? AND cancelled_at IS NULL ORDER BY created_at",
    )
    .bind(template_id)
    .bind(date)
    .fetch_all(pool)
    .await
    ?;
    Ok(bookings)
}

/// A user's own upcoming booking, joined with its class template + instructor
/// (#146) — customer `/my/bookings` view. Kept as its own row type (not
/// `BookingRow`) because `BookingRow` also backs the raw `bookings` column set
/// (see `list_bookings_for_class` here and `cancel_booking` in
/// `routes/classes.rs`, both explicit-column `SELECT`s, #164), which must stay
/// exactly in sync with the (deliberately partial — it omits `card_id`,
/// `charged_at`, `charge_transaction_id`) subset of table columns it names.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BookingWithClassRow {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub user_id: i64,
    pub start_time: String,
    pub instructor_name: Option<String>,
    /// `'manual'` | `'persistent'` (#382: the recurring mark on the card).
    pub source: String,
    /// Set once the binding 09:00 charge ran (#382: no customer cancel).
    pub charged_at: Option<String>,
}

pub async fn list_user_bookings(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<BookingWithClassRow>> {
    // "Upcoming" is relative to the gym's LOCAL day (Europe/Bratislava), bound
    // as a param — NOT SQLite's `date('now')` (UTC), which near local midnight
    // is up to 2h off from the gym's day and could drop a class dated "today"
    // from the list early / show a stale one late (#205/#222). `b.date` is a
    // bare `YYYY-MM-DD` calendar column, so an inclusive bare-date `>=` compare
    // is correct.
    let today = crate::util::today_bratislava();
    let bookings = sqlx::query_as::<_, BookingWithClassRow>(
        "SELECT b.id, b.template_id, b.date, b.user_id, \
                ct.start_time, i.name AS instructor_name, b.source, b.charged_at \
           FROM bookings b \
           JOIN class_templates ct ON ct.id = b.template_id \
           LEFT JOIN instructors i ON i.id = ct.instructor_id \
          WHERE b.user_id = ? AND b.cancelled_at IS NULL AND b.date >= ? \
          ORDER BY b.date, ct.start_time, b.created_at",
    )
    .bind(user_id)
    .bind(today)
    .fetch_all(pool)
    .await?;
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

pub async fn list_upcoming_for_user(
    pool: &SqlitePool,
    user_id: i64,
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

    // Gym-local wall clock (Europe/Bratislava) for the past/free/booked/full
    // state — NOT `chrono::Local` (server OS zone, may be UTC on the systemd
    // unit), which near local midnight would mark a class past/upcoming against
    // the wrong day (#205/#222).
    let now = crate::util::now_bratislava();
    let mut out = Vec::new();
    let span_days = (to_d - from_d).num_days().max(0);
    for offset in 0..=span_days {
        let d = from_d + Duration::days(offset);
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
                 WHERE template_id = ? AND date = ? AND user_id = ? AND cancelled_at IS NULL",
            )
            .bind(t.id)
            .bind(&date_s)
            .bind(user_id)
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
        create_user(
            pool,
            Some(email),
            None,
            "Test",
            None,
            None,
            None,
            "customer",
            None,
            None,
            None,
        )
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
        let booking_id = create_booking(&pool, tmpl_id, "2026-04-14", user_id, None, "manual")
            .await
            .unwrap();

        let bookings = list_bookings_for_class(&pool, tmpl_id, "2026-04-14")
            .await
            .unwrap();
        assert_eq!(bookings.len(), 1);
        assert_eq!(bookings[0].id, booking_id);
        assert_eq!(bookings[0].user_id, user_id);
    }

    /// #164: `list_bookings_for_class` now names its columns explicitly
    /// instead of `SELECT *`. Spot-check every `BookingRow` field decodes —
    /// including `created_by` (staff-on-behalf booking), which no other test
    /// reads back through this query (existing coverage reads it via a raw
    /// `SELECT created_by FROM bookings`, not through `BookingRow`).
    #[tokio::test]
    async fn list_bookings_for_class_decodes_every_column() {
        let pool = setup().await;
        let staff_id = make_user(&pool, "staff-created@test.com").await;
        let user_id = make_user(&pool, "b2@test.com").await;

        let tmpl_id = create_template(&pool, 2, "18:30", 45, None, 5)
            .await
            .unwrap();
        let booking_id = create_booking(
            &pool,
            tmpl_id,
            "2026-05-01",
            user_id,
            Some(staff_id),
            "persistent",
        )
        .await
        .unwrap();

        let bookings = list_bookings_for_class(&pool, tmpl_id, "2026-05-01")
            .await
            .unwrap();
        assert_eq!(bookings.len(), 1);
        let b = &bookings[0];
        assert_eq!(b.id, booking_id);
        assert_eq!(b.template_id, tmpl_id);
        assert_eq!(b.date, "2026-05-01");
        assert_eq!(b.user_id, user_id);
        assert_eq!(b.created_by, Some(staff_id));
        assert_eq!(b.source, "persistent");
        assert!(!b.created_at.is_empty());
        assert_eq!(b.cancelled_at, None);
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

        create_booking(&pool, tmpl_id, "2026-04-14", u1, None, "manual")
            .await
            .unwrap();
        create_booking(&pool, tmpl_id, "2026-04-14", u2, None, "manual")
            .await
            .unwrap();

        // Third booking should fail — class is full.
        let result = create_booking(&pool, tmpl_id, "2026-04-14", u3, None, "manual").await;
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

        let b1 = create_booking(&pool, tmpl_id, "2026-04-14", u1, None, "manual")
            .await
            .unwrap();

        // Full — u2 cannot book.
        assert!(
            create_booking(&pool, tmpl_id, "2026-04-14", u2, None, "manual")
                .await
                .is_err()
        );

        // Cancel u1's booking.
        cancel_booking(&pool, b1, Some(u1), CancelKind::Customer)
            .await
            .unwrap()
            .expect("an active booking is cancelled");

        // Now u2 can book.
        create_booking(&pool, tmpl_id, "2026-04-14", u2, None, "manual")
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
    async fn create_booking_records_user_id_and_source() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let user_id: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
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
            None,
            "persistent",
        )
        .await
        .unwrap();

        let (got_user, got_source): (i64, String) =
            sqlx::query_as("SELECT user_id, source FROM bookings WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(got_user, user_id);
        assert_eq!(got_source, "persistent");
    }

    #[tokio::test]
    async fn duplicate_booking_rejected() {
        let pool = setup().await;
        let user_id = make_user(&pool, "dup@test.com").await;
        let tmpl_id = create_template(&pool, 1, "09:00", 60, None, 10)
            .await
            .unwrap();

        create_booking(&pool, tmpl_id, "2026-04-14", user_id, None, "manual")
            .await
            .unwrap();

        // Same user, same class, same date — unique index should reject.
        let result = create_booking(&pool, tmpl_id, "2026-04-14", user_id, None, "manual").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_upcoming_for_user_joins_booking_state() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let user_id: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();

        use chrono::{Datelike, Duration};
        let today = crate::util::today_bratislava();
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
        let rows = list_upcoming_for_user(
            &pool,
            user_id,
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
            None,
            "manual",
        )
        .await
        .unwrap();
        let rows = list_upcoming_for_user(
            &pool,
            user_id,
            &today.to_string(),
            &(today + Duration::days(14)).to_string(),
        )
        .await
        .unwrap();
        let monday_row = rows.iter().find(|r| r.date == mon.to_string()).unwrap();
        assert_eq!(monday_row.state, "booked");
        assert_eq!(monday_row.booking_id, Some(bid));
    }

    /// #382 R3: a customer cancel never refunds, even of a charged booking
    /// (the route forbids that case; the db layer must not refund either).
    #[tokio::test]
    async fn customer_cancel_records_kind_and_never_refunds() {
        let pool = setup().await;
        let uid = make_user(&pool, "ck@test.com").await;
        let tid = create_template(&pool, 1, "18:00", 60, None, 10)
            .await
            .unwrap();
        let bid = create_booking(&pool, tid, "2026-04-14", uid, None, "manual")
            .await
            .unwrap();
        let out = cancel_booking(&pool, bid, Some(uid), CancelKind::Customer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.refunded, None);
        let (kind, by): (Option<String>, Option<i64>) =
            sqlx::query_as("SELECT cancel_kind, cancelled_by FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kind.as_deref(), Some("customer"));
        assert_eq!(by, Some(uid));
        // A second cancel of the same booking is a no-op, not an error.
        assert!(
            cancel_booking(&pool, bid, Some(uid), CancelKind::Customer)
                .await
                .unwrap()
                .is_none()
        );
    }

    /// #382 R3: staff cancel of a charged booking writes ONE separate storno
    /// row with the rounded positive amount + staff id, credits the user and
    /// links the row; a voided charge is not refunded again.
    #[tokio::test]
    async fn staff_cancel_refunds_a_charged_booking_once() {
        let pool = setup().await;
        let staff = make_user(&pool, "st@test.com").await;
        let uid = make_user(&pool, "rf@test.com").await;
        sqlx::query("UPDATE users SET credit = -3.3 WHERE id = ?")
            .bind(uid)
            .execute(&pool)
            .await
            .unwrap();
        let tid = create_template(&pool, 1, "18:00", 60, None, 10)
            .await
            .unwrap();
        let bid = create_booking(&pool, tid, "2026-04-14", uid, None, "manual")
            .await
            .unwrap();
        // Simulate the charger's write: a -3.301 drift charge (rounding!).
        let charge_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, service_id, amount, action)
             VALUES (?, (SELECT id FROM services WHERE kind = 'group_class'), -3.301, 'visit')
             RETURNING id",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE bookings SET charged_at = datetime('now'), charge_transaction_id = ? WHERE id = ?",
        )
        .bind(charge_id)
        .bind(bid)
        .execute(&pool)
        .await
        .unwrap();

        let out = cancel_booking(&pool, bid, Some(staff), CancelKind::Staff)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.refunded, Some(3.3));
        let refund_id = out.refund_transaction_id.unwrap();
        let (amount, staff_id, action, svc_kind): (f64, Option<i64>, String, String) =
            sqlx::query_as(
                "SELECT t.amount, t.staff_id, t.action, s.kind FROM transactions t \
                 JOIN services s ON s.id = t.service_id WHERE t.id = ?",
            )
            .bind(refund_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(amount, 3.3);
        assert_eq!(staff_id, Some(staff));
        assert_eq!(action, "storno");
        assert_eq!(svc_kind, "group_class");
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 0.0, "-3.30 + 3.30, rounded once");
        let linked: Option<i64> =
            sqlx::query_scalar("SELECT refund_transaction_id FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked, Some(refund_id));

        // A voided charge: no refund.
        let bid2 = create_booking(&pool, tid, "2026-04-21", uid, None, "manual")
            .await
            .unwrap();
        let charge2: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, amount, action, deleted_at)
             VALUES (?, -5.0, 'visit', datetime('now')) RETURNING id",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE bookings SET charged_at = datetime('now'), charge_transaction_id = ? WHERE id = ?",
        )
        .bind(charge2)
        .bind(bid2)
        .execute(&pool)
        .await
        .unwrap();
        let out2 = cancel_booking(&pool, bid2, Some(staff), CancelKind::Staff)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out2.refunded, None, "a voided charge is not refunded again");
    }

    /// #382 R7: the spinner rule — recurring subscription OR a non-cancelled
    /// spinning booking / a Spinning transaction within the last 365 days.
    #[tokio::test]
    async fn is_spinner_covers_recurring_bookings_and_365_day_spinning_visits() {
        let pool = setup().await;
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let uid = make_user(&pool, "sp@test.com").await;
        assert!(!is_spinner(&pool, uid, today).await.unwrap(), "nobody yet");

        // Gym-only visits never make a spinner (single_entry kind).
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
             VALUES (?, (SELECT id FROM services WHERE kind = 'single_entry'), -3.0,
                     'charge', '2026-10-01 10:00:00')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        assert!(!is_spinner(&pool, uid, today).await.unwrap());

        // A Spinning visit 366 days ago: outside the window.
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
             VALUES (?, (SELECT id FROM services WHERE kind = 'group_class'), 0.0,
                     'visit', '2025-10-10 12:00:00')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            !is_spinner(&pool, uid, today).await.unwrap(),
            "366 days ago"
        );
        // ...exactly 365 days ago (2025-10-12, gym-local noon): inside.
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
             VALUES (?, (SELECT id FROM services WHERE kind = 'group_class'), 0.0,
                     'visit', '2025-10-12 10:00:00')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        assert!(is_spinner(&pool, uid, today).await.unwrap(), "365 days ago");

        // A voided spinning visit does not count.
        let u2 = make_user(&pool, "sp2@test.com").await;
        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at, deleted_at)
             VALUES (?, (SELECT id FROM services WHERE kind = 'group_class'), -5.0,
                     'visit', '2026-09-01 10:00:00', '2026-09-01 11:00:00')",
        )
        .bind(u2)
        .execute(&pool)
        .await
        .unwrap();
        assert!(!is_spinner(&pool, u2, today).await.unwrap());

        // A non-cancelled booking within the window counts; a cancelled one does not.
        let tid = create_template(&pool, 4, "18:00", 60, None, 10)
            .await
            .unwrap();
        let bid = create_booking(&pool, tid, "2026-10-23", u2, None, "manual")
            .await
            .unwrap();
        assert!(
            is_spinner(&pool, u2, today).await.unwrap(),
            "future booking"
        );
        cancel_booking(&pool, bid, Some(u2), CancelKind::Customer)
            .await
            .unwrap();
        assert!(!is_spinner(&pool, u2, today).await.unwrap(), "cancelled");

        // An active recurring subscription counts on its own.
        let u3 = make_user(&pool, "sp3@test.com").await;
        crate::db::persistent_bookings::create(&pool, u3, tid)
            .await
            .unwrap();
        assert!(is_spinner(&pool, u3, today).await.unwrap(), "recurring");
    }
}
