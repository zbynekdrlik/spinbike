# Spinning Visits KPI — Replace REVENUE Card

**Date:** 2026-05-12
**Issue:** (to be filed)
**Status:** Design — awaiting user review

## Background

The CEO-facing reports view (`/reports`) shows a 4-card KPI grid:

1. REVENUE (sum of credit charges in EUR)
2. ATTENDANCE (count of Fitness + Spinning class entries, credit-paid or monthly-pass)
3. PASSES (count of monthly passes sold)
4. CASH IN (sum of credit top-ups in EUR)

The CEO reports the REVENUE card is unusable: it sums money already received earlier as credit top-ups, so it double-counts against CASH IN and does not represent any decision-relevant business number. He instead wants to see how many people actually came to spinning class on the selected day or range — paid from credit or covered by a monthly pass, treated the same.

## Goal

Replace the REVENUE card with a SPINNING card that shows the total count of spinning class entries (paid-from-credit charges + monthly-pass zero-amount visits) for the selected day or date range.

## Out of scope

- ATTENDANCE card stays as-is (Fitness + Spinning combined). Only the first card changes.
- Door entries (single_entry service, retagged from Fitness in migration v16) do not count — only the `Spinning` service.
- Door open visits and Fitness paid entries continue to count toward ATTENDANCE as today.
- No new endpoint, no new page. The change lives entirely inside the existing `/api/reports/day` and `/api/reports/range` flow.

## Definition of "spinning visit"

A row in the `transactions` table counts as one spinning visit when ALL of the following hold:

- `service_id` references the row in `services` where `name_en = 'Spinning'`
- `deleted_at IS NULL` (non-voided)
- EITHER `action = 'charge' AND amount < 0 AND valid_until IS NULL` (paid from credit)
   OR `action = 'visit'` (zero-amount monthly-pass entry)

This mirrors the existing ATTENDANCE definition but scoped to Spinning only (existing definition also matches Fitness).

## Data model change

`crates/spinbike-core/src/reports.rs::KpiSummary`:

- Remove `revenue_eur: f64`.
- Add `spinning_visits: i64` as the first field.
- Final field order: `spinning_visits, attendance, passes_sold, cash_in_eur`.

This is a breaking change on the JSON payload of `/api/reports/day` and `/api/reports/range`, but the only consumer is the in-tree Leptos frontend, which ships in the same binary. No external clients.

## Server SQL

Both `day_report` and `range_report` in `crates/spinbike-server/src/db/reports.rs` change:

- Drop the `revenue_eur` aggregation entirely.
- Add a `spinning_visits` aggregation using the same shape as `attendance`, but bound to `spinbike_core::services::SPINNING_NAME_EN` only.
- `DbKpiRow` struct updates accordingly.

The existing `attendance`, `passes_sold`, and `cash_in_eur` aggregations are unchanged.

## UI change

`spinbike-ui/src/pages/reports/kpi_cards.rs`:

- Remove the `kpi-revenue` card.
- Add a new card in slot 1 with `data-testid="kpi-spinning-visits"`, label key `kpi_spinning_visits`, formatted as a plain integer (no euro sign, matching ATTENDANCE / PASSES).
- ATTENDANCE / PASSES / CASH IN cards stay in slots 2-4 unchanged.

## i18n

`spinbike-ui/src/i18n.rs`:

- Remove `kpi_revenue` entry.
- Add `kpi_spinning_visits` → Slovak `SPINNING`, English `SPINNING` (unaccented per existing i18n convention).

## Tests

Server: extend the existing fixture `attendance_counts_only_fitness_and_spinning_visits` in `crates/spinbike-server/src/db/reports.rs` to also assert:

- `day_kpi.spinning_visits == 2` (one paid spinning charge + one zero-amount spinning visit).
- `range_kpi.spinning_visits == 2` (same row count from range_report).

The same fixture covers both new aggregation correctness and isolation from Fitness, Refreshments, monthly-pass-sale, and top-up rows.

UI: Playwright assertion in the existing reports E2E spec — the `kpi-spinning-visits` testid is present, the `kpi-revenue` testid is absent, and the value renders as a non-negative integer.

## Risks

- Any external API consumer of `revenue_eur` breaks. None exist; verified by grep.
- An older deployed frontend reading the new payload would log a JSON-deserialize error. Single-binary deploy makes this a non-issue: frontend and backend ship together.

## Acceptance criteria

- The reports page card 1 shows SPINNING with an integer count for the selected day and range.
- The REVENUE card is gone from the DOM (no `kpi-revenue` testid).
- The server fixture asserts the count under the documented spinning-visit definition.
- A Playwright test asserts the new card renders and the old one is absent.
- CI green; existing ATTENDANCE / PASSES / CASH IN tests unchanged.
