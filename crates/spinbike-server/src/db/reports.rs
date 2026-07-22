use crate::db::error::Result;
use sqlx::SqlitePool;

use spinbike_core::reports::{KpiSummary, ReportEvent};

/// Pagination cursor: `(created_at, id)` from the last row of the prior page.
/// Encoded over the wire as `"<created_at>|<id>"`. Composite key avoids
/// dropping rows when SQLite's second-precision `datetime('now')` produces
/// duplicate `created_at` values across the page boundary.
pub fn parse_before_cursor(before: &str) -> Option<(String, i64)> {
    let (ts, id) = before.split_once('|')?;
    let id: i64 = id.parse().ok()?;
    Some((ts.to_string(), id))
}

/// Fetch all non-voided transactions for a single day, joined with card + service data.
/// Returns events sorted by (created_at, id) DESC and a KpiSummary aggregated over the whole day.
///
/// `date` is the GYM-LOCAL (Europe/Bratislava) calendar day — bucketed via
/// `util::bratislava_day_range_utc`'s half-open UTC-instant range against the
/// UTC-instant `created_at` column, never a raw `date(created_at)` (#251:
/// that compared the UTC calendar date directly, silently hiding/miscounting
/// any transaction created 00:00-02:00 Bratislava-local until the UTC date
/// caught up — the same bug class #239/#240/#242/#246 already fixed at
/// every other call site, missed here).
pub async fn day_report(
    pool: &SqlitePool,
    date: chrono::NaiveDate,
    limit: i64,
    before: Option<String>,
) -> Result<(KpiSummary, Vec<ReportEvent>, bool)> {
    let (start, end) = crate::util::bratislava_day_range_utc(date);
    let start_str = start.format("%Y-%m-%d %H:%M:%S").to_string();
    let end_str = end.format("%Y-%m-%d %H:%M:%S").to_string();
    let before_parsed = before.as_deref().and_then(parse_before_cursor);

    // Events — paginated with composite (created_at, id) cursor for stable
    // ordering even when multiple rows share a second-precision timestamp.
    let mut query = String::from(
        "SELECT t.id, t.user_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                u.name AS card_name,
                u.card_code AS barcode,
                s.name_sk AS service_name_sk, s.name_en AS service_name_en, s.kind AS service_kind, t.note
         FROM transactions t
         LEFT JOIN users u ON u.id = t.user_id  -- no deleted_at filter: historical txns for soft-deleted users still display (name/code shows blank)
         LEFT JOIN services s ON s.id = t.service_id
         WHERE t.created_at >= ? AND t.created_at < ?
           AND t.deleted_at IS NULL",
    );
    if before_parsed.is_some() {
        // (created_at, id) < (cursor_ts, cursor_id) in lexicographic order.
        query.push_str(" AND (t.created_at < ? OR (t.created_at = ? AND t.id < ?))");
    }
    query.push_str(" ORDER BY t.created_at DESC, t.id DESC LIMIT ?");

    let mut q = sqlx::query_as::<_, DbEventRow>(&query)
        .bind(&start_str)
        .bind(&end_str);
    if let Some((ref ts, id)) = before_parsed {
        q = q.bind(ts).bind(ts).bind(id);
    }
    q = q.bind(limit + 1); // fetch one extra to know if there's more

    let mut rows = q.fetch_all(pool).await?;
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    let events: Vec<ReportEvent> = rows.into_iter().map(Into::into).collect();

    // Class-visit names bound from spinbike_core::services constants — `?3`
    // is Spinning (for the new spinning_visits aggregate) and `?4` is
    // Fitness (so the attendance aggregate still counts both).
    // NOTE: `ELSE 0.0` (not `ELSE 0`) is required for cash_in_eur — otherwise
    // SQLite returns INTEGER for the SUM when no rows match and sqlx refuses
    // to decode that into f64.
    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE name_en = ?3)
                 AND (
                   (action = 'charge' AND amount < 0 AND valid_until IS NULL)
                   OR action = 'visit'
                 )
                THEN 1 ELSE 0
              END
            ), 0) AS spinning_visits,
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE name_en IN (?3, ?4))
                 AND (
                   (action = 'charge' AND amount < 0 AND valid_until IS NULL)
                   OR action = 'visit'
                 )
                THEN 1 ELSE 0
              END
            ), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE created_at >= ?1 AND created_at < ?2 AND deleted_at IS NULL",
    )
    .bind(&start_str)
    .bind(&end_str)
    .bind(spinbike_core::services::SPINNING_NAME_EN)
    .bind(spinbike_core::services::FITNESS_NAME_EN)
    .fetch_one(pool)
    .await?;

    let kpi = KpiSummary {
        spinning_visits: kpi_row.spinning_visits,
        attendance: kpi_row.attendance,
        passes_sold: kpi_row.passes_sold,
        cash_in_eur: kpi_row.cash_in_eur,
    };

    Ok((kpi, events, has_more))
}

