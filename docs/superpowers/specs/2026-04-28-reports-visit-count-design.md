# Reports — Fix NAVSTEVY / ATTENDANCE Visit Count

**Date:** 2026-04-28
**Issue:** [#23](https://github.com/zbynekdrlik/spinbike/issues/23) — "number in visits in reports is not correct, it should sum payment for fitness, payment for spinning, visit fitness, visit spinning"

## Goal

Fix the reports-page KPI tile labelled `NAVSTEVY` (Slovak) / `ATTENDANCE` (English) so it correctly counts class visits as defined by the CEO: paid Fitness sessions + paid Spinning sessions + free Fitness pass-visits + free Spinning pass-visits. One number, same tile, same label.

## Bug

`crates/spinbike-server/src/db/reports.rs` aggregates the attendance KPI in two near-identical SQL statements (one in `day_report` at line 121, one in `range_report` at line 233). Both currently use:

```sql
COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0) AS attendance
```

That CASE expression has two flaws:

1. **Too broad.** It counts ANY paid (negative-amount) non-pass transaction. The seed creates 6 services: Spinning, Fitness, Monthly pass, Refreshments (`Občerstvenie`), Supplements (`Doplnky výživy`), Card activation fee (`Aktivácia karty`). A snack purchase from the Refreshments service inflates the visit count.
2. **Too narrow.** It excludes `amount = 0` rows, so the €0 `action='visit'` records that `POST /api/payments/log-visit` writes when a monthly-pass holder logs a class visit are never counted.

Result: the displayed visit count drifts away from the CEO's intuition in both directions on real data.

## Fix

Replace the CASE expression in BOTH `day_report` and `range_report` with:

```sql
COALESCE(SUM(
  CASE
    WHEN service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))
     AND (
       (action = 'charge' AND amount < 0 AND valid_until IS NULL)
       OR action = 'visit'
     )
    THEN 1 ELSE 0
  END
), 0) AS attendance
```

This adds two filters:

- **Service filter** — only Fitness or Spinning rows count. The `SELECT id FROM services WHERE name_en IN (...)` subquery keeps the lookup name-based so it survives any future service-id reordering. (`name_en` was chosen over `name_sk` because `name_en` is the same source the existing `is_class_visit()` predicate matches against in `spinbike-ui/src/pages/dashboard/mod.rs:103-105`.)
- **Action filter** — count both legs: a paid charge (`action='charge' AND amount < 0 AND valid_until IS NULL`) OR a logged pass-visit (`action='visit'`, which is always `amount = 0`).

Other KPIs in the same SELECT — `revenue_eur`, `passes_sold`, `cash_in_eur` — stay as they are. The CEO did not flag those as wrong, and changing them is out of scope.

## What does NOT change

- `KpiSummary` struct in `crates/spinbike-core/src/reports.rs` — `attendance: i64` field stays. JSON contract unchanged.
- UI labels (`NAVSTEVY` / `ATTENDANCE`) and the `kpi-attendance` tile.
- Other KPIs (revenue, passes_sold, cash_in_eur).
- API routes (`/api/reports/day`, `/api/reports/range`, `/api/reports/alerts`, `/api/reports/now`).
- Schema, migrations, backfill — no DB change. Historical data is fine; the bug was a calc error.
- Activity-feed event listing — still shows all transaction kinds. Only the KPI tile changes.
- `db::reports::alerts_*` and `db::reports::now_panel` — unchanged.
- Customer-facing pages (`my_balance`, `my_bookings`).
- Auto-charger (`crates/spinbike-server/src/jobs/charger.rs`).

## Files affected

| File | Change |
|---|---|
| `crates/spinbike-server/src/db/reports.rs` (~line 121) | Update `day_report` attendance SQL CASE expression |
| `crates/spinbike-server/src/db/reports.rs` (~line 233) | Update `range_report` attendance SQL CASE expression |
| `crates/spinbike-server/src/db/reports.rs` (tests module) | Add unit tests asserting the 4 included buckets count and the 4 excluded buckets don't |
| `e2e/tests/reports-attendance.spec.ts` | NEW Playwright test — seed mixed transactions via API, navigate to /reports, assert the KPI value, zero console errors |
| `VERSION` | Bump to next patch (per project version-bumping discipline) |

No backend API changes. No DB migration. No new dependencies. No frontend code changes.

## Implementation details

### SQL — `day_report` (replace the CASE in the SELECT around line 121)

```rust
let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
    "SELECT
        COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0.0 END), 0.0) AS revenue_eur,
        COALESCE(SUM(
          CASE
            WHEN service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))
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
```

The `WHERE date(created_at) = ?1 AND deleted_at IS NULL` clause is unchanged — the CASE is the only edit. Soft-deleted (voided) transactions remain excluded as before.

### SQL — `range_report` (replace the CASE in the SELECT around line 233)

```rust
let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
    "SELECT
        COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0.0 END), 0.0) AS revenue_eur,
        COALESCE(SUM(
          CASE
            WHEN service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))
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
```

The two SQL strings differ only in the WHERE clause (`= ?1` vs `BETWEEN ?1 AND ?2`). Keeping the CASE expression identical between them prevents drift.

### Rust unit tests (in `crates/spinbike-server/src/db/reports.rs` `#[cfg(test)] mod tests`)

A skeleton, scaled to the change. Both `day_report` and `range_report` need a test, and both should run against the same seeded fixture so the assertion is symmetric.

```rust
#[tokio::test]
async fn attendance_counts_only_fitness_and_spinning_visits() {
    let pool = setup_test_pool().await;
    // Seed transactions on a known date. setup_test_pool() uses the standard
    // migrations which seed services for us — we look them up by name_en.
    let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();

    let fitness_id = service_id_by_name_en(&pool, "Fitness").await;
    let spinning_id = service_id_by_name_en(&pool, "Spinning").await;
    let monthly_pass_id = service_id_by_name_en(&pool, "Monthly pass").await;
    let refreshments_id = service_id_by_name_en(&pool, "Refreshments").await;
    let card_fee_id = service_id_by_name_en(&pool, "Card activation fee").await;

    // 4 rows that SHOULD count toward attendance.
    insert_charge(&pool, fitness_id, -5.0, date).await;        // paid fitness
    insert_charge(&pool, spinning_id, -5.0, date).await;       // paid spinning
    insert_visit(&pool, fitness_id, date).await;               // free fitness on pass
    insert_visit(&pool, spinning_id, date).await;              // free spinning on pass

    // 5 rows that should NOT count toward attendance. TWO refreshments rows
    // (not one) so the buggy and fixed SQL return different totals — see
    // "make the test discriminating" note below.
    insert_charge(&pool, refreshments_id, -2.50, date).await;  // snack #1
    insert_charge(&pool, refreshments_id, -2.50, date).await;  // snack #2
    insert_charge(&pool, card_fee_id, -3.0, date).await;       // card activation fee
    insert_pass_sale(&pool, monthly_pass_id, -35.0, date).await; // valid_until set
    insert_topup(&pool, 10.0, date).await;                     // topup, no service

    let (kpi, _, _) = day_report(&pool, date, 50, None).await.unwrap();
    assert_eq!(kpi.attendance, 4, "only the 4 class-visit rows should count");

    // Same fixture must give the same answer through the range_report path.
    let (range_kpi, _, _) = range_report(&pool, date, date, 50, None).await.unwrap();
    assert_eq!(range_kpi.attendance, 4, "range_report must agree with day_report");
}
```

The helpers (`insert_charge`, `insert_visit`, `insert_pass_sale`, `insert_topup`, `service_id_by_name_en`) belong in the same `tests` module — keep them small and local. If `setup_test_pool` isn't already there, look at how other tests in `db::reports` set up a pool and reuse that pattern; if no test pool helper exists, add one that uses an in-memory SQLite + `run_migrations`.

**Important — make the test discriminating.** Today's SQL counts `amount < 0 AND valid_until IS NULL`. Against the fixture above, the OLD SQL would count 4 rows (paid Fitness, paid Spinning, paid Refreshments, paid card fee), and the NEW SQL would also count 4 rows (paid Fitness, paid Spinning, free Fitness visit, free Spinning visit). The compositions differ but the totals match — the test would pass against the buggy code and provide no signal.

Fix: add a SECOND paid Refreshments row to the fixture. Then the OLD SQL counts 5 and the NEW SQL counts 4. The test goes RED on unchanged code and GREEN after the fix. The plan must reflect this fixture adjustment, and the implementer must run the test against unchanged code to confirm the RED before applying the SQL fix.

### NEW Playwright test — `e2e/tests/reports-attendance.spec.ts`

Skeleton (the implementer fills it in following project conventions):

1. Login as `staff@test.com` (the seed creates this user; admin role is required for `/api/reports/*`, so use `admin@test.com` if staff isn't admin — check `e2e/tests/helpers.ts` for which user has the right role).
2. Activate one or two unique cards via `/api/cards/activate`.
3. Drive at least one of each transaction kind via the existing payment APIs:
   - `/api/payments/charge` with Fitness service (paid fitness)
   - `/api/payments/charge` with Spinning service (paid spinning)
   - `/api/payments/sell-pass` with Monthly pass (valid_until set, NOT counted)
   - `/api/payments/log-visit` with Fitness service (free pass-visit)
   - `/api/payments/log-visit` with Spinning service (free pass-visit)
   - `/api/payments/charge` with Refreshments (paid non-class, NOT counted)
   - `/api/payments/topup` (positive amount, no service, NOT counted)
4. Navigate to `/reports` (admin-only — login session must have admin role).
5. Read `[data-testid="kpi-attendance"]` value. Assert it equals 4 (paid F + paid S + visit F + visit S).
6. Standard zero-console-errors assertion at end (per `browser-console-zero-errors.md`).

The Playwright test exists in addition to the Rust unit tests, not instead of them — per `e2e-real-user-testing.md`, every fix that ships through the UI gets a real-browser assertion alongside the unit-level coverage.

## Acceptance criteria

- [ ] `db::reports::day_report` returns `attendance` equal to the count of (Fitness | Spinning) AND (paid charge | logged visit).
- [ ] `db::reports::range_report` returns the same definition over the date range.
- [ ] The 4 buckets count: paid Fitness, paid Spinning, free Fitness pass-visit, free Spinning pass-visit.
- [ ] These do NOT count: Refreshments, Supplements, Card activation fee, Monthly pass sale, Topup, voided/soft-deleted transactions.
- [ ] Existing `revenue_eur`, `passes_sold`, `cash_in_eur` KPI values remain unchanged on the same fixture (no regression on adjacent KPIs).
- [ ] New Rust unit test in `db::reports` asserts the 4 included + 4+ excluded buckets, RED before fix, GREEN after.
- [ ] New Playwright test `e2e/tests/reports-attendance.spec.ts` reads `[data-testid="kpi-attendance"]` from the live UI, zero console errors.
- [ ] All existing E2E tests still pass.
- [ ] CI green on the PR (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing, Smoke (dev) after deploy).
- [ ] Post-deploy verification on dev frontend reads the corrected KPI value via Playwright.

## Out of scope

- Per-service attendance breakdown (CEO chose single total).
- Adjusting `revenue_eur`, `passes_sold`, or `cash_in_eur` calculations.
- Adding a new KPI or a separate "free vs paid" split tile.
- Renaming `NAVSTEVY` / `ATTENDANCE` labels.
- Activity-feed event filtering (the per-event list still shows everything).
- Schema or migration changes.
- Backfilling historical data (the calc fix corrects historical reports automatically).

## Versioning

This work bundles into the existing OPEN PR #25 per CEO direction (same precedent as #17). PR #25 currently ships v0.13.7. The next patch (e.g. v0.13.8) is bumped on `dev` as the FIRST commit of this work, and PR #25's title/body get updated to reflect the bundled scope (#13 + #17 + #23).
