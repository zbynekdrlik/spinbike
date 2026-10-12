//! Booking cancellation with provenance + the staff refund (#382 R2/R3).
//!
//! The deadline POLICY (a customer only until 09:00 on the class day) lives
//! in `crate::booking_rules` and is checked by the route; this module records
//! the cancellation and, for a staff cancel of a charged booking, reverses
//! the charge — all in ONE transaction. `bookings.cancel_kind` (V31) also
//! tells the materialiser which dates it may never re-create (see
//! `.claude/rules/spinning-booking.md`).

use crate::db::error::Result;
use sqlx::SqlitePool;

/// In which capacity a booking is cancelled — persisted as
/// `bookings.cancel_kind` (V31). It decides two things: whether a charged
/// booking is refunded (only [`CancelKind::Staff`]), and whether the
/// materialiser may ever re-create that (user, template, date) — never after
/// a `Customer`/`Staff` cancellation, but yes after an `Unsubscribe` (the
/// system ending a subscription the customer may later re-enable; written by
/// `db::persistent_bookings::end_and_cancel_future`, the only other cancel
/// path).
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

/// What a successful cancellation did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CancelOutcome {
    /// The (rounded) amount credited back, when a staff cancel reversed a
    /// non-zero charge.
    pub refunded: Option<f64>,
    /// The separate refund `transactions` row, linked from the booking.
    pub refund_transaction_id: Option<i64>,
}

/// Result of [`cancel_booking`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CancelResult {
    /// No ACTIVE booking with that id (missing, or already cancelled — e.g. a
    /// raced double cancel).
    NotActive,
    /// A non-staff cancel of a booking that is already CHARGED — binding,
    /// nothing changed. Closes the race where the 09:00 charger charges the
    /// booking between the route's deadline check and this transaction.
    AlreadyCharged,
    /// Cancelled (and refunded when applicable).
    Cancelled(CancelOutcome),
}

