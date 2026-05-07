# Edit transaction date — design spec

**Issue:** [#76](https://github.com/zbynekdrlik/spinbike/issues/76) — "add possibility to change date of move in user history when some eg topup been forgoten it needs be added to some previous day"

**Date:** 2026-05-07

## Goal

Let staff backdate an already-recorded transaction (topup / charge / visit) to a previous day so it lands on the correct day in user history and in daily revenue reports.

## User workflow

1. Staff records topup/charge/visit normally — transaction is created with today's date.
2. If the entry should belong to an earlier day, staff opens the user history list, clicks a small "edit date" pencil icon on the row.
3. A bottom sheet appears with a date picker pre-filled with the current date. Staff picks a new date (within the last 30 days), clicks Save.
4. The transaction's `created_at` date is overwritten in place. Reports for both the old and new days update automatically.

There is **no entry-time picker** on the action form. The action form keeps creating transactions with today's date; backdating is an after-the-fact correction made via the row pencil. This collapses both "forgot yesterday's topup" and "wrong day on a recorded entry" into the same single path.

## Out of scope

- Editing the time component of `created_at`. Only the date portion is editable; the existing time-of-day is preserved so list-row ordering is stable.
- Audit trail (no `original_created_at`, no edit log).
- Future dates (forward-dating).
- Editing voided rows. The pencil is hidden on rows with `deleted_at IS NOT NULL`.
- Bulk re-date.
- Editing `valid_until` (already covered by `EditPassDateSheet`).

## Architecture

A new bottom sheet `EditTxDateSheet` mirrors the existing `EditPassDateSheet` pattern. A new backend route `PATCH /api/transactions/{id}/created-at` accepts a date and overwrites the date portion of `created_at`, keeping the existing time. Reports follow automatically because every report groups by `date(created_at)`.

The two sheets stay separate (no shared abstraction) — they edit different fields with different validation windows, and forcing a parameterised wrapper costs more than two short focused files.

## Files touched

**Backend:**
- `crates/spinbike-server/src/routes/transactions.rs` — register `PATCH /api/transactions/{id}/created-at` next to existing `valid-until` and `note` routes; add `patch_created_at` handler.

**Frontend:**
- `spinbike-ui/src/api.rs` — no change; existing `api::patch` is reused.
- `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs` — NEW. Bottom-sheet component modelled on `edit_pass_date.rs`.
- `spinbike-ui/src/pages/dashboard/sheets/mod.rs` — export `EditTxDateSheet`.
- `spinbike-ui/src/pages/dashboard/transactions_list.rs` — add the date-pencil affordance on each row; open `EditTxDateSheet` on click; refresh on save.
- `spinbike-ui/src/i18n.rs` — three new keys (see below).

**E2E:**
- `e2e/tests/edit-tx-date.spec.ts` — NEW. Real-browser test that creates a topup, opens the row pencil, picks a date 3 days back, saves, and asserts the row's date column reflects the new day.

## Backend: `PATCH /api/transactions/{id}/created-at`

**Request body:** `{ "created_at_date": "YYYY-MM-DD" }`

**Role gate:** `can_manage_cards` (same as existing `patch_note` and `valid-until` routes).

**Request deserialization:** `created_at_date` is typed as `chrono::NaiveDate` so `serde` rejects malformed dates with axum's default 422 / 400 — no custom message needed for that path (frontend never sends malformed dates because the picker emits canonical `YYYY-MM-DD`).

**Validation rules** (return 400/404/409 with a specific error message for each):

1. The new date is between `today − 30 days` and `today` inclusive. Otherwise 400 `"Date must be within last 30 days"` (covers both "older than 30d" and "future date").
2. The transaction exists. Otherwise 404 `"Transaction not found"` (check before the voided gate).
3. The transaction is not voided (`deleted_at IS NULL`). Otherwise 409 `"Cannot edit date on voided transaction"`.

**Update SQL:**

```sql
UPDATE transactions
SET created_at = ?
WHERE id = ?
```

Where the bound `created_at` is built as `format!("{} {}", new_date, existing_time_part)` — the existing `created_at` is split on the first space; the date part is replaced; the time part is preserved verbatim.

**Response:** 200 with the updated transaction's relevant fields (`{id, created_at}`) so the frontend can update its row signal in place if useful.

**Tests** (Rust integration, in `transactions.rs` test module):

- happy path: backdate by 3 days returns 200, DB row reflects the new date AND the time-of-day from the original `created_at` is preserved.
- 31 days back: returns 400 with "Date must be within last 30 days".
- future date (today + 1): returns 400 with "Date must be within last 30 days".
- non-existent id: returns 404.
- voided txn (`deleted_at` set): returns 409.
- non-staff role: returns 403.

## Frontend: `EditTxDateSheet`

Modelled directly on `EditPassDateSheet` (`spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs`). Differences:

- Props: `transaction_id: i64`, `current_date: NaiveDate`, `on_close`, `on_saved`.
- The save button POSTs to `PATCH /api/transactions/{id}/created-at` with `{created_at_date: NaiveDate}`.
- Title i18n key: `"edit_tx_date"` ("Zmenit datum zaznamu" / "Change entry date").
- Date label i18n key reused: `"modal_date"` (add if missing).
- Validation in the sheet on save: if `draft < today - 30 days || draft > today`, show inline error `"Date must be within last 30 days"` (i18n key `tx_date_window_error`) and DO NOT send the request. The shared `<DateInput>` does not currently have min/max props — adding them is out of scope; sheet-level validation is sufficient since this is the only consumer of bounded date editing right now.
- On save success, call `on_saved()` which the parent uses to refresh its txn list signal.
- Backend remains the source of truth (also returns 400 if somehow bypassed) — sheet just avoids a round-trip for the obvious case.

## Row affordance in `transactions_list.rs`

- Add a small icon button next to the existing note-edit pencil (`\u{270e}` ✎) and void (`\u{2715}` ✕). Use the calendar glyph `\u{1F4C5}` (📅) inside a `<button class="btn btn--compact btn--ghost">` — same wrapper class the existing two icons use.
- Visibility: hidden on rows with `deleted_at IS NOT NULL`. Same pattern the void icon already uses.
- `data-testid="txn-date-edit"` on the pencil button. `data-testid="sheet-edit-tx-date"` on the sheet root. `data-testid="tx-date-input"` on the date input. `data-testid="tx-date-save"` on the save button.
- Per-row signal `editing_date` mirrors the existing `editing` (note-edit) signal pattern: open one row at a time. Closing happens via the sheet's `on_close`.

## i18n keys (Slovak unaccented per project convention)

```rust
m.insert("edit_tx_date", ("Zmenit datum zaznamu", "Change entry date"));
m.insert("tx_date_edit_tooltip", ("Zmenit datum", "Change date"));
m.insert("tx_date_window_error", ("Datum musi byt v poslednych 30 dnoch", "Date must be within last 30 days"));
// modal_date label: if not already present in i18n.rs, add
// m.insert("modal_date", ("Datum", "Date"));
```

## Test coverage / mutation pressure

**Backend integration tests** (Rust, see "Tests" list in the route section above): seven cases covering happy path + each validation branch + role gate.

**Frontend wasm-bindgen tests:** none required for the sheet component itself — the existing `EditPassDateSheet` is not unit-tested in WASM either, and the meaningful behavior (date validation, payload shape, on-save refresh) is observable in E2E.

**E2E** (`e2e/tests/edit-tx-date.spec.ts`):

- Login as staff, open a card, record a topup (creates a transaction with today's date).
- Find that row in the txn list, click `[data-testid="txn-date-edit"]`.
- Wait for `[data-testid="sheet-edit-tx-date"]` to appear.
- Set the date input to `today − 3 days`.
- Click `[data-testid="tx-date-save"]`.
- Assert the sheet closes.
- Assert the row's visible date column now shows `today − 3 days` formatted in DD.MM.YYYY.
- Assert no console errors via `assertCleanConsole(msgs)`.

This single test exercises the full path: pencil → sheet → date pick → PATCH → re-render. It locks in the row affordance, the route contract, and the refresh wiring.

## Mutation pressure

The backend validation branches are the most likely targets for surviving mutants. The seven integration tests above kill the obvious ones (off-by-one on the 30-day window, missing voided check, missing role gate). The E2E covers the date-format round-trip and the fact that the row actually updates in-place.

## Migration / data shape

No migration needed. `created_at` is `TEXT NOT NULL DEFAULT (datetime('now'))` — already mutable. No new columns, no new tables.

## Versioning

Bumping `VERSION` from current dev version to the next patch (e.g. `0.13.26 → 0.13.27`) is the first commit of the feature branch, per project rule.

## Acceptance

- A staff user can edit the date of any non-voided transaction within the last 30 days, and the change is visible in user history immediately.
- The same edit moves the row between days in the daily revenue report (`reports/trzby` or equivalent) without any extra action.
- All seven backend tests pass; the E2E test passes; `cargo fmt --all --check` clean; CI green including mutation testing on the diff.
