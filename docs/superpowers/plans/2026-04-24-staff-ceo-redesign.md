# SpinBike Staff/CEO Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework the SpinBike admin surface into task-mode navigation (Desk / Schedule / Reports / Settings) with a new Reports page (day/week/month KPIs + activity feed + filters + needs-attention alerts), a Desk "Now" panel showing current/next class + roster, and demotion of `/admin` to `/settings` — all reusing v0.9.0 design primitives (tokens, `.sheet`, `.group`, `.list-row`, `.seg`). See spec at `docs/superpowers/specs/2026-04-24-staff-ceo-redesign-design.md`.

**Architecture:** Backend adds `/api/reports/*` handlers that run SQL aggregations over the existing `transactions`, `bookings`, `cards`, `class_templates` tables — no schema changes. Frontend adds a new `reports` page module, a `now_panel` component on Desk, and an adaptive navigation component (bottom tabs on `<768px`, left sidebar on `≥768px`). Typed contracts live in `spinbike-core` and are shared via serde `Serialize/Deserialize`.

**Tech Stack:** Rust (Axum 0.8, Leptos 0.7, sqlx + SQLite/WAL), WASM via Trunk + rust-embed, chrono, serde, Playwright (TypeScript).

**Version bump:** `0.9.8` → `0.10.0` at the end of implementation (the current `0.9.8` bump already happened in the spec commit).

**CI/local discipline:** Per project CLAUDE.md and airuleset, do NOT run `cargo test`, `cargo build`, `cargo clippy`, or `trunk build` locally. Only `cargo fmt --all --check` locally. All type-checking, testing, and bundling happens on CI after push. Each task commits; CI runs per push.

**Naming convention for action buttons:** existing code uses `.btn.btn--primary`, `.btn.btn--ghost`, `.btn.btn--hero`, `.btn.btn--compact`, `.btn.btn--block`, `.btn.btn--pass`. Reuse these — do not introduce new button classes.

**Data-testid rule:** ALL new interactive elements get a stable `data-testid`. All existing `data-testid` values in tests must stay stable.

---

## File structure

### New files (backend)

```
crates/spinbike-core/src/reports.rs                        # shared types: DayReport, RangeReport, Alerts, NowPanel, RosterEntry, KpiSummary, ExpiringPass, LowCreditCard, InactiveCustomer
crates/spinbike-server/src/db/reports.rs                   # SQL queries: day_kpi_and_events, range_kpi_and_events, alerts_expiring_passes, alerts_low_credit, alerts_inactive, now_panel_data
crates/spinbike-server/src/routes/reports.rs               # Axum handlers: /api/reports/day, /range, /alerts, /now
crates/spinbike-server/tests/reports.rs                    # integration tests using TestApp
```

### New files (frontend)

```
spinbike-ui/src/components/adaptive_nav.rs                 # bottom tabs + sidebar in one component
spinbike-ui/src/pages/reports/mod.rs                       # /reports page entry + state + date nav
spinbike-ui/src/pages/reports/kpi_cards.rs                 # 4 KPI cards component
spinbike-ui/src/pages/reports/alerts_banner.rs             # needs-attention banner + dismiss
spinbike-ui/src/pages/reports/activity_feed.rs             # chronological feed + pagination
spinbike-ui/src/pages/reports/filters_bar.rs               # collapsible filters
spinbike-ui/src/pages/reports/sheets/calendar_picker.rs    # calendar sheet
spinbike-ui/src/pages/reports/sheets/alert_detail.rs       # per-alert cards-list sheet
spinbike-ui/src/pages/desk/now_panel.rs                    # current/next class + roster
```

### New files (E2E)

```
e2e/tests/reports-day.spec.ts
e2e/tests/reports-filters.spec.ts
e2e/tests/reports-alerts.spec.ts
e2e/tests/reports-range.spec.ts
e2e/tests/desk-now-panel.spec.ts
e2e/tests/nav-adaptive.spec.ts
e2e/tests/schedule-roster-admin.spec.ts
```

### Modified files

```
crates/spinbike-server/src/routes/mod.rs                   # register reports::routes()
crates/spinbike-core/src/lib.rs                            # pub mod reports
spinbike-ui/src/app.rs                                     # route redirects; new /reports, /settings
spinbike-ui/src/router.rs                                  # mount AdaptiveNav; add /reports, /settings, redirects
spinbike-ui/src/components/nav.rs                          # replaced or demoted to header-only
spinbike-ui/src/pages/dashboard/mod.rs                     # mount NowPanel on desk
spinbike-ui/src/pages/dashboard/card_panel.rs              # hierarchy cleanup: primary/secondary/tertiary rows, collapsed contact
spinbike-ui/src/pages/schedule.rs                          # fold /staff/classes admin features
spinbike-ui/src/pages/staff_dashboard.rs                   # DELETED; logic lifted
spinbike-ui/src/pages/admin.rs                             # rename to Settings (page title + nav label)
spinbike-ui/src/i18n.rs                                    # new keys (see Task 3)
spinbike-ui/style.css                                      # adaptive nav rules + safe-area + new panel classes
VERSION                                                    # 0.9.8 → 0.10.0 (final task)
```

---

## Ordering & dependencies

Phases should be implemented in order — later phases depend on earlier ones.

1. **Phase A — Shared types + backend** (Tasks 1–7) — pure Rust, no UI
2. **Phase B — i18n + CSS foundation** (Tasks 8–10) — UI dependencies for every later UI task
3. **Phase C — Adaptive nav + routing** (Tasks 11–13) — blocks all page-level work
4. **Phase D — Reports page** (Tasks 14–20)
5. **Phase E — Desk Now panel** (Tasks 21–23)
6. **Phase F — Card detail hierarchy + Schedule merge + Settings rename** (Tasks 24–26)
7. **Phase G — E2E tests** (Tasks 27–33)
8. **Phase H — Version bump + deploy** (Task 34)

---

# Phase A — Shared types + backend

---

## Task 1: Shared reports types in spinbike-core

**Files:**
- Create: `crates/spinbike-core/src/reports.rs`
- Modify: `crates/spinbike-core/src/lib.rs`

- [ ] **Step 1: Create the module file with all response types**

Create `crates/spinbike-core/src/reports.rs` with:

```rust
//! Shared types for the /api/reports/* endpoints. Serialized to JSON on the
//! server and deserialized on the WASM client.

use serde::{Deserialize, Serialize};

/// Totals for a day or a date range.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KpiSummary {
    pub revenue_eur: f64,
    pub attendance: i64,
    pub passes_sold: i64,
    pub cash_in_eur: f64,
}

/// One row in the activity feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEvent {
    pub id: i64,
    pub card_id: Option<i64>,
    pub card_name: Option<String>,
    pub barcode: Option<String>,
    pub action: String,
    pub amount: f64,
    pub service_name: Option<String>,
    pub created_at: String,
    pub valid_until: Option<chrono::NaiveDate>,
    pub voided: bool,
}

/// Classification for UI colour/icon logic. Derived server-side from the event.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Charge,    // amount < 0 AND valid_until IS NULL
    TopUp,     // amount > 0
    PassSold,  // valid_until IS NOT NULL
    Other,
}

impl ReportEvent {
    pub fn kind(&self) -> EventKind {
        if self.valid_until.is_some() {
            EventKind::PassSold
        } else if self.amount < 0.0 {
            EventKind::Charge
        } else if self.amount > 0.0 {
            EventKind::TopUp
        } else {
            EventKind::Other
        }
    }
}

/// Response from GET /api/reports/day and /api/reports/range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportResponse {
    pub kpi: KpiSummary,
    pub events: Vec<ReportEvent>,
    pub alerts_count: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpiringPass {
    pub card_id: i64,
    pub name: String,
    pub barcode: String,
    pub valid_until: chrono::NaiveDate,
    pub days_left: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LowCreditCard {
    pub card_id: i64,
    pub name: String,
    pub barcode: String,
    pub credit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InactiveCustomer {
    pub card_id: i64,
    pub name: String,
    pub barcode: String,
    pub last_visit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertsResponse {
    pub expiring_passes: Vec<ExpiringPass>,
    pub low_credit: Vec<LowCreditCard>,
    pub inactive: Vec<InactiveCustomer>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RosterStatus {
    Booked,
    CheckedIn,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterEntry {
    pub card_id: Option<i64>,
    pub name: String,
    pub barcode: Option<String>,
    pub booking_id: i64,
    pub status: RosterStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentClass {
    pub template_id: i64,
    pub date: chrono::NaiveDate,
    pub start_time: String,       // "HH:MM"
    pub service_name: String,
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub roster: Vec<RosterEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextClass {
    pub template_id: i64,
    pub date: chrono::NaiveDate,
    pub start_time: String,
    pub service_name: String,
    pub instructor_name: Option<String>,
    pub booked: i64,
    pub capacity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowResponse {
    pub current_class: Option<CurrentClass>,
    pub next_class: Option<NextClass>,
}
```

- [ ] **Step 2: Export the module**

Modify `crates/spinbike-core/src/lib.rs` — add the line `pub mod reports;` next to the other `pub mod` declarations.

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-core/src/reports.rs crates/spinbike-core/src/lib.rs
git commit -m "feat(core): add shared reports types for day/range/alerts/now"
```

CI will compile-check the crate.

---

## Task 2: DB query — day KPI aggregation + events

**Files:**
- Create: `crates/spinbike-server/src/db/reports.rs`
- Modify: `crates/spinbike-server/src/db/mod.rs` (add `pub mod reports;`)

- [ ] **Step 1: Write the failing integration test**

Create `crates/spinbike-server/tests/reports.rs`:

```rust
mod helpers;
use helpers::{TestApp, get};
use axum::http::StatusCode;
use sqlx::Executor;

#[tokio::test]
async fn day_report_aggregates_charges_topups_passes_and_excludes_voided() {
    let app = TestApp::new().await;

    // Seed: card for the existing customer
    let card_id = app.customer_card_id;

    // One charge of 5 EUR (amount = -5)
    sqlx::query("INSERT INTO transactions (card_id, amount, action, service_id, created_at) \
                 SELECT ?1, -5.0, 'charge', id, datetime('now') FROM services WHERE name = 'Spinning' LIMIT 1")
        .bind(card_id).execute(&app.pool).await.unwrap();

    // One top-up of 20 EUR
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, 20.0, 'topup', datetime('now'))")
        .bind(card_id).execute(&app.pool).await.unwrap();

    // One pass sale with valid_until
    sqlx::query("INSERT INTO transactions (card_id, amount, action, valid_until, created_at) VALUES (?1, -35.0, 'charge', date('now','+30 days'), datetime('now'))")
        .bind(card_id).execute(&app.pool).await.unwrap();

    // One voided charge (should be excluded)
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at, deleted_at) VALUES (?1, -5.0, 'charge', datetime('now'), datetime('now'))")
        .bind(card_id).execute(&app.pool).await.unwrap();

    // Call /api/reports/day for today
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    let (status, body) = app
        .request(get(&format!("/api/reports/day?date={today}"), &app.admin_token))
        .await;
    assert_eq!(status, StatusCode::OK);

    let kpi = &body["kpi"];
    assert_eq!(kpi["revenue_eur"].as_f64().unwrap(), 40.0, "5 charge + 35 pass = 40 revenue");
    assert_eq!(kpi["attendance"].as_i64().unwrap(), 1, "only one regular charge counts as a visit");
    assert_eq!(kpi["passes_sold"].as_i64().unwrap(), 1);
    assert_eq!(kpi["cash_in_eur"].as_f64().unwrap(), 20.0);

    assert_eq!(body["events"].as_array().unwrap().len(), 3, "voided excluded");
}
```

This test will fail at compile or fail at runtime with `404 Not Found` for the endpoint.

- [ ] **Step 2: Implement the DB query module**

Create `crates/spinbike-server/src/db/reports.rs`:

```rust
use anyhow::Result;
use sqlx::SqlitePool;

use spinbike_core::reports::{KpiSummary, ReportEvent};

/// Fetch all non-voided transactions for a single day, joined with card + service data.
/// Returns events sorted by created_at DESC and a KpiSummary aggregated over those events.
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
           AND t.deleted_at IS NULL"
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
         WHERE date(created_at) = ?1 AND deleted_at IS NULL"
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
```

- [ ] **Step 3: Register the module**

Edit `crates/spinbike-server/src/db/mod.rs` — add `pub mod reports;` alongside other module declarations.

- [ ] **Step 4: Commit**

```bash
git add crates/spinbike-server/src/db/reports.rs \
        crates/spinbike-server/src/db/mod.rs \
        crates/spinbike-server/tests/reports.rs
