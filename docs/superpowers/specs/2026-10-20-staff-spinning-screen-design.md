# Staff spinning screen — design (#385)

Issue: zbynekdrlik/spinbike#385. Owner decisions recorded on the ticket
(2026-10-19 horizon + focus, 2026-10-20 layout approval).

## Goal

The CEO (Stevo) runs the spinning classes and today has a worse interface
than customers: the staff dashboard shows only the current week, the
booked people are coloured chips that keep piling up, he cannot see
occupancy weeks ahead and cannot enrol a person permanently from the
class itself. Many regulars may never install the app, so **staff must be
able to manage a regular's whole booking life from one screen**, in a
table. The customer side of the app is out of scope.

## Decisions (owner)

1. Planning horizon = **8 weeks** — both what staff see and how far the
   materialiser creates recurring bookings (`WINDOW_DAYS` 14 → 56).
2. Tables, not chips. The `.participant-chip` list in the staff dashboard
   is removed.
3. Layout (approved): occupancy grid weeks × class slots, click a cell →
   class detail table.

## Screen

New route **`/staff/spinning`** (`spinbike-ui/src/pages/staff_spinning/`),
linked from the staff menu as "Spinning". The current week's class block
in `StaffDashboardPage` is replaced by a link/summary to this screen (the
chip list and its participant panel are deleted).

### Occupancy grid

```
Tyzden        Po 18:00   Ut 18:30   St 18:00   St 18:30
              Stopa      Vlada      Stopa      Vlada
20.-26.10.    12/19      15/19      19/19 ●    9/19
...           (8 weeks starting with the current week)
```

- Rows = 8 weeks (Mon–Sun) from the current week; columns = active class
  templates ordered by weekday, start time, header shows instructor name.
- Cell = `booked/capacity`; background tint by fill ratio (token-based,
  contrast-tested in both schemes per `.claude/rules/ui-contrast.md`);
  full = `●`; cancelled class = "zrusene"; past class = muted.
- Data: ONE `GET /api/classes?from=<mon>&to=<mon+55>` call.
- Click/tap a cell → detail of that class (same page, below the grid on
  desktop; full-width section on mobile). Selected cell is highlighted.
- Grid sits in `.table-scroll` so it scrolls inside its box on phones.

### Class detail table

Header: date, time, instructor, `booked/capacity`, button **Zrusit hodinu**
(existing `POST /api/admin/cancel-class`, with confirmation).

| # | Meno | Telefon | Kredit | Prihlaseny | Stav | Akcie |
|---|------|---------|--------|------------|------|-------|

- Meno links to the customer's detail (existing dashboard customer page).
- Kredit = current credit (negative shown in the existing negative style).
- Prihlaseny = `natrvalo` (source `persistent`) / `jednorazovo` (`manual`).
- Stav = `caka na 9:00` (not yet charged) / `strhnute` (charged) /
  `zadarmo` (charged 0 — permanentka/free) .
- Akcie: **Odhlasit** (existing `DELETE /api/bookings/{id}`; staff cancel
  refunds a charged booking, confirm dialog says so); for `natrvalo` rows
  also **Ukoncit natrvalo** (existing
  `DELETE /api/users/{uid}/persistent-bookings/{template_id}`, confirm says
  "zrusi aj buduce terminy").
- Rows sorted by surname.

Below the table: **Pridat** — the existing walk-in search
(`/api/users/search`), each hit offers two buttons:
- **Len na tento termin** → existing `POST /api/bookings` for that user/date.
- **Natrvalo kazdy <den cas>** → existing
  `POST /api/users/{uid}/persistent-bookings` (it sweeps the materialiser,
  so the person appears in all 8 weeks immediately).
Already-booked hits show "uz prihlaseny" instead of buttons (#378 pattern).
After any action the grid and the table reload.

## Server changes

1. `jobs/materialiser.rs`: `WINDOW_DAYS = 56`. Existing tests that count
   materialised dates are updated to the new window (the window constant is
   referenced, not a literal).
2. `ParticipantResponse` gains `phone: Option<String>`, `credit: f64`,
   `charged: bool` and `charged_amount: Option<f64>` (charge transaction
   linked to the booking, not voided). Query stays one SELECT with joins.
3. `GET /api/classes`: reject a range longer than 62 days with 400
   `range_too_long` (new `ErrorCode`, i18n key per
   `.claude/rules/error-codes.md`) — the grid asks for 56.

No migration needed.

## Error handling

Every write follows `.claude/rules/admin-ui-error-handling.md`: a failed
call shows the server's error message in the detail panel, nothing is
silently ignored; 409s (`already_booked`, `class_full`, ...) render their
i18n text.

## Testing

- Server (`cargo test` in CI): materialiser creates bookings up to day 56
  and not 57; participants carry phone/credit/charged correctly for
  uncharged, charged and refunded bookings; `/api/classes` 63-day range →
  400.
- Playwright (`e2e/tests/staff-spinning.spec.ts`, chromium + `webkit-iphone`):
  staff opens `/staff/spinning`, sees 8 week rows; taps a future cell →
  table rows match the API; adds a customer "Len na tento termin" → row
  appears, cell count +1; adds another "Natrvalo" → row in this class AND
  the same slot next week shows them; "Odhlasit" removes the row;
  "Ukoncit natrvalo" removes future rows. Zero console errors.
- Existing specs that assert `participant-chip` are rewritten to the table.
- Contrast test covers the new grid tint tokens in both schemes.

## Out of scope

Customer-side screens; SMS/phone notifications for people without the app
(filed separately if the owner asks).
