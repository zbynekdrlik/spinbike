# Credit Improvements Design

**Issue:** #6 — credit improvements
**Date:** 2026-04-20
**Goal:** Fix a regression that blocks drinks/food purchases when a monthly pass is active, give staff tools to correct pass end-dates and history entries, and split the long card page into tabs.

## Problem

Four friction points on the staff card page that appeared after the spin-booking rollout:

1. **Drinks/food purchases blocked with active pass.** The UI swaps the charge form for "Log visit" buttons when the card has an active pass, so staff cannot charge credit for a drink. The backend `POST /api/payments/charge` endpoint already works in that state — the block is UI-only.
2. **Monthly pass end date is immutable.** Once sold, there is no way to extend or shorten the pass (except by selling another one). Staff need to fix mistakes and handle special cases like client vacations.
3. **History log entries cannot be corrected.** Wrong topups or charges stay forever. Staff need a way to void mistakes.
4. **Card detail page is too long.** The spin-booking PR added two components (Upcoming classes, Persistent bookings) on top of the existing sections, pushing the transaction history off-screen.

## Scope

In scope:

- UI change to show charge form alongside Log-visit when pass is active
- New `PATCH /api/transactions/{id}/valid-until` route (staff role)
- New `DELETE /api/transactions/{id}` route (staff role, soft-delete)
- New `deleted_at` column on `transactions`
- Tab container on card detail page: **History / Upcoming / Persistent** (top bar with card info, credit, pass, primary actions stays always-visible)

Out of scope:

- Hard-deleting transactions (soft-delete only, to preserve audit)
- Editing transaction amount or type (delete-and-re-add for corrections)
- Bulk operations on history
- Reworking admin-side views

## Architecture

All four items fit the existing Axum + Leptos stack. No new crates, no new patterns.

- **Storage:** add `deleted_at TEXT` (ISO datetime) column to `transactions` via migration V7. All queries that list transactions add `WHERE deleted_at IS NULL` when hiding void entries; the card-page query keeps them visible but marks state.
- **Backend:** two new handlers in `routes/transactions.rs` (new file, co-located by resource). Both require staff role (`can_cancel_any_booking` or a new `can_edit_transactions` method — reuse staff gate).
- **Frontend:** extend `ActionPanel` with a 3-tab container; refactor the current linear layout so the always-visible block and the tab block are two sibling regions. Transaction rows get a ✕ button; pass banner gets a pencil.
- **No backend change for item 1** — the server already allows charging during an active pass; only the UI hides the form.

## Components

### 1. Pass-active charge form

**File:** `spinbike-ui/src/pages/dashboard.rs` (ActionPanel → ChargeSection)
Today the `{move || if pass_is_active() { log_visit_buttons } else { charge_form }}` branches mutually. Change to always render the charge form and, when the pass is active, render the Log-visit buttons above it as the primary suggested action. Label the charge form "Charge for drinks / food / other" to make it clear it is not for the class itself.

### 2. Edit pass end date

**UI:** Pencil icon next to the end-date on `PassBanner`. Click → inline `<input type="date">` + Save/Cancel. Calendar defaults to current `valid_until`. Save calls the new route.

**Route:** `PATCH /api/transactions/{id}/valid-until`
- Body: `{ valid_until: "YYYY-MM-DD" }`
- Staff-only
- Validates the target transaction is a pass-sale (has existing non-null `valid_until`)
- Updates the row
- Returns the updated transaction

### 3. Soft-delete history entries

**UI:** ✕ button on each row of `TransactionsList`. Click → confirm modal "Void this entry?" → DELETE call. Voided rows stay visible: greyed background, strikethrough amount, a "voided" tag.

**Route:** `DELETE /api/transactions/{id}`
- Staff-only
- Atomic: set `deleted_at = datetime('now')` AND adjust `cards.credit` to reverse the transaction's impact: `UPDATE cards SET credit = ROUND(credit - transaction.amount, 2)`. Signed amounts already encode direction (charges are negative, topups positive), so subtracting the amount reverses every kind of transaction with one formula.
- Returns 204

**Read queries:**
- Pass-validity query (`get_card_pass_valid_until`): add `AND deleted_at IS NULL` — voided pass sale no longer counts toward active pass
- History list query: keep voided rows; return `deleted_at` in the response so UI can grey them out
- Credit column (`cards.credit`) is stored, not computed — the DELETE handler updates it at the same time as the soft-delete so the running balance stays consistent

### 4. Tabs on card detail page

**File:** `spinbike-ui/src/pages/dashboard.rs` (ActionPanel)

Top bar (always visible):
- Name, barcode, phone, company (header)
- PassBanner (with new pencil)
- Credit display
- Primary action row: Log visit / charge / topup / sell pass / edit info / block

Tabs below (signal-driven, mirroring `admin.rs`):
- **History** (default) → TransactionsList
- **Upcoming** → UpcomingClasses component
- **Persistent** → PersistentToggles component

One tab visible at a time. Tab state lives in local signal (no URL routing needed; the card page is modal-like).

## Data Flow

Editing pass end date:
`PassBanner → set_editing(true) → <input type=date> → Save → fetch PATCH → success → refetch card details → PassBanner shows new date.`

Voiding a transaction:
`TransactionsList row ✕ → confirm modal → fetch DELETE → success → refetch transactions → greyed row with "voided".`

Tab switch:
`Tab button click → set_tab(name) → match tab.get() renders matching component. Each tab keeps its own internal signals.`

## Error Handling

- `PATCH /api/transactions/{id}/valid-until` 404 if missing, 400 if target is not a pass-sale, 403 if not staff
- `DELETE /api/transactions/{id}` 404 if missing or already deleted, 403 if not staff
- UI surfaces errors via existing toast/alert pattern
- Migration V7 is version-gated like V1-V6 — the runner skips it on subsequent starts

## Testing

**Integration tests (Rust, `tests/transactions_routes.rs` new file):**
- PATCH valid-until happy path + role-forbidden + 404 + 400-when-not-pass
- DELETE soft-delete happy path + role-forbidden + 404
- After DELETE: pass-validity returns None (voided pass no longer active)
- After DELETE: `cards.credit` is adjusted to reverse the voided row (e.g. voiding a +10€ topup decreases credit by 10)

**DB tests:**
- Migration V7 is idempotent (run twice, schema unchanged)
- List queries respect `deleted_at`

**Playwright E2E (`e2e/tests/credit-improvements.spec.ts` new file):**
- Staff with active-pass card: charge form visible, charge for drink succeeds
- Staff edits pass end date: date picker opens, save updates banner
- Staff voids a topup: row greys out and shows "voided"; credit total drops
- Staff switches tabs on card page: History / Upcoming / Persistent render correctly

**Mutation testing:** automatic via existing cargo-mutants CI job on PR diff.

## Rollout

One PR from `dev` to `main` after PR #5 merges and version bumps (0.7.0). Single migration V7, idempotent, auto-applied on server start. No data migration needed — existing rows have `deleted_at = NULL` by default.
