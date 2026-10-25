//! Guests on a spinning booking — the host pays for them (#389, owner
//! ROZHODNUTÉ 2026-10-25, "Approach A": a guest is a COUNT on the host's own
//! booking, no name, no account).
//!
//! - **Seats.** Every guest takes one seat: a class is full when
//!   `SUM(1 + guests)` over its active bookings reaches the capacity
//!   ([`SEATS_SQL`] is the one expression every capacity check uses).
//! - **Charge.** At 09:00 the charger writes, besides the host's own row, one
//!   `visit` row per guest ([`charge_guest_seats`]) — ALWAYS the Spinning
//!   price from the host's credit (a monthly pass covers only the host;
//!   negative credit allowed, like the host's own charge). Each row is linked
//!   from `booking_guest_charges` so attendance counts every seat and a
//!   later refund is found by id, never by note text.
//! - **Change.** [`set_guests`] changes the count: a customer only until the
//!   09:00 cutoff (the route checks `booking_rules::customer_may_cancel`,
//!   this function refuses an already-charged booking for non-staff — the
//!   same race guard as `booking_cancel`); staff at any time. After the
//!   charge, staff adding guests charges them at once (the charger only
//!   processes uncharged bookings), staff removing guests refunds those
//!   seats ([`release_guest_seats`], one `storno` row each, `service_id`
//!   NULL like every refund — `spinning-booking.md`).

use crate::db::error::Result;
use sqlx::{SqliteConnection, SqlitePool};

/// SQL for the seats an active booking occupies (host + guests). Capacity
/// checks sum this over `cancelled_at IS NULL` rows.
pub const SEATS_SQL: &str = "COALESCE(SUM(1 + guests), 0)";

/// Display note on a guest's charge row. Display ONLY — the row is linked
/// from `booking_guest_charges.transaction_id`.
pub const GUEST_CHARGE_NOTE: &str = "Host (spinning)";

/// Display note on a guest seat's refund row (linked by
/// `booking_guest_charges.refund_transaction_id`).
pub const GUEST_REFUND_NOTE: &str = "Vratenie spinningu za hosta";

/// The Spinning service id and its price, rounded ONCE where the price
/// enters the operation (`money-rounding.md`) — reused for every ledger
/// amount AND credit update of that operation. #329: identified by its
/// stable `kind`, never the admin-editable name. The single definition the
/// charger and a staff guest increase both use.
pub async fn spinning_price(conn: &mut SqliteConnection) -> Result<(i64, f64)> {
    let (service_id, price): (i64, f64) =
        sqlx::query_as("SELECT id, default_price FROM services WHERE kind = ?1 AND active = 1")
            .bind(spinbike_core::services::SPINNING_KIND)
            .fetch_one(&mut *conn)
            .await?;
    Ok((service_id, crate::db::users::round_cents(price)))
}

/// Write `count` guest charges for `booking_id` on the host's account, inside
/// the caller's transaction: per guest one `visit` row of `-price` (the
/// already-rounded Spinning price), the credit debit, and the link row.
/// Returns the total debited.
pub async fn charge_guest_seats(
    conn: &mut SqliteConnection,
    booking_id: i64,
    host_user_id: i64,
    staff_id: Option<i64>,
    service_id: i64,
    price: f64,
    count: i64,
) -> Result<f64> {
    let mut total = 0.0;
    for _ in 0..count.max(0) {
        let txn_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, note)
             VALUES (?, ?, ?, ?, 'visit', ?) RETURNING id",
        )
        .bind(host_user_id)
        .bind(staff_id)
        .bind(service_id)
        .bind(-price)
        .bind(GUEST_CHARGE_NOTE)
        .fetch_one(&mut *conn)
        .await?;
        sqlx::query("UPDATE users SET credit = ROUND(credit - ?, 2) WHERE id = ?")
            .bind(price)
            .bind(host_user_id)
            .execute(&mut *conn)
            .await?;
        sqlx::query("INSERT INTO booking_guest_charges (booking_id, transaction_id) VALUES (?, ?)")
            .bind(booking_id)
            .bind(txn_id)
            .execute(&mut *conn)
            .await?;
        total += price;
    }
    Ok(crate::db::users::round_cents(total))
}

/// What [`release_guest_seats`] gave back.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GuestRelease {
    /// Charged guest seats released.
    pub released: i64,
    /// Credit refunded in total (a voided or 0 EUR seat refunds nothing).
    pub refunded: f64,
}

