use anyhow::Result;
use chrono::Datelike;
use sqlx::SqlitePool;

use spinbike_core::reports::{
    AlertsResponse, CurrentClass, ExpiringPass, InactiveCustomer, KpiSummary, LowCreditCard,
    NextClass, NowResponse, ReportEvent, RosterEntry, RosterStatus,
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

#[derive(sqlx::FromRow)]
struct NowTmplRow {
    id: i64,
    start_time: String,
    duration_minutes: i64,
    capacity: i64,
    instructor_name: Option<String>,
}

#[derive(sqlx::FromRow)]
struct NowRosterRow {
    card_id: Option<i64>,
    name: String,
    barcode: Option<String>,
    booking_id: i64,
    cancelled_at: Option<String>,
    charge_transaction_id: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct NowFutureTmplRow {
    id: i64,
    start_time: String,
    capacity: i64,
    instructor_name: Option<String>,
}

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

pub async fn now_panel(pool: &SqlitePool) -> Result<NowResponse> {
    let now = chrono::Local::now();
    let today: chrono::NaiveDate = now.date_naive();
    let weekday: i64 = now.weekday().num_days_from_monday() as i64;
    let hhmm = now.format("%H:%M").to_string();

    let templates: Vec<NowTmplRow> = sqlx::query_as::<_, NowTmplRow>(
        "SELECT ct.id, ct.start_time, ct.duration_minutes, ct.capacity,
                i.name AS instructor_name
         FROM class_templates ct
         LEFT JOIN instructors i ON i.id = ct.instructor_id
         WHERE ct.active = 1 AND ct.weekday = ?1
         ORDER BY ct.start_time ASC",
    )
    .bind(weekday)
    .fetch_all(pool)
    .await?;

    let mut current: Option<NowTmplRow> = None;
    let mut next: Option<NowTmplRow> = None;
    for t in templates {
        let start_mins = parse_hhmm_to_mins(&t.start_time);
        let now_mins = parse_hhmm_to_mins(&hhmm);
        let end_mins = start_mins + t.duration_minutes;
        if now_mins >= start_mins && now_mins < end_mins && current.is_none() {
            current = Some(t);
        } else if now_mins < start_mins && next.is_none() {
            next = Some(t);
        }
    }

    let current_class = if let Some(t) = current {
        let roster = roster_for(pool, t.id, today).await?;
        Some(CurrentClass {
            template_id: t.id,
            date: today,
            start_time: t.start_time.clone(),
            service_name: "Spinning".to_string(),
            instructor_name: t.instructor_name.clone(),
            capacity: t.capacity,
            roster,
        })
    } else {
        None
    };

    let next_class = if let Some(t) = next {
        let booked = booking_count(pool, t.id, today).await?;
        Some(NextClass {
            template_id: t.id,
            date: today,
            start_time: t.start_time,
            service_name: "Spinning".to_string(),
            instructor_name: t.instructor_name,
            booked,
            capacity: t.capacity,
        })
    } else {
        next_class_future(pool, today).await?
    };

    Ok(NowResponse {
        current_class,
        next_class,
    })
}

fn parse_hhmm_to_mins(s: &str) -> i64 {
    let (h, m) = s.split_once(':').unwrap_or(("0", "0"));
    h.parse::<i64>().unwrap_or(0) * 60 + m.parse::<i64>().unwrap_or(0)
}

async fn roster_for(
    pool: &SqlitePool,
    template_id: i64,
    date: chrono::NaiveDate,
) -> Result<Vec<RosterEntry>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let rows: Vec<NowRosterRow> = sqlx::query_as::<_, NowRosterRow>(
        "SELECT b.card_id,
                COALESCE(NULLIF(TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')), ''),
                         u.name,
                         '(unknown)') AS name,
                c.barcode,
                b.id AS booking_id,
                b.cancelled_at,
                b.charge_transaction_id
         FROM bookings b
         LEFT JOIN cards c ON c.id = b.card_id
         LEFT JOIN users u ON u.id = b.user_id
         WHERE b.template_id = ?1 AND b.date = ?2
         ORDER BY b.created_at ASC",
    )
    .bind(template_id)
    .bind(&date_str)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let status = if r.cancelled_at.is_some() {
                RosterStatus::Cancelled
            } else if r.charge_transaction_id.is_some() {
                RosterStatus::CheckedIn
            } else {
                RosterStatus::Booked
            };
            RosterEntry {
                card_id: r.card_id,
                name: r.name,
                barcode: r.barcode,
                booking_id: r.booking_id,
                status,
            }
        })
        .collect())
}

async fn booking_count(
    pool: &SqlitePool,
    template_id: i64,
    date: chrono::NaiveDate,
) -> Result<i64> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL",
    )
    .bind(template_id)
    .bind(&date_str)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

async fn next_class_future(
    pool: &SqlitePool,
    today: chrono::NaiveDate,
) -> Result<Option<NextClass>> {
    for off in 1..=7 {
        let d = today + chrono::Duration::days(off);
        let weekday = d.weekday().num_days_from_monday() as i64;
        let opt: Option<NowFutureTmplRow> = sqlx::query_as::<_, NowFutureTmplRow>(
            "SELECT ct.id, ct.start_time, ct.capacity,
                    i.name AS instructor_name
             FROM class_templates ct
             LEFT JOIN instructors i ON i.id = ct.instructor_id
             WHERE ct.active = 1 AND ct.weekday = ?1
             ORDER BY ct.start_time ASC LIMIT 1",
        )
        .bind(weekday)
        .fetch_optional(pool)
        .await?;
        if let Some(t) = opt {
            let booked = booking_count(pool, t.id, d).await?;
            return Ok(Some(NextClass {
                template_id: t.id,
                date: d,
                start_time: t.start_time,
                service_name: "Spinning".to_string(),
                instructor_name: t.instructor_name,
                booked,
                capacity: t.capacity,
            }));
        }
    }
    Ok(None)
}
