# Visit-button feedback design (issue #53)

**Issue:** https://github.com/zbynekdrlik/spinbike/issues/53 — "visits button has no reaction so CEO not know if it was pushed or not and if visits has been added or not"

**Goal:** Make the Fitness/Spinning "Log visit" buttons on a card with an active monthly pass produce clear per-press feedback: a loading-disabled state during the in-flight POST, a success message in the existing banner, and auto-clearing of the message so consecutive identical presses each show fresh feedback.

**Constraint:** mirror the existing charge-button pattern. No new toast component, no flash animation. Reuse the `msg` / `err` infrastructure already in `dashboard/mod.rs`.

## Surface

Affected file: `spinbike-ui/src/pages/dashboard/action_form.rs`

- Visit buttons rendered at `action_form.rs:408-415` (inside the `pass_active` branch). `data-testid="log-visit-btn"`. Two buttons (Fitness, Spinning) when both classes are services on the active pass.
- Click handler `visit_click_for` (`action_form.rs:241-274`). Currently:
  - No `set_loading.set(true)` → no in-flight indicator, no disabled binding.
  - On `Ok(_)`: only `set_txn_refresh.update(...)` + `clear_note()` — no banner message.
  - On `Err(e)`: `set_msg.set(i18n::tf(lang, "error_format", &[&e]))` — wrong banner (msg renders `.alert-success` in green; errors should use `set_err` which renders `.alert-error` in red).
- Banner rendering (`dashboard/mod.rs:438-446`):
  - `err` signal → `<div class="alert alert-error">` (red)
  - `msg` signal → `<div class="alert alert-success">` (green)

## Behavior changes

1. **Loading state.** On visit-button click, if `loading.get_untracked()` is true, no-op (re-entry guard against double-tap before signal propagates). Otherwise, set `loading=true` synchronously, fire the POST, and on response (Ok or Err) set `loading=false`.
2. **Disabled binding.** Both visit buttons get `disabled=move || loading.get()` — same pattern as the Spinning quick-charge chip (`action_form.rs:373`).
3. **Success banner.** On `Ok(_)`, format `i18n::tf(lang, "visit_added_format", &[&svc_name])` and call `set_msg.set(...)` with that string. The service name is looked up from `services.get_untracked()` by matching `service_id`, then `display_name(lang)` gives the localized name (same call already used for the button label).
4. **Auto-clear.** After setting the success message, spawn a `gloo_timers::future::TimeoutFuture::new(2500)` and, when it resolves, clear the message **only if `msg.get_untracked() == captured_message`**. The captured-message comparison ensures a second visit press during the 2.5 s window doesn't get its message cleared early — the second press replaces `msg` with new text, so the first timer's comparison fails and it does nothing.
5. **Error path fix.** On `Err(e)`, call `set_err.set(e)` (red banner) instead of routing the translated error string into `set_msg`. This is a bug fix that lives naturally in the same closure.

## Non-changes

- Charge-button banner persistence behavior is unchanged — only visit-button success messages auto-clear.
- No new toast primitive, no button-flash animation.
- No server-side changes (the bug is client-only).
- No CSS changes — `.alert-success` and `[disabled]` styles already exist.

## i18n keys

Add ONE key to `spinbike-ui/src/i18n.rs`:

| key | Slovak (unaccented per project convention) | English |
|---|---|---|
| `visit_added_format` | `Vstup pridany: {}` | `Visit added: {}` |

The Slovak word `vstup` ("entry") is consistent with the existing Overview column `overview_col_visits = "Vstupy"`. The chosen wording was validated against alternatives ("Navsteva zaznamenana", "Zapisane") during brainstorming.

## Tests

### Unit (wasm-bindgen)

In `spinbike-ui/src/i18n.rs` next to the existing format-key tests, add:

```rust
#[wasm_bindgen_test]
fn visit_added_format_renders_slovak() {
    assert_eq!(tf(Lang::SK, "visit_added_format", &["Fitness"]), "Vstup pridany: Fitness");
}

#[wasm_bindgen_test]
fn visit_added_format_renders_english() {
    assert_eq!(tf(Lang::EN, "visit_added_format", &["Spinning"]), "Visit added: Spinning");
}
```

Mutation kills: format-string flip (`"Vstup pridany: {}"` → `"Vstup: {}"`), key swap, language swap.

### E2E (Playwright)

New file: `e2e/tests/visit-button-feedback.spec.ts`. Seed a fresh card with an active monthly pass via `/api/test/seed-transactions` (the same endpoint used by other E2E tests; see `e2e/tests/last-visit-display.spec.ts` and `cards-stats.spec.ts` for the pattern). Use `loginViaAPI` + `setupConsoleCheck` + `assertCleanConsole`.

Assertions in order:

1. Open the seeded card, find the Fitness `log-visit-btn`.
2. Click it.
3. Assert `await expect(button).toBeDisabled()` is satisfied within ~500 ms (button greys out during the POST). Use `expect(button).toBeDisabled({ timeout: 1000 })`.
4. Assert `.alert-success` becomes visible with text `Visit added: Fitness` within 2 s of click.
5. Assert button re-enabled after response: `await expect(button).toBeEnabled({ timeout: 3000 })`.
6. Assert `.alert-success` no longer visible after 3.5 s (auto-clear): `await expect(page.locator('.alert-success')).not.toBeVisible({ timeout: 3500 })`.
7. `assertCleanConsole(consoleMessages)`.

Optional second test: click TWICE rapidly, assert only ONE `/api/payments/log-visit` request fires (verify via `page.on('request', ...)` listener filtered by URL). This guards the re-entry / disabled binding.

### Mutation testing anticipation

The `cargo mutants --in-diff` job runs on the server diff. Since this fix is **UI-only** (no server changes), the mutation gate is a no-op for this PR — no SQL, no Rust server logic to mutate. UI mutation testing is disabled per #47. **Mutation pressure is therefore concentrated in the wasm-bindgen unit tests above** plus the E2E assertions on the visible message text and disabled state.

## Risks and edge cases

- **Auto-clear timer racing with logout / route change.** If staff navigates away during the 2.5 s window, the timer still fires and writes `set_msg.set("")`. Setting an empty string on a destroyed signal is a no-op in Leptos — verified safe.
- **Service lookup fallback.** `services.get_untracked().iter().find(|s| s.id == service_id)` is expected to always succeed because the buttons are rendered from the same `services` signal. If somehow it returns `None`, fall back to the empty string `""` — yields `"Visit added: "` which is graceful (still distinguishable from no-banner). Logged via `log::warn!` for debugging.
- **Spinning button vs Spinning quick-charge chip.** These are two different buttons with two different effects (the visit button on a pass logs a zero-amount visit; the quick-charge chip on a no-pass card runs a full charge). The fix only touches the visit-button path. The chip is already correct.
- **CEO multilingual.** Site is Slovak-default. Tests assert both languages exist; the deployed UI shows Slovak.

## Out of scope

- Auto-clear for charge success banner (different UX context, separate decision if needed).
- Toast component refactor.
- Button-flash animation.
- Hardening visit-button against server-side double-submission (server is non-idempotent today; #58 and similar perf items are tracked separately). The disabled binding + re-entry guard already eliminates double-submit at the client level.
