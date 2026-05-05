# Negative-Balance List on the Desk — Design

**Issue:** [#49](https://github.com/zbynekdrlik/spinbike/issues/49) — "we need find way to alert ceo about cards/users which has credit less then zero"

**Goal.** Surface cards with `credit < 0` so the CEO sees who owes money the moment he opens the desk, and so any negative-balance card is visually flagged when it appears in a regular search.

## Surfaces

Two coordinated UI surfaces sharing one definition of "negative":

1. **Idle desk** (`/staff`, no card selected, search box empty): a card titled *"Karty s dlhom" / "Cards with negative balance"* renders under the search box, listing every card with `credit < 0`. Hidden when the list is empty (clean ledger ⇒ no clutter). Hidden while a card is selected or while the search box has any text (the dropdown takes that space).

2. **Active search** (`/staff`, dropdown open): each search-result row that represents a card with `credit < 0` gets a `search-result--negative` modifier class. Visual: 3px red left border + subtle red-tinted background. The existing red colour on the credit number (`.credit-negative`) is unchanged.

Both surfaces use the same SQL truth: `credit < 0`. No threshold. Blocked cards included (still owe money).

## Idle-desk list — row content

Sorted most-negative-first (`ORDER BY credit ASC`). Four fields per row:

```
Anna Kovacova          -12.50 €    Posledna navsteva: 2026-04-22    Posledna platba: 2026-03-05
Stefan Horvath         -8.00 €     Posledna navsteva: 2026-04-30    Posledna platba: 2026-02-18
Maria Novakova         -3.20 €     Posledna navsteva: nikdy          Posledna platba: 2025-11-10
```

(Slovak labels shown unaccented per project convention; English labels read "Last visit" and "Last payment", with "never" for the missing case. Dates rendered using the existing `relative-time` Slovak/English helper added in PR #59.)

Click a row → same behaviour as the existing search-dropdown row: sets `selected` to that card, opens the action panel, refreshes the txn list.

## Backend

**New endpoint:** `GET /api/cards/negative-balance`

- Auth: `admin` or `staff` role (same gate as `/api/cards/search`).
- Returns: `Vec<NegativeBalanceCard>` where each row carries `{id, barcode, first_name, last_name, company, credit, last_visit_at, last_payment_at}` — the same identity fields surfaced today by `/api/cards/search` plus the two timestamp subqueries.
- SQL shape:

```sql
SELECT
    c.id, c.barcode, c.first_name, c.last_name, c.company, c.credit,
    (SELECT MAX(t.timestamp) FROM transactions t
        WHERE t.card_id = c.id AND t.action = 'visit') AS last_visit_at,
    (SELECT MAX(t.timestamp) FROM transactions t
        WHERE t.card_id = c.id AND t.action IN ('topup','correction')) AS last_payment_at
FROM cards c
WHERE c.credit < 0
ORDER BY c.credit ASC;
```

- Action vocabulary for "payment" matches the legacy-normalised set introduced in PR #51.
- No new index required — `cards.credit` filter against ~550 rows is negligible; the `transactions` subqueries already benefit from the existing `(card_id, timestamp)` index.

**No change to `/api/cards/search`.** Surface 2 reuses the `credit` field already on every dropdown row.

## Frontend

**New component:** `pages/dashboard/negative_balance_list.rs` — a Leptos component that:
- Fetches `/api/cards/negative-balance` on mount.
- Re-fetches when the existing `txn_refresh` signal increments (so logging a visit, charging a card, or topping up updates the list immediately if the affected card crosses zero).
- Renders nothing when the response is empty.
- Renders nothing when (a) `selected.is_some()` or (b) the current search query is non-empty (shape-checked at the parent in `mod.rs` rather than inside this component, so the component stays presentational).

**Wiring in `pages/dashboard/mod.rs`:**

```text
[ search box ]
[ search dropdown / no-match / loading hint ]
[ alert-error / alert-success ]
[ <NegativeBalanceList /> ]   ← shown only when selected.is_none() && query.is_empty()
[ <CardActionPanel /> for selected ]
```

**Search-dropdown highlight (`pages/dashboard/mod.rs`, existing render block at line ~382):**

The row `<div>` already takes a `move || ...` class for keyboard highlight. Extend that closure so it returns one of four classes:

- `search-result-row` — default
- `search-result-row search-result-active` — keyboard-highlighted
- `search-result-row search-result--negative` — credit < 0
- `search-result-row search-result-active search-result--negative` — both

A small helper `result_row_class(highlighted: bool, credit: f64) -> &'static str` returning the right combination keeps the `view!` block readable and gives the unit/wasm test a single function to mutate.

## i18n keys

Three new entries in `spinbike-ui/src/i18n.rs` (Slovak unaccented per project convention):

| key | Slovak | English |
|---|---|---|
| `negative_balance_heading` | `Karty s dlhom` | `Cards with negative balance` |
| `last_payment_label` | `Posledna platba` | `Last payment` |
| `never_label` | `nikdy` | `never` |

The "last visit" label reuses the existing `last_visit_label` key from PR #59.

## CSS

One new rule plus a modifier on the row:

```css
.search-result--negative {
    border-left: 3px solid var(--danger, #dc3545);
    background: rgba(220, 53, 69, 0.04);
}
```

Plus styles for the new list card (`.negative-balance-list` + nested rows) following the same `.card` / `.card__body` patterns used by the existing search-results card.

## Tests

**Server (`crates/spinbike-server/src/db/cards.rs` or new `db/reports.rs` block):**

- Unit test: insert 3 cards (`credit = 5.0`, `credit = -3.5`, `credit = -10.0`) plus mixed transactions; assert the query returns exactly the two negatives in order `-10.0` then `-3.5`, and that `last_visit_at` and `last_payment_at` reflect the seeded transactions correctly.
- Unit test: a card with `credit < 0` and zero transactions returns `last_visit_at = NULL` and `last_payment_at = NULL`.
- Route test: non-staff user gets 403; staff user gets 200 with the expected JSON shape.

**UI (`spinbike-ui/src/pages/dashboard/`):**

- `wasm-bindgen-test` for `result_row_class(highlighted, credit)`: covers all 4 branches (4 cases). Mutation pressure on `<` vs `<=` and on the `highlighted` boolean.
- `wasm-bindgen-test` for the date formatter handling `None` → returns the `never_label` translation, in both Slovak and English. (No `wasm_bindgen_test_configure!(run_in_browser)` — CI runs `wasm-pack test --node`.)

**Playwright E2E (`e2e/tests/negative-balance.spec.ts`, new file):**

- Seed 3 cards: `NB-POS` (credit `+5.00`), `NB-NEG-A` (credit `-3.50`, last visit yesterday, last topup last week), `NB-NEG-B` (credit `-10.00`, no visits, last topup a month ago).
- Log in as staff via API (`loginViaAPI`), navigate to `/staff`.
- Assert the negative-balance card is visible with heading "Cards with negative balance".
- Assert two rows in DOM order: `NB-NEG-B` first (-10.00), `NB-NEG-A` second (-3.50). Both rows show name, balance, last visit (or "never"), last payment.
- Assert `NB-POS` is not in the list.
- Click `NB-NEG-A` row → assert action panel opens for that card.
- Search for the shared prefix `"NB"` in the search box → assert dropdown shows all three; the two negative rows have the `search-result--negative` class, the positive row does not.
- Clear search → assert the negative-balance list reappears.
- `assertCleanConsole` — zero browser console errors / warnings throughout.

## Risks and mitigations

- **Stale list after txn:** mitigated by hooking into the existing `txn_refresh` signal — every visit / charge / topup already increments it, so the list re-fetches automatically.
- **Auth bypass via API URL guess:** mitigated by reusing the same auth gate as `/api/cards/search` and adding the explicit 403 route test.
- **Subquery cost grows with transactions:** ~550 cards × O(1) indexed subqueries each = sub-millisecond. Re-validate against the prod-synced dev DB during implementation per memory `feedback_validate_against_real_data.md`.
- **List visible to a customer who somehow lands on `/staff`:** the `/staff` route already short-circuits to `MyDashboard` for non-staff (see `router.rs:33`), so this surface is staff-only by construction.

## Out of scope

- Configurable low-credit threshold (issue #61 territory: warning bands).
- SMS / email outreach automation (#55 belongs there).
- Inactive-card report (#56 belongs there).
- Fixing `my_balance`'s unused `last_visit_at` subquery (#58, separate small PR).

## Versioning and workflow

- VERSION file currently `0.13.20` on both `main` and `dev` (PR #62 just merged). First commit on `dev` for this feature MUST bump to `0.13.21` per `version-bumping.md`.
- Branch `dev`. PR `dev → main`.
- Subagents run only `cargo fmt --all --check` locally; CI is authoritative for compile/test/lint per memory `feedback_subagent_no_local_build.md`.
- Stage with explicit paths or `git add -u` — never `git add -A` per memory `feedback_no_git_add_A.md`.