#[derive(sqlx::FromRow)]
struct DbKpiRow {
    spinning_visits: i64,
    attendance: i64,
    passes_sold: i64,
    cash_in_eur: f64,
}

#[derive(sqlx::FromRow)]
struct DbEventRow {
    id: i64,
    user_id: Option<i64>,
    card_name: Option<String>,
    barcode: Option<String>,
    action: String,
    amount: f64,
    service_name_sk: Option<String>,
    service_name_en: Option<String>,
    service_kind: Option<String>,
    created_at: String,
    valid_until: Option<chrono::NaiveDate>,
    deleted_at: Option<String>,
    /// Free-text staff note (≤200 chars). NULL when no note was recorded.
    /// Migration v10 guarantees the column exists, so no `#[sqlx(default)]` —
    /// a missing column should error loudly.
    note: Option<String>,
}

impl From<DbEventRow> for ReportEvent {
    fn from(r: DbEventRow) -> Self {
        ReportEvent {
            id: r.id,
            user_id: r.user_id,
            card_name: r.card_name.filter(|s| !s.trim().is_empty()),
            barcode: r.barcode,
            action: r.action,
            amount: r.amount,
            service_name_sk: r.service_name_sk,
            service_name_en: r.service_name_en,
            service_kind: r.service_kind,
            created_at: r.created_at,
            valid_until: r.valid_until,
            voided: r.deleted_at.is_some(),
            note: r.note,
        }
    }
}

pub const RANGE_MAX_DAYS: i64 = 93;

/// Fetch all non-voided transactions across a date range, aggregated.
/// Caller is responsible for enforcing `RANGE_MAX_DAYS`.
///
/// `from`/`to` are GYM-LOCAL (Europe/Bratislava) calendar days — bucketed via
/// the combined half-open UTC-instant range `[bratislava_day_range_utc(from).0,
/// bratislava_day_range_utc(to).1)`, never a raw `date(created_at) BETWEEN`
/// (#251 — see `day_report`'s doc comment for the full bug).
pub async fn range_report(
    pool: &SqlitePool,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    limit: i64,
    before: Option<String>,
) -> Result<(KpiSummary, Vec<ReportEvent>, bool)> {
    let (from_start, _) = crate::util::bratislava_day_range_utc(from);
    let (_, to_end) = crate::util::bratislava_day_range_utc(to);
    let from_str = from_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let to_str = to_end.format("%Y-%m-%d %H:%M:%S").to_string();
    let before_parsed = before.as_deref().and_then(parse_before_cursor);

    let mut query = String::from(
        "SELECT t.id, t.user_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                u.name AS card_name,
                u.card_code AS barcode,
                s.name_sk AS service_name_sk, s.name_en AS service_name_en, s.kind AS service_kind, t.note
         FROM transactions t
         LEFT JOIN users u ON u.id = t.user_id  -- no deleted_at filter: historical txns for soft-deleted users still display (name/code shows blank)
         LEFT JOIN services s ON s.id = t.service_id
         WHERE t.created_at >= ? AND t.created_at < ?
           AND t.deleted_at IS NULL",
    );
    if before_parsed.is_some() {
        query.push_str(" AND (t.created_at < ? OR (t.created_at = ? AND t.id < ?))");
    }
    query.push_str(" ORDER BY t.created_at DESC, t.id DESC LIMIT ?");

    let mut q = sqlx::query_as::<_, DbEventRow>(&query)
        .bind(&from_str)
        .bind(&to_str);
    if let Some((ref ts, id)) = before_parsed {
        q = q.bind(ts).bind(ts).bind(id);
    }
    q = q.bind(limit + 1);

    let mut rows = q.fetch_all(pool).await?;
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    let events: Vec<ReportEvent> = rows.into_iter().map(Into::into).collect();

    // Class-visit names bound from spinbike_core::services constants — see
    // day_report. Bind order: `?3` Spinning, `?4` Fitness.
    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE name_en = ?3)
                 AND (
                   (action = 'charge' AND amount < 0 AND valid_until IS NULL)
                   OR action = 'visit'
                 )
                THEN 1 ELSE 0
              END
            ), 0) AS spinning_visits,
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE name_en IN (?3, ?4))
                 AND (
                   (action = 'charge' AND amount < 0 AND valid_until IS NULL)
                   OR action = 'visit'
                 )
                THEN 1 ELSE 0
              END
            ), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE created_at >= ?1 AND created_at < ?2 AND deleted_at IS NULL",
    )
    .bind(&from_str)
    .bind(&to_str)
    .bind(spinbike_core::services::SPINNING_NAME_EN)
    .bind(spinbike_core::services::FITNESS_NAME_EN)
    .fetch_one(pool)
    .await?;

    Ok((
        KpiSummary {
            spinning_visits: kpi_row.spinning_visits,
            attendance: kpi_row.attendance,
            passes_sold: kpi_row.passes_sold,
            cash_in_eur: kpi_row.cash_in_eur,
        },
        events,
        has_more,
    ))
}

