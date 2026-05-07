# Negative-balance row — single-line layout

**Issue:** [#78](https://github.com/zbynekdrlik/spinbike/issues/78) — "user in minus wasting space on two lines - needs be in one"

**Date:** 2026-05-07

**Goal:** Collapse each row in the desk negative-balance list from two lines (name + meta-block) to one line: `{name} ({last_visit_label}: {last_visit})` followed by the credit on the right. Drop the per-row `last_payment` rendering entirely.

## Why

User reported: `RITKA mon.Podracka (posledna navsteva: vcera)` — wants name + last-visit on a single row. Current layout shows name on line 1 and "posledna navsteva: X · posledna platba: Y" on line 2, doubling row height and reducing how many negative-balance rows fit on the desk above the fold.

## Scope

**In scope:**
- Frontend: `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` row render.
- CSS: `spinbike-ui/style.css` — replace `.negative-balance-row__main` / `__name` / `__meta` with a single inline layout class.
- E2E: extend `e2e/tests/negative-balance.spec.ts` row assertions.

**Out of scope:**
- API/DB removal of `last_payment_at` field. Server still returns it; client deserializes but no longer renders. Cleanup is a follow-up issue if useful.
- Heading suffix `count + sum` (already shipped in PR #77).

## Files touched

| File | Change |
|---|---|
| `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` | Drop `last_payment` rendering; collapse two divs into one element with inline muted span; extract `meta_inline()` helper + 2 wasm-bindgen tests. |
| `spinbike-ui/style.css` | Drop `.negative-balance-row__main`, `__name`, `__meta`; add `.negative-balance-row__meta-inline` (smaller font, muted color, no wrap on parent). |
| `e2e/tests/negative-balance.spec.ts` | Add row-text assertions: contains `(posledna navsteva: ` AND does NOT contain `posledna platba`. |
| `VERSION` + Cargo.toml files | Bump 0.13.25 → 0.13.26 via `bash scripts/sync-version.sh`. |

## Render shape

Current (HTML):
```html
<div class="negative-balance-row">
  <div class="negative-balance-row__main">
    <div class="negative-balance-row__name">RITKA mon.Podracka</div>
    <div class="negative-balance-row__meta">posledna navsteva: vcera · posledna platba: 2 mesiace</div>
  </div>
  <div class="negative-balance-row__credit credit-negative">-3.50 €</div>
</div>
```

After:
```html
<div class="negative-balance-row">
  <div class="negative-balance-row__label">
    RITKA mon.Podracka<span class="negative-balance-row__meta-inline"> (posledna navsteva: vcera)</span>
  </div>
  <div class="negative-balance-row__credit credit-negative">-3.50 €</div>
</div>
```

`__label` is a flex/inline element with `min-width: 0`, `overflow: hidden`, `text-overflow: ellipsis`, `white-space: nowrap` so long names truncate gracefully on narrow screens. Credit stays right-aligned via the existing `.negative-balance-row` flex container.

## Helper extraction

Add a private helper at module scope:

```rust
/// Inline meta suffix appended after the user's name in a negative-balance row.
/// Format: " ({label}: {value})" — leading space, parens, colon. Caller passes
/// the localized label (e.g. "posledna navsteva") and pre-formatted value
/// (e.g. "vcera", "2 dni", "nikdy").
pub(super) fn meta_inline(label: &str, value: &str) -> String {
    format!(" ({label}: {value})")
}
```

Two `#[wasm_bindgen_test]` cases pin format string against mutation:

- `meta_inline_typical`: `meta_inline("posledna navsteva", "vcera")` → `" (posledna navsteva: vcera)"` (kills paren-drop, colon-drop, leading-space-drop mutants).
- `meta_inline_never_label`: `meta_inline("last visit", "never")` → `" (last visit: never)"` (kills label-swap mutants).

## i18n

No new strings. Existing keys unchanged:
- `last_visit_label` ("posledna navsteva" / "last visit") — still used.
- `last_payment_label` ("posledna platba" / "last payment") — no longer rendered in this list. Other call sites unaffected (search via grep first).

## Test coverage

**Wasm-bindgen unit tests (Test (UI) job):**
- `meta_inline_typical` → exact-string assertion.
- `meta_inline_never_label` → exact-string assertion.

**E2E (Playwright):** extend the existing test in `e2e/tests/negative-balance.spec.ts` after the heading-regex block. For Alpha row:

```typescript
const alphaRowFull = rows.filter({ hasText: `Alpha${RUN_TAG}` }).first();
const alphaText = (await alphaRowFull.textContent()) ?? '';
// Single-line layout: name + " (posledna navsteva: …)" + credit.
expect(alphaText).toContain('(posledna navsteva: ');
expect(alphaText).not.toContain('posledna platba');
```

Mutation pressure summary:
- `format!` string in `meta_inline` → killed by 2 wasm-bindgen tests.
- Removal of `last_payment` rendering → killed by E2E `not.toContain('posledna platba')`.
- CSS class rename from `__name`/`__meta` to `__label`/`__meta-inline` → not mutation-tested (CSS), but layout regression caught visually post-deploy.

## Verification

Post-deploy:
1. Open `https://spinbike.newlevel.media/staff` in Playwright.
2. Confirm DOM `[data-testid="version"]` = `v0.13.26`.
3. Read first `[data-testid="negative-balance-row"]` text — must match shape `^[^\(]+ \(posledna navsteva: [^)]+\)\s+-?\d+\.\d{2}\s*€$`.
4. Confirm 0 console errors.

## Risks

None significant — frontend-only change, no DB or API touched, prod-synced dev DB will exercise the layout against real prod-shape rows.