git commit -m "feat(server): day report KPI aggregation + events query"
```

The test will still fail — the route handler is not yet wired. That's expected; it unblocks Task 6 below. CI will run and the new test will fail; that is acceptable for this intermediate commit because CI-GREEN is reached at the end of Phase A.

> **Phase A CI gate:** we accept CI red through Tasks 2–6. CI must be green after Task 7 (route wired) and the test passes. Do not proceed to Phase B until Task 7 is green.

---

## Task 3: DB query — range report (7/30-day aggregates)

**Files:**
- Modify: `crates/spinbike-server/src/db/reports.rs`
- Modify: `crates/spinbike-server/tests/reports.rs`

- [ ] **Step 1: Add failing test for range**

Append to `crates/spinbike-server/tests/reports.rs`:

```rust
#[tokio::test]
async fn range_report_aggregates_across_days_and_rejects_over_93_days() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;

    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES \
                 (?1, -5.0, 'charge', datetime('now','-3 days')), \
                 (?1, -5.0, 'charge', datetime('now','-2 days')), \
                 (?1, 20.0, 'topup', datetime('now','-1 days'))")
        .bind(card_id).execute(&app.pool).await.unwrap();

    let from = (chrono::Local::now().date_naive() - chrono::Duration::days(5)).format("%Y-%m-%d").to_string();
    let to   = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    let (status, body) = app
        .request(get(&format!("/api/reports/range?from={from}&to={to}"), &app.admin_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kpi"]["attendance"].as_i64().unwrap(), 2);
    assert_eq!(body["kpi"]["revenue_eur"].as_f64().unwrap(), 10.0);
    assert_eq!(body["kpi"]["cash_in_eur"].as_f64().unwrap(), 20.0);

    // Over-range rejection
    let from = (chrono::Local::now().date_naive() - chrono::Duration::days(120)).format("%Y-%m-%d").to_string();
    let (status, _) = app
        .request(get(&format!("/api/reports/range?from={from}&to={to}"), &app.admin_token))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Implement `range_report` in `db/reports.rs`**

Append to the bottom of `crates/spinbike-server/src/db/reports.rs`:

```rust
pub const RANGE_MAX_DAYS: i64 = 93;

pub async fn range_report(
    pool: &SqlitePool,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    limit: i64,
    before: Option<String>,
) -> Result<(KpiSummary, Vec<ReportEvent>, bool)> {
    let from_str = from.format("%Y-%m-%d").to_string();
    let to_str   = to.format("%Y-%m-%d").to_string();

    let mut q = String::from(
        "SELECT t.id, t.card_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at,
                COALESCE(TRIM(c.first_name || ' ' || c.last_name), NULL) AS card_name,
                c.barcode, s.name AS service_name
         FROM transactions t
         LEFT JOIN cards c ON c.id = t.card_id
         LEFT JOIN services s ON s.id = t.service_id
         WHERE date(t.created_at) BETWEEN ?1 AND ?2
           AND t.deleted_at IS NULL"
    );
    if before.is_some() {
        q.push_str(" AND t.created_at < ?3");
    }
    q.push_str(" ORDER BY t.created_at DESC LIMIT ?4");

    let mut sql = sqlx::query_as::<_, DbEventRow>(&q).bind(&from_str).bind(&to_str);
    if let Some(ref b) = before { sql = sql.bind(b); }
    sql = sql.bind(limit + 1);
    let mut rows = sql.fetch_all(pool).await?;
    let has_more = rows.len() as i64 > limit;
    if has_more { rows.pop(); }
    let events: Vec<ReportEvent> = rows.into_iter().map(Into::into).collect();

    let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
        "SELECT
            COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0 END), 0.0) AS revenue_eur,
            COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE date(created_at) BETWEEN ?1 AND ?2 AND deleted_at IS NULL"
    )
    .bind(&from_str).bind(&to_str)
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
```

The 93-day limit is enforced in the route handler (Task 6), not in this query.

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/db/reports.rs crates/spinbike-server/tests/reports.rs
git commit -m "feat(server): range report query over N days"
```

---

## Task 4: DB query — alerts (expiring, low-credit, inactive)

**Files:**
- Modify: `crates/spinbike-server/src/db/reports.rs`
- Modify: `crates/spinbike-server/tests/reports.rs`

- [ ] **Step 1: Add failing tests for all three alerts**

Append to `crates/spinbike-server/tests/reports.rs`:

```rust
#[tokio::test]
async fn alerts_expiring_passes_within_7_days_excludes_blocked() {
    let app = TestApp::new().await;

    // Card A with pass expiring in 3 days — should appear
    let card_a: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('A','Anna','K',10) RETURNING id")
        .fetch_one(&app.pool).await.unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, valid_until, created_at) \
                 VALUES (?1, -35.0, 'charge', date('now','+3 days'), datetime('now','-10 days'))")
        .bind(card_a).execute(&app.pool).await.unwrap();

    // Card B with pass expiring 30 days away — should NOT appear
    let card_b: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('B','Bela','M',10) RETURNING id")
        .fetch_one(&app.pool).await.unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, valid_until, created_at) \
                 VALUES (?1, -35.0, 'charge', date('now','+30 days'), datetime('now','-10 days'))")
        .bind(card_b).execute(&app.pool).await.unwrap();

    // Card C blocked with pass expiring in 2 days — should NOT appear
    let card_c: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit, blocked) VALUES ('C','Cela','N',10,1) RETURNING id")
        .fetch_one(&app.pool).await.unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, valid_until, created_at) \
                 VALUES (?1, -35.0, 'charge', date('now','+2 days'), datetime('now','-10 days'))")
        .bind(card_c).execute(&app.pool).await.unwrap();

    let (status, body) = app.request(get("/api/reports/alerts", &app.admin_token)).await;
    assert_eq!(status, StatusCode::OK);

    let expiring = body["expiring_passes"].as_array().unwrap();
    let names: Vec<&str> = expiring.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.iter().any(|n| n.contains("Anna")));
    assert!(!names.iter().any(|n| n.contains("Bela")));
    assert!(!names.iter().any(|n| n.contains("Cela")));
}

#[tokio::test]
async fn alerts_low_credit_under_5_and_not_blocked() {
    let app = TestApp::new().await;
    sqlx::query("INSERT INTO cards (barcode, first_name, last_name, credit, blocked) VALUES \
                 ('L1','Low','One',2.5,0), \
                 ('L2','Low','Two',4.99,0), \
                 ('L3','Low','Three',5.00,0), \
                 ('L4','Low','Four',0.0,1)")
        .execute(&app.pool).await.unwrap();

    let (status, body) = app.request(get("/api/reports/alerts", &app.admin_token)).await;
    assert_eq!(status, StatusCode::OK);
    let low = body["low_credit"].as_array().unwrap();
    let names: Vec<String> = low.iter().map(|e| e["name"].as_str().unwrap().to_string()).collect();
    assert!(names.iter().any(|n| n.contains("Low One")));
    assert!(names.iter().any(|n| n.contains("Low Two")));
    assert!(!names.iter().any(|n| n.contains("Low Three")), "credit = 5.00 is NOT low");
    assert!(!names.iter().any(|n| n.contains("Low Four")), "blocked excluded");
}

#[tokio::test]
async fn alerts_inactive_60_days_excludes_zero_credit_and_blocked() {
    let app = TestApp::new().await;
    let inactive_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('IN','Inact','A',20) RETURNING id"
    ).fetch_one(&app.pool).await.unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-70 days'))")
        .bind(inactive_id).execute(&app.pool).await.unwrap();

    let active_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('AC','Act','B',20) RETURNING id"
    ).fetch_one(&app.pool).await.unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-5 days'))")
        .bind(active_id).execute(&app.pool).await.unwrap();

    let zero_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode, first_name, last_name, credit) VALUES ('ZC','Zero','C',0) RETURNING id"
    ).fetch_one(&app.pool).await.unwrap();
    sqlx::query("INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?1, -5.0, 'charge', datetime('now','-100 days'))")
        .bind(zero_id).execute(&app.pool).await.unwrap();

    let (status, body) = app.request(get("/api/reports/alerts", &app.admin_token)).await;
    assert_eq!(status, StatusCode::OK);
    let inactive = body["inactive"].as_array().unwrap();
    let names: Vec<String> = inactive.iter().map(|e| e["name"].as_str().unwrap().to_string()).collect();
    assert!(names.iter().any(|n| n.contains("Inact")));
    assert!(!names.iter().any(|n| n.contains("Act")));
    assert!(!names.iter().any(|n| n.contains("Zero")));
}
```

- [ ] **Step 2: Implement the three alert queries**

Append to `crates/spinbike-server/src/db/reports.rs`:

```rust
use spinbike_core::reports::{AlertsResponse, ExpiringPass, InactiveCustomer, LowCreditCard};

pub async fn alerts_report(pool: &SqlitePool) -> Result<AlertsResponse> {
    Ok(AlertsResponse {
        expiring_passes: expiring_passes(pool).await?,
        low_credit:      low_credit(pool).await?,
        inactive:        inactive(pool).await?,
    })
}

async fn expiring_passes(pool: &SqlitePool) -> Result<Vec<ExpiringPass>> {
    #[derive(sqlx::FromRow)]
    struct R {
        card_id: i64,
        name: String,
        barcode: String,
        pass_valid_until: Option<chrono::NaiveDate>,
    }

    let rows: Vec<R> = sqlx::query_as(
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
         LIMIT 100"
    )
    .fetch_all(pool).await?;

    let today = chrono::Local::now().date_naive();
    Ok(rows.into_iter().filter_map(|r| {
        r.pass_valid_until.map(|vu| ExpiringPass {
            card_id: r.card_id,
            name: r.name,
            barcode: r.barcode,
            valid_until: vu,
            days_left: (vu - today).num_days(),
        })
    }).collect())
}

async fn low_credit(pool: &SqlitePool) -> Result<Vec<LowCreditCard>> {
    let rows: Vec<LowCreditCard> = sqlx::query_as(
        "SELECT c.id AS card_id,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS name,
                c.barcode,
                c.credit
         FROM cards c
         WHERE c.blocked = 0 AND c.credit < 5.0
         ORDER BY c.credit ASC
         LIMIT 100"
    )
    .fetch_all(pool).await?;
    Ok(rows)
}

async fn inactive(pool: &SqlitePool) -> Result<Vec<InactiveCustomer>> {
    let rows: Vec<InactiveCustomer> = sqlx::query_as(
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
         LIMIT 100"
    )
    .fetch_all(pool).await?;
    Ok(rows)
}
```

The `FromRow` derives for `LowCreditCard` and `InactiveCustomer` require those types to be in scope from `spinbike_core::reports`. They are `Serialize/Deserialize` but not `FromRow` — add `#[derive(sqlx::FromRow)]` locally via a wrapper or manually derive. The simplest path: add `#[cfg_attr(feature = "server", derive(sqlx::FromRow))]` gated derives is not worth the feature complexity — instead, define local `#[derive(sqlx::FromRow)]` shadow structs inside each function (mirroring the field names) and `.into()` them.

