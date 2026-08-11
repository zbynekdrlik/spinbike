use crate::db::error::Result;
use sqlx::SqlitePool;

use spinbike_core::reports::{CategoryRevenue, KpiSummary, ReportEvent};

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
) -> Result<(KpiSummary, Vec<CategoryRevenue>, Vec<ReportEvent>, bool)> {
    let (start, end) = crate::util::bratislava_day_range_utc(date);
    let start_str = start.format("%Y-%m-%d %H:%M:%S").to_string();
    let end_str = end.format("%Y-%m-%d %H:%M:%S").to_string();
    let before_parsed = before.as_deref().and_then(parse_before_cursor);

    // Events, KPI totals and per-category revenue are three independent
    // reads over the same [start_str, end_str) range — none consumes
    // another's output, so run them concurrently instead of one after
    // another (#341; the pool is max_connections(5), see db/mod.rs).
    let (events_result, kpi_result, category_result) = tokio::join!(
        events_between(pool, &start_str, &end_str, limit, before_parsed),
        kpi_between(pool, &start_str, &end_str),
        category_revenue_between(pool, &start_str, &end_str),
    );
    let (events, has_more) = events_result?;
    let kpi_row = kpi_result?;
    let kpi = KpiSummary {
        spinning_visits: kpi_row.spinning_visits,
        attendance: kpi_row.attendance,
        passes_sold: kpi_row.passes_sold,
        cash_in_eur: kpi_row.cash_in_eur,
    };
    let category_revenue = category_result?;

    Ok((kpi, category_revenue, events, has_more))
}

/// Paginated, non-voided transaction events for the half-open UTC-instant
/// range `[start_str, end_str)`, joined with card + service data — the
/// composite `(created_at, id)`-cursor query `day_report` and `range_report`
/// both need (#341: previously duplicated verbatim in each; the same
/// duplication `kpi_between`/`category_revenue_between` below were already
/// extracted to avoid).
async fn events_between(
    pool: &SqlitePool,
    start_str: &str,
    end_str: &str,
    limit: i64,
    before_parsed: Option<(String, i64)>,
) -> Result<(Vec<ReportEvent>, bool)> {
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
        .bind(start_str)
        .bind(end_str);
    if let Some((ref ts, id)) = before_parsed {
        q = q.bind(ts).bind(ts).bind(id);
    }
    q = q.bind(limit + 1); // fetch one extra to know if there's more

    let mut rows = q.fetch_all(pool).await?;
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    Ok((rows.into_iter().map(Into::into).collect(), has_more))
}

/// KPI counts/sums over the half-open UTC-instant range `[start_str,
/// end_str)` — shared by `day_report` and `range_report` (#339: this used
/// to be an identical ~30-line query literal hand-copied into both
/// functions, same pattern `category_revenue_between` below already uses
/// to avoid the equivalent duplication for the category-revenue query).
///
/// Class-visit kinds are bound from `spinbike_core::services` constants
/// (#329: the stable `kind` column, not the admin-editable `name_en`)
/// using SQLite's NUMBERED `?N` parameters, not the anonymous-`?`
/// `class_visit_filter_sql` helper `db/users.rs`/`routes/*.rs` use: `?3`
/// (Spinning) is referenced TWICE in this query text (the `spinning_visits`
/// aggregate AND inside the `attendance` IN-list) but bound only ONCE —
/// SQLite lets every occurrence of `?3` share that single bound value.
/// `class_visit_filter_sql`'s anonymous placeholders can't do that: each
/// occurrence would need its own `.bind()` call, so this query keeps its
/// own hand-written fragment instead of routing through that helper.
///
/// NOTE: `ELSE 0.0` (not `ELSE 0`) is required for cash_in_eur — otherwise
/// SQLite returns INTEGER for the SUM when no rows match and sqlx refuses
/// to decode that into f64.
async fn kpi_between(pool: &SqlitePool, start_str: &str, end_str: &str) -> Result<DbKpiRow> {
    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE kind = ?3)
                 AND (
                   (action = 'charge' AND amount < 0 AND valid_until IS NULL)
                   OR action = 'visit'
                 )
                THEN 1 ELSE 0
              END
            ), 0) AS spinning_visits,
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE kind IN (?3, ?4))
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
    .bind(start_str)
    .bind(end_str)
    .bind(spinbike_core::services::SPINNING_KIND)
    .bind(spinbike_core::services::FITNESS_KIND)
    .fetch_one(pool)
    .await?;
    Ok(kpi_row)
}

