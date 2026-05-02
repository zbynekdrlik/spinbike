use anyhow::Result;
use chrono::Datelike;
use sqlx::SqlitePool;

use spinbike_core::reports::{
    AlertsResponse, ExpiringPass, InactiveCustomer, KpiSummary, LowCreditCard, ReportEvent,
};

#[derive(sqlx::FromRow)]
struct ExpiringRow {
    card_id: i64,
    name: String,
    barcode: String,
    pass_valid_until: Option<chrono::NaiveDate>,
}

#[derive(sqlx::FromRow)]
struct LowCreditRow {
    card_id: i64,
    name: String,
    barcode: String,
    credit: f64,
}

#[derive(sqlx::FromRow)]
struct InactiveRow {
    card_id: i64,
    name: String,
    barcode: String,
    last_visit: Option<String>,
}

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
pub async fn day_report(
    pool: &SqlitePool,
    date: chrono::NaiveDate,
    limit: i64,
    before: Option<String>,
) -> Result<(KpiSummary, Vec<ReportEvent>, bool)> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let before_parsed = before.as_deref().and_then(parse_before_cursor);

    // Events — paginated with composite (created_at, id) cursor for stable
    // ordering even when multiple rows share a second-precision timestamp.
    let mut query = String::from(
        "SELECT t.id, t.card_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS card_name,
                c.barcode,
                s.name_sk AS service_name_sk, s.name_en AS service_name_en, s.kind AS service_kind, t.note
         FROM transactions t
         LEFT JOIN cards c ON c.id = t.card_id
         LEFT JOIN services s ON s.id = t.service_id
         WHERE date(t.created_at) = ?
           AND t.deleted_at IS NULL",
    );
    if before_parsed.is_some() {
        // (created_at, id) < (cursor_ts, cursor_id) in lexicographic order.
        query.push_str(" AND (t.created_at < ? OR (t.created_at = ? AND t.id < ?))");
    }
    query.push_str(" ORDER BY t.created_at DESC, t.id DESC LIMIT ?");

    let mut q = sqlx::query_as::<_, DbEventRow>(&query).bind(&date_str);
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

    // KPIs — a separate aggregation over the entire day (not just this page).
    // NOTE: `ELSE 0.0` (not `ELSE 0`) is required — otherwise SQLite returns an
    // INTEGER for the SUM when no rows match, and sqlx refuses to decode that
    // into f64 (the KPI struct's revenue_eur/cash_in_eur fields).
    // Class-visit names bound from spinbike_core::services constants so renaming
    // a service in the Rust constant updates this query automatically.
    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0.0 END), 0.0) AS revenue_eur,
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE name_en IN (?2, ?3))
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
         WHERE date(created_at) = ?1 AND deleted_at IS NULL",
    )
    .bind(&date_str)
    .bind(spinbike_core::services::FITNESS_NAME_EN)
    .bind(spinbike_core::services::SPINNING_NAME_EN)
    .fetch_one(pool)
    .await?;

    let kpi = KpiSummary {
        revenue_eur: kpi_row.revenue_eur,
        attendance: kpi_row.attendance,
        passes_sold: kpi_row.passes_sold,
        cash_in_eur: kpi_row.cash_in_eur,
    };

    Ok((kpi, events, has_more))
}

#[derive(sqlx::FromRow)]
struct DbKpiRow {
    revenue_eur: f64,
    attendance: i64,
    passes_sold: i64,
    cash_in_eur: f64,
}

#[derive(sqlx::FromRow)]
struct DbEventRow {
    id: i64,
    card_id: Option<i64>,
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
            card_id: r.card_id,
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
pub async fn range_report(
    pool: &SqlitePool,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    limit: i64,
    before: Option<String>,
) -> Result<(KpiSummary, Vec<ReportEvent>, bool)> {
    let from_str = from.format("%Y-%m-%d").to_string();
    let to_str = to.format("%Y-%m-%d").to_string();
    let before_parsed = before.as_deref().and_then(parse_before_cursor);

    let mut query = String::from(
        "SELECT t.id, t.card_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS card_name,
                c.barcode,
                s.name_sk AS service_name_sk, s.name_en AS service_name_en, s.kind AS service_kind, t.note
         FROM transactions t
         LEFT JOIN cards c ON c.id = t.card_id
         LEFT JOIN services s ON s.id = t.service_id
         WHERE date(t.created_at) BETWEEN ? AND ?
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
    // day_report for the rationale.
    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0.0 END), 0.0) AS revenue_eur,
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
         WHERE date(created_at) BETWEEN ?1 AND ?2 AND deleted_at IS NULL",
    )
    .bind(&from_str)
    .bind(&to_str)
    .bind(spinbike_core::services::FITNESS_NAME_EN)
    .bind(spinbike_core::services::SPINNING_NAME_EN)
    .fetch_one(pool)
    .await?;

    Ok((
        KpiSummary {
            revenue_eur: kpi_row.revenue_eur,
            attendance: kpi_row.attendance,
            passes_sold: kpi_row.passes_sold,
            cash_in_eur: kpi_row.cash_in_eur,
        },
        events,
        has_more,
    ))
}