#[cfg(test)]
mod tests {
    // ----- Issue #23: NAVSTEVY/ATTENDANCE visit-count fix -----
    //
    // Today's attendance SQL counts ANY `amount < 0 AND valid_until IS NULL`
    // row, which wrongly includes Refreshments/Supplements/Card-activation-fee
    // charges AND wrongly excludes €0 `action='visit'` rows logged for
    // monthly-pass holders. Per CEO direction (#23), attendance should equal
    // (Fitness | Spinning) AND (paid charge | logged visit).
    //
    // The fixture is intentionally discriminating: it inserts 2 Refreshments
    // charges so the OLD SQL returns 5 (paid Fitness + paid Spinning + 2 ×
    // Refreshments + Card-fee) while the NEW SQL returns 4 (paid Fitness +
    // paid Spinning + free Fitness visit + free Spinning visit). A 1×
    // Refreshments fixture would coincidentally return 4 under both SQLs and
    // the test would not detect the bug. Do not change the count of
    // Refreshments rows without re-running the discriminator math.
    use crate::db::transactions::{create_transaction, create_transaction_with_valid_until};
    use crate::db::users::create_user;
    use crate::db::{create_memory_pool, run_migrations};
    use sqlx::SqlitePool;

    async fn setup_pool_with_user() -> (SqlitePool, i64) {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Create a test user to associate transactions with.
        let user_id = create_user(
            &pool,
            None,
            None,
            "Test User",
            None,
            None,
            None,
            "customer",
            Some(100.0),
            None,
            None,
        )
        .await
        .unwrap();
        (pool, user_id)
    }

