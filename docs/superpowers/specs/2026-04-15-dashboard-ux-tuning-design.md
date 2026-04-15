# Dashboard UX Tuning — Design

Date: 2026-04-15

Small round of tuning on the staff card dashboard after first real use.

## Goals

1. Make the card search keyboard-driven (no mouse needed for the common case).
2. Route staff/admin users straight to the card dashboard instead of the class schedule.
3. Surface negative credit visually — it must demand attention.
4. Fix the date format for legacy-imported transactions.

## 1. Keyboard navigation in search

**State:** add `highlighted_idx: i32` signal next to `query`, `results`. Initial value
0 (so the first suggestion is pre-selected as soon as results arrive).

**Reset rules:** any change to `query` or `results` resets `highlighted_idx` to 0.

**Render:** each row in the dropdown receives a conditional class when its index
equals `highlighted_idx` — a soft background tint (reuse existing `--surface-2` or
similar from `style.css`). Mouse hover sets `highlighted_idx` to that row's index
so keyboard + mouse states stay coherent.

**Input handlers** on the `<input type="search">`:
- `ArrowDown` → `(idx + 1) % len` if `len > 0`; prevent default.
- `ArrowUp` → `(idx + len - 1) % len`; prevent default.
- `Enter` → if `len > 0`, run the same selection logic as the click handler
  (set_selected, clear query/results); prevent default so no form submit.
- `Escape` → clear query and results; blur is NOT forced (keeps focus in the box).

Screen-reader hint: the input gets `aria-activedescendant` pointing at the
highlighted row's id — optional polish, skip if it complicates the code.

## 2. Staff default landing

**`SchedulePage` guard:** on component mount, call `auth::get_user()`. If the role
is `staff` or `admin`, call `leptos_router::use_navigate()("/staff", Default::default())`.
Customers and anonymous visitors continue to see the schedule.

**Login redirect:** `login.rs::navigate_home()` already calls `set_href("/")`. Change
it to route by role — staff/admin → `/staff`, else `/`. Reuse the user object just
saved to localStorage.

Why both: the mount-time guard handles bookmarks and deep links; the login change
avoids the extra redirect hop on fresh login.

## 3. Red negative credit

- Selected-card action panel: conditionally apply a `.credit-negative` class on the
  big balance readout when `credit < 0`. CSS rule: `color: var(--danger); font-weight: 700;`.
- Search-result rows: same class on the per-row credit when negative. Tint alone
  is enough here — no need to animate or add an icon.

Edge case: credit of exactly 0.0 is NOT red. The card is usable; only negative
balance means the card has overdrafted and needs a top-up.

## 4. Date format for legacy transactions

Current `format_sk_datetime` only matches two forms. The legacy migration imported
transactions in MS Access's `MM/dd/yy` style, which falls through to the raw
string (what the user sees on-screen as "English format").

**Extend the parser with these formats, tried in order:**
1. `%Y-%m-%d %H:%M:%S` — current SQLite `datetime('now')` output.
2. `%Y-%m-%dT%H:%M:%S` — ISO 8601.
3. `%Y-%m-%d %H:%M:%S%.f` — SQLite with fractional seconds.
4. `%m/%d/%y %H:%M:%S` — legacy MS Access 2-digit year.
5. `%m/%d/%Y %H:%M:%S` — legacy MS Access 4-digit year.

All successful parses format to `dd.MM.yyyy HH:mm`. Failure path returns the raw
string (preserves the current "show something" behavior).

## Testing

**Unit (`dashboard.rs`):**
- `format_sk_datetime` snapshot tests for each accepted format + the fallback.

**E2E (`e2e/tests/dashboard.spec.ts`):**
- Existing suite unchanged; add a "keyboard nav" test: type a partial name,
  press `Enter` without clicking, assert the action panel opens for the
  first result.
- Add a "negative credit" test: seed a card with −10 EUR in `global-setup.ts`,
  verify the search-result row has class `credit-negative` and the action
  panel's balance is red (computed style).

**Manual verification via Playwright** after the auto-deploy completes.

## Out of scope

- Full Slovak localization of the rest of the UI (schedule page etc.) — separate
  follow-up if needed.
- Accessibility polish beyond `aria-activedescendant`.
- Adjusting legacy data in place (keep stored format; format on read).
