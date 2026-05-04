# Last-Visit Display + Quick Search Sort — Design

**Status:** approved 2026-05-04, ready for implementation plan
**Issue:** [#57](https://github.com/zbynekdrlik/spinbike/issues/57)
**Scope:** one PR, `dev` → `main`, version `0.13.19`

## Goal

CEO meeting follow-up. Two related staff-UX items:

1. On the **open card page**, show "Posledná návšteva: 28.04.2026 (pred 6 dňami)" below the name + barcode. Hidden entirely when the card has no qualifying visit.
2. In **Quick Search** (the staff card-lookup search box on the dashboard), results from the same query are ordered newest-visit-first. Active customers float to the top.

## Visit definition

Reuse the same definition the v0.13.18 Overview tab uses: any non-deleted transaction whose service is **Spinning** or **Fitness**, identified by the `CLASS_VISIT_NAMES_EN` constant in `spinbike-core::services`.

This covers two real-world cases:

- **Per-visit charges** — `INSERT INTO transactions ... action='charge', service_id=Spinning|Fitness, amount<0` from `/api/payments/charge`.
- **Pass-holder visit logs** — `INSERT INTO transactions ... action='visit_pass', service_id=Spinning|Fitness, amount=0` from `/api/payments/log-visit`.

Soft-deleted rows (`deleted_at IS NOT NULL`) are excluded everywhere.

Top-ups, refreshments, supplements, monthly-pass purchases, card activation fees do NOT count as a "last visit".

## Backend

### Surface

- New field on `CardResponse` (`crates/spinbike-server/src/routes/cards.rs:89`): `pub last_visit_at: Option<String>` — same shape as the existing `created_at: String` on `TransactionResponse`, no chrono serde feature required, round-trips cleanly to the UI.
- Both `/api/cards` (list) and `/api/cards/search` populate it.

### SQL change

Both `db::list_all_cards_with_pass` and `db::search_cards_with_pass` (`crates/spinbike-server/src/db/cards.rs:207, 230`) gain one additional correlated subquery next to the existing pass subquery:

```sql
(SELECT MAX(t.created_at) FROM transactions t
 INNER JOIN services s ON s.id = t.service_id
 WHERE t.card_id = c.id
   AND t.deleted_at IS NULL
   AND s.name_en IN ('Spinning','Fitness')) AS last_visit_at
```

The handler binds placeholders from `CLASS_VISIT_NAMES_EN` (matching the pattern `card_stats` already established in `cards.rs`). The literal names above are illustrative; the actual SQL string uses `IN (?, ?)` with values bound from the constant so a future addition to `CLASS_VISIT_NAMES_EN` flows through automatically.

### Sort change

`search_cards_with_pass`'s `ORDER BY` clause changes from:

```
ORDER BY
  CASE WHEN c.barcode LIKE ? THEN 0 ELSE 1 END,
  c.last_name IS NULL, c.last_name ASC,
  c.first_name IS NULL, c.first_name ASC,
  c.barcode ASC
```

to:

```
ORDER BY
  CASE WHEN c.barcode LIKE ? THEN 0 ELSE 1 END,    -- barcode-prefix match still wins (unchanged)
  last_visit_at IS NULL,                             -- visited cards before never-visited (NULLS LAST)
  last_visit_at DESC,                                -- newest visit first
  c.last_name IS NULL, c.last_name ASC,             -- alphabetic tiebreak (unchanged)
  c.first_name IS NULL, c.first_name ASC,
  c.barcode ASC
```

Note `last_visit_at` references the alias from the SELECT — SQLite supports this in ORDER BY.

`list_all_cards_with_pass` keeps its existing `ORDER BY c.barcode` — `/api/cards` is for full lists/exports, not search; changing its order is out of scope.

### No new index

The existing `idx_transactions_card_id` (and any index on `card_id`) is sufficient: the subquery filters by `card_id = c.id` first. With at most 50 results per search and a typical < 100k transactions table, the per-card MAX is fast. Plan should NOT add a new index unless `EXPLAIN QUERY PLAN` shows a problem.

### Auth

Reuses `claims.role.can_manage_cards()`, same as existing `search_cards`. No auth change.

## Frontend

### Relative-time helper

New module `spinbike-ui/src/relative_date.rs`. Public function:

```rust
pub fn format_last_visit(visited: chrono::NaiveDate, today: chrono::NaiveDate, lang: Lang) -> String
```

Returns the combined string `"28.04.2026 (pred 6 dňami)"` (Slovak) or `"28.04.2026 (6 days ago)"` (English).

Granularity (Slovak). Bucket = floor of days/unit. Boundaries are exact and exclusive on the upper side of the previous bucket:

| Days ago        | Bucket | N value           | Output                                                  |
|-----------------|--------|-------------------|---------------------------------------------------------|
| 0               | today  | —                 | "dnes"                                                  |
| 1               | yesterday | —              | "včera"                                                 |
| 2 ≤ d ≤ 7       | days   | d                 | "pred N dňami"                                          |
| 8 ≤ d ≤ 60      | weeks  | floor(d/7)        | N=1 → "pred 1 týždňom"; N≥2 → "pred N týždňami"         |
| 61 ≤ d ≤ 364    | months | floor(d/30)       | N=1 → "pred 1 mesiacom"; N≥2 → "pred N mesiacmi"        |
| d ≥ 365         | years  | floor(d/365)      | N=1 → "pred 1 rokom"; N≥2 → "pred N rokmi"              |

Two i18n forms per unit are sufficient — Slovak instrumental plural collapses 2-4 and 5+ for these specific words (dňami / týždňami / mesiacmi / rokmi):

- N = 1 → singular (`_one` key: dňom / týždňom / mesiacom / rokom)
- N ≥ 2 → plural (`_few` key: dňami / týždňami / mesiacmi / rokmi)

The combined output for non-special cases is `"<DD.MM.YYYY> (pred N <unit>)"`. For "today"/"yesterday" the bracketed form replaces the relative bit only: `"28.04.2026 (dnes)"` / `"27.04.2026 (včera)"` — keeps the date visible everywhere for staff cross-reference with transactions.

Granularity (English): `"today"`, `"yesterday"`, `"N days ago"`, `"N weeks ago"`, `"N months ago"`, `"N years ago"` (no plural complexity).

### CardInfo extension

`spinbike-ui/src/pages/dashboard/mod.rs:48` `CardInfo` gains:

```rust
#[serde(default)]
pub last_visit_at: Option<String>,
```

`Option<String>` matches the existing pattern (`created_at` is also `String` on the UI side).

### Card panel header

`spinbike-ui/src/pages/dashboard/card_panel.rs:51` — extend the `<div class="card-header__main">` block:

```rust
<div class="card-title">
    <span class="card-title__name">{name}</span>
    " "
    <code class="card-title__barcode">{barcode.clone()}</code>
</div>
{move || {
    match parse_last_visit(&card.last_visit_at) {
        Some(date) => view! {
            <div class="card-title__last-visit" data-testid="card-last-visit">
                {i18n::t(lang.get(), "last_visit_label")} ": " {format_last_visit(date, today, lang.get())}
            </div>
        }.into_any(),
        None => view! { <span></span> }.into_any(),
    }
}}
```

Empty/None → no DOM at all (so the Playwright "absent" assertion works cleanly).

### i18n keys (additions)

- `last_visit_label` — "Posledná návšteva" / "Last visit"
- `rel_today` — "dnes" / "today"
- `rel_yesterday` — "včera" / "yesterday"
- `rel_days_one` — "pred 1 dňom" / "1 day ago"
- `rel_days_few` — "pred {n} dňami" / "{n} days ago"
- `rel_weeks_one` — "pred 1 týždňom" / "1 week ago"
- `rel_weeks_few` — "pred {n} týždňami" / "{n} weeks ago"
- `rel_months_one` — "pred 1 mesiacom" / "1 month ago"
- `rel_months_few` — "pred {n} mesiacmi" / "{n} months ago"
- `rel_years_one` — "pred 1 rokom" / "1 year ago"
- `rel_years_few` — "pred {n} rokmi" / "{n} years ago"

The `_one` / `_few` split keeps Slovak grammar in i18n (not hardcoded) so future tweaks don't need a code change. The `{n}` placeholder is replaced by the relative-time helper.

### CSS

Append to `spinbike-ui/style.css`:

```css
.card-title__last-visit {
    font-size: 0.875rem;
    color: var(--text-muted, #6c757d);
    margin-top: 0.125rem;
}
```

Small, subdued. The CSS variable falls back if `--text-muted` doesn't exist.

## Tests

### Server integration — `tests/cards_last_visit.rs` (new)

Seed scenarios on a fresh test pool:

| Card | Transactions seeded | Expected `last_visit_at` |
|------|---------------------|--------------------------|
| A    | Spinning charge yesterday                                | yesterday |
| B    | Spinning charge 5 days ago                              | 5 days ago |
| C    | Refreshments charge today                               | None      |
| D    | (none)                                                  | None      |
| E    | Spinning charge today, then UPDATE deleted_at=now()     | None      |
| F    | Zero-amount Spinning visit-log row (action='visit_pass', amount=0) | today |
| G    | Fitness charge 30 days ago, plus Spinning charge 10 days ago | 10 days ago |

Tests:

1. `last_visit_at_populated_correctly` — GET /api/cards/search?q=…, assert each card's `last_visit_at` matches the table.
2. `search_results_sort_by_last_visit_desc` — search returns F (today) before A (yesterday) before B (5d) before G (10d) before C/D/E (NULL).
3. `barcode_prefix_match_overrides_last_visit_sort` — seed a card with old visit but a barcode prefix that matches the query; assert it appears first regardless of `last_visit_at`.
4. `customer_role_forbidden` — GET search with customer token → 403 (regression check, not new behavior).

These seven scenarios kill the highest-risk mutants:

- "drop the soft-delete filter" (E becomes today instead of None)
- "drop the IN filter for Spinning/Fitness" (C becomes today instead of None)
- "DESC → ASC" (sort reverses)
- "IS NULL → IS NOT NULL" or NULLS-FIRST flip (NULL cards float to top)
- "MAX → MIN" (G shows 30d ago, not 10d ago)

### UI unit — inside `spinbike-ui/src/relative_date.rs` (`#[wasm_bindgen_test]`)

Lock every boundary:

| Days | SK output                  | EN output       |
|------|---------------------------|-----------------|
| 0    | "dnes"                    | "today"         |
| 1    | "včera"                   | "yesterday"     |
| 2    | "pred 2 dňami"            | "2 days ago"    |
| 4    | "pred 4 dňami"            | "4 days ago"    |
| 5    | "pred 5 dňami"            | "5 days ago"    |
| 7    | "pred 7 dňami"            | "7 days ago"    |
| 8    | "pred 1 týždňom"          | "1 week ago"    |
| 14   | "pred 2 týždňami"         | "2 weeks ago"   |
| 60   | "pred 8 týždňami"         | "8 weeks ago"   |
| 61   | "pred 2 mesiacmi"         | "2 months ago"  |
| 364  | "pred 12 mesiacmi"        | "12 months ago" |
| 365  | "pred 1 rokom"            | "1 year ago"    |
| 730  | "pred 2 rokmi"            | "2 years ago"   |
| 1825 | "pred 5 rokmi"            | "5 years ago"   |

Plus a `format_last_visit` test that asserts the combined string:

- 6 days ago, Slovak, date `2026-04-28` → `"28.04.2026 (pred 6 dňami)"`
- today, Slovak, date `2026-05-04` → `"04.05.2026 (dnes)"`

Boundary tests kill `<8 → <=8` and `<60 → <=60` mutants and `>=` flips.

### E2E — `e2e/tests/last-visit-display.spec.ts` (new)

Seed via `/api/test/seed-transactions` (already used by other E2E):

- AlphaTestCard57: Spinning charge 1 day ago
- ZuluTestCard57: Spinning charge 100 days ago
- NeverTestCard57: card exists, no Spinning/Fitness ever (top-up only)

Steps:

1. `loginViaAPI` as staff, `setupConsoleCheck`.
2. Navigate to staff dashboard, type "TestCard57" in Quick Search.
3. Wait for results. Assert AlphaTestCard57 is the first result, ZuluTestCard57 the second, NeverTestCard57 the third (or last).
4. Click AlphaTestCard57 → assert `[data-testid="card-last-visit"]` is visible and contains "(pred 1 dňom)".
5. Close, click ZuluTestCard57 → assert `[data-testid="card-last-visit"]` contains "(pred 3 mesiacmi)".
6. Close, click NeverTestCard57 → assert `[data-testid="card-last-visit"]` is NOT in DOM (`expect(locator).toHaveCount(0)`).
7. `assertCleanConsole` — zero browser errors/warnings.

## Files touched

```
VERSION                                                    bump 0.13.18 → 0.13.19
crates/spinbike-server/src/routes/cards.rs                 CardResponse.last_visit_at + helper
crates/spinbike-server/src/db/cards.rs                     two SQL queries + CardRowWithPass type
crates/spinbike-server/tests/cards_last_visit.rs           NEW — 4 integration tests
spinbike-ui/src/relative_date.rs                           NEW — Slovak/English helper + 14 wasm tests
spinbike-ui/src/lib.rs                                     register relative_date module
spinbike-ui/src/pages/dashboard/mod.rs                     CardInfo.last_visit_at field
spinbike-ui/src/pages/dashboard/card_panel.rs              header line
spinbike-ui/src/i18n.rs                                    11 new keys
spinbike-ui/style.css                                      .card-title__last-visit rule
e2e/tests/last-visit-display.spec.ts                       NEW — Playwright E2E
```

## Out of scope

- Smaller "walkin" card search in `spinbike-ui/src/pages/staff_dashboard.rs:388`. Different surface, CEO referenced the main Quick Search. Sort order can be revisited there separately if needed.
- Issue [#50](https://github.com/zbynekdrlik/spinbike/issues/50) Slovak rename — separate concern, separate PR.
- Issue [#49](https://github.com/zbynekdrlik/spinbike/issues/49) negative-balance alert — already deduped from #57's first bullet; will be its own PR later.

## Workflow

Per project rules:

- VERSION bump 0.13.18 → 0.13.19 is the FIRST commit on `dev`.
- Work directly on `dev` (no worktree) per recent PR pattern.
- Subagent prompts must NOT instruct cargo build / test / clippy locally; CI is authoritative. Local check is `cargo fmt --all --check`.
- NEVER `git add -A` / `git add .` — explicit paths or `git add -u` only.
- One PR `dev` → `main`. Mutation Testing CI gate must reach 0 surviving mutants on the diff.
- Post-deploy verification: read `[data-testid="version"]` from prod after main CI deploys; open card 70701712 (lots of history) and confirm last-visit line renders correctly.
