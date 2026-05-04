# Per-card Overview tab — Design

**Date:** 2026-05-03
**Status:** Approved (brainstorming complete)
**Audience:** Staff only — visible inside `/staff?card=...` card panel.

## Motivation

Staff at the desk currently see a card's `credit` balance and a flat scrolling history. There is no way to answer questions like "how often does this customer come?", "did they top up more this year than last?", "is their attendance trending up or down?" without scrolling and counting manually. This spec adds a per-card **Overview** tab that surfaces those answers in compact, scannable form.

The original ask included a "switch to a monthly pass" upsell prompt; that is **out of scope** for this spec — the user opted to read the numbers and decide manually.

## Architecture

One new tab inside the existing card panel, one new server endpoint, no new tables, no schema migration. All aggregation is a single GROUP-BY on `transactions`.

```
spinbike-ui
└─ src/pages/dashboard/
   ├─ card_panel.rs      ← add 4th tab "overview"
   └─ overview_tab.rs    ← NEW: fetches stats, renders KPI grid + 2 bar charts

spinbike-core
└─ src/stats.rs          ← NEW: shared StatsResponse / PeriodAgg / MonthlyBucket types

spinbike-server
└─ src/routes/cards.rs   ← add GET /api/cards/{id}/stats handler
```

Pure CSS bars — no chart library, no JS interop, no new WASM-side dependencies.

## Backend

### Endpoint

```
GET /api/cards/{id}/stats         (auth: staff role)
```

Returns:

```json
{
  "totals": {
    "this_month": { "visits": 11, "topped_up_eur": 50.00 },
    "this_year":  { "visits": 47, "topped_up_eur": 200.00 },
    "all_time":   { "visits": 812, "topped_up_eur": 3000.00 }
  },
  "monthly": [
    { "year_month": "2025-06", "visits":  4, "topped_up_eur":  0.00 },
    { "year_month": "2025-07", "visits":  2, "topped_up_eur":  0.00 },
    /* … exactly 12 entries, oldest → newest, no gaps … */
    { "year_month": "2026-05", "visits": 11, "topped_up_eur": 30.00 }
  ]
}
```

The `monthly` array always contains exactly 12 entries — the server fills zero-buckets for months with no rows so the UI can render a fixed-width chart without nullability handling.