    async fn service_id_by_name_en(pool: &SqlitePool, name_en: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?")
            .bind(name_en)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| panic!("service '{name_en}' missing from seed"))
    }

    #[tokio::test]
    async fn attendance_counts_only_fitness_and_spinning_visits() {
        let (pool, user_id) = setup_pool_with_user().await;

        let fitness_id =
            service_id_by_name_en(&pool, spinbike_core::services::FITNESS_NAME_EN).await;
        let spinning_id =
            service_id_by_name_en(&pool, spinbike_core::services::SPINNING_NAME_EN).await;
        let monthly_pass_id = service_id_by_name_en(&pool, "Monthly pass").await;
        let refreshments_id = service_id_by_name_en(&pool, "Refreshments").await;
        let card_fee_id = service_id_by_name_en(&pool, "Card activation fee").await;

        // 4 rows that SHOULD count.
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(fitness_id),
            -5.0,
            "charge",
            None,
        )
        .await
        .unwrap();
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(spinning_id),
            -5.0,
            "charge",
            None,
        )
        .await
        .unwrap();
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(fitness_id),
            0.0,
            "visit",
            None,
        )
        .await
        .unwrap();
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(spinning_id),
            0.0,
            "visit",
            None,
        )
        .await
        .unwrap();

        // 5 rows that should NOT count. TWO Refreshments rows so the buggy SQL
        // returns 5 and the fixed SQL returns 4 — the test would otherwise
        // pass against the bug. See header comment.
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(refreshments_id),
            -2.50,
            "charge",
            None,
        )
        .await
        .unwrap();
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(refreshments_id),
            -2.50,
            "charge",
            None,
        )
        .await
        .unwrap();
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(card_fee_id),
            -3.0,
            "charge",
            None,
        )
        .await
        .unwrap();
        let valid_until = chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap();
        create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(monthly_pass_id),
            -35.0,
            "charge",
            Some(valid_until),
            None,
        )
        .await
        .unwrap();
        create_transaction(&pool, Some(user_id), None, None, 10.0, "topup", None)
            .await
            .unwrap();

        // Use today's date — all `create_transaction*` calls default
        // `created_at = datetime('now')`, so day_report(today) sees them all.
        let today = chrono::Local::now().naive_local().date();

        let (day_kpi, _, _) = super::day_report(&pool, today, 50, None).await.unwrap();
        assert_eq!(
            day_kpi.attendance, 4,
            "day_report attendance must count only Fitness/Spinning paid+visit rows"
        );
        assert_eq!(
            day_kpi.spinning_visits, 2,
            "day_report spinning_visits = 1 paid Spinning charge + 1 zero-amount Spinning visit"
        );

        let (range_kpi, _, _) = super::range_report(&pool, today, today, 50, None)
            .await
            .unwrap();
        assert_eq!(
            range_kpi.attendance, 4,
            "range_report attendance must agree with day_report on the same date"
        );
        assert_eq!(
            range_kpi.spinning_visits, 2,
            "range_report spinning_visits must agree with day_report on the same date"
        );

        // Sanity: adjacent KPIs aren't disturbed by the change.
        // passes_sold counts valid_until-set rows: exactly 1.
        assert_eq!(day_kpi.passes_sold, 1);
        // cash_in_eur sums positive-amount rows: just the topup.
        assert!((day_kpi.cash_in_eur - 10.00).abs() < 0.001);
    }

    // ----- #251: day/range reports must bucket by Bratislava-LOCAL day, not
    // raw UTC calendar date -----
    //
    // Bratislava-local midnight of 2026-07-15 is UTC 2026-07-14 22:00:00
    // (CEST, UTC+2 in July). A transaction created at UTC 2026-07-14
    // 23:00:00 is therefore on Bratislava-LOCAL day 2026-07-15 (01:00 local)
    // but on raw-UTC-CALENDAR day 2026-07-14 — exactly the 00:00-02:00
    // Bratislava-local window the pre-fix `date(created_at) = ?` SQL gets
    // wrong (it matches the UTC date, not the gym's local day).
    #[tokio::test]
    async fn day_and_range_reports_bucket_by_bratislava_local_day_not_raw_utc_date() {
        let (pool, user_id) = setup_pool_with_user().await;
        let fitness_id =
            service_id_by_name_en(&pool, spinbike_core::services::FITNESS_NAME_EN).await;

        sqlx::query(
            "INSERT INTO transactions (user_id, service_id, amount, action, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(fitness_id)
        .bind(-5.0_f64)
        .bind("charge")
        .bind("2026-07-14 23:00:00")
        .execute(&pool)
        .await
        .unwrap();

        let bratislava_day = chrono::NaiveDate::from_ymd_opt(2026, 7, 15).unwrap();
        let (day_kpi, day_events, _) = super::day_report(&pool, bratislava_day, 50, None)
            .await
            .unwrap();
        assert_eq!(
            day_events.len(),
            1,
            "a transaction at 01:00 Bratislava-local on 2026-07-15 (23:00 UTC on \
             2026-07-14) must appear in day_report(2026-07-15) — the Bratislava-local \
             day, not the raw UTC calendar date"
        );
        assert_eq!(
            day_kpi.attendance, 1,
            "must count toward the Bratislava-local day's attendance"
        );

        let (range_kpi, range_events, _) =
            super::range_report(&pool, bratislava_day, bratislava_day, 50, None)
                .await
                .unwrap();
        assert_eq!(
            range_events.len(),
            1,
            "range_report must agree with day_report on the same Bratislava-local day"
        );
        assert_eq!(range_kpi.attendance, 1);

        // Must NOT also appear under the raw UTC calendar day — a range fix
        // that widened the match to include both days would be equally wrong
        // (double-counting), not just a different way to miss the boundary.
        let utc_day = chrono::NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
        let (utc_day_kpi, utc_day_events, _) =
            super::day_report(&pool, utc_day, 50, None).await.unwrap();
        assert_eq!(
            utc_day_events.len(),
            0,
            "must NOT appear under the raw UTC calendar day — it belongs to the \
             Bratislava-local day 2026-07-15 only"
        );
        assert_eq!(utc_day_kpi.attendance, 0);
    }
}