/// Release up to `count` (`None` = all) still-held CHARGED guest seats of
/// `booking_id`, newest first, inside the caller's transaction. A seat whose
/// charge row is non-voided and negative gets ONE `storno` refund row
/// (positive, `released_by` as staff, `service_id` NULL) and the host is
/// credited; the amount is rounded once and reused for both writes.
pub async fn release_guest_seats(
    conn: &mut SqliteConnection,
    booking_id: i64,
    host_user_id: i64,
    released_by: Option<i64>,
    count: Option<i64>,
) -> Result<GuestRelease> {
    let held: Vec<(i64, Option<f64>)> = sqlx::query_as(
        "SELECT g.id, CASE WHEN t.deleted_at IS NULL THEN t.amount END
           FROM booking_guest_charges g
           JOIN transactions t ON t.id = g.transaction_id
          WHERE g.booking_id = ? AND g.released_at IS NULL
          ORDER BY g.id DESC
          LIMIT ?",
    )
    .bind(booking_id)
    .bind(count.unwrap_or(-1))
    .fetch_all(&mut *conn)
    .await?;

    let mut out = GuestRelease::default();
    for (link_id, amount) in held {
        let refund_id = match amount {
            Some(a) if a < 0.0 => {
                let refund = crate::db::users::round_cents(-a);
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, note)
                     VALUES (?, ?, NULL, ?, 'storno', ?) RETURNING id",
                )
                .bind(host_user_id)
                .bind(released_by)
                .bind(refund)
                .bind(GUEST_REFUND_NOTE)
                .fetch_one(&mut *conn)
                .await?;
                sqlx::query("UPDATE users SET credit = ROUND(credit + ?, 2) WHERE id = ?")
                    .bind(refund)
                    .bind(host_user_id)
                    .execute(&mut *conn)
                    .await?;
                out.refunded = crate::db::users::round_cents(out.refunded + refund);
                Some(id)
            }
            _ => None,
        };
        sqlx::query(
            "UPDATE booking_guest_charges
                SET released_at = datetime('now'), refund_transaction_id = ?
              WHERE id = ?",
        )
        .bind(refund_id)
        .bind(link_id)
        .execute(&mut *conn)
        .await?;
        out.released += 1;
    }
    Ok(out)
}

/// A successful guest-count change.
#[derive(Debug, Clone, PartialEq)]
pub struct GuestChange {
    pub template_id: i64,
    pub date: String,
    pub old_guests: i64,
    pub guests: i64,
    /// Credit debited now (staff added guests to an already-charged booking).
    pub charged: f64,
    /// Credit refunded now (staff removed guests from a charged booking).
    pub refunded: f64,
}

/// Result of [`set_guests`].
#[derive(Debug, Clone, PartialEq)]
pub enum SetGuestsResult {
    /// No ACTIVE booking with that id.
    NotActive,
    /// A non-staff change of an already CHARGED booking — binding, nothing
    /// changed (the charger won the race against the route's deadline check).
    AlreadyCharged,
    /// The extra guests do not fit into the class's remaining seats.
    ClassFull,
    Changed(GuestChange),
}

