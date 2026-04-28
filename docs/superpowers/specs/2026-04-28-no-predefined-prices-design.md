# No Predefined Prices on Staff Dashboard

**Date:** 2026-04-28
**Issue:** [#17](https://github.com/zbynekdrlik/spinbike/issues/17) — "ceo not like that any items has predefined price, he want always put price by himself"

## Goal

Staff types the price every time they Charge a service or Sell a monthly pass. The auto-fill from `default_price` goes away, and the price label next to the service name in the dropdown goes away. Admin still sets prices, and the 4-hour Spinning class auto-charger still uses them — both unchanged.

## Background — what `default_price` does today

`services.default_price` is a `REAL NOT NULL` column on the `services` table with two unrelated consumers:

1. **Staff dashboard convenience** — when staff picks a service in the action form, the amount input auto-fills (`spinbike-ui/src/pages/dashboard/action_form.rs:72`) and the dropdown label shows the price (`action_form.rs:307`, e.g. `Spinning (5.00 €)`). **This is what the CEO is complaining about.**
2. **Automatic class billing** — the 4-hour charger (`crates/spinbike-server/src/jobs/charger.rs:34`) reads Spinning's `default_price` to bill booked classes 4 hours after the class starts. If the customer has an active monthly pass the booking is stamped charged for €0; otherwise the Spinning price is debited from card credit. The job runs at startup and every 60 seconds. **This is the entire revenue path for class billing.**

These two uses share the same column today. The change splits them in INTENT (staff stops seeing predefined prices) without splitting them in STORAGE (the column stays where it is so the auto-charger keeps working).

## Behavior changes (staff dashboard only)

### Service dropdown label

| | Before | After |
|---|---|---|
| Spinning | `Spinning (5.00 €)` | `Spinning` |
| Fitness | `Fitness (5.00 €)` | `Fitness` |
| Monthly pass | `Monthly pass (35.00 €)` | `Monthly pass` |

The dropdown labels are localized via `s.display_name(lang_now)` — the format string `"{} ({:.2} €)"` becomes just the localized name.

### Amount input on service change

| | Before | After |
|---|---|---|
| Staff picks Spinning | input auto-fills with `5.00` | input stays empty |
| Staff picks Fitness | input auto-fills with `5.00` | input stays empty |
| Staff picks Monthly pass | input auto-fills with `35.00` | input stays empty |
| Staff picks "(select service)" | (no change) | (no change — input untouched) |

### Submit guard

The existing `parse_money` + `v > 0.0` guard already no-ops on empty / zero / non-numeric input (`action_form.rs` `do_charge` and `do_topup` paths). No new guard is needed — empty submit silently does nothing today and will continue to.

### Topup

The Topup button has no service selector and never read `default_price`. **No change.**

## What does NOT change

- `services.default_price` column stays in the DB. No migration.
- Admin services CRUD page (`spinbike-ui/src/pages/admin.rs`) still lists, creates, and edits services with their price. No label rename.
- The 4-hour Spinning class auto-charger (`crates/spinbike-server/src/jobs/charger.rs`) keeps reading `default_price` to bill booked classes.
- `/api/services/active` API contract still returns `default_price` for each service. The UI just stops using that field for display or auto-fill. (Removing it from the DTO would be churn — every test fixture, every related route, every type that derives `Serialize` over `Service` would need updating, for zero behavior change.)
- Customer pages (`my_balance`, `my_bookings`) display past transaction amounts (already-charged), not predefined prices. No change.
- Reports — no `default_price` reads.
- E2E global setup (`e2e/global-setup.ts:164`) seeds a Spinning service with `default_price: 120`. The seed stays — the auto-charger still needs a price for booked-class billing in tests.

## Files affected

| File | Change |
|---|---|
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Delete the `el.set_value(&format!("{:.2}", svc.default_price));` auto-fill line in `on_service_change` (~line 72). Change the dropdown label `format!("{} ({:.2} €)", ...)` to use just the localized service name (~line 307). |
| `e2e/tests/no-predefined-prices.spec.ts` | NEW — Playwright test asserting empty input + label without price + staff-typed amount still works. |
| `e2e/tests/dashboard.spec.ts:141` | Test currently relies on auto-fill (comment: "The amount input should auto-fill from default_price. Override to 5 for a..."). Update: type the amount instead of overriding the auto-filled value. |
| `e2e/tests/card-action-form.spec.ts:83` | Test currently relies on auto-fill (comment: "Amount auto-filled from default_price (35.00). Accept it."). Update: type 35.00 explicitly. |
| `e2e/tests/card-action-form.spec.ts:127` | Test currently picks a non-pass service so default_price auto-fills, then clears it. Update: pick the service, then type the test value (the "clear it" step is no longer needed because the input is already empty). |
| `VERSION` | Bump (post-merge of PR #25). Sync to all `Cargo.toml` via `scripts/sync-version.sh`. |

No backend changes. No DB migration. No new dependencies. No public API contract change.

## Implementation details

### action_form.rs change 1 — kill the auto-fill (current ~line 72)

```rust
let on_service_change = move |_ev: web_sys::Event| {
    let raw = service_ref
        .get()
        .map(|el| {
            let el: &HtmlSelectElement = &el;
            el.value()
        })
        .unwrap_or_default();
    let id: Option<i64> = raw.parse().ok();
    set_selected_service_id.set(id);
    // Auto-fill of `amount` from svc.default_price was removed (#17).
    // Staff now types the price every time.
};
```

The `if let Some(id) = id { ... el.set_value(...) }` block is deleted. The `services.get().iter().find(...)` lookup is unused after the deletion and goes with it. The `amount_ref` capture in this closure is also unused after the deletion — remove the binding from the closure's environment to keep clippy happy. (`set_selected_service_id` is still set so the per-pass UI logic that depends on `is_monthly_pass()` keeps working.)

### action_form.rs change 2 — drop the price from the dropdown label (current ~line 307)

```rust
{move || {
    let lang_now = lang.get();
    services.get().into_iter().map(|s| {
        let val = s.id.to_string();
        let kind = s.kind.clone();
        let label = s.display_name(lang_now).to_string();
        view! { <option value=val data-kind=kind>{label}</option> }
    }).collect::<Vec<_>>()
}}
```

The `format!("{} ({:.2} €)", s.display_name(lang_now), s.default_price)` becomes `s.display_name(lang_now).to_string()`.

### New E2E test: `e2e/tests/no-predefined-prices.spec.ts`

Skeleton:

1. Standard test setup: log in as staff, activate a unique card, search by last name, open card detail.
2. **Dropdown labels assertion** — read every option text in `[data-testid="charge-service"]` and assert each non-empty label matches a localized service name (e.g. `Spinning`, `Fitness`, `Monthly pass` for English; SK equivalents for Slovak). Assert no option label contains `€` and no option label matches `\d+\.\d+`. Use `lang=en` to keep the assertion locale-stable.
3. **Empty input on service change** — for each of `Spinning`, `Fitness`, `Monthly pass` (looked up by `option.filter({ hasText: ... })`):
   - Select the option via `selectOption`
   - Read `[data-testid="charge-amount"]` value via `inputValue()` and assert it equals `''`
4. **Submit empty is a no-op** — without typing anything, click `[data-testid="charge-submit"]` and assert no network request to `/api/payments/*` was made (use `page.waitForResponse` with a short timeout that should fail, OR observe the action panel didn't change state).
5. **Typed amount still works** — pick `Spinning`, type `7.50` into the amount input, click Charge, assert success (transaction recorded, card balance reduced).
6. Standard zero-console-errors assertion at end (per `browser-console-zero-errors.md`).

This is a new feature, so per `e2e-real-user-testing.md`, it requires its own dedicated Playwright test file — `no-predefined-prices.spec.ts` — committed in the same PR.

### Existing E2E test updates

The 3 tests below currently depend on the auto-fill. After this change they must type the amount explicitly. The `data-testid` selectors are unchanged, so the only edits are to:

- Replace any "amount auto-filled — accept it" comment with "amount stays empty — staff types it" and add a `.fill('<value>')` step.
- For `card-action-form.spec.ts:127`, drop the "clear it" step entirely (the input is already empty).

The 3 affected tests (per `grep`):

- `e2e/tests/dashboard.spec.ts:141` — "charge for service reduces balance" path
- `e2e/tests/card-action-form.spec.ts:83` — Monthly pass charge
- `e2e/tests/card-action-form.spec.ts:127` — non-pass service auto-fill clear-and-overwrite

## Testing

Per `tdd-workflow.md`:

1. Write the new Playwright test first.
2. Run it — it FAILS (the auto-fill kicks in, the input is `5.00` not `''`, and the dropdown label contains `€`).
3. Make the two `action_form.rs` edits.
4. Re-run the new test — PASSES.
5. Update the 3 existing tests.
6. Run the full E2E suite — all green.

Per `mutation-testing.md`: `cargo mutants --in-diff` should still pass on the diff. The change deletes a code path and simplifies a format string — there's nothing meaningful to mutate in the deletion, and the simpler format string has no operators to mutate. No new mutants expected.

## Acceptance criteria

- [ ] Picking Spinning / Fitness / Monthly pass leaves `[data-testid="charge-amount"]` empty (`''`).
- [ ] Service dropdown labels are exactly the localized service name — no `€`, no numeric price, no parentheses.
- [ ] Submitting empty amount is a no-op (no network request, no error message, no state change). Existing behavior preserved.
- [ ] Typing `7.50` (or any positive value) and clicking Charge still creates the transaction with the typed amount.
- [ ] Auto-charger still bills booked Spinning classes at the admin-configured `default_price` (verify via existing `charger_*` unit tests in `crates/spinbike-server/src/jobs/charger.rs` — they must remain green without modification).
- [ ] Admin services page still lists, creates, and edits services with their price.
- [ ] New Playwright test `no-predefined-prices.spec.ts` is committed and asserts all four behaviors above + zero console errors.
- [ ] All existing E2E tests pass after the 3 auto-fill-dependent tests are updated to type the amount.
- [ ] CI green on the PR (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing, Smoke (dev) after deploy).
- [ ] Post-deploy verification on dev frontend: Playwright opens the staff dashboard, picks each service, confirms input stays empty, and charges €7 against a test card to confirm the typed amount path works end-to-end.

## Out of scope

- Changing the 4-hour Spinning class auto-charger or its pricing source.
- Removing `default_price` from the API DTO, the database, or the admin form.
- Renaming the admin field to "Auto-charge price" or similar.
- Customer-facing pages (`my_balance`, `my_bookings`) — already display past amounts, no predefined prices.
- Reports.
- I18n string changes — the `select_service` and `amount` labels stay as they are.
- Touch / mobile-specific tweaks.
- Backend changes — none.

## Versioning

This work targets a fresh PR after PR #25 (button layout, v0.13.6) merges to main. Bump `VERSION` to the next patch (e.g. v0.13.7) on dev as the FIRST commit, then implement.
