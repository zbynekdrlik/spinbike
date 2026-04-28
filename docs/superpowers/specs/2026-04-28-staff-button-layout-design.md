# Staff Action Panel — Button Layout & Color Differentiation

**Date:** 2026-04-28
**Issue:** [#13](https://github.com/zbynekdrlik/spinbike/issues/13) — change order of topup/charge + log-visit buttons; differentiate colors

## Goal

Make the staff card-detail action panel match real-world frequency of use and remove the visual ambiguity caused by every action being the same green button.

## Current behavior

`spinbike-ui/src/pages/dashboard/action_form.rs` renders the staff action panel with:

- **Action row:** Topup (left) → Charge (right). Both `btn btn--primary` (solid green).
- **Log Visit row** (only when card has active monthly pass): Spinning + Fitness chips, both `btn btn--compact btn--primary`. Order is whatever order `services` returns from the API — no explicit sort. Currently Spinning then Fitness because that is the DB seed order, but it is not stable across re-seeds.

All four buttons look identical. Charge is the highest-frequency staff action; Topup is the rarest.

## Desired behavior

### 1. Action-row reorder + same-hue soft sibling for Topup

```
[ Charge €  ]  [ + Topup ]
  green solid   green soft tint
 (.btn--primary)  (.btn--primary-soft)
```

- Charge moves to the left (most-used action), keeps `.btn--primary` (solid green).
- Topup moves to the right and uses `.btn--primary-soft` — same green hue, low saturation. The pair reads primary / secondary within one color family.
- An earlier iteration tried `.btn--ghost` for Topup; the CEO rejected it on PR #25 v0.13.5 because the transparent treatment looked invisible against the page surface. The soft-tinted variant is the small difference asked for.

### 2. Log-Visit row reorder + same-hue soft sibling for Spinning

```
[ Visit Fitness ]   [ Visit Spinning ]
  blue solid          blue soft tint
  (.btn--info)        (.btn--info-soft)
```

- **Fitness on the left** (CEO defines Fitness as the more-used activity), keeps `.btn--info` (solid blue, eye-catching).
- **Spinning on the right** uses `.btn--info-soft` — same blue hue, low saturation. The two visit buttons stay in the blue family so staff see one row of "visits" with internal primary / secondary emphasis.
- An earlier iteration paired Fitness `.btn--info` with Spinning `.btn--pass` (yellow-green); the CEO rejected the radical color contrast and confirmed Spinning should be the LESS eye-catching of the pair.

The chip-row keeps its `.chip-row--spaced` layout. Buttons keep `.btn--compact` for the smaller chip size.

### Design rule (applies to both rows)

Within a paired action row, the more-used button on the left uses the solid color and the less-used button on the right uses its same-hue soft sibling. Different ROWS use different hues (action = green, visits = blue) so staff can tell at a glance which kind of action they're looking at, while WITHIN a row the difference is small (saturation only).

### 3. CSS — three new modifiers

`spinbike-ui/style.css` already defines the `--info` / `--info-fg` / `--info-hover` color tokens AND the `--success-soft` / `--success-border` / `--success-fg` / `--info-soft` / `--info-border` / `--info-soft-fg` soft-tint tokens (used today by `.alert-success` and `.alert-info`). Three new button modifiers wire those tokens up:

```css
.btn--info {
    background: var(--info);
    border-color: var(--info);
    color: var(--info-fg);
    font-weight: 600;
}
.btn--info:hover:not(:disabled) {
    background: var(--info-hover);
    border-color: var(--info-hover);
}

.btn--primary-soft {
    background: var(--success-soft);
    border-color: var(--success-border);
    color: var(--success-fg);
    font-weight: 600;
}
.btn--primary-soft:hover:not(:disabled) {
    background: color-mix(in srgb, var(--brand) 28%, var(--surface));
    border-color: color-mix(in srgb, var(--brand) 55%, var(--surface));
}

.btn--info-soft {
    background: var(--info-soft);
    border-color: var(--info-border);
    color: var(--info-soft-fg);
    font-weight: 600;
}
.btn--info-soft:hover:not(:disabled) {
    background: color-mix(in srgb, var(--info) 26%, var(--surface));
    border-color: color-mix(in srgb, var(--info) 60%, var(--surface));
}
```

Place them in the "Colour variants" block after `.btn--ghost`.

## Files affected

| File | Change |
|---|---|
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Swap action-row JSX; sort visit row by `name_en`; conditional class per visit name |
| `spinbike-ui/style.css` | Add `.btn--info`, `.btn--primary-soft`, `.btn--info-soft` + hovers (≈30 lines after `.btn--ghost`) |
| `e2e/tests/dashboard-button-layout.spec.ts` | NEW — assert order + classes for all 4 buttons |
| `VERSION` | Bump (post-merge of PR #25, see "Versioning" below) |

No backend changes. No DB changes. No new dependencies.

## Implementation details

### Action-row JSX swap

In the existing `<div class="action-row">` block (`action_form.rs:331-354`):

- Place the **Charge** `<button>` first.
- Place the **Topup** `<button>` second, with `class="btn btn--primary-soft"` (was `btn btn--primary`).
- Keep all `data-testid` attributes unchanged: `topup-submit`, `charge-submit`. Do **not** rename them — every existing E2E test depends on these IDs.
- Keep `disabled=move || loading.get()` on both.

### Visit-row sort + per-visit class

Currently (`action_form.rs:255-269`):

```rust
{services.get().into_iter()
    .filter(|svc| svc.is_class_visit())
    .map(|svc| { ... view! { <button class="btn btn--compact btn--primary" ... /> } }).collect()}
```

Replace with:

```rust
{
    let mut visits: Vec<_> = services.get().into_iter()
        .filter(|svc| svc.is_class_visit())
        .collect();
    // Stable order: Fitness left, Spinning right. is_class_visit() guarantees
    // name_en is one of "Fitness" | "Spinning", so a plain alphabetical sort
    // works (Fitness < Spinning).
    visits.sort_by(|a, b| a.name_en.cmp(&b.name_en));
    visits.into_iter().map(|svc| {
        let service_id = svc.id;
        let svc_name = svc.display_name(lang.get_untracked()).to_string();
        let color_cls = if svc.name_en == "Fitness" { "btn--info" } else { "btn--info-soft" };
        view! {
            <button
                class=format!("btn btn--compact {color_cls}")
                data-testid="log-visit-btn"
                on:click=visit_click_for(service_id)
            >
                {move || i18n::t(lang.get(), "log_visit")}" "{svc_name.clone()}
            </button>
        }
    }).collect::<Vec<_>>()
}
```

The `is_class_visit()` invariant (`mod.rs:103-105` matches only "Spinning" | "Fitness") makes the binary `if name_en == "Fitness"` branch exhaustive in practice. The doc comment on `is_class_visit` already warns that renaming Spinning/Fitness in admin will silently break this filter — same caveat applies here.

### New E2E test (`e2e/tests/dashboard-button-layout.spec.ts`)

Skeleton:

1. Standard test setup: login as staff, search for a card with active monthly pass, open card detail.
2. Assert action-row DOM order: `charge-submit` precedes `topup-submit` (use `evaluate` to inspect `compareDocumentPosition`, OR rely on the `.action-row > button:nth-child(1)` having `data-testid="charge-submit"`).
3. Assert visit-row DOM order: first `[data-testid="log-visit-btn"]` text contains "Fitness", second contains "Spinning". Test must be locale-aware — assert against the SK label OR set `lang=en`. Use `lang=en` and assert `Visit Fitness`, `Visit Spinning` literal text.
4. Assert classes:
   - `charge-submit` has `btn--primary` (and not `btn--primary-soft`)
   - `topup-submit` has `btn--primary-soft` (and not `btn--ghost`)
   - The Fitness visit button has `btn--info` (and not `btn--info-soft`)
   - The Spinning visit button has `btn--info-soft` (and not `btn--pass`)
5. Standard zero-console-errors assertion at end (per `browser-console-zero-errors.md`).

This is a new feature, so per `e2e-real-user-testing.md`, it requires its own dedicated Playwright test file — `dashboard-button-layout.spec.ts` — committed in the same PR.

## Compatibility with existing E2E

All existing tests use `[data-testid="..."]` selectors which we preserve. Three tests use `.first()` on `log-visit-btn`:

- `monthly-pass.spec.ts:58` — clicks the first visit button to log a visit; doesn't care which.
- `credit-improvements.spec.ts:49` — visibility check; doesn't care which.
- `log-visit-class-only.spec.ts:64` — checks count = 2; order-independent.

The only test that filters `btn--primary` is `schedule.spec.ts:195` on the schedule page (BOOK button) — unaffected by this change.

`monthly-pass-expired.spec.ts:28` asserts `toHaveCount(0)` for `log-visit-btn` when the pass is expired — independent of visit-row internals.

No existing tests need editing.

## Versioning

- This work bundles into PR #25 (open, dev → main).
- v0.13.5 was the first cut (Topup → ghost; Spinning → pass yellow-green). The CEO rejected ghost (invisible) and the yellow-green Spinning (too eye-catching, contradicting "Fitness more used").
- v0.13.6 is the corrective revision — same-hue soft siblings for both Topup and Spinning.
- PR #25 title and body remain accurate after the bump (the scope label moves from v0.13.5 to v0.13.6).

## Out of scope

- Other action surfaces (transactions list, admin panels, reports buttons, search results, modal dialogs) — only the staff card-detail action panel.
- I18n string changes — labels remain `log_visit` + service name as today.
- Touch / mobile-specific tweaks beyond what `.btn--compact` already provides.
- Backend changes — none.
- Color palette changes — adds `.btn--info`, `.btn--primary-soft`, `.btn--info-soft` modifiers; reuses existing tokens.

## Acceptance criteria

- [ ] Charge button is to the left of Topup in the action-row.
- [ ] Topup uses `.btn--primary-soft`, Charge uses `.btn--primary`.
- [ ] Visit-row shows Fitness button on the left of Spinning button.
- [ ] Fitness visit button uses `.btn--info`, Spinning visit button uses `.btn--info-soft`.
- [ ] All 4 buttons remain clickable, keyboard-focusable, and disabled-state respects `loading` signal.
- [ ] New Playwright test `dashboard-button-layout.spec.ts` is committed and asserts all four positions + class names + zero console errors.
- [ ] All existing E2E tests still pass without modification.
- [ ] CI green on the PR (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing, Smoke (dev) after deploy).
- [ ] Post-deploy verification on the dev frontend reads the new layout from the DOM via Playwright.