### Shared types (`spinbike-core/src/stats.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodAgg {
    pub visits: i64,
    pub topped_up_eur: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodTotals {
    pub this_month: PeriodAgg,
    pub this_year: PeriodAgg,
    pub all_time: PeriodAgg,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonthlyBucket {
    /// Calendar month label "YYYY-MM" in the server's local timezone.
    pub year_month: String,
    pub visits: i64,
    pub topped_up_eur: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatsResponse {
    pub totals: PeriodTotals,
    /// Exactly 12 entries, oldest → newest. Zero-buckets included.
    pub monthly: Vec<MonthlyBucket>,
}
```

### Definitions

- **Visit:** a transaction row whose `service_id` resolves to a service with `name_sk IN ('Spinning','Fitness')`. Counts both:
  - paid charges (`action='charge'` against Spinning / Fitness), AND
  - pass-covered visits (`action='visit'` with the same service).

  Excludes Refreshments, Supplements, Card-activation fee, and pass purchases (`kind='monthly_pass'`).

- **Top-up:** a transaction row with `action='topup' AND amount > 0`. Negative-amount rows or other actions are not counted.

- **"This month":** rows whose `created_at` falls in the current calendar month in the server's local timezone (`strftime('%Y-%m', created_at, 'localtime') = strftime('%Y-%m','now','localtime')`).

- **"This year":** current calendar year, same locale rule.

- **"All time":** no date filter.

- **"Last 12 months":** the 12 calendar months ending with the current one (inclusive). E.g. on 2026-05-03 → `2025-06 … 2026-05`.

### SQL — single round-trip

The handler builds a `WITH visits AS (…), topups AS (…)` CTE, aggregates each, joins by year-month against a 12-row generated month series, and returns the response. Outline:

```sql
-- visit count per month (Spinning + Fitness, any action)
WITH visit_rows AS (
  SELECT strftime('%Y-%m', t.created_at, 'localtime') AS ym, t.created_at
  FROM transactions t
  JOIN services s ON s.id = t.service_id
  WHERE t.card_id = ?
    AND t.deleted_at IS NULL
    AND s.name_sk IN ('Spinning','Fitness')
),
topup_rows AS (
  SELECT strftime('%Y-%m', t.created_at, 'localtime') AS ym, t.amount
  FROM transactions t
  WHERE t.card_id = ?
    AND t.deleted_at IS NULL
    AND t.action = 'topup'
    AND t.amount > 0
)
SELECT
  -- one row of totals
  (SELECT COUNT(*) FROM visit_rows
     WHERE ym = strftime('%Y-%m','now','localtime'))                     AS visits_month,
  (SELECT COALESCE(SUM(amount),0) FROM topup_rows
     WHERE ym = strftime('%Y-%m','now','localtime'))                     AS topup_month,
  (SELECT COUNT(*) FROM visit_rows
     WHERE substr(ym,1,4) = strftime('%Y','now','localtime'))            AS visits_year,
  (SELECT COALESCE(SUM(amount),0) FROM topup_rows
     WHERE substr(ym,1,4) = strftime('%Y','now','localtime'))            AS topup_year,
  (SELECT COUNT(*) FROM visit_rows)                                      AS visits_all,
  (SELECT COALESCE(SUM(amount),0) FROM topup_rows)                       AS topup_all
;

-- monthly buckets: one row per (ym, visits, topped_up)
SELECT ym,
  (SELECT COUNT(*) FROM visit_rows v WHERE v.ym = m.ym)                  AS visits,
  (SELECT COALESCE(SUM(amount),0) FROM topup_rows tu WHERE tu.ym = m.ym) AS topped_up
FROM (/* 12-row month-series via recursive CTE */) m
ORDER BY ym ASC;
```

Two queries (totals + buckets) is acceptable. The handler implementation may inline both.

### Authorization

Reuse the `AuthUser` extractor and require `claims.role.can_process_payments()` (same gate as `seed_transactions` and other staff-side endpoints). Customers cannot hit this endpoint even by guessing the card id.

## Frontend

### Tab wiring (`card_panel.rs`)

Add a 4th item to `tab_items`:

```rust
("overview".to_string(), i18n::t(lang.get_untracked(), "tab_overview").to_string()),
```

Plus a 4th branch in the `match tab.as_str()`:

```rust
"overview" => view! { <OverviewTab card_id=card_id /> }.into_any(),
```

### `overview_tab.rs`

Fetches `/api/cards/{id}/stats` once on mount via existing `crate::api` helpers. Renders three sections:

1. **KPI table** — three rows × two columns:

   ```
                   Visits   Topped up
   This month        11      50.00 €
   This year         47     200.00 €
   All time         812   3,000.00 €
   ```

   Plain `<table>` with class `stats-kpi`. Right-aligned numbers. Currency is project-wide `€` (not localized — matches existing pages).

2. **Visits per month — last 12 months** — a horizontal-bar list:

   ```
   May'26  ███████████ 11
   Apr'26  ███████████ 11
   Mar'26  █████████    9
   …
   Jun'25  ████          4
   ```

   Newest at top so it matches the natural "history scrolls down into the past" mental model already used by the History tab. Each row is a `<div class="stats-row">` with three children: `<span class="stats-row__label">May'26</span>`, `<div class="stats-row__bar" style="width: 100%"></div>`, `<span class="stats-row__value">11</span>`. Bar width = `value / max(values_in_chart) * 100` percent. If `max == 0` (all zero), all bars render `width: 0`.

3. **€ topped up per month — last 12 months** — same shape, value formatted as `XX.XX €`.

If the response has zero data anywhere, the KPI rows still show `0` and `0.00 €` and the chart rows still render (twelve entries, all `width: 0`). No conditional "empty state" copy needed; an empty card is self-evident.

### Pure-CSS bars (~30 lines added to existing main SCSS)

```scss
.stats-kpi {
  width: 100%;
  border-collapse: collapse;
  td, th { padding: 0.4rem 0.6rem; text-align: right; }
  td:first-child, th:first-child { text-align: left; }
}

.stats-chart {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  margin-top: 0.75rem;
}

.stats-row {
  display: grid;
  grid-template-columns: 4.5rem 1fr 4rem;
  align-items: center;
  gap: 0.5rem;
  font-variant-numeric: tabular-nums;
}

.stats-row__bar {
  height: 0.9rem;
  background: var(--accent, #4caf50);
  border-radius: 2px;
  min-width: 0;
}

.stats-row__value { text-align: right; }
```

(Final values may differ; the design only specifies "horizontal bars rendered with `width: NN%`, accent colour matches existing palette".)

### i18n

New keys in `spinbike-ui/src/i18n.rs`:

| key                       | sk                            | en                           |
|---------------------------|-------------------------------|------------------------------|
| `tab_overview`            | `Prehľad`                     | `Overview`                   |
| `overview_kpi_heading`    | (none — table has no caption) | (none)                       |
| `overview_period_month`   | `Tento mesiac`                | `This month`                 |
| `overview_period_year`    | `Tento rok`                   | `This year`                  |
| `overview_period_all`     | `Spolu`                       | `All time`                   |
| `overview_col_visits`     | `Vstupy`                      | `Visits`                     |
| `overview_col_topup`      | `Dobitie`                     | `Topped up`                  |
| `overview_chart_visits`   | `Vstupy po mesiacoch`         | `Visits per month`           |
| `overview_chart_topup`    | `Dobitie po mesiacoch (€)`    | `Topped up per month (€)`    |

Month labels in the chart (`May'26`, `Apr'26`, …) are formatted client-side from the `year_month` string. Slovak short month names handled the same way as on `/reports`.

## Tests

### Server unit tests (`crates/spinbike-server/src/routes/cards.rs` test module)

Seed a card and exercise the handler against an in-memory DB. Test cases:

1. **Empty card** → totals all zero, 12 zero buckets returned, `year_month` strings cover the last 12 months.
2. **Mixed-service rows** → seed Spinning, Fitness, Refreshments, Supplements, monthly pass purchase. Assert visit count = (Spinning + Fitness rows only), top-up sum = (only `action='topup'` rows).
3. **Multi-month spread** → seed visits in 3 different months including current. Assert each month's bucket has the right visit count, others are zero.
4. **Visits older than 12 months** → assert `monthly` excludes them but `all_time` totals include them.
5. **Soft-deleted rows** (`deleted_at IS NOT NULL`) are excluded from both visit count and top-ups.
6. **Negative-amount topup row** (data anomaly) is excluded from top-up sum (defensive — protects against future migration mistakes).

### E2E test (`e2e/tests/per-card-overview.spec.ts`)

1. Login as staff via existing `loginViaAPI` helper.
2. Seed a card via `/api/test/seed-transactions` with a known shape: 2 Spinning visits last month, 3 Fitness visits this month, one €50 top-up this month, one €30 top-up 4 months ago, one Refreshments charge (must NOT count as a visit).
3. Navigate to `/staff?card=<barcode>`, click the "Prehľad" tab.
4. Assert:
   - "Tento mesiac" row shows `Vstupy: 3`, `Dobitie: 50.00 €`.
   - "Tento rok" row shows `Vstupy: 5`, `Dobitie: 80.00 €`.
   - "Spolu" row matches the same numbers (everything happened this year).
   - The visits chart contains a `[data-testid="stats-row"]` element with text `3` for this month and `2` for last month.
   - The top-ups chart contains a row with `50.00 €` for this month.
5. Assert zero browser console errors via the existing `setupConsoleCheck` / `assertCleanConsole` helpers.

### Mutation testing

Existing `cargo-mutants` job will diff-mutate the new aggregation function. The unit tests above are designed to kill mutants on the visit-counting predicate (Spinning/Fitness filter) and the top-up predicate (`amount > 0`).

## Versioning

This is one feature added on `dev`. Bump `VERSION` from `0.13.17` → `0.14.0` (new user-visible feature → minor bump per project convention) before any code changes. Run `bash scripts/sync-version.sh` to propagate.

## Out of scope

- "Switch to monthly pass" recommendation — user dropped during brainstorming.
- Year-over-year overlay on the bar charts — user picked single-series.
- Customer-facing analytics on `/my-balance` — staff-only.
- Sparkline / mini-chart on the History tab summary — possible follow-up.
- CSV / PDF export of the per-card stats — not requested.
- Charts beyond 12-month window — "all time" lives in the KPI total only.

## Risks / open considerations

- **Locale of `strftime('%Y-%m', …, 'localtime')`** — SQLite's `localtime` modifier uses the server's TZ. Production runs in `Europe/Bratislava`; "this month" correctly maps to Slovak calendar boundaries. Documented here so a future operator who relocates the server understands why month boundaries are timezone-sensitive.
- **Service-name string match (`'Spinning','Fitness'`)** — coupled to current service catalogue. If those names ever change (issue #50 is an unrelated SK rename for the monthly-pass label), this query needs updating. Mitigation: a unit test will assert the visit-count handler matches exactly the rows whose service name is `Spinning` or `Fitness`, so a rename will fail the test loudly.
- **All-time visit counts on long-history cards** (~800 rows) — query is `SELECT COUNT(*) WHERE service_id IN (…)`, indexed, executes in milliseconds. Not a concern.
