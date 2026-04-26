# Card Management Rework + Blue Theme — Design Spec

**Date:** 2026-04-26
**Version target:** 0.11.0
**Status:** Approved

## Problem

After three rework attempts, the staff card-detail panel still feels like "a big mess of buttons" to the operator (Štefan):

1. Two separate flows for charging and topping up — each has its own amount input and its own button. Two inputs for what is conceptually one operation ("move money on this card").
2. Light green on white is hard to read — primary buttons and accent text don't contrast enough. Owner wants the app's brand color to be **blue**.
3. The "Sell Monthly Pass" button is the visually largest button (`btn--hero btn--block`) yet is used about once a month per customer. The biggest button should be the most-used action, not the rarest.
4. The top-bar "SpinBike" wordmark navigates to `/` (Schedule) but the bottom/sidebar `AdaptiveNav` highlights "Desk" as active because `desk_active = path == "/" || starts_with("/staff")`. Users see a clicked Desk tab while looking at the Schedule page.

## Goals

- Reduce the card-detail action surface to one amount input plus two equal-weight buttons (Top up, Charge).
- Make Monthly Pass live inside the same service dropdown as Fitness/Spinning, so the action stays in the unified form rather than its own giant button.
- Repaint the brand color to vibrant blue (#2563eb light / #60a5fa dark) so primary actions read clearly on both themes.
- Fix the SpinBike-vs-Desk nav state mismatch.
- Keep all existing functionality (log-visit chips, history/upcoming/persistent tabs, edit info, block, pass banner, custom valid-until on pass sale) — only the layout and colors change.

## Non-goals

- No backend/API changes. `/api/payments/charge`, `/api/payments/sell-pass`, `/api/cards/topup`, `/api/payments/log-visit` keep their current contracts.
- No database schema changes.
- No customer-facing page redesign (login, my/balance, my/bookings keep their current shape; they inherit the new color tokens automatically).
- No change to the History/Upcoming/Persistent tab implementations.
- No changes to Edit Info or Block buttons.

## Architecture

### 1. Brand color — Vibrant Blue

Update the token block in `spinbike-ui/style.css`:

| Token | Old (green) | New (blue) | Theme |
|---|---|---|---|
| `--brand` | `#22c55e` | `#60a5fa` | dark |
| `--brand` | `#16a34a` | `#2563eb` | light |
| `--brand-tint` | `rgba(34,197,94,0.14)` | `rgba(96,165,250,0.14)` | dark |
| `--brand-tint` | `rgba(22,163,74,0.10)` | `rgba(37,99,235,0.10)` | light |
| `--primary-fg` | `#fff` | `#fff` | both (unchanged, AA on blue) |

`--info` is already blue; after the swap `--info` and `--brand` are visually similar. To keep things tidy:

- Keep `--info` as a separate semantic token but redefine it as `var(--brand)` so there is one source of truth for "primary blue".
- `--info-soft`, `--info-border`, `--info-soft-fg`, `--info-hover` already derive from `--info` via `color-mix` and require no further edits.

`--pass` (currently lime `#84cc16` / `#65a30d`) reads poorly on a blue brand. Re-tone it to a warmer accent that still distinguishes "active pass" from primary-action blue:

- dark: `#fbbf24` (amber-400)
- light: `#d97706` (amber-600)

`--pass-fg` flips to dark text on amber: `#1a1306`.

All other tokens (surfaces, text, danger, borders) stay the same.

### 2. Unified action form

A new component `spinbike-ui/src/pages/dashboard/action_form.rs` replaces three existing components:

- `charge_section.rs` (kept only as a removed file)
- `topup_section.rs` (kept only as a removed file)
- `sell_pass_modal.rs` (kept only as a removed file)

#### Layout

```
┌─ Card detail ──────────────────────────────────┐
│ Stefan Sumerling   • 1234567               ✕  │
│ [Show contact]                                 │
│ ── Pass banner (if active) ──                  │
│                                                │
│   Balance: 12.50 €                             │
│                                                │
│   Service: [ Fitness                  ▾ ]      │
│   Amount:  [ 3.50                  ]   €      │
│                                                │
│   ┌─ if Monthly pass selected ──────────┐     │
│   │ Valid until: [ 26.05.2026         ] │     │
│   └─────────────────────────────────────┘     │
│                                                │
│   [  + Top up  ]   [  Charge / Sell pass  ]   │
│                                                │
│   Quick log-visit chips (only when pass active)│
│                                                │
│ [Edit info]  [Block]                           │
│ ── Tabs: History | Upcoming | Persistent ──   │
└────────────────────────────────────────────────┘
```

#### Behavior

- **Service select** — populated from `services` (already passed to `CardActionPanel`). Includes `Monthly pass` (no longer filtered out). On change, the amount input auto-fills with `service.default_price`. The user may overwrite.
- **Amount input** — single text input, `inputmode="decimal"`, parses via existing `crate::util::parse_money`.
- **Valid-until row** — rendered conditionally when the selected service is `Monthly pass`. Default value is `max(card.pass.valid_until, today) + 30 days`. Uses the existing `DateInput` component.
- **Top up button** — always-on `btn btn--primary btn--lg`. Submits `card_id + amount` to `POST /api/cards/topup`. Ignores service. On success: refresh card, success toast `topup_ok_format`.
- **Charge button** — always-on `btn btn--primary btn--lg`. Label is `i18n("charge")` until the service is `Monthly pass`, then it flips to `i18n("sell_pass_action")`.
  - Default route: `POST /api/payments/charge` with `card_id + amount + service_id`.
  - Monthly-pass route: `POST /api/payments/sell-pass` with `card_id + price + valid_until`.
- **Quick log-visit chips** — render only when the card has an active pass, exactly as today, but read service list directly (no longer nested inside the deleted `ChargeSection`).
- **Submit guards** — disable both buttons while `loading == true`. Empty/zero amount disables Charge for non-pass; for Monthly pass, an explicit `0` is allowed (matches existing sell-pass behavior; backend accepts promotional `0 €` passes).
- **Error surface** — single inline `<div class="alert alert-error">` below the buttons replaces the dispersed `set_msg(...)` calls. Existing `set_msg` (top-of-panel toast) stays for parent-level success messages so the existing E2E selectors continue to match.

### 3. Pass banner — unchanged

`spinbike-ui/src/pages/dashboard/pass_banner.rs` stays as-is. It already does the right thing: shows "Active until DD.MM.YYYY" with days remaining, an "Edit pass date" button, and an "Expired N days ago" state. It does not have a sell-pass CTA today, and we are not adding one — to sell a new pass, pick "Monthly pass" in the service dropdown.

### 4. Nav routing

Two changes:

#### `spinbike-ui/src/components/nav.rs`

```rust
let brand_href = move || {
    match auth::get_user() {
        Some(u) if u.role == "admin" || u.role == "staff" => "/staff",
        _ => "/",
    }
};
view! {
    <a href=brand_href class="navbar-brand">"SpinBike"</a>
    ...
}
```

`brand_href` is a closure so it re-evaluates after login state changes (uses `auth_ver` indirectly through `get_user()` reading localStorage).

#### `spinbike-ui/src/components/adaptive_nav.rs:31`

```rust
// Before:
let desk_active = path.starts_with("/staff") || path == "/";
// After:
let desk_active = path.starts_with("/staff");
```

#### `spinbike-ui/src/router.rs`

Add a `RedirectTo` mount at `/` for staff/admin so the staff Desk is the home page when they're logged in. Customers and anonymous visitors continue to see the Schedule at `/`.

```rust
// Pseudocode — route guard: if user is staff/admin and path == "/",
// render <RedirectTo path="/staff" />, else render <SchedulePage/>
```

Implementation pattern follows the existing `/admin -> /settings` redirect already in `router.rs`.

## Data flow

No new data flow. All three actions (top-up, charge, sell-pass) call the same backend endpoints they call today. The unified form is purely a UI consolidation.

## Error handling

- Empty amount → button stays disabled; no API call.
- API error → render in inline `.alert.alert-error` below the buttons; toast also updates via existing `set_msg`.
- Negative amount → blocked client-side (parse_money returns None for negative input string after `-` trim) and server returns 400.

## Testing

### Existing E2E updated for selector changes

The old flow used `[data-testid=sell-pass-btn]` to open `[data-testid=sheet-sell-pass]`. The new flow picks "Monthly pass" from `[data-testid=charge-service]`, types the price into `[data-testid=charge-amount]`, types a date into `[data-testid=sell-pass-date]` (DateInput component), then clicks `[data-testid=charge-submit]` (which displays "Predať preukaz" / "Sell pass" in this state).

| Spec | Update |
|---|---|
| `redesign-sheets.spec.ts` | Delete the three tests referencing the sell-pass sheet (lines 34, 100, 118). Keep edit-info and edit-pass-date sheet tests. |
| `monthly-pass.spec.ts` | Rewrite "sell pass → banner → visit logs 0 EUR" to use the dropdown flow. |
| `monthly-pass-expired.spec.ts:29` | Replace `[data-testid=sell-pass-btn]` visibility assertion with: open service dropdown and assert `Monthly pass` option is present. |
| `sell-pass-price-input.spec.ts` | Rewrite the regression specs to drive the unified form: type price into `charge-amount`, type date into `sell-pass-date`, click `charge-submit`. The "empty price shows error" case becomes "empty amount keeps Charge button disabled". |
| `redesign-history-pagination.spec.ts` | No change — uses unchanged selectors. |
| `spin-booking-*.spec.ts` | Re-target topup chip selectors only if their `data-testid` values change in the new component (we will preserve the existing names where possible). |
| `redesign-theme.spec.ts` | Update expected primary-button background sample to the new blue token. |

### New E2E specs

| Spec | What it verifies |
|---|---|
| `e2e/tests/card-action-form.spec.ts` | Default state shows Charge button. Picking Monthly pass reveals valid-until input AND flips the button label to "Sell pass". Submitting calls `/api/payments/sell-pass` (intercept) with the typed price + date. Switching back to Fitness restores Charge label and hides the date row. Submitting Charge calls `/api/payments/charge` with selected service. Top up calls `/api/cards/topup` regardless of selected service. |
| `e2e/tests/card-action-form-pass.spec.ts` | Card with active pass: log-visit chips render above the form. Form's Top up + Charge still work. |
| `e2e/tests/nav-brand-link.spec.ts` | As staff: click the SpinBike wordmark → URL becomes `/staff`. The `data-testid="nav-desk"` element has `aria-current="page"`; no other nav item does. As customer: click the wordmark → URL becomes `/` and the customer Schedule view renders. |
| `e2e/tests/theme-blue.spec.ts` | Sample the computed background-color of `.btn--primary`. In dark mode, RGB matches `#60a5fa` (within 1-channel tolerance). In light mode, RGB matches `#2563eb`. The pass banner background reads as amber, not lime green. |

All Playwright specs assert zero browser console errors as the last assertion (project rule).

### Unit tests

The new `action_form.rs` is mostly Leptos view code; behavior is exercised by Playwright. No new Rust unit tests are required for this rework.

### Mutation testing

Diff-mode `cargo mutants` covers the (unchanged) backend. Frontend mutation coverage is not in scope today.

## Migration / rollout

- Single PR from `dev` → `main`. No DB migration. No backwards-compat shim — components removed are not used elsewhere.
- After deploy, the only user-visible change is the form layout, button hierarchy, and color theme. No retraining needed.

## Risks

- Dropdown discoverability: An owner who's used to the giant "Sell Monthly Pass" button may not immediately notice that the same action lives inside the service dropdown. Mitigation: the dropdown's first time it lists Monthly pass, the option label includes the price (`Monthly pass (35.00 €)`) so it's obvious the option is functional.
- Color regression: any component reading the green-only token must be checked. Audit grep for `#22c55e`, `#16a34a`, `--brand`, `--pass`, hardcoded greens.

## Out of scope (future)

- Customer-facing pages may still feel green-leaning until their own next pass.
- Pass banner could later show a one-tap "Renew" shortcut that pre-selects Monthly pass in the dropdown — not part of this rework.
