# Spinning Quick-Charge Chip — Design (#34)

**Date:** 2026-05-01
**Issue:** [#34 Re-add Spinning quick-charge chip on card desk](https://github.com/zbynekdrlik/spinbike/issues/34)
**Predecessors:** PR #35 (#30 reverted in `9918d34`), PR #37 (#33 Fitness preselect, lifecycle pattern)
**Bundled cleanup:** [#38 Use bare FITNESS_NAME_EN at action_form.rs:336](https://github.com/zbynekdrlik/spinbike/issues/38)

## Goal

Give staff a one-click charge for the most common transaction (Spinning visit). Open card → click `Spinning {price}€` button → credit charged, txn logged. Replaces the multi-step Charge → pick service → enter amount flow for the dominant case.

## Why this needs a careful design

The first attempt (commit `fa7b34d`) shipped on PR #35 and was reverted in `9918d34` because it broke unrelated E2E tests: after charging via the chip OR the regular Charge button, the transactions list rendered "No transactions". Bisecting confirmed the chip's outer reactive wrapper `{move || services.get() ...}` was the regression source — same lifecycle-cascade class as the `prop:value` issue with #29 Fitness preselect (fixed in PR #37 by switching to imperative `set_value()`).

The pattern is clear: **reactive subscriptions inside `ActionForm` that re-render during `set_selected.update` interleave with the parent's `match selected.get()` re-mount in `dashboard/mod.rs`, dropping side effects.** The fix is to keep `ActionForm`'s view tree static once mounted; do not subscribe its DOM to signals that the click handler updates.

## Architecture

**Compute the chip view at component mount, render as static DOM, never subscribe to `services`.**

```rust
// In ActionForm body, before the view! macro:
let spinning_chip = services.get_untracked()
    .into_iter()
    .find(|s| s.name_en == SPINNING_NAME_EN && s.active != 0)
    .map(|svc| {
        let svc_id = svc.id;
        let price = svc.default_price;
        let on_click = move |_ev: web_sys::MouseEvent| { /* charge — see below */ };
        view! {
            <div class="chip-row quick-charge-row">
                <button class="btn btn--info"
                        data-testid="quick-charge-spinning"
                        on:click=on_click
                        disabled=move || loading.get()>
                    {format!("Spinning {price:.2} €")}
                </button>
            </div>
        }
        .into_any()
    });

// Inside view! macro:
view! {
    <form ...>
        ...existing blocks...
        {spinning_chip}            // Option<AnyView> — renders nothing when None
        ...service select etc...
    </form>
}
```

### Why this works

| Property | Mechanism |
|---|---|
| No reactive subscription to `services` | `get_untracked()` reads without subscribing |
| `svc_id`, `price` are plain Rust values | No signal access in click handler beyond what's already there |
| Chip view is static DOM after mount | Same lifecycle as the existing Charge button |
| `disabled=move \|\| loading.get()` is OK | `loading` is local to ActionForm; doesn't trigger parent re-mount |
| Hidden when Spinning missing | `Option<AnyView>::None` renders nothing — no placeholder DOM |

### Why "next card open" comes free (Q1 answer)

`ActionForm` re-mounts whenever the parent's `selected` signal changes (`dashboard/mod.rs:411`'s `match selected.get() { Some(c) => CardActionPanel(c) }`). Each fresh mount runs the `services.get_untracked()` snapshot again. Admin price edits made between card opens are picked up automatically — no Effect, no signal subscription, no extra code.

### Click handler (identical to `fa7b34d`'s working internals)

```rust
let on_click = move |_ev: web_sys::MouseEvent| {
    set_err.set(String::new());
    set_loading.set(true);
    let card_id_for_click = card_id;
    spawn_local(async move {
        #[derive(serde::Serialize)]
        struct Req {
            card_id: i64,
            amount: f64,
            service_id: Option<i64>,
            note: Option<String>,
        }
        match api::post::<Req, PaymentResp>(
            "/api/payments/charge",
            &Req {
                card_id: card_id_for_click,
                amount: price,
                service_id: Some(svc_id),
                note: None,
            },
        )
        .await
        {
            Ok(r) => {
                set_msg.set(i18n::tf(
                    lang.get_untracked(),
                    "charge_ok_format",
                    &[&format!("{:.2}", r.new_credit)],
                ));
                set_selected.update(|s| {
                    if let Some(c) = s {
                        c.credit = r.new_credit;
                    }
                });
                set_txn_refresh.update(|n| *n += 1);
            }
            Err(e) => set_err.set(e),
        }
        set_loading.set(false);
    });
};
```

This handler is identical in shape to the existing regular Charge button's flow. The PR #35 regression was caused by *the wrapper around the view*, not the handler. Re-using the same handler logic confirms that point.

## Edge cases

| Case | Behavior | Verified by |
|---|---|---|
| Spinning service active in DB | Chip renders with current price | E2E test 1 |
| Spinning missing or `active = 0` | Chip absent (silent), regular Charge still works | E2E test 2 |
| Admin edits price mid-session | Picked up on next card open | Out of E2E scope (out-of-band admin action) |
| `services` empty at mount (race window) | Chip absent until next mount | Negligible — `services` is fetched on dashboard mount, well before any card click; only matters at very first dashboard paint |
| Card has zero or insufficient credit | Server returns 4xx; `set_err` displays — same as regular Charge | Existing payments tests |
| Concurrent click while loading | `disabled=loading.get()` blocks | Inherent to handler |
| Network error mid-request | `Err(e) => set_err.set(e)` displays the error | Existing payments tests |

## Decisions (from brainstorming)

| Question | Decision | Rationale |
|---|---|---|
| Q1 — Price freshness | On next card open | Admin price edits are rare (~quarterly); staff cycles cards every few minutes. Snapshot-at-mount avoids the signal cascade entirely. |
| Q2 — Missing Spinning | Hide chip silently | Same fallback pattern as #33 Fitness preselect ("staff picks manually"). No placeholder, no warning banner. |
| Q3 — PR scope | Bundle #38 (bare `FITNESS_NAME_EN` cleanup) | Both touch `action_form.rs`, #38 is one-line, single CI cycle. |

## Layout

Chip occupies the same position as the original `fa7b34d`: between the existing Quick Action block and the service `<select>` (`action_form.rs` ~line 325). One row, full-width-ish primary button (`btn btn--info`), `chip-row quick-charge-row` for spacing.

The existing class-visit chip row at line ~310 (`chip-row chip-row--spaced chip-row--readable`) is for **logging visits**, not charging. Different feature, different DOM. The Spinning quick-charge chip is a separate row.

## Imports & cleanup (bundled #38)

In `spinbike-ui/src/pages/dashboard/action_form.rs`:

```rust
// Add to import block (above `use super::*`, alongside FITNESS_NAME_EN):
use spinbike_core::services::SPINNING_NAME_EN;

// Change line 336:
// before: if svc.name_en == spinbike_core::services::FITNESS_NAME_EN
// after:  if svc.name_en == FITNESS_NAME_EN
```

`FITNESS_NAME_EN` is already imported (from PR #37). The bundled #38 change makes the inline use site consistent with the rest of the file.

## Testing

### E2E tests (added to `e2e/tests/desk-ux.spec.ts`)

All three with `setupConsoleCheck` / `assertCleanConsole`. Use existing helpers `loginViaAPI`, `activateUniqueCard`, `openCardByLastName`.

**Test 1 — Spinning chip charges card in one click (#34)**

```text
1. Login as staff via API
2. GET /api/services to learn Spinning's default_price
3. Activate fresh card with credit (e.g. 50 €)
4. Open the card
5. Wait for [data-testid="action-form"]
6. Read initial credit from [data-testid="card-credit"]
7. Click [data-testid="quick-charge-spinning"]
8. Wait for success toast / message
9. Assert credit decreased by Spinning's default_price
10. Assert [data-testid="transactions-list"] is visible AND
    [data-testid="transactions-list-empty"] is NOT visible AND
    transactions-list contains at least one [data-testid="transaction-row"]
```

**Test 2 — Spinning chip is absent when service is inactive (#34)**

```text
1. Login as admin via API (admin endpoint requires admin role)
2. GET /api/services to find Spinning's id and current full record
3. PUT /api/admin/services/{id} with active=0 (preserve other fields)
4. Login as staff via API
5. Activate fresh card
6. Open it
7. Assert page.locator('[data-testid="quick-charge-spinning"]').count() === 0
8. Assert [data-testid="charge-submit"] (the regular Charge button) is still visible
9. Cleanup (in finally / afterEach): PUT /api/admin/services/{id} with active=1
   so subsequent tests see Spinning active again
```

The existing `PUT /api/admin/services/{id}` admin endpoint (`crates/spinbike-server/src/routes/admin.rs`) already supports the full update. No new test fixture needed.

**Test 3 — Regression fence: txn list still populates after chip charge (#34)**

This is the exact scenario that broke in PR #35. Distinct from Test 1: explicitly asserts the absence of the empty-state element to catch the cascade-regression class even if a future refactor inverts the empty-state logic.

```text
1. Login as staff via API
2. Activate fresh card with credit
3. Open the card
4. Click [data-testid="quick-charge-spinning"]
5. Wait for response
6. Assert [data-testid="transactions-list-empty"] has count === 0 (no "No transactions")
7. Assert [data-testid="transaction-row"] count >= 1 in the [data-testid="transactions-list"] container
```

### Unit tests

None added. The chip is purely UI/lifecycle behavior — only Playwright catches the regression class. The `/api/payments/charge` endpoint already has unit + integration coverage.

## Acceptance criteria

- Open card with active Spinning service → chip shows `Spinning {price:.2} €` with current `default_price`
- One click → credit decreases by `price`, txn row appears in list, success toast shown
- Spinning deactivated → chip absent, no UI artifacts, regular Charge button still works
- Three E2E tests above pass on CI
- No regression in existing dashboard / charge / desk-UX tests
- Browser console clean (zero errors, zero warnings) per `setupConsoleCheck`
- Bundled #38: `action_form.rs:336` uses bare `FITNESS_NAME_EN`

## Out of scope

- Live (signal-driven) price updates — by design (Q1); admin edits visible on next card open
- Localizing "Spinning" — service name is identical in Slovak and English per existing pattern
- Showing chip on pass-holders only / hiding for them — chip works regardless of pass status (matches `fa7b34d`); a pass-holder may still want a paid Spinning visit outside their pass scope
- Multi-service quick-charge chips (e.g. Cycling, Fitness) — only Spinning is the dominant transaction; future expansion is its own design

## Risk register

| Risk | Mitigation |
|---|---|
| Re-introduces the PR #35 txn-list regression | Architectural choice (no reactive wrapper) + dedicated regression-fence E2E test (#3 above) |
| `services.get_untracked()` returns empty at mount | Negligible — `services` is fetched on dashboard mount, well before any card click; if it ever did happen the chip would be absent until next card mount, which is graceful |
| Admin renames "Spinning" service in DB | Chip silently hides; `SPINNING_NAME_EN` const is the contract — admin must keep the canonical English name |
| Concurrent admin price edit during charge | Click captures `price` at mount; charge POSTs that captured value. The `/api/payments/charge` server route only validates `amount > 0` (and rejects monthly-pass via `service_id`), NOT amount-vs-`default_price` — so a mid-session admin price edit charges the staff's snapshot price without server-side rejection. This is correct: the design's snapshot-at-mount semantics rely on the server NOT comparing amount to `default_price`. |

## File changes summary

| File | Change |
|---|---|
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Add `use spinbike_core::services::SPINNING_NAME_EN;` import. Add `spinning_chip` Option<AnyView> at component body. Insert `{spinning_chip}` in view! macro between Quick Action block and service `<select>`. Bundled #38: change line 336 to bare `FITNESS_NAME_EN`. |
| `spinbike-ui/src/pages/dashboard/transactions_list.rs` | The outer wrapper at the bottom (`<div class="group" data-testid="transactions-list">`) already exists. Add two new testids: `data-testid="transactions-list-empty"` on the `<div class="empty-state">` (line 49) and `data-testid="transaction-row"` on each `<div class=row_class>` (line ~120). Required by Tests 1 and 3 below. |
| `e2e/tests/desk-ux.spec.ts` | Add three tests inside the existing `Staff desk UX cluster — issues #29 #30 #31 #32` describe block (rename describe to include #34). |

## VERSION bump

`VERSION` 0.13.11 → 0.13.12 (PR #37 just merged; main and dev now match → next dev commit MUST bump per `version-bumping.md`). Run `bash scripts/sync-version.sh` after editing.
