# Fitness Preselect on Charge Form — Design

**Issue:** #33 — Re-add Fitness preselect to charge form (#29 follow-up)
**Date:** 2026-04-30
**Target version:** 0.13.11

## Goal

When staff opens a card on the desk, the service `<select>` should default to "Fitness" instead of an empty placeholder. Staff currently re-types "Fitness" on the majority of charges; this saves one click per fitness charge while keeping manual selection a single click away.

## Why the prior attempt failed

Commit `c533d7c` (reverted in `471a0c0`) used a reactive `Effect` plus `prop:value` binding on the `<select>` element. Mechanism: `prop:value` re-evaluated on every signal change → triggered `<select>` re-render → interfered with the parent's `set_selected.update` → `CardActionPanel` re-mounted with fresh child signals → `txn_refresh` reset → txn list rendered as "No transactions" after a successful charge. Six E2E tests across txn-note, desk-ux, and reports-attendance suites failed with this regression.

**Constraint enforced by this design:** no `prop:value`, no signal-driven option selection, no reactive structure changes on the existing `<select>`.

## Architecture

One additional `Effect` inside the existing `ActionForm` component. The Effect:

1. Tracks the `services` signal.
2. Skips if `selected_service_id.get_untracked()` is `Some` (manual staff selection wins).
3. Looks up Fitness: `services.get().iter().find(|s| s.name_en == FITNESS_NAME_EN && s.active).map(|s| s.id)`.
4. If found:
   - Calls `set_selected_service_id.set(Some(id))` to update the signal (so `do_charge` reads the right id).
   - Imperatively calls `service_ref.get().unwrap().set_value(&id.to_string())` so the DOM `<select>` shows "Fitness" as the selected option.
5. If not found: no-op. Dropdown stays at the empty placeholder (the missing-Fitness fallback chosen during brainstorming).

The empty `<option value="">{select_service}</option>` placeholder is **kept**. The brief originally proposed removing it ("so the UI cannot send null service_id"), but the brainstorming-resolved fallback for the missing-Fitness edge case requires an empty default state. Both intents are satisfied this way: in the normal case the Effect preselects Fitness via `service_ref.set_value()` so the placeholder is dormant and the UI never sends null; in the missing-Fitness edge case the placeholder is the default selection, staff is forced to pick, and the server-side null-service guard from #31 catches the rare slip-up if they hit Charge without picking.

### Why imperative `set_value()` works

`set_value()` is a one-shot DOM mutation, not a reactive binding. Leptos doesn't re-render the `<select>` in response. The signal stays in sync because we set it explicitly. No subscription on the `<select>` element means no parent re-render cascade.

### Behavior on card switch

When staff clicks a different card in the dashboard list, the parent's `set_selected.update` causes `CardActionPanel` to re-mount with fresh signals. `selected_service_id` resets to `None`. The Effect re-runs (services is already loaded by now), the `is_none()` guard passes, and Fitness gets preselected for the new card. This is the desired UX — most cards are getting a fitness charge.

### Behavior when staff manually picks Spinning

`on_service_change` sets `selected_service_id` to `Some(spinning_id)`. The Effect's `is_none()` guard fails on subsequent services changes (none expected during a desk session anyway, but defensive). Manual selection persists for that card.

### Behavior when Fitness is disabled or missing

The `find` returns `None`. Effect does nothing. Dropdown shows the empty placeholder as default. Staff picks manually. Server-side null-service guard from #31 (already shipped in PR #35) returns 400 if they hit Charge without a selection.

### Behavior during services-fetch race window

`services` is fetched async on dashboard mount via `/api/services`. Before it arrives, `services.get()` returns `[]`. The `<select>` renders with the empty placeholder visible. Staff can't open a useful dropdown yet — but on LAN this fetch completes in <100ms, well before staff clicks a card. When the services signal updates from `[]` to non-empty, the Effect fires (it tracks `services`), and Fitness gets selected.

## Components

**Modified:**

- `spinbike-ui/src/pages/dashboard/action_form.rs`
  - Add `use leptos::prelude::Effect`
  - Add `use spinbike_core::services::FITNESS_NAME_EN`
  - Add `Effect::new(move |_| { ... })` block before the `view!` macro
  - Empty `<option value="">{...}</option>` placeholder stays in the options list (no removal)

**New tests:**

