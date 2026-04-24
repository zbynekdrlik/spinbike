use anyhow::Result;
use sqlx::SqlitePool;

use spinbike_core::reports::{KpiSummary, ReportEvent};

/// Fetch all non-voided transactions for a single day, joined with card + service data.
/// Returns events sorted by created_at DESC and a KpiSummary aggregated over the whole day.
pub async fn day_report(
    pool: &SqlitePool,
    date: chrono::NaiveDate,
    limit: i64,
    before: Option<String>,
) -> Result<(KpiSummary, Vec<ReportEvent>, bool)> {
    let date_str = date.format("%Y-%m-%d").to_string();

    // Events — paginated with optional `before` cursor.
    let mut query = String::from(
        "SELECT t.id, t.card_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                COALESCE(TRIM(c.first_name || ' ' || c.last_name), NULL) AS card_name,
                c.barcode,
                s.name AS service_name
         FROM transactions t
         LEFT JOIN cards c ON c.id = t.card_id
         LEFT JOIN services s ON s.id = t.service_id
         WHERE date(t.created_at) = ?1
           AND t.deleted_at IS NULL",
    );
    if before.is_some() {
        query.push_str(" AND t.created_at < ?2");
    }
    query.push_str(" ORDER BY t.created_at DESC LIMIT ?3");

    let mut q = sqlx::query_as::<_, DbEventRow>(&query).bind(&date_str);
    if let Some(ref b) = before {
        q = q.bind(b);
    }
    q = q.bind(limit + 1); // fetch one extra to know if there's more

    let mut rows = q.fetch_all(pool).await?;
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    let events: Vec<ReportEvent> = rows.into_iter().map(Into::into).collect();

    // KPIs — a separate aggregation over the entire day (not just this page).
    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0 END), 0.0) AS revenue_eur,
            COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0)   AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE date(created_at) = ?1 AND deleted_at IS NULL",
    )
    .bind(&date_str)
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
    service_name: Option<String>,
    created_at: String,
    valid_until: Option<chrono::NaiveDate>,
    deleted_at: Option<String>,
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
            service_name: r.service_name,
            created_at: r.created_at,
            valid_until: r.valid_until,
            voided: r.deleted_at.is_some(),
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

    let mut query = String::from(
        "SELECT t.id, t.card_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                COALESCE(TRIM(c.first_name || ' ' || c.last_name), NULL) AS card_name,
                c.barcode,
                s.name AS service_name
         FROM transactions t
         LEFT JOIN cards c ON c.id = t.card_id
         LEFT JOIN services s ON s.id = t.service_id
         WHERE date(t.created_at) BETWEEN ?1 AND ?2
           AND t.deleted_at IS NULL",
    );
    if before.is_some() {
        query.push_str(" AND t.created_at < ?3");
    }
    query.push_str(" ORDER BY t.created_at DESC LIMIT ?4");

    let mut q = sqlx::query_as::<_, DbEventRow>(&query)
        .bind(&from_str)
        .bind(&to_str);
    if let Some(ref b) = before {
        q = q.bind(b);
    }
    q = q.bind(limit + 1);

    let mut rows = q.fetch_all(pool).await?;
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    let events: Vec<ReportEvent> = rows.into_iter().map(Into::into).collect();

    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0 END), 0.0) AS revenue_eur,
            COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE date(created_at) BETWEEN ?1 AND ?2 AND deleted_at IS NULL",
    )
    .bind(&from_str)
    .bind(&to_str)
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
