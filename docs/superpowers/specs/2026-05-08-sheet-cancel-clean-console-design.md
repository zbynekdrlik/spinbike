# Sheet Cancel — Clean Console Design

**Issue:** [#84](https://github.com/zbynekdrlik/spinbike/issues/84) — Leptos closure-after-drop console errors on sheet Cancel button.

## Goal

Clicking Cancel on `DeleteUserSheet`, `EditTxDateSheet`, or `EditPassDateSheet` closes the sheet without emitting `closure invoked recursively or after being dropped` (or any other) console error. The fix must be detectable by `assertCleanConsole` in CI, which today filters too broadly and masks the bug.

## Root cause

All three sheets share the pattern:

```rust
view! {
    {move || {
        if !show.get() { return ().into_any(); }
        let (err, set_err) = signal(String::new());
        let on_cancel = move |_| {
            set_err.set(String::new());   // (1) synchronous reactive run on err subscriber
            show.set(false);              // (2) outer move|| re-runs, returns (), drops err subscriber
        };
        view! { <Sheet ...> ... </Sheet> }.into_any()
    }}
}
```

When the user clicks Cancel:

1. `set_err.set(String::new())` triggers a synchronous reactive run on the `move || { let e = err.get(); ... }` subscriber that renders the error alert.
2. `show.set(false)` re-runs the outer reactive closure, which returns `()` and drops the entire view subtree — including the err subscriber that just ran.
3. Leptos detects the subscriber was reachable mid-event but is now dropped → emits `closure invoked recursively or after being dropped`.

The Confirm path does not currently emit the error because `show.set(false)` runs inside a `spawn_local` async block, decoupling the timing.

## Fix

Three changes in one PR.

### 1. Drop redundant `set_err.set(String::new())` from `on_cancel`

In each of the three sheet files, simplify `on_cancel` to a single statement:

```rust
let on_cancel = move |_| {
    show.set(false);
};
```

Files:

- `spinbike-ui/src/pages/dashboard/sheets/delete_user.rs`
- `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs`
- `spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs`

The `set_err` write was dead code — every open of a sheet creates a fresh `(err, set_err)` pair via the per-mount state pattern, so err resets to empty automatically on next mount.

### 2. Tighten the `wasm` substring filter in `setupConsoleCheck`

In `e2e/tests/helpers.ts`, delete the broad clause:

```typescript
text.includes('wasm') ||
```

The two `using deprecated parameters` clauses (lines 25–26) already pin the only legitimate-noise messages we know about (wasm-bindgen 0.2.x deprecation warning that Trunk emits at bootstrap). Removing the broad `wasm` substring match means errors whose stack traces mention `wasm-function[NNNN]` (such as the closure-after-drop runtime error from #84) will no longer be silently swallowed.

If a different benign wasm message reappears later, add a *specific* substring for it next to the existing two — never re-introduce the broad `wasm` match.

### 3. Add Cancel-branch tests in three existing specs

Each test: navigate to the surface that mounts the sheet → open the sheet → click its Cancel button → assert sheet element no longer visible → `assertCleanConsole(msgs)`.

- `e2e/tests/users-by-movement.spec.ts` — DeleteUserSheet (open via Reports → Users tab → row → Delete user button → Cancel).
- `e2e/tests/edit-tx-date.spec.ts` — EditTxDateSheet (open via card panel → transaction row → date → Cancel).
- `e2e/tests/redesign-sheets.spec.ts` — already covers EditPassDateSheet Cancel at lines 72–75; verify the test still passes after the filter tightening (this proves the regression test now catches the bug class).

## Acceptance

- Click Cancel on each of the three sheets → zero console messages collected by `setupConsoleCheck`.
- All three new/updated E2E tests green on CI.
- Existing tests remain green; if the tightened filter surfaces a latent console error in any other spec, it is fixed inline within this PR (silencing via re-broadening the filter is banned per `browser-console-zero-errors.md`).

## Risk: latent errors surfaced by tightened filter

Narrowing the filter may reveal previously hidden `console.error` / `console.warn` events in other specs. Mitigation:

- First CI run after the filter change is the discovery moment.
- Each surfaced error is triaged: real bug → fix in this PR; genuinely external/transient → add a *specific* allow-list substring (not a broad `text.includes('wasm')` reintroduction).
- If the surfaced volume is large enough that #84 scope grows beyond reason, decompose: ship the sheet-fix and filter-narrowing as PR A, file individual issues for surfaced bug classes as PR-B…N.

## Out of scope

- Confirm-success path (`show.set(false)` then `set_saving.set(false)` inside `spawn_local`) — same antipattern, no current symptoms. Filed as follow-up only if surfaced by the tightened filter.
- Backdrop-click close path (Sheet's `on_close` callback `show.set(false)`) — same antipattern, no current symptoms. Same treatment as above.
- Generalising the sheet's mount/unmount pattern (e.g. switching to Leptos `<Show>` component) — premature; current pattern works after these three deltas.

## Workflow

- First commit: bump VERSION 0.13.28 → 0.13.29 + run `bash scripts/sync-version.sh`.
- Branch `dev`. PR from `dev` to `main` when CI green. Never merge — wait for explicit user instruction.
- Expected commit count: ~5 (version bump, sheet `on_cancel` fixes, helpers filter narrowing, E2E additions, any inline filter triage).

## Files touched

| File | Change |
|---|---|
| `VERSION` | 0.13.28 → 0.13.29 |
| `Cargo.toml`, `spinbike-ui/Cargo.toml` | version sync via `scripts/sync-version.sh` |
| `spinbike-ui/src/pages/dashboard/sheets/delete_user.rs` | drop `set_err.set` from `on_cancel` |
| `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs` | drop `set_err.set` from `on_cancel` |
| `spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs` | drop `set_err.set` from `on_cancel` |
| `e2e/tests/helpers.ts` | remove broad `wasm` substring filter |
| `e2e/tests/users-by-movement.spec.ts` | add DeleteUserSheet Cancel test |
| `e2e/tests/edit-tx-date.spec.ts` | add EditTxDateSheet Cancel test |
| `e2e/tests/redesign-sheets.spec.ts` | verify existing Cancel test still green |