- `e2e/tests/desk-ux.spec.ts` — 2 new test cases (see Testing section)

**Bumped:**

- `VERSION` 0.13.10 → 0.13.11
- `Cargo.toml` (workspace root) version 0.13.10 → 0.13.11
- `spinbike-ui/Cargo.toml` version 0.13.10 → 0.13.11

## Data flow

```
Page mount
  → AdaptiveDashboard fetches /api/services → services signal populated
  → User clicks card row
  → set_selected.update(Some(card)) → CardActionPanel renders ActionForm
  → ActionForm mount:
      - selected_service_id = None
      - service_ref = NodeRef
      - Effect registered (will fire on services tracking)
  → Effect first run:
      - services already loaded, non-empty
      - selected_service_id is None
      - find Fitness, set signal, set DOM value
      - <select> shows "Fitness"
  → Staff clicks Charge → do_charge reads selected_service_id (Some(fitness_id))
  → POST /api/payments/charge with valid service_id → 200
  → Txn list refreshes via set_txn_refresh
```

## Error handling

- Fitness not found / inactive → no preselect, staff picks manually. Acceptable degradation.
- `service_ref.get()` returns `None` (component unmounted between tracking call and DOM update) → skip the `set_value` call, signal still set. Next render of the form will reflect the signal value via the option's natural selection? No — without `prop:value` we can't bind that. So if `service_ref` is `None`, just skip and rely on next Effect run when component re-mounts.
- `id.to_string().parse::<i64>()` round-trip is safe — service ids are i64 and the form already handles them as strings via the existing `on_service_change`.

## Testing

### E2E (Playwright)

In `e2e/tests/desk-ux.spec.ts`, add 2 test cases:

1. **`'Fitness preselected when staff opens a card'`**
   - Login via API, navigate to /staff
   - Click a card row that has an active pass (use existing helper or seed)
   - Wait for `[data-testid="action-form"]` to be visible
   - Get `[data-testid="charge-service"]` element value
   - Look up Fitness service id via `/api/services` response
   - Assert the `<select>` value equals the Fitness id
   - Optional: assert the visible text in the selected option is "Fitness" (English) or "Fitness" (Slovak — same word)
   - `assertCleanConsole`

2. **`'Empty option is not the active selection when Fitness preselect succeeds'`**
   - Same setup
   - Get the `<select>` value via `[data-testid="charge-service"]`
   - Assert the value is non-empty (string parses to a positive integer)
   - The empty `<option value="">` placeholder still exists in the DOM as a missing-Fitness fallback, but is not the active selection in the normal case
   - `assertCleanConsole`

Both tests use the existing `setupConsoleCheck`, `assertCleanConsole`, `loginViaAPI` helpers.

### Regression coverage

The 6 tests that broke in `c533d7c` (txn-note posting, desk-ux charge flows, reports-attendance) become regression checks for this PR. If the imperative `set_value` approach also breaks the parent re-render cycle, those tests will fail again.

## What's deliberately NOT included (YAGNI)

- No "loading…" placeholder in the `<select>` during the services-fetch race window. Staff would have to be unrealistically fast to hit it on LAN.
- No fallback to "first active service" if Fitness is missing. Empty selection is the chosen behavior.
- No persistence of staff's last-used service. Solo operator wants Fitness as default; one click is enough.
- No quick-charge chip (#34 Spinning chip is a separate ticket).

## Out of scope

- Issue #34 (Spinning quick-charge chip) — same file but separate scope.
- Issue #28 (transactions.note CHECK constraint) — DB migration, unrelated.
- Issue #36 (cargo-mutants/Axum compatibility) — CI tooling, unrelated.

## Acceptance criteria

- [ ] On opening any card with `[data-testid="action-form"]` rendered, the `<select>` shows "Fitness" as selected by default.
- [ ] The empty `<option value="">` placeholder is not the active selection in the normal case (Fitness exists and is active).
- [ ] Manually selecting "Spinning" persists for that card session (does not flip back to Fitness).
- [ ] Switching to a different card preselects Fitness again (fresh card, fresh default).
- [ ] All existing E2E tests still pass (no txn-list regression).
- [ ] 2 new desk-ux E2E tests pass.
- [ ] Browser console clean (zero errors/warnings).
- [ ] Server-side null-service guard from #31 still rejects null service_id with 400 (verified by `payments_charge_validation::charge_rejects_null_service_id_with_400`).