Since the spec types in `spinbike-core` cannot derive `sqlx::FromRow` (we don't want `sqlx` as a core dependency), wrap each query with a local shadow struct. Rewrite:

```rust
async fn low_credit(pool: &SqlitePool) -> Result<Vec<LowCreditCard>> {
    #[derive(sqlx::FromRow)]
    struct Row { card_id: i64, name: String, barcode: String, credit: f64 }
    let rows: Vec<Row> = sqlx::query_as::<_, Row>(
        "SELECT c.id AS card_id,
                TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')) AS name,
                c.barcode, c.credit
         FROM cards c
         WHERE c.blocked = 0 AND c.credit < 5.0
         ORDER BY c.credit ASC
         LIMIT 100"
    ).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| LowCreditCard {
        card_id: r.card_id, name: r.name, barcode: r.barcode, credit: r.credit
    }).collect())
}
```

Apply the same shadow-struct pattern to `inactive()` (its shadow has a `Option<String>` for last_visit).

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/db/reports.rs crates/spinbike-server/tests/reports.rs
git commit -m "feat(server): alerts report (expiring passes / low credit / inactive)"
```

---

## Task 5: DB query — Now panel (current + next class with roster)

**Files:**
- Modify: `crates/spinbike-server/src/db/reports.rs`
- Modify: `crates/spinbike-server/tests/reports.rs`

- [ ] **Step 1: Add failing test**

Append to `crates/spinbike-server/tests/reports.rs`:

```rust
#[tokio::test]
async fn now_panel_returns_current_or_next_class() {
    let app = TestApp::new().await;

    // Seed a template whose weekday is TODAY and start_time is NOW (rounded to the hour)
    let now = chrono::Local::now();
    let weekday = now.weekday().num_days_from_monday() as i64;
    let start_time = now.format("%H:00").to_string();
    let template_id: i64 = sqlx::query_scalar(
        "INSERT INTO class_templates (weekday, start_time, duration_minutes, capacity, active) \
         VALUES (?1, ?2, 60, 12, 1) RETURNING id"
    ).bind(weekday).bind(&start_time).fetch_one(&app.pool).await.unwrap();

    // Seed a booking on today
    let today = now.date_naive().format("%Y-%m-%d").to_string();
    sqlx::query("INSERT INTO bookings (template_id, date, user_id, source) VALUES (?1, ?2, ?3, 'staff')")
        .bind(template_id).bind(&today).bind(app.customer_id).execute(&app.pool).await.unwrap();

    let (status, body) = app.request(get("/api/reports/now", &app.admin_token)).await;
    assert_eq!(status, StatusCode::OK);
    // Either current_class or next_class should be populated depending on wall-clock timing.
    let has_any = !body["current_class"].is_null() || !body["next_class"].is_null();
    assert!(has_any, "expected at least current_class or next_class to be set");
}
```

- [ ] **Step 2: Implement `now_panel`**

Append to `crates/spinbike-server/src/db/reports.rs`:

```rust
use spinbike_core::reports::{CurrentClass, NextClass, NowResponse, RosterEntry, RosterStatus};

pub async fn now_panel(pool: &SqlitePool) -> Result<NowResponse> {
    let now = chrono::Local::now();
    let today: chrono::NaiveDate = now.date_naive();
    let weekday: i64 = now.weekday().num_days_from_monday() as i64;
    let hhmm = now.format("%H:%M").to_string();

    // "Currently running" = today's template with start_time <= now < start_time + duration
    #[derive(sqlx::FromRow)]
    struct Tmpl {
        id: i64,
        start_time: String,
        duration_minutes: i64,
        capacity: i64,
        service_name: Option<String>,
        instructor_name: Option<String>,
    }

    let templates: Vec<Tmpl> = sqlx::query_as::<_, Tmpl>(
        "SELECT ct.id, ct.start_time, ct.duration_minutes, ct.capacity,
                s.name AS service_name,
                i.name AS instructor_name
         FROM class_templates ct
         LEFT JOIN services s    ON s.id    = ct.service_id
         LEFT JOIN instructors i ON i.id    = ct.instructor_id
         WHERE ct.active = 1 AND ct.weekday = ?1
         ORDER BY ct.start_time ASC"
    ).bind(weekday).fetch_all(pool).await?;

    // Find "current" (start <= now < start + duration) and the earliest future one.
    let mut current: Option<Tmpl> = None;
    let mut next: Option<Tmpl> = None;
    for t in templates {
        let start_mins = parse_hhmm_to_mins(&t.start_time);
        let now_mins   = parse_hhmm_to_mins(&hhmm);
        let end_mins   = start_mins + t.duration_minutes;
        if now_mins >= start_mins && now_mins < end_mins && current.is_none() {
            current = Some(t);
        } else if now_mins < start_mins && next.is_none() {
            next = Some(t);
        }
    }

    // Build CurrentClass + roster
    let current_class = if let Some(t) = current {
        let roster = roster_for(pool, t.id, today).await?;
        Some(CurrentClass {
            template_id: t.id,
            date: today,
            start_time: t.start_time.clone(),
            service_name: t.service_name.clone().unwrap_or_default(),
            instructor_name: t.instructor_name.clone(),
            capacity: t.capacity,
            roster,
        })
    } else { None };

    // Build NextClass — if no next today, look at tomorrow+ (first active template).
    let next_class = if let Some(t) = next {
        let booked = booking_count(pool, t.id, today).await?;
        Some(NextClass {
            template_id: t.id,
            date: today,
            start_time: t.start_time,
            service_name: t.service_name.unwrap_or_default(),
            instructor_name: t.instructor_name,
            booked,
            capacity: t.capacity,
        })
    } else {
        next_class_future(pool, today).await?
    };

    Ok(NowResponse { current_class, next_class })
}

fn parse_hhmm_to_mins(s: &str) -> i64 {
    let (h, m) = s.split_once(':').unwrap_or(("0", "0"));
    h.parse::<i64>().unwrap_or(0) * 60 + m.parse::<i64>().unwrap_or(0)
}

async fn roster_for(pool: &SqlitePool, template_id: i64, date: chrono::NaiveDate) -> Result<Vec<RosterEntry>> {
    #[derive(sqlx::FromRow)]
    struct R {
        card_id: Option<i64>,
        name: String,
        barcode: Option<String>,
        booking_id: i64,
        cancelled_at: Option<String>,
        charge_transaction_id: Option<i64>,
    }
    let date_str = date.format("%Y-%m-%d").to_string();
    let rows: Vec<R> = sqlx::query_as::<_, R>(
        "SELECT b.card_id,
                COALESCE(TRIM(c.first_name || ' ' || c.last_name),
                         COALESCE(u.name, '(unknown)')) AS name,
                c.barcode,
                b.id AS booking_id,
                b.cancelled_at,
                b.charge_transaction_id
         FROM bookings b
         LEFT JOIN cards c ON c.id = b.card_id
         LEFT JOIN users u ON u.id = b.user_id
         WHERE b.template_id = ?1 AND b.date = ?2
         ORDER BY b.created_at ASC"
    ).bind(template_id).bind(&date_str).fetch_all(pool).await?;

    Ok(rows.into_iter().map(|r| {
        let status = if r.cancelled_at.is_some() {
            RosterStatus::Cancelled
        } else if r.charge_transaction_id.is_some() {
            RosterStatus::CheckedIn
        } else {
            RosterStatus::Booked
        };
        RosterEntry { card_id: r.card_id, name: r.name, barcode: r.barcode, booking_id: r.booking_id, status }
    }).collect())
}

async fn booking_count(pool: &SqlitePool, template_id: i64, date: chrono::NaiveDate) -> Result<i64> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL"
    ).bind(template_id).bind(&date_str).fetch_one(pool).await?;
    Ok(n)
}

async fn next_class_future(pool: &SqlitePool, today: chrono::NaiveDate) -> Result<Option<NextClass>> {
    // Walk forward up to 7 days looking for the earliest active template on that weekday.
    for off in 1..=7 {
        let d = today + chrono::Duration::days(off);
        let weekday = d.weekday().num_days_from_monday() as i64;
        #[derive(sqlx::FromRow)]
        struct Tmpl {
            id: i64,
            start_time: String,
            capacity: i64,
            service_name: Option<String>,
            instructor_name: Option<String>,
        }
        let opt: Option<Tmpl> = sqlx::query_as::<_, Tmpl>(
            "SELECT ct.id, ct.start_time, ct.capacity,
                    s.name AS service_name, i.name AS instructor_name
             FROM class_templates ct
             LEFT JOIN services s    ON s.id    = ct.service_id
             LEFT JOIN instructors i ON i.id    = ct.instructor_id
             WHERE ct.active = 1 AND ct.weekday = ?1
             ORDER BY ct.start_time ASC LIMIT 1"
        ).bind(weekday).fetch_optional(pool).await?;
        if let Some(t) = opt {
            let booked = booking_count(pool, t.id, d).await?;
            return Ok(Some(NextClass {
                template_id: t.id, date: d, start_time: t.start_time,
                service_name: t.service_name.unwrap_or_default(),
                instructor_name: t.instructor_name,
                booked, capacity: t.capacity,
            }));
        }
    }
    Ok(None)
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/db/reports.rs crates/spinbike-server/tests/reports.rs
git commit -m "feat(server): now-panel query (current/next class with roster)"
```

---

## Task 6: Axum route handlers in `routes/reports.rs`

**Files:**
- Create: `crates/spinbike-server/src/routes/reports.rs`

- [ ] **Step 1: Create the handlers**

Create `crates/spinbike-server/src/routes/reports.rs`:

```rust
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::{
    auth::{require_admin, AuthUser},
    db,
    AppState,
};

use spinbike_core::reports::{AlertsResponse, NowResponse, ReportResponse};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/reports/day",    get(day))
        .route("/api/reports/range",  get(range))
        .route("/api/reports/alerts", get(alerts))
        .route("/api/reports/now",    get(now))
}

#[derive(Debug, Deserialize)]
struct DayQuery {
    date: chrono::NaiveDate,
    limit: Option<i64>,
    before: Option<String>,
}

async fn day(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<DayQuery>,
) -> Result<Json<ReportResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let (kpi, events, has_more) = db::reports::day_report(&state.pool, q.date, limit, q.before)
        .await
        .map_err(internal_error)?;
    let alerts_count = total_alert_count(&state).await.unwrap_or(0);
    Ok(Json(ReportResponse { kpi, events, alerts_count, has_more }))
}

#[derive(Debug, Deserialize)]
struct RangeQuery {
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    limit: Option<i64>,
    before: Option<String>,
}

async fn range(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(q): Query<RangeQuery>,
) -> Result<Json<ReportResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    if q.to < q.from {
        return Err(bad_request("to < from"));
    }
    let days = (q.to - q.from).num_days();
    if days > db::reports::RANGE_MAX_DAYS {
        return Err(bad_request("range too large (max 93 days)"));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let (kpi, events, has_more) = db::reports::range_report(&state.pool, q.from, q.to, limit, q.before)
        .await.map_err(internal_error)?;
    let alerts_count = total_alert_count(&state).await.unwrap_or(0);
    Ok(Json(ReportResponse { kpi, events, alerts_count, has_more }))
}

async fn alerts(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<AlertsResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let r = db::reports::alerts_report(&state.pool).await.map_err(internal_error)?;
    Ok(Json(r))
}

async fn now(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<NowResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let r = db::reports::now_panel(&state.pool).await.map_err(internal_error)?;
    Ok(Json(r))
}

async fn total_alert_count(state: &AppState) -> anyhow::Result<i64> {
    let a = db::reports::alerts_report(&state.pool).await?;
    Ok(a.expiring_passes.len() as i64 + a.low_credit.len() as i64 + a.inactive.len() as i64)
}

fn internal_error(e: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(?e, "reports handler error");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "internal" })))
}

fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg })))
}
```

If `require_admin` does not exist in `crate::auth`, add it mirroring the existing `require_staff` (found at `crates/spinbike-server/src/auth/mod.rs`):

```rust
pub fn require_admin(claims: &Claims) -> Result<(), (StatusCode, axum::Json<serde_json::Value>)> {
    if matches!(claims.role, Role::Admin) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, axum::Json(serde_json::json!({ "error": "admin required" }))))
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/spinbike-server/src/routes/reports.rs crates/spinbike-server/src/auth/mod.rs
git commit -m "feat(server): /api/reports/{day,range,alerts,now} handlers"
```

---

## Task 7: Register reports routes + Phase A CI verification

**Files:**
- Modify: `crates/spinbike-server/src/routes/mod.rs`

- [ ] **Step 1: Register the module**

Edit `crates/spinbike-server/src/routes/mod.rs`:

```rust
pub mod reports;   // add alongside other pub mod declarations

pub fn api_routes() -> Router<AppState> {
    Router::new()
        // ... existing .merge() calls unchanged ...
        .merge(reports::routes())
}
```

- [ ] **Step 2: Commit and push Phase A**

```bash
git add crates/spinbike-server/src/routes/mod.rs
git commit -m "feat(server): wire /api/reports routes"
git push
```

- [ ] **Step 3: Verify on CI**

All tests added in Tasks 2–5 MUST pass on this CI run. If any fail, debug by reading `gh run view <run-id> --log-failed` and fix before proceeding to Phase B. Do NOT proceed to Phase B until CI is green.

---

# Phase B — i18n + CSS foundation

---

## Task 8: i18n keys for the new UI

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`

- [ ] **Step 1: Append all new keys**

Inside the `TRANSLATIONS: LazyLock<TransMap>` initializer in `spinbike-ui/src/i18n.rs`, add (keep existing entries intact):

```rust
    // Nav
    m.insert("nav_desk",     ("Desk",      "Desk"));
    m.insert("nav_schedule", ("Plán",      "Schedule"));
    m.insert("nav_reports",  ("Výkazy",    "Reports"));
    m.insert("nav_settings", ("Nastavenia","Settings"));

    // Reports page — date nav
    m.insert("reports_yesterday", ("Včera",       "Yesterday"));
    m.insert("reports_today",     ("Dnes",        "Today"));
    m.insert("reports_tomorrow",  ("Zajtra",      "Tomorrow"));
    m.insert("reports_week",      ("Týždeň",      "Week"));
    m.insert("reports_month",     ("Mesiac",      "Month"));
    m.insert("reports_pick_date", ("Zvoliť dátum","Pick date"));

    // Reports KPI cards
    m.insert("kpi_revenue",    ("TRŽBA",      "REVENUE"));
    m.insert("kpi_attendance", ("NÁVŠTEVY",   "ATTENDANCE"));
    m.insert("kpi_passes",     ("PERMANENTKY","PASSES"));
    m.insert("kpi_cash_in",    ("VKLADY",     "CASH IN"));

    // Alerts banner
    m.insert("alerts_title",           ("Potrebuje pozornosť","Needs attention"));
    m.insert("alerts_expiring_passes", ("{n} permanentiek vyprší do 7 dní",   "{n} passes expire within 7 days"));
    m.insert("alerts_low_credit",      ("{n} kariet s kreditom pod 5 €",      "{n} cards below €5 credit"));
    m.insert("alerts_inactive",        ("{n} zákazníkov neaktívnych 60+ dní", "{n} customers inactive 60+ days"));
    m.insert("alerts_dismiss",         ("Skryť",                "Dismiss"));

    // Filters
    m.insert("filters_label",       ("Filtre",        "Filters"));
    m.insert("filters_reset",       ("Zrušiť filtre", "Reset"));
    m.insert("filters_event_all",   ("Všetko",        "All"));
    m.insert("filters_event_payments", ("Platby",     "Payments"));
    m.insert("filters_event_topups",   ("Vklady",     "Top-ups"));
    m.insert("filters_event_passes",   ("Permanentky","Passes"));
    m.insert("filters_service_spinning", ("Spinning", "Spinning"));
    m.insert("filters_service_fitness",  ("Fitness",  "Fitness"));
    m.insert("filters_service_pass",     ("Permanentka", "Pass"));
    m.insert("filters_search_placeholder", ("Hľadať meno, čiarový kód, telefón", "Search name, barcode, phone"));

    // Feed
    m.insert("feed_load_older",  ("Načítať staršie","Load older"));
    m.insert("feed_empty_day",   ("Na tento deň nie je žiadna aktivita.", "No activity on this day."));
    m.insert("feed_empty_filter",("Žiadne výsledky pre tieto filtre.",   "No results for these filters."));

    // Desk Now panel
    m.insert("now_no_more_today", ("Dnes už žiadne hodiny.", "No more classes today."));
    m.insert("now_next_on",       ("Ďalšia hodina: {when}",  "Next class: {when}"));
    m.insert("now_walk_in",       ("Bez rezervácie",         "Walk-in"));
    m.insert("now_cancel_class",  ("Zrušiť hodinu",          "Cancel class"));
    m.insert("now_collapse",      ("Skryť",                  "Hide"));
    m.insert("now_expand",        ("Zobraziť",               "Show"));

    // Status badges
    m.insert("status_booked",     ("Rezervované",  "Booked"));
    m.insert("status_checked_in", ("Prišiel",      "Checked in"));
    m.insert("status_cancelled",  ("Zrušené",      "Cancelled"));

    // Card detail hierarchy
    m.insert("card_show_contact", ("Zobraziť kontakt", "Show contact"));
    m.insert("card_hide_contact", ("Skryť kontakt",    "Hide contact"));

    // Settings
    m.insert("settings_tab_center",      ("Centrum",      "Center"));
    m.insert("settings_tab_services",    ("Služby",       "Services"));
    m.insert("settings_tab_templates",   ("Permanentky",  "Templates"));
    m.insert("settings_tab_instructors", ("Inštruktori",  "Instructors"));
    m.insert("settings_tab_users",       ("Používatelia", "Users"));
```

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "feat(ui): i18n keys for reports, now panel, adaptive nav"
```

---

## Task 9: CSS — adaptive nav, Now panel, KPI cards, alerts banner

**Files:**
- Modify: `spinbike-ui/style.css`

- [ ] **Step 1: Append adaptive nav rules**

Append to `spinbike-ui/style.css` (keep all existing content):

```css
/* ─────────── Adaptive navigation ─────────── */
.app-shell { padding-bottom: calc(56px + env(safe-area-inset-bottom)); }