/// Revenue per active service over the half-open UTC-instant range
/// `[start_str, end_str)` — same bound shape as the KPI aggregate above
/// (`#251`: never a raw `date(created_at)` compare). LEFT JOIN so every
/// active service appears even with zero sales in the period (`total_eur:
/// 0.0`). Only `action='charge' AND amount < 0` counts — excludes `visit`
/// (amount 0, free door entry) and `topup` (already covered by the
/// cash_in_eur KPI tile); `storno` rows have `service_id IS NULL` so they
/// never join to a service row at all. See `#255`.
async fn category_revenue_between(
    pool: &SqlitePool,
    start_str: &str,
    end_str: &str,
) -> Result<Vec<CategoryRevenue>> {
    let rows: Vec<DbCategoryRow> = sqlx::query_as::<_, DbCategoryRow>(
        "SELECT s.id AS service_id, s.name_sk, s.name_en,
                COALESCE(ROUND(SUM(CASE WHEN t.action = 'charge' AND t.amount < 0 THEN -t.amount ELSE 0.0 END), 2), 0.0) AS total_eur
         FROM services s
         LEFT JOIN transactions t
                ON t.service_id = s.id
               AND t.created_at >= ?1 AND t.created_at < ?2
               AND t.deleted_at IS NULL
         WHERE s.active = 1
         GROUP BY s.id
         ORDER BY total_eur DESC, s.name_sk",
    )
    .bind(start_str)
    .bind(end_str)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

#[derive(sqlx::FromRow)]
struct DbCategoryRow {
    service_id: i64,
    name_sk: String,
    name_en: String,
    total_eur: f64,
}

impl From<DbCategoryRow> for CategoryRevenue {
    fn from(r: DbCategoryRow) -> Self {
        CategoryRevenue {
            service_id: r.service_id,
            name_sk: r.name_sk,
            name_en: r.name_en,
            total_eur: r.total_eur,
        }
    }
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
) -> Result<(KpiSummary, Vec<CategoryRevenue>, Vec<ReportEvent>, bool)> {
    let (from_start, _) = crate::util::bratislava_day_range_utc(from);
    let (_, to_end) = crate::util::bratislava_day_range_utc(to);
    let from_str = from_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let to_str = to_end.format("%Y-%m-%d %H:%M:%S").to_string();
    let before_parsed = before.as_deref().and_then(parse_before_cursor);

    // Same three independent reads as day_report, run concurrently (#341) —
    // see that function's doc comment.
    let (events_result, kpi_result, category_result) = tokio::join!(
        events_between(pool, &from_str, &to_str, limit, before_parsed),
        kpi_between(pool, &from_str, &to_str),
        category_revenue_between(pool, &from_str, &to_str),
    );
    let (events, has_more) = events_result?;
    let kpi_row = kpi_result?;
    let category_revenue = category_result?;

    Ok((
        KpiSummary {
            spinning_visits: kpi_row.spinning_visits,
            attendance: kpi_row.attendance,
            passes_sold: kpi_row.passes_sold,
            cash_in_eur: kpi_row.cash_in_eur,
        },
        category_revenue,
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

        let fitness_id = service_id_by_name_en(&pool, "Fitness").await;
        let spinning_id = service_id_by_name_en(&pool, "Spinning").await;
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
        // `created_at = datetime('now')` (a UTC instant), and day_report now
        // buckets by the Bratislava-LOCAL day (#251) — so "today" here MUST
        // be `today_bratislava()`, the same anchor the code under test uses,
        // not `chrono::Local` (the OS/runner timezone: agrees with Bratislava
        // on a Bratislava-TZ dev box, but disagrees with it on a UTC CI
        // runner during the 00:00-02:00 Bratislava window — which is exactly
        // how this test flaked live on CI run 29962390657, in the same
        // ~22:00-24:00 UTC slice that exposed #251 itself).
        let today = crate::util::today_bratislava();

        let (day_kpi, _, _, _) = super::day_report(&pool, today, 50, None).await.unwrap();
        assert_eq!(
            day_kpi.attendance, 4,
            "day_report attendance must count only Fitness/Spinning paid+visit rows"
        );
        assert_eq!(
            day_kpi.spinning_visits, 2,
            "day_report spinning_visits = 1 paid Spinning charge + 1 zero-amount Spinning visit"
        );

        let (range_kpi, _, _, _) = super::range_report(&pool, today, today, 50, None)
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
        let fitness_id = service_id_by_name_en(&pool, "Fitness").await;

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
        let (day_kpi, _, day_events, _) = super::day_report(&pool, bratislava_day, 50, None)
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

        let (range_kpi, _, range_events, _) =
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
        let (utc_day_kpi, _, utc_day_events, _) =
            super::day_report(&pool, utc_day, 50, None).await.unwrap();
        assert_eq!(
            utc_day_events.len(),
            0,
            "must NOT appear under the raw UTC calendar day — it belongs to the \
             Bratislava-local day 2026-07-15 only"
        );
        assert_eq!(utc_day_kpi.attendance, 0);
    }

    // ----- #255: revenue-per-category breakdown -----
    //
    // Full per-active-service breakdown (Doplnky vyzivy is one row among all
    // active services, not its own KPI tile — see the CEO decision on #255).
    // Discriminating fixture: charges on >=2 services, a zero-amount `visit`,
    // a `topup` (no service), and a €0 monthly-pass sale — proving the SUM
    // only counts `action='charge' AND amount<0`, every active service
    // appears (LEFT JOIN, 0.0 for no sales), and the rows are sorted
    // total_eur DESC. Checked against BOTH day_report and range_report on
    // the same day.
    #[tokio::test]
    async fn category_revenue_sums_charges_per_service_excludes_visit_and_topup() {
        let (pool, user_id) = setup_pool_with_user().await;

        let fitness_id = service_id_by_name_en(&pool, "Fitness").await;
        let supplements_id = service_id_by_name_en(&pool, "Supplements").await;
        let monthly_pass_id = service_id_by_name_en(&pool, "Monthly pass").await;

        // Two charges on Fitness: 5.0 + 2.5 = 7.5.
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
            Some(fitness_id),
            -2.5,
            "charge",
            None,
        )
        .await
        .unwrap();

        // One charge on Supplements (Doplnky vyzivy): 3.0.
        create_transaction(
            &pool,
            Some(user_id),
            None,
            Some(supplements_id),
            -3.0,
            "charge",
            None,
        )
        .await
        .unwrap();

        // A free (€0) monthly-pass sale — amount is not < 0, so it must NOT
        // add to Monthly pass revenue even though it's a real sale event.
        let valid_until = chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap();
        create_transaction_with_valid_until(
            &pool,
            Some(user_id),
            None,
            Some(monthly_pass_id),
            0.0,
            "charge",
            Some(valid_until),
            None,
        )
        .await
        .unwrap();

        // A zero-amount `visit` on Fitness — must NOT add to Fitness revenue
        // (action != 'charge', even though it's the same service).
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

        // A topup — positive amount, no service_id — must not count anywhere
        // (it's already covered by the separate cash_in_eur KPI tile).
        create_transaction(&pool, Some(user_id), None, None, 10.0, "topup", None)
            .await
            .unwrap();

        let active_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM services WHERE active = 1")
                .fetch_one(&pool)
                .await
                .unwrap();

        let today = crate::util::today_bratislava();

        let (_, day_category, _, _) = super::day_report(&pool, today, 50, None).await.unwrap();
        assert_category_revenue_fixture(
            &day_category,
            active_count,
            fitness_id,
            supplements_id,
            monthly_pass_id,
        );

        let (_, range_category, _, _) = super::range_report(&pool, today, today, 50, None)
            .await
            .unwrap();
        assert_category_revenue_fixture(
            &range_category,
            active_count,
            fitness_id,
            supplements_id,
            monthly_pass_id,
        );
    }

    fn assert_category_revenue_fixture(
        rows: &[spinbike_core::reports::CategoryRevenue],
        active_count: i64,
        fitness_id: i64,
        supplements_id: i64,
        monthly_pass_id: i64,
    ) {
        assert_eq!(
            rows.len() as i64,
            active_count,
            "every active service must appear via LEFT JOIN, even with zero sales"
        );

        let by_id = |id: i64| {
            rows.iter()
                .find(|r| r.service_id == id)
                .unwrap_or_else(|| panic!("service {id} missing from category_revenue"))
        };

        let fitness = by_id(fitness_id);
        assert!(
            (fitness.total_eur - 7.5).abs() < 0.001,
            "fitness revenue must be 5.0 + 2.5 = 7.5, excluding the zero-amount visit; got {}",
            fitness.total_eur
        );

        let supplements = by_id(supplements_id);
        assert!(
            (supplements.total_eur - 3.0).abs() < 0.001,
            "supplements revenue must be 3.0; got {}",
            supplements.total_eur
        );
        assert_eq!(supplements.name_en, "Supplements");

        let monthly_pass = by_id(monthly_pass_id);
        assert_eq!(
            monthly_pass.total_eur, 0.0,
            "a free (€0) monthly-pass sale must contribute 0 to its category total"
        );

        // Every other active service with no sales this period is present at 0.0.
        for r in rows
            .iter()
            .filter(|r| r.service_id != fitness_id && r.service_id != supplements_id)
        {
            assert_eq!(
                r.total_eur, 0.0,
                "service {} had no charges this period and must read 0.0",
                r.service_id
            );
        }

        // Sorted by total_eur DESC.
        for w in rows.windows(2) {
            assert!(
                w[0].total_eur >= w[1].total_eur,
                "category_revenue rows must be sorted total_eur DESC: {} then {}",
                w[0].total_eur,
                w[1].total_eur
            );
        }
    }
}
