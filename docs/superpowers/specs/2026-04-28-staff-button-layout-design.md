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

### 1. Action-row reorder + color differentiation

```
[ Charge €  ]  [ + Topup ]
  green solid   gray outline
 (primary)     (ghost)
```

- Charge moves to the left (most-used action).
- Topup stays a real button but loses its primary green and becomes a `.btn--ghost` (outlined / muted) so it visually recedes.
- Charge keeps `.btn--primary`.

### 2. Log-Visit row reorder + activity-coded colors

```
[ Visit Fitness ]   [ Visit Spinning ]
  blue solid          yellow-green solid
  (.btn--info)        (.btn--pass)
```

- **Fitness on the left**, Spinning on the right (CEO preference).
- Each visit type carries a distinct, stable color so staff can identify the activity at a glance:
  - **Fitness → `.btn--info`** (a new modifier — see CSS section below).
  - **Spinning → `.btn--pass`** (existing yellow-green token).

The chip-row keeps its `.chip-row--spaced` layout. Buttons keep `.btn--compact` for the smaller chip size.

### 3. CSS — add `.btn--info` modifier

`spinbike-ui/style.css` already defines the `--info`, `--info-fg`, and `--info-hover` color tokens (lines 14–18 of the palette block). Add a button modifier that uses them, mirroring the structure of `.btn--primary`:

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
```

Place it in the "Colour variants" block after `.btn--ghost` (style.css:371).

## Files affected

| File | Change |
|---|---|
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Swap action-row JSX; sort visit row by `name_en`; conditional class per visit name |
| `spinbike-ui/style.css` | Add `.btn--info` + hover (≈10 lines after `.btn--ghost`) |
| `e2e/tests/dashboard-button-layout.spec.ts` | NEW — assert order + classes for all 4 buttons |
| `VERSION` | Bump (post-merge of PR #25, see "Versioning" below) |

No backend changes. No DB changes. No new dependencies.

## Implementation details

### Action-row JSX swap

In the existing `<div class="action-row">` block (`action_form.rs:331-354`):

- Place the **Charge** `<button>` first.
- Place the **Topup** `<button>` second, with `class="btn btn--ghost"` (was `btn btn--primary`).
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
        let color_cls = if svc.name_en == "Fitness" { "btn--info" } else { "btn--pass" };
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
   - `charge-submit` has `btn--primary`
   - `topup-submit` has `btn--ghost`
   - The Fitness visit button has `btn--info`
   - The Spinning visit button has `btn--pass`
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

- This work bundles into PR #25 (currently open, dev → main, mergeStateStatus CLEAN).
- PR #25's existing scope: CI cache + E2E diagnostics at v0.13.4. After bundling: CI infra + button-layout UX at v0.13.5.
- The first commit of this work bumps `VERSION` from 0.13.4 → **0.13.5** via `scripts/sync-version.sh`.
- PR #25's title and body update to reflect the combined scope.

## Out of scope

- Other action surfaces (transactions list, admin panels, reports buttons, search results, modal dialogs) — only the staff card-detail action panel.
- I18n string changes — labels remain `log_visit` + service name as today.
- Touch / mobile-specific tweaks beyond what `.btn--compact` already provides.
- Backend changes — none.
- Color palette changes — only adds `.btn--info` modifier; reuses existing tokens.

## Acceptance criteria

- [ ] Charge button is to the left of Topup in the action-row.
- [ ] Topup uses `.btn--ghost`, Charge uses `.btn--primary`.
- [ ] Visit-row shows Fitness button on the left of Spinning button.
- [ ] Fitness visit button uses `.btn--info`, Spinning visit button uses `.btn--pass`.
- [ ] All 4 buttons remain clickable, keyboard-focusable, and disabled-state respects `loading` signal.
- [ ] New Playwright test `dashboard-button-layout.spec.ts` is committed and asserts all four positions + class names + zero console errors.
- [ ] All existing E2E tests still pass without modification.
- [ ] CI green on the PR (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing, Smoke (dev) after deploy).
- [ ] Post-deploy verification on the dev frontend reads the new layout from the DOM via Playwright.