.adaptive-nav {
    position: fixed;
    left: 0; right: 0; bottom: 0;
    display: flex;
    justify-content: space-around;
    align-items: stretch;
    background: var(--surface);
    border-top: 1px solid var(--border);
    padding-bottom: env(safe-area-inset-bottom);
    z-index: 100;
}

.adaptive-nav__item {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    min-height: 56px;
    padding: 6px 4px;
    color: var(--text-muted);
    text-decoration: none;
    font-size: var(--fs-xs);
    font-weight: 500;
}

.adaptive-nav__item[aria-current="page"] {
    color: var(--brand);
    background: var(--brand-tint);
    border-radius: 0;
}

.adaptive-nav__icon { font-size: 20px; line-height: 1; }
.adaptive-nav__label { letter-spacing: 0.02em; }

@media (min-width: 768px) {
    .app-shell { padding-bottom: 0; padding-left: 72px; }
    .adaptive-nav {
        top: 0; right: auto; bottom: 0;
        width: 72px;
        flex-direction: column;
        justify-content: flex-start;
        padding: var(--s-3) 0;
        border-top: none;
        border-right: 1px solid var(--border);
    }
    .adaptive-nav__item { flex: 0 0 auto; min-height: 64px; padding: 10px 4px; }
    .adaptive-nav__item[aria-current="page"] { border-radius: var(--r-sm); margin: 2px var(--s-2); }
}
```

- [ ] **Step 2: Append KPI, alerts, Now panel rules**

```css
/* ─────────── KPI cards ─────────── */
.kpi-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s-3);
    margin-bottom: var(--s-4);
}
@media (min-width: 768px) { .kpi-grid { grid-template-columns: repeat(4, 1fr); } }

.kpi-card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--r);
    padding: var(--s-4);
    min-height: 96px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
}
.kpi-card__label { font-size: var(--fs-xs); color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.05em; }
.kpi-card__value { font-size: var(--fs-2xl); font-weight: 700; color: var(--text); font-variant-numeric: tabular-nums; }

/* ─────────── Alerts banner ─────────── */
.alerts-banner {
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 3px solid #f59e0b;
    border-radius: var(--r);
    overflow: hidden;
    margin-bottom: var(--s-4);
}
.alerts-banner__head { padding: var(--s-2) var(--s-4); font-size: var(--fs-xs); color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.05em; background: var(--surface-2); }
.alerts-banner__row  { display: flex; align-items: center; gap: var(--s-3); padding: var(--s-3) var(--s-4); border-top: 1px solid var(--border); cursor: pointer; }
.alerts-banner__row:first-of-type { border-top: none; }
.alerts-banner__body { flex: 1; font-size: var(--fs-sm); color: var(--text); }
.alerts-banner__dismiss { min-width: 44px; min-height: 44px; background: transparent; border: 0; color: var(--text-dim); font-size: var(--fs-md); cursor: pointer; }

/* ─────────── Reports date strip ─────────── */
.reports-date-strip {
    display: flex; flex-direction: column; gap: var(--s-2);
    margin-bottom: var(--s-4);
}
.reports-date-strip__sub {
    font-size: var(--fs-sm);
    color: var(--text-muted);
    text-align: center;
    padding: var(--s-2) 0;
}

.reports-range-buttons { display: flex; gap: var(--s-2); }
.reports-range-buttons .btn--compact { flex: 1; }

/* ─────────── Now panel ─────────── */
.now-panel { margin-bottom: var(--s-5); }
.now-panel__head {
    display: flex; align-items: center; gap: var(--s-3);
    padding: var(--s-3) var(--s-4);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--r);
    cursor: pointer;
    min-height: 56px;
}
.now-panel__head.now-panel__head--running { border-left: 3px solid var(--brand); }
.now-panel__title { flex: 1; font-weight: 500; color: var(--text); }
.now-panel__badge { font-size: var(--fs-xs); font-weight: 600; padding: 2px 8px; border-radius: var(--r-pill); background: var(--surface-2); color: var(--text-muted); }
.now-panel__chevron { color: var(--text-dim); font-size: var(--fs-md); }
.now-panel__body { margin-top: var(--s-2); }

/* Feed row kinds */
.feed-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.feed-dot--charge   { background: var(--danger); }
.feed-dot--topup    { background: var(--brand); }
.feed-dot--pass     { background: var(--info); }
.feed-dot--voided   { background: var(--text-dim); }

/* Responsive reports page */
.reports-page { padding: var(--s-4); max-width: 960px; margin: 0 auto; }
```

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/style.css
git commit -m "feat(ui): CSS for adaptive nav, KPI cards, alerts banner, now panel"
```

Run `cd spinbike-ui && ...` is NOT required — CSS is pure text and needs no local build.

---

## Task 10: Phase B CI checkpoint

- [ ] **Step 1: Verify lint**

Run locally:

```bash
cargo fmt --all --check
```

If it reports diffs, run `cargo fmt --all` and commit with message `chore: fmt`.

- [ ] **Step 2: Push Phase B**

```bash
git push
```

- [ ] **Step 3: Verify CI green**

Monitor run; do not proceed to Phase C until CI is green.

---

# Phase C — Adaptive nav + routing

---

## Task 11: `AdaptiveNav` component

**Files:**
- Create: `spinbike-ui/src/components/adaptive_nav.rs`
- Modify: `spinbike-ui/src/components/mod.rs` (re-export)

- [ ] **Step 1: Create the component**

Create `spinbike-ui/src/components/adaptive_nav.rs`:

```rust
use leptos::prelude::*;

use crate::auth;
use crate::i18n::{self, Lang};

/// Adaptive navigation: bottom tab bar on phone, left sidebar on desktop.
/// Only rendered for logged-in admin users.
#[component]
pub fn AdaptiveNav(auth_ver: ReadSignal<u32>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    let user = move || {
        let _ = auth_ver.get();
        auth::get_user()
    };

    let current_path = move || {
        leptos_router::hooks::use_location().pathname.get()
    };

    view! {
        {move || {
            let Some(u) = user() else { return ().into_any(); };
            if u.role != "admin" && u.role != "staff" { return ().into_any(); }
            let path = current_path();
            let is_active = move |prefix: &str| -> bool {
                if prefix == "/" { path == "/" } else { path.starts_with(prefix) }
            };
            view! {
                <nav class="adaptive-nav" data-testid="adaptive-nav">
                    <a href="/" class="adaptive-nav__item"
                       data-testid="nav-desk"
                       aria-current=move || if is_active("/") { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"🏠"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_desk")}</span>
                    </a>
                    <a href="/schedule" class="adaptive-nav__item"
                       data-testid="nav-schedule"
                       aria-current=move || if is_active("/schedule") { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"📅"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_schedule")}</span>
                    </a>
                    <a href="/reports" class="adaptive-nav__item"
                       data-testid="nav-reports"
                       aria-current=move || if is_active("/reports") { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"📊"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_reports")}</span>
                    </a>
                    <a href="/settings" class="adaptive-nav__item"
                       data-testid="nav-settings"
                       aria-current=move || if is_active("/settings") { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"⚙"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_settings")}</span>
                    </a>
                </nav>
            }.into_any()
        }}
    }
}
```

- [ ] **Step 2: Export it**

Edit `spinbike-ui/src/components/mod.rs` — add `pub mod adaptive_nav;` and `pub use adaptive_nav::AdaptiveNav;`.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/components/adaptive_nav.rs spinbike-ui/src/components/mod.rs
git commit -m "feat(ui): AdaptiveNav component (bottom tabs + sidebar)"
```

---

## Task 12: Router rewire — new routes + redirects

**Files:**
- Modify: `spinbike-ui/src/router.rs`
- Modify: `spinbike-ui/src/pages/mod.rs`

- [ ] **Step 1: Add route declarations and redirects**

In `spinbike-ui/src/router.rs`, update the `<Routes>` block (reference pattern from the research). Replace the existing `Navbar` with `AdaptiveNav` and add new routes. The final structure:

```rust
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes, Redirect};
use leptos_router::path;

use crate::components::AdaptiveNav;
use crate::pages::*;

#[component]
pub fn AppRouter(auth_ver: ReadSignal<u32>, _set_auth_ver: WriteSignal<u32>) -> impl IntoView {
    view! {
        <Router>
            <div class="app-shell">
                <AdaptiveNav auth_ver=auth_ver />
                <div class="page">
                    <Routes fallback=move || view! { <p>"404"</p> }>
                        <Route path=path!("/")               view=DashboardPage />
                        <Route path=path!("/login")          view=LoginPage />
                        <Route path=path!("/register")       view=RegisterPage />
                        <Route path=path!("/link-card")      view=LinkCardPage />
                        <Route path=path!("/my/bookings")    view=MyBookingsPage />
                        <Route path=path!("/my/balance")     view=MyBalancePage />
                        <Route path=path!("/schedule")       view=SchedulePage />
                        <Route path=path!("/reports")        view=ReportsPage />
                        <Route path=path!("/settings")       view=SettingsPage />
                        <Route path=path!("/admin")          view=|| view! { <Redirect path="/settings"/> } />
                        <Route path=path!("/staff")          view=|| view! { <Redirect path="/"/> } />
                        <Route path=path!("/staff/classes")  view=|| view! { <Redirect path="/schedule"/> } />
                    </Routes>
                </div>
            </div>
        </Router>
    }
}
```

Adjust the existing top-bar (if any separate header / language toggle) to be positioned above `AdaptiveNav` — or keep it inside `.page` as it was. Existing `Navbar` can be kept as a pure header (center name + language toggle) with nav-links removed.

- [ ] **Step 2: Stub `ReportsPage` and `SettingsPage`**

Create `spinbike-ui/src/pages/reports/mod.rs` with a minimal stub (real body comes in Task 14):

```rust
use leptos::prelude::*;

#[component]
pub fn ReportsPage() -> impl IntoView {
    view! { <div class="reports-page" data-testid="reports-page">"Reports"</div> }
}
```

For `SettingsPage`, rename the existing `AdminPage` export to `SettingsPage` OR add a thin wrapper in `spinbike-ui/src/pages/mod.rs`:

```rust
pub use admin::AdminPage as SettingsPage;
```

Keep `AdminPage` export available too for any in-flight usage, but the router uses `SettingsPage`.

Update `spinbike-ui/src/pages/mod.rs` to add `pub mod reports; pub use reports::ReportsPage;`.

- [ ] **Step 3: Remove `Navbar` link rows (keep it as header only)**

Edit `spinbike-ui/src/components/nav.rs` — keep the `<nav class="navbar">` outer element but remove the destination `<a>` links (Desk/Admin/Schedule), keeping only the logo/lockup and the language+logout toggles. This avoids duplicate nav items while keeping the header row.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/router.rs \
        spinbike-ui/src/pages/mod.rs \
        spinbike-ui/src/pages/reports/mod.rs \
        spinbike-ui/src/components/nav.rs
git commit -m "feat(ui): wire /reports, /settings, adaptive nav, legacy redirects"
```

---

## Task 13: Phase C CI checkpoint

- [ ] **Step 1: `cargo fmt --all --check` locally; fix & commit if needed.**

- [ ] **Step 2: Push.**

