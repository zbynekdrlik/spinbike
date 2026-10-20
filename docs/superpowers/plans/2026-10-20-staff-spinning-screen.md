# Staff spinning screen — implementation plan (#385)

Spec (owner-approved 2026-10-20): `docs/superpowers/specs/2026-10-20-staff-spinning-screen-design.md`.
Branch: worktree branch off `dev`, version `0.17.0-dev.1`. Tier-0: no local cargo
build/test/clippy, no trunk — only `cargo fmt --check` (root + `spinbike-ui`) and
`npx tsc --noEmit` in `e2e/`; CI is authoritative.

## Task 1 — materialiser window 14 → 56 days

- `jobs/materialiser.rs`: `WINDOW_DAYS = 56`, module doc updated.
- Tests that hard-coded "3 matching dates (offsets 0, 7, 14)" derive the count
  from the constant (`WINDOW_DAYS / 7 + 1`).
- New test: a subscription on today's weekday materialises day `WINDOW_DAYS`
  (= 56) and nothing at day 63; `WINDOW_DAYS == 56` pinned.

## Task 2 — `/api/classes` range cap

- `spinbike-core` `ErrorCode::RangeTooLong` (`range_too_long`) + `message()` +
  `ALL` table row.
- `ApiError::BadRequestCode(ErrorCode)` — a 400 carrying a specific code (the
  existing `BadRequest(String)` always says `bad_request`); unit test in
  `error.rs`.
- `list_classes`: inclusive day count `> MAX_RANGE_DAYS` (62) → 400
  `range_too_long`, logged. Integration tests: 62 days → 200, 63 days → 400.
- UI `i18n::error_code_key`: `RangeTooLong => Some("err_range_too_long")`
  (+ `ClassFull`/`ClassCancelled` now get their own keys — the spec wants
  409s to render i18n text).

## Task 3 — richer participant payload

- `ParticipantResponse` gains `phone`, `card_code`, `credit`, `charged`,
  `charged_amount` (non-voided linked charge row amount, absolute value). One
  SELECT, same joins.
- Integration test: uncharged (`charged=false`, `charged_amount=null`),
  charged (`charged=true`, amount = Spinning price), charge voided
  (`charged=true`, `charged_amount=null`), phone/card_code/credit echoed.

## Task 4 — UI: `/staff/spinning`

Module `spinbike-ui/src/pages/staff_spinning/`:

- `model.rs` — pure, unit-tested (`wasm_bindgen_test`): week Mondays from a
  given today, grid columns from occurrences (weekday, start, instructor),
  cell lookup, fill level (`low`/`mid`/`full`), stav label key, surname sort
  key.
- `mod.rs` — `StaffSpinningPage`: ONE `/api/classes?from=<mon>&to=<mon+55>`
  fetch, occupancy grid in `.table-scroll`, selection signal, reload counter.
- `detail.rs` — class detail header (date, time, instructor, booked/capacity,
  Zrusit hodinu with inline confirm) + participants table (# | Meno | Telefon
  | Kredit | Prihlaseny | Stav | Akcie), Odhlasit / Ukoncit natrvalo with
  inline confirm, errors shown in the panel (`localize_api_error`).
- `add_person.rs` — debounced `/api/users/search`; each hit offers "Len na
  tento termin" (`POST /api/bookings`) and "Natrvalo kazdy <den cas>"
  (`POST /api/users/{uid}/persistent-bookings`); booked hits show "uz
  prihlaseny".
- Router `/staff/spinning` (staff_gated). Adaptive nav `nav-schedule` tab →
  `/staff/spinning`, label "Spinning".
- `StaffDashboardPage` (`/schedule`, `/staff/classes`): chips, participant
  panel, walk-in form and cancel button removed; read-only current-week
  summary rows + "Otvorit Spinning" link.
- CSS: `.occ-grid` + tint tokens `--occ-*` in both schemes;
  `.participant-chip*` rules deleted.

## Task 5 — E2E

- New `e2e/tests/staff-spinning.spec.ts`, chromium + `webkit-iphone`: 8 week
  rows; tap a future cell → table rows == API participants; add "Len na
  tento termin" → row + cell +1; add "Natrvalo" → row here AND next week's
  same slot; Odhlasit removes the row; Ukoncit natrvalo removes the future
  rows; grid tint contrast >= 4.5 in dark + light; zero console errors.
- Rewrite `staff-participant-chip.spec.ts` (contrast + walk-in "already
  booked") and the refund test in `spinning-recurring.spec.ts` to the table.
- `staff.spec.ts`, `nav-adaptive.spec.ts`: adjust to the summary page / the
  re-pointed tab.
- `playwright.config.ts`: `staff-spinning.spec.ts` runs in both projects.

## Task 6 — docs

- `.claude/rules/spinning-booking.md`: 56-day window, staff screen, participant
  fields. `docs/autopilot-log.md` line.
