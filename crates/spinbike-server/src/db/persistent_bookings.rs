use crate::db::error::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct PersistentBookingRow {
    pub id: i64,
    pub user_id: i64,
    pub template_id: i64,
    pub created_at: String,
    pub ended_at: Option<String>,
}

/// Create a persistent booking subscription, or return the existing active
/// subscription's id if one already exists for (user, template).
pub async fn create(pool: &SqlitePool, user_id: i64, template_id: i64) -> Result<i64> {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM persistent_bookings
         WHERE user_id = ? AND template_id = ? AND ended_at IS NULL",
    )
    .bind(user_id)
    .bind(template_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO persistent_bookings (user_id, template_id) VALUES (?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(template_id)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// End the active subscription for (user, template). Returns rows affected.
pub async fn end(pool: &SqlitePool, user_id: i64, template_id: i64) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE persistent_bookings SET ended_at = datetime('now')
         WHERE user_id = ? AND template_id = ? AND ended_at IS NULL",
    )
    .bind(user_id)
    .bind(template_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// End the active subscription for (user, template) AND cancel that user's
/// FUTURE, still-uncharged bookings it created (#382 R5) — `source =
/// 'persistent'` only (a manual booking for the same class stays), and only
/// while the class day's 09:00 binding cutoff has not passed yet at the
/// gym-local `now` (a booking past the cutoff is binding: it is or is about
/// to be charged by the next charger tick). The cancellations carry
/// `cancel_kind = 'unsubscribe'`, so a later re-subscribe may re-create those
/// dates (unlike a customer/staff cancel). Returns `(subscriptions ended,
/// bookings cancelled)`. One transaction.
pub async fn end_and_cancel_future(
    pool: &SqlitePool,
    user_id: i64,
    template_id: i64,
    now: chrono::NaiveDateTime,
) -> Result<(u64, u64)> {
    let mut tx = pool.begin().await?;
    let counts = end_and_cancel_in(&mut tx, user_id, template_id, now).await?;
    tx.commit().await?;
    Ok(counts)
}

/// [`end_and_cancel_future`] on an open connection/transaction, so
/// [`set_subscriptions`] can apply a whole slot set atomically.
async fn end_and_cancel_in(
    conn: &mut sqlx::SqliteConnection,
    user_id: i64,
    template_id: i64,
    now: chrono::NaiveDateTime,
) -> Result<(u64, u64)> {
    let ended = sqlx::query(
        "UPDATE persistent_bookings SET ended_at = datetime('now')
         WHERE user_id = ? AND template_id = ? AND ended_at IS NULL",
    )
    .bind(user_id)
    .bind(template_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    // `date || ' ' || '09:00:00'` vs the bound `YYYY-MM-DD HH:MM:SS` gym-local
    // now: both 19-char space-ISO strings, so byte order == chronological.
    let cancelled = sqlx::query(
        "UPDATE bookings
            SET cancelled_at = datetime('now'), cancel_kind = 'unsubscribe'
          WHERE user_id = ? AND template_id = ? AND source = 'persistent'
            AND charged_at IS NULL AND cancelled_at IS NULL
            AND (date || ' ' || ?) > ?",
    )
    .bind(user_id)
    .bind(template_id)
    .bind(crate::booking_rules::CHARGE_CUTOFF_TIME)
    .bind(now.format("%Y-%m-%d %H:%M:%S").to_string())
    .execute(&mut *conn)
    .await?
    .rows_affected();
    tracing::info!(
        user_id,
        template_id,
        ended,
        cancelled,
        "persistent booking ended; future uncharged bookings cancelled"
    );
    Ok((ended, cancelled))
}

/// One active class template as offered to a customer for a recurring
/// weekly slot (#382 R5), with whether they are subscribed to it.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct SlotRow {
    pub template_id: i64,
    pub weekday: i64,
    pub start_time: String,
    pub instructor_name: Option<String>,
    pub subscribed: bool,
}

/// Every ACTIVE class template with the user's subscription flag, in
/// weekday/start order — the customer's "Pravidelne terminy" list.
pub async fn list_slots_for_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<SlotRow>> {
    let rows = sqlx::query_as::<_, SlotRow>(
        "SELECT t.id AS template_id, t.weekday, t.start_time, i.name AS instructor_name,
                EXISTS (SELECT 1 FROM persistent_bookings pb
                         WHERE pb.user_id = ? AND pb.template_id = t.id
                           AND pb.ended_at IS NULL) AS subscribed
           FROM class_templates t
           LEFT JOIN instructors i ON i.id = t.instructor_id
          WHERE t.active = 1
          ORDER BY t.weekday, t.start_time",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Make the user's set of active subscriptions EXACTLY `template_ids`
/// (#382 R5, `PUT /api/my/persistent-bookings`): subscribe the new ones,
/// end (and cancel the future uncharged bookings of) the dropped ones. The
/// caller validates that every id is an active template. Returns
/// `(added, removed)` template ids. Idempotent — re-sending the same set
/// changes nothing.
pub async fn set_subscriptions(
    pool: &SqlitePool,
    user_id: i64,
    template_ids: &[i64],
    now: chrono::NaiveDateTime,
) -> Result<(Vec<i64>, Vec<i64>)> {
    // One transaction: a failure part-way never leaves a half-applied set.
    let mut tx = pool.begin().await?;
    let current: Vec<i64> = sqlx::query_scalar(
        "SELECT template_id FROM persistent_bookings
         WHERE user_id = ? AND ended_at IS NULL ORDER BY template_id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut added = Vec::new();
    for &tid in template_ids {
        if !current.contains(&tid) && !added.contains(&tid) {
            sqlx::query("INSERT INTO persistent_bookings (user_id, template_id) VALUES (?, ?)")
                .bind(user_id)
                .bind(tid)
                .execute(&mut *tx)
                .await?;
            added.push(tid);
        }
    }
    let mut removed = Vec::new();
    for tid in current {
        if !template_ids.contains(&tid) {
            end_and_cancel_in(&mut tx, user_id, tid, now).await?;
            removed.push(tid);
        }
    }
    tx.commit().await?;
    tracing::info!(
        user_id,
        ?added,
        ?removed,
        "recurring spinning slots updated"
    );
    Ok((added, removed))
}

pub async fn list_for_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<PersistentBookingRow>> {
    let rows = sqlx::query_as::<_, PersistentBookingRow>(
        "SELECT id, user_id, template_id, created_at, ended_at
         FROM persistent_bookings WHERE user_id = ? AND ended_at IS NULL
         ORDER BY template_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_active_all(pool: &SqlitePool) -> Result<Vec<PersistentBookingRow>> {
    let rows = sqlx::query_as::<_, PersistentBookingRow>(
        "SELECT id, user_id, template_id, created_at, ended_at
         FROM persistent_bookings WHERE ended_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed(pool: &SqlitePool) -> (i64, i64) {
        let uid: i64 =
            sqlx::query_scalar("INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id")
                .fetch_one(pool)
                .await
                .unwrap();
        // V6 already seeded Mon 18:00 — reuse that template.
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        (uid, tid)
    }

    #[tokio::test]
    async fn create_then_list_returns_row() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let _ = create(&pool, uid, tid).await.unwrap();
        let rows = list_for_user(&pool, uid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].template_id, tid);
    }

    #[tokio::test]
    async fn end_marks_row_and_removes_from_active_list() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let _ = create(&pool, uid, tid).await.unwrap();
        let affected = end(&pool, uid, tid).await.unwrap();
        assert_eq!(affected, 1);
        let rows = list_for_user(&pool, uid).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn create_is_idempotent_when_active_row_exists() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let first = create(&pool, uid, tid).await.unwrap();
        let second = create(&pool, uid, tid).await.unwrap();
        assert_eq!(first, second, "second call must return the same id");
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM persistent_bookings WHERE user_id=? AND template_id=? AND ended_at IS NULL"
        ).bind(uid).bind(tid).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn list_active_all_returns_everything_unended() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        let _ = create(&pool, uid, tid).await.unwrap();
        let all = list_active_all(&pool).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    /// #382 R5: ending a subscription cancels only FUTURE, still-uncharged
    /// PERSISTENT bookings before their 09:00 cutoff, marked 'unsubscribe';
    /// a manual booking, a charged one and one past its cutoff stay.
    #[tokio::test]
    async fn end_and_cancel_future_cancels_only_cancellable_persistent_bookings() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, tid) = seed(&pool).await;
        create(&pool, uid, tid).await.unwrap();
        let book = |date: &'static str, source: &'static str| {
            let pool = pool.clone();
            async move {
                crate::db::classes::create_booking(&pool, tid, date, uid, None, source)
                    .await
                    .unwrap()
            }
        };
        let future = book("2026-10-26", "persistent").await;
        let today_before_cutoff = book("2026-10-12", "persistent").await;
        let manual = book("2026-11-02", "manual").await;
        let charged = book("2026-11-09", "persistent").await;
        sqlx::query("UPDATE bookings SET charged_at = datetime('now') WHERE id = ?")
            .bind(charged)
            .execute(&pool)
            .await
            .unwrap();

        // 08:30 on 2026-10-12: that day's cutoff has not passed yet.
        let now = chrono::NaiveDateTime::parse_from_str("2026-10-12 08:30:00", "%Y-%m-%d %H:%M:%S")
            .unwrap();
        let (ended, cancelled) = end_and_cancel_future(&pool, uid, tid, now).await.unwrap();
        assert_eq!((ended, cancelled), (1, 2));

        let kind = |id: i64| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, Option<String>>(
                    "SELECT cancel_kind FROM bookings WHERE id = ?",
                )
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
            }
        };
        assert_eq!(kind(future).await.as_deref(), Some("unsubscribe"));
        assert_eq!(
            kind(today_before_cutoff).await.as_deref(),
            Some("unsubscribe")
        );
        assert_eq!(kind(manual).await, None, "manual booking untouched");
        assert_eq!(kind(charged).await, None, "charged booking is binding");

        // Past the cutoff the same-day booking is binding.
        create(&pool, uid, tid).await.unwrap();
        let today2 = book("2026-10-19", "persistent").await;
        let at_nine =
            chrono::NaiveDateTime::parse_from_str("2026-10-19 09:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap();
        let (_, cancelled2) = end_and_cancel_future(&pool, uid, tid, at_nine)
            .await
            .unwrap();
        assert_eq!(cancelled2, 0);
        assert_eq!(kind(today2).await, None);
    }

    /// #382 R5: `set_subscriptions` makes the active set exactly the given
    /// ids and is idempotent; `list_slots_for_user` reflects it.
    #[tokio::test]
    async fn set_subscriptions_is_exact_and_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (uid, _) = seed(&pool).await;
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM class_templates ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        let now = chrono::NaiveDateTime::parse_from_str("2026-10-12 08:00:00", "%Y-%m-%d %H:%M:%S")
            .unwrap();

        let (added, removed) = set_subscriptions(&pool, uid, &[ids[0], ids[1]], now)
            .await
            .unwrap();
        assert_eq!((added, removed), (vec![ids[0], ids[1]], vec![]));
        let (added, removed) = set_subscriptions(&pool, uid, &[ids[0], ids[1]], now)
            .await
            .unwrap();
        assert!(added.is_empty() && removed.is_empty(), "idempotent");
        let (added, removed) = set_subscriptions(&pool, uid, &[ids[1], ids[2]], now)
            .await
            .unwrap();
        assert_eq!((added, removed), (vec![ids[2]], vec![ids[0]]));

        let slots = list_slots_for_user(&pool, uid).await.unwrap();
        assert_eq!(slots.len(), 4, "every active template is offered");
        let subscribed: Vec<i64> = slots
            .iter()
            .filter(|s| s.subscribed)
            .map(|s| s.template_id)
            .collect();
        assert_eq!(subscribed, vec![ids[1], ids[2]]);
        assert!(slots.iter().all(|s| s.instructor_name.is_some()));
    }
}
