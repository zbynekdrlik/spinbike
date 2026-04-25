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
                s.name AS service_name
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
    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0.0 END), 0.0) AS revenue_eur,
            COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0)   AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
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
    let before_parsed = before.as_deref().and_then(parse_before_cursor);

    let mut query = String::from(
        "SELECT t.id, t.card_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS card_name,
                c.barcode,
                s.name AS service_name
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

    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0.0 END), 0.0) AS revenue_eur,
            COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
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

/// Lightweight aggregate — sum of the three alert counts. Used by day/range
/// handlers to populate `alerts_count` without materialising the full
/// `AlertsResponse` (which is fetched separately by the banner).
pub async fn alerts_count(pool: &SqlitePool) -> Result<i64> {
    let n: i64 = sqlx::query_scalar(
        "SELECT
            (SELECT COUNT(*) FROM cards c
             WHERE c.blocked = 0
               AND EXISTS (SELECT 1 FROM transactions t
                           WHERE t.card_id = c.id
                             AND t.valid_until IS NOT NULL
                             AND t.deleted_at IS NULL
                             AND t.valid_until BETWEEN date('now') AND date('now','+7 days'))) +
            (SELECT COUNT(*) FROM cards c
             WHERE c.blocked = 0 AND c.credit < 5.0) +
            (SELECT COUNT(*) FROM (
                SELECT c.id
                FROM cards c
                LEFT JOIN transactions t
                  ON t.card_id = c.id AND t.amount < 0 AND t.deleted_at IS NULL
                WHERE c.blocked = 0 AND c.credit > 0
                GROUP BY c.id
                HAVING MAX(t.created_at) IS NULL OR MAX(t.created_at) < datetime('now','-60 days')
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

pub async fn now_panel(pool: &SqlitePool) -> Result<NowResponse> {
    let now = chrono::Local::now();
    let today: chrono::NaiveDate = now.date_naive();
    let weekday: i64 = now.weekday().num_days_from_monday() as i64;
    let hhmm = now.format("%H:%M").to_string();

    // Exclude templates whose occurrence has been explicitly cancelled today —
    // a cancelled class must not show as the running/next class on the Desk.
    let today_str = today.format("%Y-%m-%d").to_string();
    let templates: Vec<NowTmplRow> = sqlx::query_as::<_, NowTmplRow>(
        "SELECT ct.id, ct.start_time, ct.duration_minutes, ct.capacity,
                i.name AS instructor_name
         FROM class_templates ct
         LEFT JOIN instructors i ON i.id = ct.instructor_id
         WHERE ct.active = 1 AND ct.weekday = ?
           AND NOT EXISTS (SELECT 1 FROM class_cancellations cc
                           WHERE cc.template_id = ct.id AND cc.date = ?)
         ORDER BY ct.start_time ASC",
    )
    .bind(weekday)
    .bind(&today_str)
    .fetch_all(pool)
    .await?;

    let now_mins = parse_hhmm_to_mins(&hhmm);
    let summary: Vec<(i64, i64)> = templates
        .iter()
        .map(|t| (parse_hhmm_to_mins(&t.start_time), t.duration_minutes))
        .collect();
    let (current_idx, next_idx) = pick_current_and_next(&summary, now_mins);

    // Single-pass extraction: consume `templates` once, splitting out the
    // current and next rows by index without intermediate allocations.
    let mut current: Option<NowTmplRow> = None;
    let mut next: Option<NowTmplRow> = None;
    for (i, t) in templates.into_iter().enumerate() {
        if Some(i) == current_idx {
            current = Some(t);
        } else if Some(i) == next_idx {
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
        let date_str = d.format("%Y-%m-%d").to_string();
        let opt: Option<NowFutureTmplRow> = sqlx::query_as::<_, NowFutureTmplRow>(
            "SELECT ct.id, ct.start_time, ct.capacity,
                    i.name AS instructor_name
             FROM class_templates ct
             LEFT JOIN instructors i ON i.id = ct.instructor_id
             WHERE ct.active = 1 AND ct.weekday = ?
               AND NOT EXISTS (SELECT 1 FROM class_cancellations cc
                               WHERE cc.template_id = ct.id AND cc.date = ?)
             ORDER BY ct.start_time ASC LIMIT 1",
        )
        .bind(weekday)
        .bind(&date_str)
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

/// Pure logic for picking the currently-running and next-upcoming class template
/// from a list of (start_mins, duration_minutes) pairs relative to `now_mins`.
/// Extracted so time-window behaviour can be unit-tested without a clock dependency.
fn pick_current_and_next(
    templates: &[(i64, i64)],
    now_mins: i64,
) -> (Option<usize>, Option<usize>) {
    let mut current: Option<usize> = None;
    let mut next: Option<usize> = None;
    for (i, &(start_mins, duration_minutes)) in templates.iter().enumerate() {
        let end_mins = start_mins + duration_minutes;
        if now_mins >= start_mins && now_mins < end_mins && current.is_none() {
            current = Some(i);
        } else if now_mins < start_mins && next.is_none() {
            next = Some(i);
        }
    }
    (current, next)
}

#[cfg(test)]
mod tests {
    use super::{parse_hhmm_to_mins, pick_current_and_next};

    #[test]
    fn parse_hhmm_to_mins_basic() {
        assert_eq!(parse_hhmm_to_mins("00:00"), 0);
        assert_eq!(parse_hhmm_to_mins("00:30"), 30);
        assert_eq!(parse_hhmm_to_mins("01:00"), 60);
        assert_eq!(parse_hhmm_to_mins("18:00"), 1080);
        assert_eq!(parse_hhmm_to_mins("23:59"), 1439);
    }

    // Two templates at the same start; ensures `<` (strict) rather than `<=`
    // at the `else if` boundary — a `<=` mutant would set next on the second
    // equal-start template after current was already taken by the first.
    #[test]
    fn equal_start_templates_only_first_becomes_current_no_next() {
        let tmpls = vec![(1080, 60), (1080, 60)];
        let (current, next) = pick_current_and_next(&tmpls, 1080);
        assert_eq!(current, Some(0));
        assert_eq!(next, None);
    }

    // now = 18:30 (1110 min); class 18:00-19:00 (1080..1140) is current.
    #[test]
    fn picks_current_when_inside_window() {
        let tmpls = vec![(1080, 60)]; // 18:00 + 60 min
        let (current, next) = pick_current_and_next(&tmpls, 1110);
        assert_eq!(current, Some(0));
        assert_eq!(next, None);
    }

    // now = 18:00 exactly; inclusive on start — current.
    #[test]
    fn start_boundary_is_inclusive() {
        let tmpls = vec![(1080, 60)];
        let (current, _next) = pick_current_and_next(&tmpls, 1080);
        assert_eq!(current, Some(0)); // guards against `>=` → `>` mutation
    }

    // now = 19:00 exactly; end is EXCLUSIVE — class has just ended.
    #[test]
    fn end_boundary_is_exclusive() {
        let tmpls = vec![(1080, 60)];
        let (current, next) = pick_current_and_next(&tmpls, 1140);
        assert_eq!(current, None); // guards against `<` → `<=`
        assert_eq!(next, None);
    }

    // now = 17:00 (1020); class 18:00 is upcoming → next.
    #[test]
    fn picks_next_when_before_start() {
        let tmpls = vec![(1080, 60)];
        let (current, next) = pick_current_and_next(&tmpls, 1020);
        assert_eq!(current, None);
        assert_eq!(next, Some(0));
    }

    // Two classes today: current running, later one is next.
    #[test]
    fn picks_current_and_next_distinctly() {
        let tmpls = vec![(1080, 60), (1260, 60)]; // 18:00 and 21:00
        let (current, next) = pick_current_and_next(&tmpls, 1110); // 18:30
        assert_eq!(current, Some(0));
        assert_eq!(next, Some(1));
    }

    // Guards against `&&` → `||` mutations at the window check.
    // now=12:00 (720): template at 18:00 is neither current (12 < 18) nor current
    // by end (12 < 19). With `||`, a false-false-true `current.is_none()` would
    // flip the branch incorrectly. We want this to pick next, not current.
    #[test]
    fn before_window_never_classifies_as_current() {
        let tmpls = vec![(1080, 60)];
        let (current, next) = pick_current_and_next(&tmpls, 720);
        assert_eq!(current, None);
        assert_eq!(next, Some(0));
    }

    // Guards against `+` → `-` or `*` at end_mins computation. With `-`,
    // end_mins = 1080-60 = 1020, so now=1090 would be >= start (1080) and
    // >= end (1020) → original says NOT current (1090 !< 1020), mutant same.
    // Need a case where original says current and mutant says not-current.
    // now = 1100, start = 1080, dur = 60 → end = 1140. 1100 >= 1080 AND 1100 < 1140 → current.
    // Mutant `-`: end = 1020. 1100 >= 1080 AND 1100 < 1020 (false) → not current.
    // Mutant `*`: end = 64800. still current. So `*` won't be killed by this test alone;
    // we cover it via `end_boundary_is_exclusive` which passes with `*` (since 1140 < 64800).
    // For `*`, use a now strictly past the true end but well below any inflated end:
    #[test]
    fn after_end_is_not_current() {
        // start=1080, duration=60, true end=1140. now=1200 (20:00).
        // Original: 1200 < 1140 false → not current. ✓
        // Mutant `+` → `-`: end = 1020. 1200 < 1020 false → not current. (no change)
        // Mutant `+` → `*`: end = 64800. 1200 < 64800 true AND 1200 >= 1080 → CURRENT!
        // This test asserts current=None which would fail on the `*` mutant. ✓
        let tmpls = vec![(1080, 60)];
        let (current, next) = pick_current_and_next(&tmpls, 1200);
        assert_eq!(current, None);
        assert_eq!(next, None);
    }

    // Combine with above: `+` → `-` caught when now is within the real window
    // but below the mutated (start-duration) end. Already covered by
    // `picks_current_when_inside_window`: now=1110, start=1080, dur=60, true end=1140.
    // `+` → `-` mutant: end = 1020. 1110 < 1020 false → not current. Test asserts
    // current=Some(0), fails on mutant. ✓
}