/// Set the guest count of one ACTIVE booking to `guests` (the caller
/// validated `0..=MAX_GUESTS`). `actor` is recorded as `staff_id` on any
/// charge/refund row written; `staff` decides whether a charged booking may
/// still change. Everything runs in ONE transaction; the capacity guard is
/// the conditional UPDATE itself (atomic, no racy pre-check).
pub async fn set_guests(
    pool: &SqlitePool,
    booking_id: i64,
    guests: i64,
    actor: Option<i64>,
    staff: bool,
) -> Result<SetGuestsResult> {
    let mut tx = pool.begin().await?;

    let row: Option<(i64, i64, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT user_id, template_id, date, guests, charged_at
           FROM bookings WHERE id = ? AND cancelled_at IS NULL",
    )
    .bind(booking_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id, template_id, date, old_guests, charged_at)) = row else {
        tx.rollback().await?;
        return Ok(SetGuestsResult::NotActive);
    };
    let charged = charged_at.is_some();
    if charged && !staff {
        tx.rollback().await?;
        tracing::info!(
            booking_id,
            user_id,
            "guest change refused: booking already charged (binding)"
        );
        return Ok(SetGuestsResult::AlreadyCharged);
    }

    let mut change = GuestChange {
        template_id,
        date: date.clone(),
        old_guests,
        guests,
        charged: 0.0,
        refunded: 0.0,
    };
    if guests == old_guests {
        tx.rollback().await?;
        return Ok(SetGuestsResult::Changed(change));
    }

    let updated = sqlx::query(&format!(
        "UPDATE bookings SET guests = ?1
          WHERE id = ?2 AND cancelled_at IS NULL
            AND (?1 <= guests
                 OR (SELECT {SEATS_SQL} FROM bookings
                      WHERE template_id = ?3 AND date = ?4 AND cancelled_at IS NULL)
                    - guests + ?1
                    <= (SELECT capacity FROM class_templates WHERE id = ?3))"
    ))
    .bind(guests)
    .bind(booking_id)
    .bind(template_id)
    .bind(&date)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        tx.rollback().await?;
        tracing::info!(
            booking_id,
            user_id,
            template_id,
            class_date = %date,
            old_guests,
            guests,
            "guest change refused: not enough free seats"
        );
        return Ok(SetGuestsResult::ClassFull);
    }

    if charged {
        if guests > old_guests {
            let (service_id, price) = spinning_price(&mut *tx).await?;
            change.charged = charge_guest_seats(
                &mut *tx,
                booking_id,
                user_id,
                actor,
                service_id,
                price,
                guests - old_guests,
            )
            .await?;
        } else {
            change.refunded = release_guest_seats(
                &mut *tx,
                booking_id,
                user_id,
                actor,
                Some(old_guests - guests),
            )
            .await?
            .refunded;
        }
    }

    tx.commit().await?;
    tracing::info!(
        booking_id,
        user_id,
        template_id,
        class_date = %date,
        old_guests,
        guests,
        staff,
        actor = ?actor,
        charged_now = change.charged,
        refunded_now = change.refunded,
        "booking guests changed"
    );
    Ok(SetGuestsResult::Changed(change))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::classes::{create_booking, create_template, get_booking_count};
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn make_user(pool: &SqlitePool, email: &str, credit: f64) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO users (email, name, credit) VALUES (?, 'T', ?) RETURNING id",
        )
        .bind(email)
        .bind(credit)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn credit(pool: &SqlitePool, uid: i64) -> f64 {
        sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn changed(r: SetGuestsResult) -> GuestChange {
        match r {
            SetGuestsResult::Changed(c) => c,
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn guests_take_seats_and_respect_capacity() {
        let pool = setup().await;
        let tid = create_template(&pool, 1, "10:00", 60, None, 4)
            .await
            .unwrap();
        let host = make_user(&pool, "h@x", 0.0).await;
        let other = make_user(&pool, "o@x", 0.0).await;
        let late = make_user(&pool, "l@x", 0.0).await;
        let b = create_booking(&pool, tid, "2026-11-02", host, None, "manual")
            .await
            .unwrap();
        create_booking(&pool, tid, "2026-11-02", other, None, "manual")
            .await
            .unwrap();

        // 2 seats taken, 2 free: +3 guests does not fit, +2 does.
        assert_eq!(
            set_guests(&pool, b, 3, Some(host), false).await.unwrap(),
            SetGuestsResult::ClassFull
        );
        let c = changed(set_guests(&pool, b, 2, Some(host), false).await.unwrap());
        assert_eq!((c.old_guests, c.guests), (0, 2));
        assert_eq!(
            get_booking_count(&pool, tid, "2026-11-02").await.unwrap(),
            4
        );

        // The class is now full in seats — a new booking is refused.
        let r = create_booking(&pool, tid, "2026-11-02", late, None, "manual").await;
        assert!(matches!(r, Err(crate::db::DbError::ClassFull)));

        // Reducing always fits and frees the seat again.
        changed(set_guests(&pool, b, 1, Some(host), false).await.unwrap());
        create_booking(&pool, tid, "2026-11-02", late, None, "manual")
            .await
            .unwrap();
        assert_eq!(
            get_booking_count(&pool, tid, "2026-11-02").await.unwrap(),
            4
        );
    }

    #[tokio::test]
    async fn uncharged_change_writes_no_money() {
        let pool = setup().await;
        let tid = create_template(&pool, 2, "18:00", 60, None, 10)
            .await
            .unwrap();
        let host = make_user(&pool, "u@x", 10.0).await;
        let b = create_booking(&pool, tid, "2026-11-03", host, None, "manual")
            .await
            .unwrap();
        let c = changed(set_guests(&pool, b, 3, Some(host), false).await.unwrap());
        assert_eq!((c.charged, c.refunded), (0.0, 0.0));
        assert_eq!(credit(&pool, host).await, 10.0);
        let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM booking_guest_charges")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(links, 0);
    }

    #[tokio::test]
    async fn customer_cannot_change_a_charged_booking_staff_can() {
        let pool = setup().await;
        let tid = create_template(&pool, 3, "18:00", 60, None, 10)
            .await
            .unwrap();
        let host = make_user(&pool, "c@x", 20.0).await;
        let staff = make_user(&pool, "s@x", 0.0).await;
        let b = create_booking(&pool, tid, "2026-11-04", host, None, "manual")
            .await
            .unwrap();
        changed(set_guests(&pool, b, 2, Some(host), false).await.unwrap());
        assert!(crate::jobs::charger::charge_one(&pool, b).await.unwrap());
        // host 5 + 2 guests x 5 = 15 debited.
        assert_eq!(credit(&pool, host).await, 5.0);

        assert_eq!(
            set_guests(&pool, b, 0, Some(host), false).await.unwrap(),
            SetGuestsResult::AlreadyCharged
        );

        // Staff adds one more guest after the charge -> charged at once.
        let c = changed(set_guests(&pool, b, 3, Some(staff), true).await.unwrap());
        assert_eq!(c.charged, 5.0);
        assert_eq!(credit(&pool, host).await, 0.0);
        let staff_on_charge: Option<i64> = sqlx::query_scalar(
            "SELECT t.staff_id FROM booking_guest_charges g JOIN transactions t ON t.id = g.transaction_id
              ORDER BY g.id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(staff_on_charge, Some(staff));

        // Staff removes two guests -> two storno refunds, service_id NULL.
        let c = changed(set_guests(&pool, b, 1, Some(staff), true).await.unwrap());
        assert_eq!(c.refunded, 10.0);
        assert_eq!(credit(&pool, host).await, 10.0);
        let refunds: Vec<(f64, Option<i64>, Option<i64>, String)> = sqlx::query_as(
            "SELECT amount, staff_id, service_id, action FROM transactions
              WHERE id IN (SELECT refund_transaction_id FROM booking_guest_charges
                            WHERE refund_transaction_id IS NOT NULL)",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(refunds.len(), 2);
        for r in &refunds {
            assert_eq!(r, &(5.0, Some(staff), None, "storno".to_string()));
        }
        let held: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM booking_guest_charges WHERE booking_id = ? AND released_at IS NULL",
        )
        .bind(b)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(held, 1, "one charged guest seat still held");
    }

    #[tokio::test]
    async fn voided_guest_charge_is_released_without_refund() {
        let pool = setup().await;
        let tid = create_template(&pool, 4, "18:00", 60, None, 10)
            .await
            .unwrap();
        let host = make_user(&pool, "v@x", 10.0).await;
        let b = create_booking(&pool, tid, "2026-11-05", host, None, "manual")
            .await
            .unwrap();
        changed(set_guests(&pool, b, 1, Some(host), false).await.unwrap());
        crate::jobs::charger::charge_one(&pool, b).await.unwrap();
        sqlx::query(
            "UPDATE transactions SET deleted_at = datetime('now')
              WHERE id = (SELECT transaction_id FROM booking_guest_charges WHERE booking_id = ?)",
        )
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
        let c = changed(set_guests(&pool, b, 0, None, true).await.unwrap());
        assert_eq!(c.refunded, 0.0);
        let released: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM booking_guest_charges WHERE booking_id = ? AND released_at IS NOT NULL",
        )
        .bind(b)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(released, 1);
    }

    #[tokio::test]
    async fn cancelled_booking_is_not_active() {
        let pool = setup().await;
        let tid = create_template(&pool, 5, "18:00", 60, None, 10)
            .await
            .unwrap();
        let host = make_user(&pool, "n@x", 0.0).await;
        let b = create_booking(&pool, tid, "2026-11-06", host, None, "manual")
            .await
            .unwrap();
        crate::db::booking_cancel::cancel_booking(
            &pool,
            b,
            Some(host),
            crate::db::booking_cancel::CancelKind::Customer,
        )
        .await
        .unwrap();
        assert_eq!(
            set_guests(&pool, b, 1, Some(host), false).await.unwrap(),
            SetGuestsResult::NotActive
        );
    }
}
