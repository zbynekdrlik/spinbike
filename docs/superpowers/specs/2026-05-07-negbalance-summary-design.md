# Negative-balance list: count + sum summary (Issue #72)

## Goal

Desk negative-balance list shows **how many customers** are in debt and the
**total negative balance** in addition to the per-row list. Replaces obsolete
"Karty s dlhom" / "Cards with negative balance" heading with user-centric
"Klienti v minuse" / "Customers with negative balance" — the `cards` table
was dropped in PR #67, the list now operates on `users`.

## Scope

Frontend-only. No API change. No DB change. No new endpoint.

The summary (count + sum) is computed client-side from the already-fetched
`Vec<NegativeBalanceUser>`. The list is small and never paginated, so server-
side aggregation would be redundant work and an extra JSON field without
benefit (YAGNI).

## Heading format

Inline suffix on the existing `<h3 class="negative-balance-list__heading">`:

```
Klienti v minuse  ·  5  ·  -12.40 €
```

- `heading` — `i18n::t(lang, "negative_balance_heading")`
- `count` — `rows.len()` (always > 0 when this code runs; empty case
  short-circuits earlier with `<span></span>`)
- `sum_fmt` — `format!("{:.2} €", rows.iter().map(|r| r.credit).sum::<f64>())`,
  e.g. `-12.40 €` (ASCII hyphen — matches the existing per-row credit
  formatting in `format!("{:.2} €", r.credit)`)

Separator `  ·  ` (two-space middle-dot two-space) on both sides for visual
breathing.

## i18n change

In `spinbike-ui/src/i18n.rs`, key `negative_balance_heading`:

| Lang | Old                              | New                              |
|------|----------------------------------|----------------------------------|
| SK   | `Karty s dlhom`                  | `Klienti v minuse`               |
| EN   | `Cards with negative balance`    | `Customers with negative balance`|

Slovak unaccented per project convention (no `í`, no `é`).

Update the two unit tests:

- `negative_balance_heading_slovak` — assert `Klienti v minuse`
- `negative_balance_heading_english` — assert `Customers with negative balance`

## Files touched

1. **`spinbike-ui/src/pages/dashboard/negative_balance_list.rs`**
   - Add private helper `fn summary_suffix(rows: &[NegativeBalanceUser]) -> String`
     returning `"  ·  {count}  ·  {sum_fmt}"`.
   - In the render block, replace
     ```
     <h3 class="negative-balance-list__heading">{heading}</h3>
     ```
     with
     ```
     <h3 class="negative-balance-list__heading">
         {format!("{heading}{}", summary_suffix(&rows))}
     </h3>
     ```
   - Add 2 `#[wasm_bindgen_test]` cases on `summary_suffix`:
     - typical: 3 users with credits `-1.50`, `-3.10`, `-7.80` → suffix `"  ·  3  ·  -12.40 €"`
     - single user: 1 user with credit `-0.50` → suffix `"  ·  1  ·  -0.50 €"`

2. **`spinbike-ui/src/i18n.rs`**
   - Update `negative_balance_heading` SK + EN strings (table above).
   - Update the 2 existing unit tests to assert the new strings.

3. **`e2e/tests/negative-balance.spec.ts`**
   - Existing test seeds N negative-credit users and asserts list visible.
   - Extend it: read heading text, assert it contains `· {N} ·` and
     `{expected_sum_fmt}` (compute from seeded credits).
   - Existing `[data-testid="negative-balance-list"]` locator unchanged.

## Test coverage / mutation pressure

- `summary_suffix` tests assert the **exact formatted string**, including
  separator spacing and decimal precision. Mutants expected to die:
  - `rows.len()` → `0` (count mismatch)
  - `sum` → `0.0` (zero-sum mismatch)
  - `{:.2}` → `{:.0}` (decimal drop)
  - separator change (`·` → `,`, double-space → single-space)
- E2E asserts the heading on the live deployed page after seeding.
- i18n tests pin the SK + EN strings.

## Out of scope

- Server-side aggregation endpoint or new field on `NegativeBalanceUser`.
- Pagination (no current pagination; list size bounded by total
  negative-credit users — small N).
- Touching `negative_balance_heading` consumers other than this list (no
  others exist; verified via grep).
- CSS changes — the existing `.negative-balance-list__heading` rule
  (`font-size: 0.95rem; color: var(--text-muted)`) is reused as-is. The
  longer single line at small font does not need wrapping logic for the
  expected count/sum widths.

## Risks / non-risks

- **Heading rename**: only 1 backend / E2E call site for the i18n key
  (`negative_balance_list.rs`). No other consumers grep up.
- **Sum precision**: `f64` accumulation across small N (< 100) rows of
  cent-level magnitudes is well below f64 precision limits.
- **Empty list**: pre-existing early return `<span></span>` covers the
  empty case; summary code never runs with `rows.len() == 0`.
- **Mixed credits**: API only returns rows with `credit < 0`, so all
  summed values are negative — no zero/positive contamination of the
  total.
