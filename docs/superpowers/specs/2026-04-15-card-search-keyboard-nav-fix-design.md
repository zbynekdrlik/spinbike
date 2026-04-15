# Card Search Keyboard Navigation Fix

**Date:** 2026-04-15
**Status:** Approved
**File touched:** `spinbike-ui/src/pages/dashboard.rs`

## Problem

Staff card search on the dashboard advertises "type → first result auto-selected → Enter picks it, ArrowDown moves within the list". On the **first** search after page load this works. On the **second** (and subsequent) search in the same session:

- First result is not auto-highlighted.
- ArrowDown does nothing.
- Enter does nothing.

Staff then have to fall back to clicking, which defeats the purpose of the keyboard-first workflow.

## Root Cause

Two interacting defects in `spinbike-ui/src/pages/dashboard.rs`:

1. **Search input loses focus after `pick_card`.** The `<input>` relies on the `autofocus` HTML attribute, which only fires on initial mount. Once a card is picked, the `ActionPanel` renders and any click into it moves focus away. Keystrokes that look to the user like "typing in search" actually reach a different element (or nothing), so `on:keydown` on the search input never fires — ArrowDown and Enter become no-ops.

2. **`highlighted_idx` is reset to `0` only after the 250 ms debounced fetch resolves.** Between the query change and fetch completion, the old results may still be on screen with stale idx. If the mouse cursor happens to sit over a dropdown row when it re-renders, `on:mouseenter` overwrites `highlighted_idx` to that row's index, silently breaking the "first item is pre-selected" invariant for the next search.

## Fix

Three small, focused changes in `spinbike-ui/src/pages/dashboard.rs`:

### 1. Explicit focus control via `NodeRef`

Replace the `autofocus` attribute with a `NodeRef::<leptos::html::Input>` on the search field. Call `.focus()`:

- once on component mount (preserves the current autofocus behavior);
- inside `pick_card`, immediately after clearing `query` and `results`, so the staff member can keep typing without clicking;
- whenever `ActionPanel` closes (via the existing `on_close` callback).

### 2. Reset `highlighted_idx` eagerly

Move `set_highlighted_idx.set(0)` to the top of the debounced search `Effect`, fired on every `query` change — not only on successful fetch completion. This guarantees the "first suggestion is highlighted" invariant holds for every query, regardless of mouse position or in-flight fetches.

### 3. Remove `on:mouseenter` → `set_highlighted_idx`

Hover should not fight keyboard navigation. Delete the `on:mouseenter` handler on suggestion rows. Visual hover feedback remains via the existing CSS `:hover` style. Click-to-pick is unchanged.

## Testing

### Playwright E2E

New file: `e2e/tests/card-search-keyboard.spec.ts`

Single test, one logged-in staff session, exercises the full keyboard workflow:

1. Navigate to `/staff`.
2. Type a substring that matches at least two cards.
3. Assert the first `[data-testid=search-result]` row has class `search-result-active`.
4. Press `Enter`. Assert `ActionPanel` for that card is visible.
5. Close the action panel (existing close button).
6. Type a **different** substring that matches at least two cards.
7. Assert first row has `search-result-active` class **again** (this is the regression check).
8. Press `ArrowDown`. Assert second row now has `search-result-active`.
9. Press `Enter`. Assert `ActionPanel` for the correct (second) card is visible.
10. Assert zero console errors/warnings collected during the test.

Test fixtures: staff account already seeded by the existing test-data setup; two cards whose `search_text` share a common substring must exist (the legacy import provides plenty).

### No unit tests needed

The fix is purely UI wiring — focus management and reactive signal ordering. Behavior is only observable through the browser, so Playwright is the right layer. The existing dashboard unit tests for `format_sk_datetime` and card formatting are unaffected.

## Out of Scope

- No visual redesign of the dropdown.
- No new keyboard shortcuts (Tab, PgUp/PgDn, Home/End).
- No scroll-into-view logic — the result list is capped at 10 rows and fits on one screen.
- No changes to the search backend, debounce timing, or API.

## Rollout

Single commit on `dev`. Bump `VERSION`. Auto-deploys to spinbike.newlevel.media via the self-hosted runner on push. Post-deploy Playwright smoke suite confirms the fix against the live site before the PR to `main`.