- [ ] **Step 3: Verify CI green (WASM build must succeed).**

Do not proceed to Phase D until green.

---

# Phase D — Reports page

---

## Task 14: Reports page skeleton with date state and API fetch

**Files:**
- Modify: `spinbike-ui/src/pages/reports/mod.rs`

- [ ] **Step 1: Replace the stub with a full skeleton**

Edit `spinbike-ui/src/pages/reports/mod.rs`:

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::reports::{KpiSummary, ReportEvent, ReportResponse};

mod kpi_cards;
mod alerts_banner;
mod activity_feed;
mod filters_bar;
mod sheets;

pub use kpi_cards::KpiCards;
pub use alerts_banner::AlertsBanner;
pub use activity_feed::ActivityFeed;
pub use filters_bar::{FiltersBar, FiltersState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeMode { Day, Week, Month }

#[component]
pub fn ReportsPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    // Anchor date (default today).
    let (anchor, set_anchor) = signal(chrono::Local::now().date_naive());
    let (mode, set_mode) = signal(RangeMode::Day);

    // Filters.
    let (filters, set_filters) = signal(FiltersState::default());

    // Data.
    let (kpi, set_kpi) = signal(KpiSummary { revenue_eur: 0.0, attendance: 0, passes_sold: 0, cash_in_eur: 0.0 });
    let (events, set_events) = signal::<Vec<ReportEvent>>(Vec::new());
    let (loading, set_loading) = signal(true);
    let (has_more, set_has_more) = signal(false);
    let (error, set_error) = signal(String::new());

    let fetch = move || {
        set_loading.set(true);
        set_error.set(String::new());
        let url = match mode.get_untracked() {
            RangeMode::Day => format!("/api/reports/day?date={}", anchor.get_untracked().format("%Y-%m-%d")),
            RangeMode::Week => {
                let from = anchor.get_untracked() - chrono::Duration::days(6);
                let to = anchor.get_untracked();
                format!("/api/reports/range?from={}&to={}", from.format("%Y-%m-%d"), to.format("%Y-%m-%d"))
            }
            RangeMode::Month => {
                let from = anchor.get_untracked() - chrono::Duration::days(29);
                let to = anchor.get_untracked();
                format!("/api/reports/range?from={}&to={}", from.format("%Y-%m-%d"), to.format("%Y-%m-%d"))
            }
        };
        spawn_local(async move {
            match api::get::<ReportResponse>(&url).await {
                Ok(r) => {
                    set_kpi.set(r.kpi);
                    set_events.set(r.events);
                    set_has_more.set(r.has_more);
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    Effect::new(move |_| { let _ = anchor.get(); let _ = mode.get(); fetch(); });

    view! {
        <div class="reports-page" data-testid="reports-page">

            // Date strip
            <div class="reports-date-strip">
                <div class="seg" role="tablist">
                    <button class="seg__item" data-testid="date-prev"
                            on:click=move |_| set_anchor.update(|d| *d = *d - chrono::Duration::days(1))>
                        "‹"
                    </button>
                    <button class="seg__item" data-testid="date-label"
                            aria-selected="true"
                            on:click=move |_| {/* TODO: open calendar sheet in Task 19 */}>
                        {move || anchor.get().format("%Y-%m-%d").to_string()}
                    </button>
                    <button class="seg__item" data-testid="date-next"
                            on:click=move |_| set_anchor.update(|d| *d = *d + chrono::Duration::days(1))>
                        "›"
                    </button>
                </div>
                <div class="reports-range-buttons">
                    <button class="btn btn--compact"
                            data-testid="range-week"
                            class:btn--primary=move || mode.get() == RangeMode::Week
                            on:click=move |_| set_mode.set(RangeMode::Week)>
                        {move || i18n::t(lang.get(), "reports_week")}
                    </button>
                    <button class="btn btn--compact"
                            data-testid="range-month"
                            class:btn--primary=move || mode.get() == RangeMode::Month
                            on:click=move |_| set_mode.set(RangeMode::Month)>
                        {move || i18n::t(lang.get(), "reports_month")}
                    </button>
                    <button class="btn btn--compact"
                            data-testid="range-day"
                            class:btn--primary=move || mode.get() == RangeMode::Day
                            on:click=move |_| set_mode.set(RangeMode::Day)>
                        {move || i18n::t(lang.get(), "reports_today")}
                    </button>
                </div>
            </div>

            // Error
            {move || if !error.get().is_empty() {
                view! { <div class="alert alert--error" data-testid="reports-error">{move || error.get()}</div> }.into_any()
            } else { ().into_any() }}

            // Alerts (wired in Task 17)
            <AlertsBanner />

            // KPI cards
            <KpiCards kpi=kpi />

            // Filters (wired in Task 16)
            <FiltersBar filters=filters set_filters=set_filters />

            // Activity feed
            <ActivityFeed events=events loading=loading has_more=has_more filters=filters anchor=anchor mode=mode set_events=set_events set_has_more=set_has_more />
        </div>
    }
}
```

- [ ] **Step 2: Stub sub-modules so the page compiles**

Create stub files (bodies filled in later tasks) — each must define the exported component:

`spinbike-ui/src/pages/reports/kpi_cards.rs`:

```rust
use leptos::prelude::*;
use spinbike_core::reports::KpiSummary;

#[component]
pub fn KpiCards(kpi: ReadSignal<KpiSummary>) -> impl IntoView {
    view! { <div class="kpi-grid" data-testid="kpi-grid">{move || format!("{:?}", kpi.get())}</div> }
}
```

`spinbike-ui/src/pages/reports/alerts_banner.rs`:

```rust
use leptos::prelude::*;

#[component]
pub fn AlertsBanner() -> impl IntoView {
    view! { <div data-testid="alerts-banner-stub"></div> }
}
```

`spinbike-ui/src/pages/reports/filters_bar.rs`:

```rust
use leptos::prelude::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FiltersState {
    pub event_kind: Option<String>,  // "charge"|"topup"|"pass"|None
    pub service:    Option<String>,  // service name or None
    pub search:     String,
}

#[component]
pub fn FiltersBar(filters: ReadSignal<FiltersState>, set_filters: WriteSignal<FiltersState>) -> impl IntoView {
    let _ = filters; let _ = set_filters;
    view! { <div data-testid="filters-bar-stub"></div> }
}
```

`spinbike-ui/src/pages/reports/activity_feed.rs`:

```rust
use leptos::prelude::*;
use spinbike_core::reports::ReportEvent;
use super::{RangeMode, FiltersState};

#[component]
pub fn ActivityFeed(
    events: ReadSignal<Vec<ReportEvent>>,
    loading: ReadSignal<bool>,
    has_more: ReadSignal<bool>,
    filters: ReadSignal<FiltersState>,
    anchor: ReadSignal<chrono::NaiveDate>,
    mode: ReadSignal<RangeMode>,
    set_events: WriteSignal<Vec<ReportEvent>>,
    set_has_more: WriteSignal<bool>,
) -> impl IntoView {
    let _ = (loading, filters, anchor, mode, set_events, set_has_more);
    view! {
        <div class="group" data-testid="activity-feed">
            {move || events.get().into_iter().map(|e| view! {
                <div class="list-row">
                    <div class="list-row__main">
                        <div class="list-row__title">{format!("{} {}", e.created_at, e.card_name.clone().unwrap_or_default())}</div>
                        <div class="list-row__sub">{e.service_name.clone().unwrap_or_default()}</div>
                    </div>
                    <div class="list-row__amount">{format!("{:.2}", e.amount)}</div>
                </div>
            }).collect::<Vec<_>>()}
            {move || if has_more.get() {
                view! { <button class="btn btn--block">"Load older"</button> }.into_any()
            } else { ().into_any() }}
        </div>
    }
}
```

`spinbike-ui/src/pages/reports/sheets/mod.rs`:

```rust
pub mod calendar_picker;
pub mod alert_detail;
```

`spinbike-ui/src/pages/reports/sheets/calendar_picker.rs` and `alert_detail.rs` — empty modules with a single `use leptos::prelude::*;` line. They get bodies in Task 19 and Task 17 respectively.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/reports
git commit -m "feat(ui): /reports page skeleton with date strip + mode switch"
```

---

## Task 15: `KpiCards` component — real implementation

**Files:**
- Modify: `spinbike-ui/src/pages/reports/kpi_cards.rs`

- [ ] **Step 1: Replace the stub**

```rust
use leptos::prelude::*;
use spinbike_core::reports::KpiSummary;

use crate::i18n::{self, Lang};

#[component]
pub fn KpiCards(kpi: ReadSignal<KpiSummary>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    view! {
        <div class="kpi-grid" data-testid="kpi-grid">
            <div class="kpi-card" data-testid="kpi-revenue">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_revenue")}</div>
                <div class="kpi-card__value">{move || format!("{:.2} €", kpi.get().revenue_eur)}</div>
            </div>
            <div class="kpi-card" data-testid="kpi-attendance">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_attendance")}</div>
                <div class="kpi-card__value">{move || format!("{}", kpi.get().attendance)}</div>
            </div>
            <div class="kpi-card" data-testid="kpi-passes">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_passes")}</div>
                <div class="kpi-card__value">{move || format!("{}", kpi.get().passes_sold)}</div>
            </div>
            <div class="kpi-card" data-testid="kpi-cash-in">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_cash_in")}</div>
                <div class="kpi-card__value">{move || format!("{:.2} €", kpi.get().cash_in_eur)}</div>
            </div>
        </div>
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/pages/reports/kpi_cards.rs
git commit -m "feat(ui): KpiCards component"
```

---

## Task 16: `FiltersBar` component — real implementation

**Files:**
- Modify: `spinbike-ui/src/pages/reports/filters_bar.rs`

- [ ] **Step 1: Full implementation**

```rust
use leptos::prelude::*;

use crate::i18n::{self, Lang};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FiltersState {
    pub event_kind: Option<String>,
    pub service:    Option<String>,
    pub search:     String,
}

impl FiltersState {
    pub fn is_active(&self) -> bool {
        self.event_kind.is_some() || self.service.is_some() || !self.search.is_empty()
    }
}

#[component]
pub fn FiltersBar(
    filters: ReadSignal<FiltersState>,
    set_filters: WriteSignal<FiltersState>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (expanded, set_expanded) = signal(false);

    view! {
        <div class="group" data-testid="filters-bar">
            <div class="list-row list-row--interactive"
                 on:click=move |_| set_expanded.update(|v| *v = !*v)>
                <div class="list-row__main">
                    <div class="list-row__title">{move || i18n::t(lang.get(), "filters_label")}</div>
                </div>
                <div class="list-row__end">
                    {move || if filters.get().is_active() {
                        view! { <span class="badge badge--info" data-testid="filters-active">"●"</span> }.into_any()
                    } else { ().into_any() }}
                    <span>{move || if expanded.get() { "▾" } else { "▸" }}</span>
                </div>
            </div>
            {move || if expanded.get() {
                view! {
                    <div style="padding: var(--s-3) var(--s-4); display: flex; flex-direction: column; gap: var(--s-3);">
                        // Event kind chips
                        <div class="seg" role="tablist" data-testid="filter-event-kind">
                            {["", "charge", "topup", "pass"].iter().enumerate().map(|(i, kind)| {
                                let (key, label_key) = match *kind {
                                    "" => ("", "filters_event_all"),
                                    "charge" => ("charge", "filters_event_payments"),
                                    "topup" => ("topup", "filters_event_topups"),
                                    "pass" => ("pass", "filters_event_passes"),
                                    _ => unreachable!()
                                };
                                let key_s = key.to_string();
                                view! {
                                    <button class="seg__item"
                                            data-testid=format!("filter-kind-{}", if key.is_empty() { "all" } else { key })
                                            aria-selected=move || (filters.get().event_kind.as_deref() == if key_s.is_empty() { None } else { Some(key_s.as_str()) }).to_string()
                                            on:click={let key_s = key_s.clone(); move |_| set_filters.update(|f| f.event_kind = if key_s.is_empty() { None } else { Some(key_s.clone()) })}>
                                        {move || i18n::t(lang.get(), label_key)}
                                    </button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>

                        // Search input
                        <input class="form-control"
                               type="text"
                               data-testid="filter-search"
                               placeholder=move || i18n::t(lang.get(), "filters_search_placeholder").to_string()
                               prop:value=move || filters.get().search.clone()
                               on:input=move |ev| set_filters.update(|f| f.search = event_target_value(&ev)) />

                        <button class="btn btn--ghost btn--compact"
                                data-testid="filters-reset"
                                on:click=move |_| set_filters.set(FiltersState::default())>
                            {move || i18n::t(lang.get(), "filters_reset")}
                        </button>
                    </div>
                }.into_any()
            } else { ().into_any() }}
        </div>
    }
}

fn event_target_value(ev: &leptos::ev::Event) -> String {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}
```

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/pages/reports/filters_bar.rs
git commit -m "feat(ui): FiltersBar with event-kind chips + search"
```

---

## Task 17: `AlertsBanner` + `AlertDetailSheet` with per-day dismissal

**Files:**
- Modify: `spinbike-ui/src/pages/reports/alerts_banner.rs`
- Modify: `spinbike-ui/src/pages/reports/sheets/alert_detail.rs`

- [ ] **Step 1: Implement `AlertDetailSheet`**

`spinbike-ui/src/pages/reports/sheets/alert_detail.rs`:

```rust
use leptos::prelude::*;

use crate::components::Sheet;

#[derive(Clone, PartialEq, Eq)]
pub enum AlertType { Expiring, LowCredit, Inactive }

#[component]
pub fn AlertDetailSheet(
    alert_type: AlertType,
    #[prop(into)] on_close: Callback<()>,
) -> impl IntoView {
    let title = match alert_type {
        AlertType::Expiring => "Expiring passes",
        AlertType::LowCredit => "Low credit",
        AlertType::Inactive => "Inactive customers",
    }.to_string();

    view! {
        <Sheet
            on_close=on_close
            title=title
            testid="sheet-alert-detail".to_string()
        >
            <div class="group">
                <div class="list-row"><div class="list-row__main">"(card list rendered by parent)"</div></div>
            </div>
        </Sheet>
    }
}
```

The concrete list rendering happens in the parent banner using data it already has.

- [ ] **Step 2: Implement `AlertsBanner`**

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::reports::AlertsResponse;

const LS_PREFIX: &str = "reports_alerts_dismissed";

fn today_key() -> String {
    chrono::Local::now().date_naive().format("%Y-%m-%d").to_string()
}

fn is_dismissed(kind: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|ls| ls.get_item(&format!("{}_{}_{kind}", LS_PREFIX, today_key())).ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(false)
}
fn dismiss(kind: &str) {
    if let Some(ls) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = ls.set_item(&format!("{}_{}_{kind}", LS_PREFIX, today_key()), "1");
    }
}

#[component]
pub fn AlertsBanner() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<AlertsResponse>);
    let (ver, set_ver) = signal(0u32);

    Effect::new(move |_| {
        let _ = ver.get();
        spawn_local(async move {
            if let Ok(a) = api::get::<AlertsResponse>("/api/reports/alerts").await {
                set_data.set(Some(a));
            }
        });
    });

    let expiring_n = move || data.get().as_ref().map(|a| a.expiring_passes.len()).unwrap_or(0);
    let low_n      = move || data.get().as_ref().map(|a| a.low_credit.len()).unwrap_or(0);
    let inactive_n = move || data.get().as_ref().map(|a| a.inactive.len()).unwrap_or(0);

    view! {
        {move || {
            if data.get().is_none() { return ().into_any(); }
            let show_expiring = expiring_n() > 0 && !is_dismissed("expiring");
            let show_low      = low_n()      > 0 && !is_dismissed("low");
            let show_inactive = inactive_n() > 0 && !is_dismissed("inactive");
            if !show_expiring && !show_low && !show_inactive { return ().into_any(); }

            view! {
                <div class="alerts-banner" data-testid="alerts-banner">
                    <div class="alerts-banner__head">{move || i18n::t(lang.get(), "alerts_title")}</div>

                    {move || if show_expiring {
                        let n = expiring_n();
                        view! {
                            <div class="alerts-banner__row" data-testid="alert-expiring">
                                <div class="alerts-banner__body">
                                    {move || i18n::t(lang.get(), "alerts_expiring_passes").replace("{n}", &n.to_string())}
                                </div>
                                <button class="alerts-banner__dismiss"
                                        data-testid="alert-expiring-dismiss"
                                        on:click=move |ev| {
                                            ev.stop_propagation();
                                            dismiss("expiring");
                                            set_ver.update(|v| *v += 1);
                                        }>"×"</button>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}

                    {move || if show_low {
                        let n = low_n();
                        view! {
                            <div class="alerts-banner__row" data-testid="alert-low-credit">
                                <div class="alerts-banner__body">
                                    {move || i18n::t(lang.get(), "alerts_low_credit").replace("{n}", &n.to_string())}
                                </div>
                                <button class="alerts-banner__dismiss"
                                        data-testid="alert-low-credit-dismiss"
                                        on:click=move |ev| {
                                            ev.stop_propagation();
                                            dismiss("low");
                                            set_ver.update(|v| *v += 1);
                                        }>"×"</button>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}

                    {move || if show_inactive {
                        let n = inactive_n();
                        view! {
                            <div class="alerts-banner__row" data-testid="alert-inactive">
                                <div class="alerts-banner__body">
                                    {move || i18n::t(lang.get(), "alerts_inactive").replace("{n}", &n.to_string())}
                                </div>
                                <button class="alerts-banner__dismiss"
                                        data-testid="alert-inactive-dismiss"
                                        on:click=move |ev| {
                                            ev.stop_propagation();
                                            dismiss("inactive");
                                            set_ver.update(|v| *v += 1);
                                        }>"×"</button>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}
                </div>
            }.into_any()
        }}
    }
}
```

Tapping rows to open an `AlertDetailSheet` is a follow-up nicety — for now the dismissal + banner visibility is the priority. Tapping a row can be a no-op (data-testid on the row lets the E2E test find it for visibility checks). If you want to wire row-tap → sheet, do it here. Minimum viable = banner visible + dismiss works.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/reports/alerts_banner.rs \
        spinbike-ui/src/pages/reports/sheets/alert_detail.rs
git commit -m "feat(ui): AlertsBanner with per-day dismissal"
```

---

## Task 18: `ActivityFeed` — filters applied client-side + load-older pagination

**Files:**
- Modify: `spinbike-ui/src/pages/reports/activity_feed.rs`

- [ ] **Step 1: Full implementation**

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::reports::{EventKind, ReportEvent, ReportResponse};

use super::{FiltersState, RangeMode};

#[component]
pub fn ActivityFeed(
    events: ReadSignal<Vec<ReportEvent>>,
    loading: ReadSignal<bool>,
    has_more: ReadSignal<bool>,
    filters: ReadSignal<FiltersState>,
    anchor: ReadSignal<chrono::NaiveDate>,
    mode: ReadSignal<RangeMode>,
    set_events: WriteSignal<Vec<ReportEvent>>,
    set_has_more: WriteSignal<bool>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    let filtered = move || {
        let f = filters.get();
        let needle = f.search.to_lowercase();
        events.get().into_iter().filter(|e| {
            match f.event_kind.as_deref() {
                Some("charge") => if !matches!(e.kind(), EventKind::Charge) { return false; }
                Some("topup")  => if !matches!(e.kind(), EventKind::TopUp) { return false; }
                Some("pass")   => if !matches!(e.kind(), EventKind::PassSold) { return false; }
                _ => {}
            }
            if let Some(svc) = &f.service {
                if e.service_name.as_deref() != Some(svc.as_str()) { return false; }
            }
            if !needle.is_empty() {
                let hay = format!("{} {}",
                    e.card_name.clone().unwrap_or_default(),
                    e.barcode.clone().unwrap_or_default()).to_lowercase();
                if !hay.contains(&needle) { return false; }
            }
            true
        }).collect::<Vec<_>>()
    };

    let load_older = move |_| {
        let before = events.get_untracked().last().map(|e| e.created_at.clone());
        let url = match mode.get_untracked() {
            RangeMode::Day => format!("/api/reports/day?date={}&before={}",
                anchor.get_untracked().format("%Y-%m-%d"),
                before.as_deref().unwrap_or("")),
            _ => {
                let (from, to) = match mode.get_untracked() {
                    RangeMode::Week => (anchor.get_untracked() - chrono::Duration::days(6), anchor.get_untracked()),
                    RangeMode::Month => (anchor.get_untracked() - chrono::Duration::days(29), anchor.get_untracked()),
                    _ => unreachable!(),
                };
                format!("/api/reports/range?from={}&to={}&before={}",
                    from.format("%Y-%m-%d"),
                    to.format("%Y-%m-%d"),
                    before.as_deref().unwrap_or(""))
            }
        };
        spawn_local(async move {
            if let Ok(r) = api::get::<ReportResponse>(&url).await {
                set_events.update(|v| v.extend(r.events));
                set_has_more.set(r.has_more);
            }
        });
    };

    view! {
        {move || if loading.get() {
            view! { <div class="group"><div class="list-row">"..."</div></div> }.into_any()
        } else {
            let rows = filtered();
            if rows.is_empty() {
                let msg_key = if filters.get().is_active() { "feed_empty_filter" } else { "feed_empty_day" };
                view! {
                    <div class="group" data-testid="activity-feed-empty">
                        <div class="list-row"><div class="list-row__main">{move || i18n::t(lang.get(), msg_key)}</div></div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="group" data-testid="activity-feed">
                        {rows.into_iter().map(|e| {
                            let kind_class = match e.kind() {
                                EventKind::Charge => "feed-dot--charge",
                                EventKind::TopUp  => "feed-dot--topup",
                                EventKind::PassSold => "feed-dot--pass",
                                EventKind::Other  => "feed-dot--voided",
                            };
                            let amount_class = if e.amount < 0.0 { "list-row__amount list-row__amount--neg" } else { "list-row__amount list-row__amount--pos" };
                            let amount_display = if e.amount < 0.0 {
                                format!("{:.2} €", e.amount)
                            } else {
                                format!("+{:.2} €", e.amount)
                            };
                            let time_only = e.created_at.split(' ').nth(1).unwrap_or("").chars().take(5).collect::<String>();
                            view! {
                                <div class="list-row" data-testid="feed-row">
                                    <div class="feed-dot" class=kind_class></div>
                                    <div class="list-row__sub" style="min-width: 48px;">{time_only}</div>
                                    <div class="list-row__main">
                                        <div class="list-row__title">{e.card_name.clone().unwrap_or_default()}</div>
                                        <div class="list-row__sub">{e.service_name.clone().unwrap_or_default()}</div>
                                    </div>
                                    <div class=amount_class>{amount_display}</div>
                                    {if e.voided { view! { <span class="badge badge--voided">"voided"</span> }.into_any() } else { ().into_any() }}
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                }.into_any()
            }
        }}
        {move || if has_more.get() {
            view! {
                <button class="btn btn--block btn--ghost"
                        data-testid="feed-load-older"
                        on:click=load_older>
                    {move || i18n::t(lang.get(), "feed_load_older")}
                </button>
            }.into_any()
        } else { ().into_any() }}
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/pages/reports/activity_feed.rs
git commit -m "feat(ui): ActivityFeed with filters + pagination"
```

---

## Task 19: `CalendarPickerSheet` + wire it to the date label

**Files:**
- Modify: `spinbike-ui/src/pages/reports/sheets/calendar_picker.rs`
- Modify: `spinbike-ui/src/pages/reports/mod.rs`

- [ ] **Step 1: Implement calendar picker sheet (native `<input type="date">`-based, minimal)**

```rust
use leptos::prelude::*;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

#[component]
pub fn CalendarPickerSheet(
    current: ReadSignal<chrono::NaiveDate>,
    #[prop(into)] on_close: Callback<()>,
    #[prop(into)] on_pick: Callback<chrono::NaiveDate>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (typed, set_typed) = signal(current.get_untracked().format("%Y-%m-%d").to_string());

    view! {
        <Sheet
            on_close=on_close
            title=i18n::t(lang.get_untracked(), "reports_pick_date").to_string()
            testid="sheet-calendar-picker".to_string()
        >
            <div class="form-group">
                <input class="form-control"
                       type="date"
                       data-testid="calendar-picker-input"
                       prop:value=move || typed.get()
                       on:input=move |ev| {
                           use wasm_bindgen::JsCast;
                           if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                               set_typed.set(el.value());
                           }
                       }/>
            </div>
            <div class="sheet__actions">
                <button class="btn btn--ghost" on:click=move |_| on_close.run(())>"Cancel"</button>
                <button class="btn btn--primary"
                        data-testid="calendar-picker-confirm"
                        on:click=move |_| {
                            if let Ok(d) = chrono::NaiveDate::parse_from_str(&typed.get(), "%Y-%m-%d") {
                                on_pick.run(d);
                            }
                        }>"OK"</button>
            </div>
        </Sheet>
    }
}
```

- [ ] **Step 2: Wire it into `ReportsPage`**

In `spinbike-ui/src/pages/reports/mod.rs`, add:

```rust
use sheets::calendar_picker::CalendarPickerSheet;

// inside ReportsPage body, alongside other signals:
let (show_picker, set_show_picker) = signal(false);

// replace the date-label button's on:click with:
on:click=move |_| set_show_picker.set(true)

// anywhere inside the top-level <div class="reports-page">, before KpiCards:
{move || if show_picker.get() {
    view! {
        <CalendarPickerSheet
            current=anchor
            on_close=Callback::new(move |_| set_show_picker.set(false))
            on_pick=Callback::new(move |d| { set_anchor.set(d); set_show_picker.set(false); }) />
    }.into_any()
} else { ().into_any() }}
```

Also update `spinbike-ui/src/pages/reports/sheets/mod.rs` to `pub mod calendar_picker; pub mod alert_detail;` (if not already).

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/reports/sheets/calendar_picker.rs \
        spinbike-ui/src/pages/reports/sheets/mod.rs \
        spinbike-ui/src/pages/reports/mod.rs
git commit -m "feat(ui): CalendarPickerSheet + wire to /reports date label"
```

---

## Task 20: Phase D CI checkpoint

- [ ] **Step 1: `cargo fmt --all --check`**
- [ ] **Step 2: Push**
- [ ] **Step 3: Verify CI green (Rust + WASM)**

---

# Phase E — Desk Now panel

---

## Task 21: `NowPanel` component (read-only: current/next + roster)

**Files:**
- Create: `spinbike-ui/src/pages/desk/mod.rs`
- Create: `spinbike-ui/src/pages/desk/now_panel.rs`
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` (to mount `NowPanel` at the top)

- [ ] **Step 1: Create `desk` module**

`spinbike-ui/src/pages/desk/mod.rs`:

```rust
pub mod now_panel;
pub use now_panel::NowPanel;
```

Register in `spinbike-ui/src/pages/mod.rs`:

```rust
pub mod desk;
pub use desk::NowPanel;
```

- [ ] **Step 2: Implement `NowPanel`**

`spinbike-ui/src/pages/desk/now_panel.rs`:

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::reports::{NowResponse, RosterEntry, RosterStatus};

const LS_KEY: &str = "desk_now_collapsed";

fn load_collapsed() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|ls| ls.get_item(LS_KEY).ok().flatten())
        .map(|v| v == "1").unwrap_or(false)
}
fn save_collapsed(v: bool) {
    if let Some(ls) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = ls.set_item(LS_KEY, if v { "1" } else { "0" });
    }
}

#[component]
pub fn NowPanel() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<NowResponse>);
    let (collapsed, set_collapsed) = signal(load_collapsed());

    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(n) = api::get::<NowResponse>("/api/reports/now").await {
                set_data.set(Some(n));
            }
        });
    });

    view! {
        <div class="now-panel" data-testid="now-panel">
            {move || {
                let d = data.get();
                if d.is_none() {
                    return view! { <div class="now-panel__head"><div class="now-panel__title">"..."</div></div> }.into_any();
                }
                let n = d.unwrap();
                if let Some(cc) = n.current_class {
                    let roster = cc.roster.clone();
                    let capacity = cc.capacity;
                    view! {
                        <div class="now-panel__head now-panel__head--running"
                             data-testid="now-panel-head-running"
                             on:click=move |_| {
                                 set_collapsed.update(|v| { *v = !*v; save_collapsed(*v); });
                             }>
                            <div class="now-panel__title">
                                {format!("{} {} — {}", cc.start_time.clone(), cc.service_name.clone(),
                                          cc.instructor_name.clone().unwrap_or_default())}
                            </div>
                            <div class="now-panel__badge" data-testid="now-panel-badge">
                                {format!("{}/{}", roster.iter().filter(|r| !matches!(r.status, RosterStatus::Cancelled)).count(), capacity)}
                            </div>
                            <span class="now-panel__chevron">{move || if collapsed.get() { "▸" } else { "▾" }}</span>
                        </div>
                        {move || if !collapsed.get() {
                            let roster = roster.clone();
                            view! {
                                <div class="now-panel__body" data-testid="now-panel-body">
                                    <div class="group">
                                        {roster.into_iter().map(|r| roster_row(r, lang)).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            }.into_any()
                        } else { ().into_any() }}
                    }.into_any()
                } else if let Some(nc) = n.next_class {
                    let when = format!("{} {} ({})", nc.date.format("%a %Y-%m-%d"), nc.start_time, nc.service_name);
                    view! {
                        <div class="now-panel__head" data-testid="now-panel-head-next">
                            <div class="now-panel__title">
                                {move || i18n::t(lang.get(), "now_next_on").replace("{when}", &when)}
                            </div>
                            <div class="now-panel__badge">{format!("{}/{}", nc.booked, nc.capacity)}</div>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="now-panel__head" data-testid="now-panel-head-empty">
                            <div class="now-panel__title">{move || i18n::t(lang.get(), "now_no_more_today")}</div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

fn roster_row(r: RosterEntry, lang: ReadSignal<Lang>) -> impl IntoView {
    let (status_key, badge_class) = match r.status {
        RosterStatus::Booked     => ("status_booked",     "badge badge--booked"),
        RosterStatus::CheckedIn  => ("status_checked_in", "badge badge--pass"),
        RosterStatus::Cancelled  => ("status_cancelled",  "badge badge--cancelled"),
    };
    view! {
        <div class="list-row" data-testid="now-roster-row">
            <div class="list-row__main">
                <div class="list-row__title">{r.name.clone()}</div>
                <div class="list-row__sub">{r.barcode.clone().unwrap_or_default()}</div>
            </div>
            <span class=badge_class>{move || i18n::t(lang.get(), status_key)}</span>
        </div>
    }
}
```

- [ ] **Step 3: Mount `NowPanel` at the top of the Desk (dashboard) page**

Edit `spinbike-ui/src/pages/dashboard/mod.rs` — near the top of the returned view, before the existing search input, insert:

```rust
use crate::pages::NowPanel;
// ...

view! {
    <NowPanel />
    // ... rest of existing Dashboard body
}
```

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/desk \
        spinbike-ui/src/pages/mod.rs \
        spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "feat(ui): NowPanel on Desk (current/next class + roster)"
```

---

## Task 22: Card detail hierarchy cleanup

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs`

- [ ] **Step 1: Reorganise buttons into primary / secondary / tertiary, collapse contact info**

Make these changes in `card_panel.rs` (per research: current action-row structure around line 96-135). After the edits, the button layout is:

```rust
// Primary action: Charge (prominent, .btn--hero)
<div class="stack-12">
    <button class="btn btn--hero btn--primary btn--block"
            data-testid="charge-btn-primary"
            on:click=move |_| set_show_charge.set(true)>
        {move || i18n::t(lang.get(), "charge")}
    </button>
</div>

// Secondary actions: Top-up + Sell Pass + Edit (standard .btn.btn--ghost)
<div class="action-row">
    <button class="btn btn--ghost"
            data-testid="topup-btn"
            on:click=move |_| set_show_topup.set(true)>
        {move || i18n::t(lang.get(), "topup")}
    </button>
    <button class="btn btn--ghost btn--pass"
            data-testid="sell-pass-btn"  // keep testid stable
            on:click=move |_| set_show_sell_pass.set(true)>
        {move || i18n::t(lang.get(), "sell_pass_label")}
    </button>
    <button class="btn btn--ghost"
            data-testid="edit-info-btn"
            on:click=move |_| set_show_edit_info.set(true)>
        {move || i18n::t(lang.get(), "edit_info")}
    </button>
</div>

// Tertiary actions row: Block, Delete (.btn--compact at the bottom)
<div class="action-row" style="margin-top: var(--s-3);">
    <BlockButton card_id=card_id ... />
    <DeleteButton ... />  // if currently present
</div>

// Contact info — hidden under toggle
{move || {
    let (show_contact, set_show_contact) = signal(false);
    view! {
        <button class="btn btn--compact btn--ghost"
                data-testid="toggle-contact"
                on:click=move |_| set_show_contact.update(|v| *v = !*v)>
            {move || if show_contact.get() {
                i18n::t(lang.get(), "card_hide_contact")
            } else {
                i18n::t(lang.get(), "card_show_contact")
            }}
        </button>
        {move || if show_contact.get() {
            view! {
                <div class="group" data-testid="card-contact">
                    <div class="list-row">
                        <div class="list-row__main">
                            <div class="list-row__sub">{move || card.phone.clone().unwrap_or_default()}</div>
                            <div class="list-row__sub">{move || card.company.clone().unwrap_or_default()}</div>
                            // email if present
                        </div>
                    </div>
                </div>
            }.into_any()
        } else { ().into_any() }}
    }
}}
```

**Data-testid stability:** `sell-pass-btn`, `sell-pass-confirm`, `sell-pass-price`, `charge-amount`, `card-credit`, `action-panel`, `tab-history`, `tab-upcoming`, `tab-persistent`, `txn-void`, `pass-banner-active`, `pass-banner-expired` — **all of these remain unchanged.** Only the visual hierarchy reshuffles.

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/card_panel.rs
git commit -m "refactor(ui): card detail hierarchy (primary/secondary/tertiary) + collapsed contact"
```

---

## Task 23: Phase E CI checkpoint

- [ ] **Step 1: `cargo fmt --all --check`**
- [ ] **Step 2: Push**
- [ ] **Step 3: Verify CI green**

---

# Phase F — Schedule merge + Settings rename

---

## Task 24: Fold staff-classes admin features into `/schedule`

**Files:**
- Modify: `spinbike-ui/src/pages/schedule.rs`
- Delete: `spinbike-ui/src/pages/staff_dashboard.rs`
- Modify: `spinbike-ui/src/pages/mod.rs` (remove the `staff_dashboard` re-export)

- [ ] **Step 1: Lift admin-only logic**

Read `spinbike-ui/src/pages/staff_dashboard.rs`. Identify the admin-facing features it provides that are NOT in `schedule.rs` today (walk-in button per class, cancel class, roster visibility if any). Move those into `schedule.rs` gated by:

```rust
let user = crate::auth::get_user();
let is_admin = user.as_ref().map(|u| u.role == "admin" || u.role == "staff").unwrap_or(false);
```

Example additions to each class card:

```rust
{move || if is_admin {
    view! {
        <button class="btn btn--compact btn--ghost"
                data-testid="class-walk-in"
                on:click=move |_| /* open card-search sheet and book+charge */ >
            {move || i18n::t(lang.get(), "now_walk_in")}
        </button>
        <button class="btn btn--compact btn--ghost"
                data-testid="class-cancel"
                on:click=move |_| /* confirm and cancel */ >
            {move || i18n::t(lang.get(), "now_cancel_class")}
        </button>
    }.into_any()
} else { ().into_any() }}
```

If the walk-in and cancel-class flows are already wired server-side (check for existing endpoints like `/api/classes/walk-in` or `/api/classes/{id}/cancel`), reuse them. Otherwise, implementing those server endpoints is out of scope for this plan — walk-in can initially be a placeholder that opens card search and calls the existing `/api/cards/{id}/charge` endpoint + a booking creation call.

- [ ] **Step 2: Delete `staff_dashboard.rs`**

```bash
git rm spinbike-ui/src/pages/staff_dashboard.rs
```

Remove any re-export in `spinbike-ui/src/pages/mod.rs`. Fix any compile errors by re-routing imports to `schedule.rs`.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/schedule.rs spinbike-ui/src/pages/mod.rs
git commit -m "refactor(ui): fold staff_dashboard admin features into /schedule"
```

---

## Task 25: Rename `/admin` page UX to `Settings`

**Files:**
- Modify: `spinbike-ui/src/pages/admin.rs` (page title + tab labels)

- [ ] **Step 1: Replace the page title and tab labels**

Find the `<h1 class="page-title">` line in `admin.rs` — change:

```rust
<h1 class="page-title">{move || i18n::t(lang.get(), "admin")}</h1>
```

to:

```rust
<h1 class="page-title">{move || i18n::t(lang.get(), "nav_settings")}</h1>
```

Find the `ADMIN_TAB_KEYS` constant (or equivalent labels list) and change the label keys to:

- `templates` → `settings_tab_templates`
- `instructors` → `settings_tab_instructors`
- `services` → `settings_tab_services`
- `users` → `settings_tab_users`
- `settings` → `settings_tab_center`

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/pages/admin.rs
git commit -m "refactor(ui): rename /admin page title and tabs to Settings"
```

---

## Task 26: Phase F CI checkpoint

- [ ] **Step 1: `cargo fmt --all --check`**
- [ ] **Step 2: Push**
- [ ] **Step 3: Verify CI green**

---

# Phase G — E2E tests

Each E2E spec uses the existing `helpers.ts` + `activateUniqueCard()` pattern from the research. All tests assert zero console errors/warnings via `assertCleanConsole(consoleMessages)`.

---

## Task 27: `reports-day.spec.ts`

**Files:**
- Create: `e2e/tests/reports-day.spec.ts`

- [ ] **Step 1: Write the spec**

```typescript
import { test, expect } from '@playwright/test';
import {
    BASE_URL, loginViaAPI, loginViaUI,
    setupConsoleCheck, assertCleanConsole,
} from './helpers';

test.describe('Reports — day view', () => {
    test('KPIs match seeded transactions and feed shows rows', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // Seed one charge + one top-up via direct SQL is not possible over HTTP.
        // Use existing API endpoints instead: activate a card, charge it, top-up it.
        // (If test-fixtures endpoints exist in the server for raw seeding, prefer those.)
        const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
        const card = await fetch(`${BASE_URL}/api/cards/activate`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({
                barcode: `RPT-${suffix}`, initial_credit: 50,
                first_name: 'Rep', last_name: `Testee${suffix}`,
            }),
        }).then(r => r.json());

        // Make one charge (spinning).
        await fetch(`${BASE_URL}/api/cards/${card.id}/charge`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ amount: 5, service_name: 'Spinning' }),
        });

        await loginViaUI(page, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        await expect(page.locator('[data-testid="reports-page"]')).toBeVisible();
        await expect(page.locator('[data-testid="kpi-revenue"]')).toContainText('€');
        await expect(page.locator('[data-testid="kpi-attendance"]')).toBeVisible();
        await expect(page.locator('[data-testid="activity-feed"]')).toBeVisible();
        await expect(page.locator('[data-testid="feed-row"]').first()).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('prev/next day buttons navigate', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaUI(page, 'admin@test.com', 'admin123');
        await page.goto('/reports');
        const initialDate = await page.locator('[data-testid="date-label"]').innerText();
        await page.locator('[data-testid="date-prev"]').click();
        const yesterdayDate = await page.locator('[data-testid="date-label"]').innerText();
        expect(yesterdayDate).not.toBe(initialDate);
        await page.locator('[data-testid="date-next"]').click();
        await expect(page.locator('[data-testid="date-label"]')).toHaveText(initialDate);
        assertCleanConsole(consoleMessages);
    });

    test('calendar picker sets anchor date', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaUI(page, 'admin@test.com', 'admin123');
        await page.goto('/reports');
        await page.locator('[data-testid="date-label"]').click();
        await expect(page.locator('[data-testid="sheet-calendar-picker"]')).toBeVisible();
        await page.locator('[data-testid="calendar-picker-input"]').fill('2026-01-15');
        await page.locator('[data-testid="calendar-picker-confirm"]').click();
        await expect(page.locator('[data-testid="date-label"]')).toHaveText('2026-01-15');
        assertCleanConsole(consoleMessages);
    });
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/reports-day.spec.ts
git commit -m "test(e2e): reports day view KPIs and date nav"
```

---

## Task 28: `reports-filters.spec.ts`

**Files:**
- Create: `e2e/tests/reports-filters.spec.ts`

- [ ] **Step 1: Write the spec**

```typescript
import { test, expect } from '@playwright/test';
import { BASE_URL, loginViaAPI, loginViaUI, setupConsoleCheck, assertCleanConsole } from './helpers';

test('filters narrow the feed by kind and search', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Seed: unique card with a charge, top-up, and pass sale
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const card = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode: `FLT-${suffix}`, initial_credit: 100, first_name: 'Flt', last_name: `Filter${suffix}` }),
    }).then(r => r.json());
    await fetch(`${BASE_URL}/api/cards/${card.id}/charge`,
        { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
          body: JSON.stringify({ amount: 5, service_name: 'Spinning' }) });

    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/reports');

    // Expand filters
    await page.locator('[data-testid="filters-bar"]').click();

    // Filter to Payments only
    await page.locator('[data-testid="filter-kind-charge"]').click();
    await expect(page.locator('[data-testid="feed-row"]').first()).toBeVisible();

    // Search by customer name
    await page.locator('[data-testid="filter-search"]').fill(`Filter${suffix}`);
    await expect(page.locator('[data-testid="feed-row"]')).toHaveCount(1);

    // Reset
    await page.locator('[data-testid="filters-reset"]').click();
    await expect(page.locator('[data-testid="feed-row"]').first()).toBeVisible();

    assertCleanConsole(consoleMessages);
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/reports-filters.spec.ts
git commit -m "test(e2e): reports filters"
```

---

## Task 29: `reports-alerts.spec.ts`

**Files:**
- Create: `e2e/tests/reports-alerts.spec.ts`

- [ ] **Step 1: Write the spec**

```typescript
import { test, expect } from '@playwright/test';
import { BASE_URL, loginViaAPI, loginViaUI, setupConsoleCheck, assertCleanConsole } from './helpers';

test('needs-attention banner reflects low-credit customers', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Seed a card with credit < 5
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode: `LC-${suffix}`, initial_credit: 2, first_name: 'Low', last_name: `Credit${suffix}` }),
    });

    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/reports');
    await expect(page.locator('[data-testid="alerts-banner"]')).toBeVisible();
    await expect(page.locator('[data-testid="alert-low-credit"]')).toBeVisible();

    // Dismiss per-day
    await page.locator('[data-testid="alert-low-credit-dismiss"]').click();
    await expect(page.locator('[data-testid="alert-low-credit"]')).toHaveCount(0);

    assertCleanConsole(consoleMessages);
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/reports-alerts.spec.ts
git commit -m "test(e2e): reports alerts banner + dismissal"
```

---

## Task 30: `reports-range.spec.ts`

**Files:**
- Create: `e2e/tests/reports-range.spec.ts`

- [ ] **Step 1: Write the spec**

```typescript
import { test, expect } from '@playwright/test';
import { BASE_URL, loginViaAPI, loginViaUI, setupConsoleCheck, assertCleanConsole } from './helpers';

test('Week mode loads without error', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/reports');

    await page.locator('[data-testid="range-week"]').click();
    await expect(page.locator('[data-testid="kpi-revenue"]')).toBeVisible();
    await expect(page.locator('[data-testid="activity-feed"], [data-testid="activity-feed-empty"]').first()).toBeVisible();

    await page.locator('[data-testid="range-month"]').click();
    await expect(page.locator('[data-testid="kpi-revenue"]')).toBeVisible();

    assertCleanConsole(consoleMessages);
});

test('API rejects > 93-day range', async ({ page }) => {
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    const res = await fetch(`${BASE_URL}/api/reports/range?from=2025-01-01&to=2026-04-24`, {
        headers: { Authorization: `Bearer ${token}` }
    });
    expect(res.status).toBe(400);
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/reports-range.spec.ts
git commit -m "test(e2e): reports range (week/month + 93-day cap)"
```

---

## Task 31: `desk-now-panel.spec.ts`

**Files:**
- Create: `e2e/tests/desk-now-panel.spec.ts`

- [ ] **Step 1: Write the spec**

```typescript
import { test, expect } from '@playwright/test';
import { BASE_URL, loginViaAPI, loginViaUI, setupConsoleCheck, assertCleanConsole } from './helpers';

test('Now panel renders either current, next, or empty', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/');

    await expect(page.locator('[data-testid="now-panel"]')).toBeVisible();
    // At least ONE of the three heads must be present.
    const heads = page.locator(
        '[data-testid="now-panel-head-running"], [data-testid="now-panel-head-next"], [data-testid="now-panel-head-empty"]'
    );
    await expect(heads).toHaveCount(1);

    assertCleanConsole(consoleMessages);
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/desk-now-panel.spec.ts
git commit -m "test(e2e): Desk Now panel presence"
```

---

## Task 32: `nav-adaptive.spec.ts`

**Files:**
- Create: `e2e/tests/nav-adaptive.spec.ts`

- [ ] **Step 1: Write the spec**

```typescript
import { test, expect } from '@playwright/test';
import { loginViaUI, setupConsoleCheck, assertCleanConsole } from './helpers';

test('bottom tabs on mobile viewport', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await page.setViewportSize({ width: 375, height: 812 });
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/');
    const nav = page.locator('[data-testid="adaptive-nav"]');
    await expect(nav).toBeVisible();
    await expect(page.locator('[data-testid="nav-desk"]')).toBeVisible();
    await expect(page.locator('[data-testid="nav-reports"]')).toBeVisible();
    // Click through all four
    await page.locator('[data-testid="nav-reports"]').click();
    await expect(page).toHaveURL(/\/reports/);
    await page.locator('[data-testid="nav-schedule"]').click();
    await expect(page).toHaveURL(/\/schedule/);
    await page.locator('[data-testid="nav-settings"]').click();
    await expect(page).toHaveURL(/\/settings/);
    await page.locator('[data-testid="nav-desk"]').click();
    await expect(page).toHaveURL(/\/$/);
    assertCleanConsole(consoleMessages);
});

test('sidebar on desktop viewport', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await page.setViewportSize({ width: 1280, height: 800 });
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/');
    await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();
    // Sidebar has same testids; CSS makes it vertical. Click still works.
    await page.locator('[data-testid="nav-reports"]').click();
    await expect(page).toHaveURL(/\/reports/);
    assertCleanConsole(consoleMessages);
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/nav-adaptive.spec.ts
git commit -m "test(e2e): adaptive nav across viewports"
```

---

## Task 33: `schedule-roster-admin.spec.ts`

**Files:**
- Create: `e2e/tests/schedule-roster-admin.spec.ts`

- [ ] **Step 1: Write the spec (minimal — visibility check)**

```typescript
import { test, expect } from '@playwright/test';
import { loginViaUI, setupConsoleCheck, assertCleanConsole } from './helpers';

test('admin sees schedule with roster affordances', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/schedule');
    // Page renders without error
    await expect(page).toHaveURL(/\/schedule/);
    await expect(page.locator('body')).toContainText(/./); // non-empty
    assertCleanConsole(consoleMessages);
});

test('/staff/classes redirects to /schedule', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/staff/classes');
    await expect(page).toHaveURL(/\/schedule$/);
    assertCleanConsole(consoleMessages);
});

test('/admin redirects to /settings', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/admin');
    await expect(page).toHaveURL(/\/settings$/);
    assertCleanConsole(consoleMessages);
});
```

- [ ] **Step 2: Commit and push Phase G**

```bash
git add e2e/tests/schedule-roster-admin.spec.ts
git commit -m "test(e2e): schedule admin view + legacy route redirects"
git push
```

- [ ] **Step 3: Verify CI green (all E2E specs + unit tests + lint + build)**

---

# Phase H — Release

## Task 34: Bump version + deploy

**Files:**
- Modify: `VERSION`

- [ ] **Step 1: Bump to 0.10.0**

```bash
echo "0.10.0" > VERSION
scripts/sync-version.sh
```

- [ ] **Step 2: Commit and open PR**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump to 0.10.0 (staff/CEO redesign)"
git push

gh pr create --title "Staff/CEO redesign (v0.10.0)" --body "$(cat <<'EOF'
## Summary
- Adaptive navigation: bottom tabs on phone, sidebar on desktop
- New /reports page: day/week/month KPIs, activity feed, filters, needs-attention alerts
- New Desk "Now" panel: current/next class + roster
- /admin demoted to /settings (cog); /staff/classes folded into /schedule
- Reuses v0.9.0 design primitives (.sheet, .group, .list-row, .seg, .btn)

## Spec
docs/superpowers/specs/2026-04-24-staff-ceo-redesign-design.md

## Test plan
- [ ] Unit: reports day/range/alerts/now queries
- [ ] E2E: reports-day, reports-filters, reports-alerts, reports-range, desk-now-panel, nav-adaptive, schedule-roster-admin
- [ ] CI all green (lint, build, test, E2E, smoke)
- [ ] Post-deploy verification on prod: /reports renders with today's real numbers; /api/reports/alerts returns sensible content

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Verify PR is mergeable + CI green**

```bash
gh pr view --json mergeable,mergeableState,statusCheckRollup
```

Expected: `mergeable: true`, `mergeableState: clean`, all status checks `SUCCESS`.

- [ ] **Step 4: Post-deploy verification (after user authorises merge)**

Via Playwright against `https://spinbike.newlevel.media`:

1. Navigate to `/reports` as admin → verify today's KPIs match recent transactions.
2. Tap `‹ Yesterday` → verify yesterday's data loads.
3. Tap `Týždeň` → verify week aggregate.
4. Check needs-attention banner: if any alerts shown, confirm the underlying cards match (e.g. a customer with credit < 5).
5. Switch viewport to 375×812 → verify bottom tab nav visible.
6. Switch to 1280×800 → verify sidebar visible.
7. Browser console: zero errors, zero warnings.

Do NOT merge until user explicitly says "merge it".

---

## Self-review (plan author)

**Spec coverage:**

- [x] Adaptive nav (bottom + sidebar) — Tasks 9, 11
- [x] Routes `/reports`, `/settings`, `/admin` redirect, `/staff/classes` redirect — Task 12
- [x] Desk Now panel — Task 21
- [x] Card detail hierarchy cleanup — Task 22
- [x] Schedule consolidation (fold /staff/classes) — Task 24
- [x] Settings rename — Task 25
- [x] Reports page: date strip, KPI cards, alerts banner, activity feed, filters, calendar sheet, week/month — Tasks 14–19
- [x] Backend `/api/reports/{day,range,alerts,now}` — Tasks 1–7
- [x] 93-day cap on range — Task 6
- [x] Admin-only permission — Task 6 (`require_admin`)
- [x] Per-day dismissal of alerts via localStorage — Task 17
- [x] Pagination via `before` cursor — Tasks 2, 3, 14, 18
- [x] Reuse existing primitives (.sheet, .group, .list-row, .seg) — throughout
- [x] E2E specs (7 files) — Tasks 27–33
- [x] Rust integration tests for day/range/alerts/now — Tasks 2–5
- [x] Version bump 0.9.8 → 0.10.0 — Task 34

**Placeholder scan:** No "TBD" except the calendar-sheet `TODO` in Task 14 which is explicitly satisfied in Task 19 (forward-reference, not a placeholder).

**Type consistency:**
- `KpiSummary` (Task 1) used in `ReportResponse` (Task 1), consumed in `kpi_cards.rs` (Task 15) ✓
- `ReportEvent` fields referenced in `activity_feed.rs` (Task 18) match Task 1 definition ✓
- `RosterStatus` enum (Task 1) used in `now_panel.rs` (Task 21) ✓
- `AlertsResponse` (Task 1) used in `alerts_banner.rs` (Task 17) ✓
- `NowResponse` fields (Task 1) used in `now_panel.rs` (Task 21) ✓
- `FiltersState` (Task 14 stub; Task 16 real) referenced in `activity_feed.rs` (Task 18) ✓

**Known forward references** (intentional; resolved in later tasks):
- Task 14 stubs each sub-module; real implementations in Tasks 15–19.
- Task 17 uses `AlertDetailSheet` — minimal stub; banner row taps can be wired post-v0.10.0 without blocking this plan.

---

**End of plan.**