/// Cancel one ACTIVE booking.
///
/// For [`CancelKind::Staff`], a booking charged a NON-ZERO amount (a pass
/// holder's 0 EUR visit needs nothing; a charge row someone already voided
/// is not refunded again) gets ONE separate `storno` ledger row with the
/// positive amount and the cancelling staff id, the customer is credited,
/// and the row is linked via `bookings.refund_transaction_id`. The refund row
/// carries NO `service_id` — like the manual desk storno, so the per-service
/// visit counters (`class_visit_filter_sql`) and `last_visit_at` never count
/// a refund as another visit; the link to the booking is the id. The amount
/// is rounded ONCE and reused for the credit UPDATE and the ledger INSERT
/// (`money-rounding.md`). Everything runs in ONE transaction.
pub async fn cancel_booking(
    pool: &SqlitePool,
    booking_id: i64,
    cancelled_by: Option<i64>,
    kind: CancelKind,
) -> Result<CancelResult> {
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
        return Ok(CancelResult::NotActive);
    };

    if kind != CancelKind::Staff && charged_at.is_some() {
        tx.rollback().await?;
        tracing::info!(
            booking_id,
            user_id,
            kind = kind.as_str(),
            "booking cancel refused: already charged (binding)"
        );
        return Ok(CancelResult::AlreadyCharged);
    }

    let mut outcome = CancelOutcome {
        refunded: None,
        refund_transaction_id: None,
    };

    if kind == CancelKind::Staff
        && charged_at.is_some()
        && let Some(charge_id) = charge_txn_id
    {
        let amount: Option<f64> = sqlx::query_scalar(
            "SELECT amount FROM transactions WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(charge_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(amount) = amount
            && amount < 0.0
        {
            let refund = crate::db::users::round_cents(-amount);
            let refund_id: i64 = sqlx::query_scalar(
                "INSERT INTO transactions (user_id, staff_id, service_id, amount, action, note)
                 VALUES (?, ?, NULL, ?, 'storno', ?) RETURNING id",
            )
            .bind(user_id)
            .bind(cancelled_by)
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

    let updated = sqlx::query(
        "UPDATE bookings
            SET cancelled_at = datetime('now'), cancelled_by = ?, cancel_kind = ?,
                refund_transaction_id = ?
          WHERE id = ? AND cancelled_at IS NULL",
    )
    .bind(cancelled_by)
    .bind(kind.as_str())
    .bind(outcome.refund_transaction_id)
    .bind(booking_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        // Cancelled by someone else since the SELECT above: undo our refund.
        tx.rollback().await?;
        return Ok(CancelResult::NotActive);
    }

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
    Ok(CancelResult::Cancelled(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::classes::{create_booking, create_template};
    use crate::db::{create_memory_pool, run_migrations};

    async fn setup() -> SqlitePool {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn make_user(pool: &SqlitePool, email: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO users (email, name) VALUES (?, 'T') RETURNING id")
            .bind(email)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Simulate the charger's write on `booking_id`: a charge row of `amount`
    /// (optionally already voided) linked from the booking.
    async fn charge(pool: &SqlitePool, booking_id: i64, user_id: i64, amount: f64, voided: bool) {
        let charge_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, service_id, amount, action, deleted_at)
             VALUES (?, (SELECT id FROM services WHERE kind = 'group_class'), ?, 'visit',
                     CASE WHEN ? THEN datetime('now') END)
             RETURNING id",
        )
        .bind(user_id)
        .bind(amount)
        .bind(voided)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE bookings SET charged_at = datetime('now'), charge_transaction_id = ? WHERE id = ?",
        )
        .bind(charge_id)
        .bind(booking_id)
        .execute(pool)
        .await
        .unwrap();
    }

    fn outcome(res: CancelResult) -> CancelOutcome {
        match res {
            CancelResult::Cancelled(o) => o,
            other => panic!("expected Cancelled, got {other:?}"),
        }
    }

    /// A customer cancel records its kind, never refunds; a second cancel of
    /// the same booking is a no-op (`NotActive`), not an error.
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
        let out = outcome(
            cancel_booking(&pool, bid, Some(uid), CancelKind::Customer)
                .await
                .unwrap(),
        );
        assert_eq!(out.refunded, None);
        let (kind, by): (Option<String>, Option<i64>) =
            sqlx::query_as("SELECT cancel_kind, cancelled_by FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kind.as_deref(), Some("customer"));
        assert_eq!(by, Some(uid));
        assert_eq!(
            cancel_booking(&pool, bid, Some(uid), CancelKind::Customer)
                .await
                .unwrap(),
            CancelResult::NotActive
        );
    }

    /// The race the route cannot see: the 09:00 charger charged the booking
    /// after the route's deadline check — a customer cancel must then be
    /// refused (binding), nothing written, booking still active.
    #[tokio::test]
    async fn customer_cancel_of_a_charged_booking_is_refused() {
        let pool = setup().await;
        let uid = make_user(&pool, "race@test.com").await;
        let tid = create_template(&pool, 1, "18:00", 60, None, 10)
            .await
            .unwrap();
        let bid = create_booking(&pool, tid, "2026-04-14", uid, None, "manual")
            .await
            .unwrap();
        charge(&pool, bid, uid, -5.0, false).await;
        assert_eq!(
            cancel_booking(&pool, bid, Some(uid), CancelKind::Customer)
                .await
                .unwrap(),
            CancelResult::AlreadyCharged
        );
        let cancelled: Option<String> =
            sqlx::query_scalar("SELECT cancelled_at FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cancelled, None);
    }

    /// Staff cancel of a charged booking writes ONE separate storno row with
    /// the rounded positive amount + staff id and NO service (so visit
    /// counters never count it), credits the user and links the row; a voided
    /// charge is not refunded again.
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
        // A -3.301 drift charge proves the refund is rounded once.
        charge(&pool, bid, uid, -3.301, false).await;

        let out = outcome(
            cancel_booking(&pool, bid, Some(staff), CancelKind::Staff)
                .await
                .unwrap(),
        );
        assert_eq!(out.refunded, Some(3.3));
        let refund_id = out.refund_transaction_id.unwrap();
        let (amount, staff_id, action, service_id): (f64, Option<i64>, String, Option<i64>) =
            sqlx::query_as(
                "SELECT amount, staff_id, action, service_id FROM transactions WHERE id = ?",
            )
            .bind(refund_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(amount, 3.3);
        assert_eq!(staff_id, Some(staff));
        assert_eq!(action, "storno");
        assert_eq!(service_id, None, "a refund is never a class visit");
        let credit: f64 = sqlx::query_scalar("SELECT credit FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(credit, 0.0, "-3.30 + 3.30, rounded once");
        let (linked, kind): (Option<i64>, Option<String>) =
            sqlx::query_as("SELECT refund_transaction_id, cancel_kind FROM bookings WHERE id = ?")
                .bind(bid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked, Some(refund_id));
        assert_eq!(kind.as_deref(), Some("staff"));

        // A voided charge: no refund.
        let bid2 = create_booking(&pool, tid, "2026-04-21", uid, None, "manual")
            .await
            .unwrap();
        charge(&pool, bid2, uid, -5.0, true).await;
        let out2 = outcome(
            cancel_booking(&pool, bid2, Some(staff), CancelKind::Staff)
                .await
                .unwrap(),
        );
        assert_eq!(out2.refunded, None, "a voided charge is not refunded again");
    }

    /// A pass holder's 0 EUR visit: staff cancel writes no refund row.
    #[tokio::test]
    async fn staff_cancel_of_a_zero_visit_writes_no_refund() {
        let pool = setup().await;
        let staff = make_user(&pool, "st0@test.com").await;
        let uid = make_user(&pool, "pass@test.com").await;
        let tid = create_template(&pool, 1, "18:00", 60, None, 10)
            .await
            .unwrap();
        let bid = create_booking(&pool, tid, "2026-04-14", uid, None, "manual")
            .await
            .unwrap();
        charge(&pool, bid, uid, 0.0, false).await;
        let out = outcome(
            cancel_booking(&pool, bid, Some(staff), CancelKind::Staff)
                .await
                .unwrap(),
        );
        assert_eq!(
            out,
            CancelOutcome {
                refunded: None,
                refund_transaction_id: None
            }
        );
        let stornos: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transactions WHERE user_id = ? AND action = 'storno'",
        )
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stornos, 0);
    }
}