/// Lightweight aggregate — sum of the three alert counts, capped per category
/// at 100 to match `alerts_report`'s LIMIT 100 (so the banner number cannot
/// exceed what the detail sheet would render).
pub async fn alerts_count(pool: &SqlitePool) -> Result<i64> {
    let n: i64 = sqlx::query_scalar(
        "SELECT
            (SELECT COUNT(*) FROM (SELECT c.id
                FROM cards c
                WHERE c.blocked = 0
                  AND EXISTS (SELECT 1 FROM transactions t
                              WHERE t.card_id = c.id
                                AND t.valid_until IS NOT NULL
                                AND t.deleted_at IS NULL
                                AND t.valid_until BETWEEN date('now') AND date('now','+7 days'))
                LIMIT 100)) +
            (SELECT COUNT(*) FROM (SELECT c.id
                FROM cards c
                WHERE c.blocked = 0 AND c.credit < 5.0
                LIMIT 100)) +
            (SELECT COUNT(*) FROM (
                SELECT c.id
                FROM cards c
                LEFT JOIN transactions t
                  ON t.card_id = c.id AND t.amount < 0 AND t.deleted_at IS NULL
                WHERE c.blocked = 0 AND c.credit > 0
                GROUP BY c.id
                HAVING MAX(t.created_at) IS NULL OR MAX(t.created_at) < datetime('now','-60 days')
                LIMIT 100
            ))",
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn alerts_report(pool: &SqlitePool) -> Result<AlertsResponse> {
    Ok(AlertsResponse {
        expiring_passes: expiring_passes(pool).await?,
        low_credit: low_credit(pool).await?,
        inactive: inactive(pool).await?,
    })
}

async fn expiring_passes(pool: &SqlitePool) -> Result<Vec<ExpiringPass>> {
    let rows: Vec<ExpiringRow> = sqlx::query_as::<_, ExpiringRow>(
        "SELECT c.id AS card_id,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS name,
                c.barcode,
                (SELECT MAX(valid_until) FROM transactions
                 WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
                ) AS pass_valid_until
         FROM cards c
         WHERE c.blocked = 0
           AND EXISTS (SELECT 1 FROM transactions t
                       WHERE t.card_id = c.id
                         AND t.valid_until IS NOT NULL
                         AND t.deleted_at IS NULL
                         AND t.valid_until BETWEEN date('now') AND date('now','+7 days'))
         ORDER BY pass_valid_until ASC
         LIMIT 100",
    )
    .fetch_all(pool)
    .await?;

    let today = chrono::Local::now().date_naive();
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            r.pass_valid_until.map(|vu| ExpiringPass {
                card_id: r.card_id,
                name: r.name,
                barcode: r.barcode,
                valid_until: vu,
                days_left: (vu - today).num_days(),
            })
        })
        .collect())
}

async fn low_credit(pool: &SqlitePool) -> Result<Vec<LowCreditCard>> {
    let rows: Vec<LowCreditRow> = sqlx::query_as::<_, LowCreditRow>(
        "SELECT c.id AS card_id,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS name,
                c.barcode,
                c.credit
         FROM cards c
         WHERE c.blocked = 0 AND c.credit < 5.0
         ORDER BY c.credit ASC
         LIMIT 100",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LowCreditCard {
            card_id: r.card_id,
            name: r.name,
            barcode: r.barcode,
            credit: r.credit,
        })
        .collect())
}

async fn inactive(pool: &SqlitePool) -> Result<Vec<InactiveCustomer>> {
    let rows: Vec<InactiveRow> = sqlx::query_as::<_, InactiveRow>(
        "SELECT c.id AS card_id,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS name,
                c.barcode,
                MAX(t.created_at) AS last_visit
         FROM cards c
         LEFT JOIN transactions t
           ON t.card_id = c.id AND t.amount < 0 AND t.deleted_at IS NULL
         WHERE c.blocked = 0 AND c.credit > 0
         GROUP BY c.id
         HAVING last_visit IS NULL OR last_visit < datetime('now','-60 days')
         ORDER BY last_visit ASC
         LIMIT 100",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| InactiveCustomer {
            card_id: r.card_id,
            name: r.name,
            barcode: r.barcode,
            last_visit: r.last_visit,
        })
        .collect())
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
    use crate::db::{create_memory_pool, run_migrations};
    use sqlx::SqlitePool;

    async fn setup_pool_with_card() -> (SqlitePool, i64) {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // The standard migrations seed Spinning, Fitness, Monthly pass,
        // Refreshments, Supplements, Card activation fee. We need a card to
        // satisfy NOT-NULL-ish FK semantics on `transactions.card_id`.
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, credit, allow_debit) VALUES ('T-23', 100.0, 1) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        (pool, card_id)
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
        let (pool, card_id) = setup_pool_with_card().await;

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
            None,
            Some(card_id),
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
            None,
            Some(card_id),
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
            None,
            Some(card_id),
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
            None,
            Some(card_id),
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
            None,
            Some(card_id),
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
            None,
            Some(card_id),
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
            None,
            Some(card_id),
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
            None,
            Some(card_id),
            None,
            Some(monthly_pass_id),
            -35.0,
            "charge",
            Some(valid_until),
            None,
        )
        .await
        .unwrap();
        create_transaction(&pool, None, Some(card_id), None, None, 10.0, "topup", None)
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

        let (range_kpi, _, _) = super::range_report(&pool, today, today, 50, None)
            .await
            .unwrap();
        assert_eq!(
            range_kpi.attendance, 4,
            "range_report attendance must agree with day_report on the same date"
        );

        // Sanity: adjacent KPIs aren't disturbed by the change.
        // revenue_eur sums all negative amounts: 5+5+2.50+2.50+3+35 = 53.00.
        assert!((day_kpi.revenue_eur - 53.00).abs() < 0.001);
        // passes_sold counts valid_until-set rows: exactly 1.
        assert_eq!(day_kpi.passes_sold, 1);
        // cash_in_eur sums positive-amount rows: just the topup.
        assert!((day_kpi.cash_in_eur - 10.00).abs() < 0.001);
    }
}
