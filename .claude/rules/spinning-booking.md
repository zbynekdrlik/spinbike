---
paths:
  - "crates/spinbike-server/src/booking_rules.rs"
  - "crates/spinbike-server/src/jobs/charger.rs"
  - "crates/spinbike-server/src/jobs/materialiser.rs"
  - "crates/spinbike-server/src/jobs/spinning_reminder.rs"
  - "crates/spinbike-server/src/db/classes.rs"
  - "crates/spinbike-server/src/db/booking_cancel.rs"
  - "crates/spinbike-server/src/db/booking_guests.rs"
  - "crates/spinbike-server/src/db/spinner.rs"
  - "crates/spinbike-server/src/db/persistent_bookings.rs"
  - "crates/spinbike-server/src/routes/classes.rs"
  - "crates/spinbike-server/src/routes/persistent_bookings.rs"
  - "spinbike-ui/src/components/my_spinning.rs"
  - "spinbike-ui/src/components/class_card.rs"
  - "spinbike-ui/src/components/guest_stepper.rs"
  - "spinbike-ui/src/pages/schedule.rs"
  - "spinbike-ui/src/pages/staff_spinning/**"
  - "e2e/tests/spinning-*.ts"
  - "e2e/tests/schedule.spec.ts"
  - "e2e/tests/staff-participant-table.spec.ts"
  - "e2e/tests/staff-spinning.spec.ts"
---

# Spinning bookings: 09:00 binding charge, deadlines, recurring slots (#382)

Owner decisions (ROZHODNUTÉ on #382, spec
`docs/superpowers/specs/2026-10-12-spinning-recurring-bookings-design.md`):
bikes are unnumbered (capacity only); a booking is BINDING at **09:00
Europe/Bratislava on the class day**; the customer may cancel any time
before that (a week ahead is fine), staff/admin always (with a refund when
charged); reminder the day before at 18:00; "spinner" = active recurring slot
OR spinning in the last **365** days (not 90 — spinning pauses in summer).

## One place for every time rule — `crate::booking_rules`

Pure functions taking an injected gym-local `now`: `charge_cutoff`,
`is_charge_due` (R1), `customer_may_cancel(date, charged, now)` (R2),
`has_started(date, start_time, now)` (R4). Callers pass
`crate::util::now_bratislava()`. The charger's SQL uses
`booking_rules::CHARGE_CUTOFF_TIME` (`"09:00:00"`, pinned against
`CHARGE_CUTOFF_HOUR` by a unit test) — never a second literal. Boundaries
are unit-tested there; HTTP tests (`tests/spinning_booking_rules.rs`) only
use dates whose verdict cannot depend on the real time of day (yesterday /
a week ahead / a 00:00 start).

**The server computes the flags, the UI never does time-zone math:**
`/api/classes` carries `started` + `cancel_open`, `/api/my/bookings` carries
`source` + `cancel_open`, participants carry `source` + `refundable`.

## Cancellation provenance — `bookings.cancel_kind` (V31)

Two cancel paths, nothing else: `db::booking_cancel::cancel_booking(pool,
id, cancelled_by, CancelKind)` for a single booking (customer / staff), and
`db::persistent_bookings::end_and_cancel_future` for the system's
`unsubscribe` of a whole slot. The kind decides:

- **refund** — only `Staff`, only when the booking's charge row is a
  non-voided NEGATIVE amount: ONE separate `storno` row (rounded once,
  `staff_id`, **`service_id` NULL** — like the manual desk storno, so the
  `class_visit_filter_sql` visit counters and `last_visit_at` never count a
  refund as one more visit), credit + `bookings.refund_transaction_id` in the
  same transaction. Never classified by its note text.
- **binding** — a non-staff cancel of an already CHARGED booking returns
  `CancelResult::AlreadyCharged` (route: 409 `cancel_deadline_passed`); this
  closes the race where the charger charges between the route's deadline
  check and the cancel transaction.
- **resurrection** — the materialiser never re-creates a (user, template,
  date) that has an active row OR a `customer`/`staff`/legacy-NULL
  cancellation. Only an `unsubscribe` cancellation (the system ending a
  slot) may be re-created when the customer subscribes again.

A new cancel site must go through this function with the right kind — a raw
`UPDATE bookings SET cancelled_at` leaves `cancel_kind` NULL, which the
materialiser treats as a permanent opt-out (safe, but wrong for a system
cancel).

## Recurring slots

`db::persistent_bookings::set_subscriptions` (customer `PUT
/api/my/persistent-bookings`) and `end_and_cancel_future` (also the staff
`DELETE`) are shared. Ending a slot cancels only `source='persistent'`,
uncharged bookings whose 09:00 cutoff is still ahead. After saving, the route
runs a full materialiser sweep so the home card shows the bookings at once.
The materialiser does NOT book an occurrence whose cutoff already passed (a
slot created on the class day after 09:00 must not bind + charge a class the
customer never chose) and counts + warns a full class (`skipped_full`).

## 8-week horizon + the staff spinning screen (#385)

`materialiser::WINDOW_DAYS = 56` (offsets `0..=56`, i.e. today's weekday
recurs 9 times) — the owner's 8-week planning horizon, shared with the staff
screen `/staff/spinning` (`spinbike-ui/src/pages/staff_spinning/`): an
occupancy grid of the current week + 7 from ONE `GET /api/classes` (capped
server-side at 62 inclusive days -> 400 `range_too_long`), and a class
detail TABLE (no chips) that reuses the existing booking / persistent /
cancel endpoints. Materialiser tests derive their date counts from the
constant (`MATCHING_DATES`), never a literal.

The staff participants payload carries `phone`, `card_code`, current
`credit`, `charged` and `charged_amount` (the NON-voided linked charge row,
positive; `0.0` = free pass visit). The UI's "Stav" is computed from those
two fields only (`model::charge_state`) — never from the time of day.

## Guests paid by the host (#389, `db::booking_guests`)

Owner ROZHODNUTÉ 2026-10-25 ("Approach A"): a guest is a COUNT on the
host's own booking (`bookings.guests`, V32, CHECK 0..=4 =
`spinbike_core::booking::MAX_GUESTS`), no name, no account.

- **Seats, not rows.** Every capacity read uses `booking_guests::SEATS_SQL`
  (`COALESCE(SUM(1 + guests), 0)`): `create_booking`'s guard,
  `get_booking_count` (`/api/classes` `booked`, the WS broadcast, the staff
  grid), `list_upcoming_for_user`, the materialiser pre-check. A new
  occupancy query that counts `COUNT(*)` rows is a bug.
- **Charge.** The charger writes, besides the host's row, one `visit` row
  per guest — ALWAYS the Spinning price from the host's credit (the host's
  pass covers only the host), each linked from `booking_guest_charges`
  (`transaction_id`; `released_at` + `refund_transaction_id` once given
  back). Attendance (KPI) counts every seat; the host's OWN per-customer
  visit counter (`/api/users/{id}/stats`) excludes the guest rows.
- **Change** = `PUT /api/bookings/{id}/guests` -> `booking_guests::set_guests`:
  customer until the cancel deadline (`customer_may_cancel`, 409
  `guests_deadline_passed`), staff any time; over capacity 409 `class_full`
  (the conditional UPDATE is the authority). After the charge a staff
  increase charges the added seats at once, a decrease refunds them
  (voided seats released first), and a staff cancel refunds host + every
  held guest seat. All refunds go through `booking_cancel::write_refund`
  (storno, `service_id` NULL).
- **Recurring slots never carry guests** — guests live on the dated booking
  only; the materialiser creates bookings with `guests = 0`.
- UI: `components::guest_stepper` (customer card "+ host" while
  `cancel_open`; staff table "Hostia" any time). The reminder says
  "s N hostami ... aj za hosti".

## Charger also skips cancelled class occurrences

Before #382 the charger billed bookings of a class the staff cancelled
(`class_cancellations`). The due query now has `NOT EXISTS (class_cancellations
...)`; the reminder job has the same filter. Keep both in sync.

## Reminder job (`jobs::spinning_reminder`, DAILY_RUN_HOUR = 18)

Per-BOOKING dedupe via `bookings.reminded_at`, stamped only after a successful
push (any stored subscription → push only) or e-mail fallback (no
subscription + address). NOT `push_notify_log` — that per-(user, reason)
cooldown would eat next week's reminder. `startup_tick` sends only after
18:00 (a morning restart must not remind 8 hours early).

## E2E: pick classes by the server's flags

`e2e/tests/spinning-fixtures.ts`: `cancellableSlotWithinTwoWeeks()` (this or
next week — the schedule has a this/next-week switch, `week-next`),
`bookableSlotThisWeek()` (creates + later deactivates a 23:50 template when
the current week has no future class, i.e. Fri–Sun), `seedBooking()` /
`chargeBooking()` via the test-only `/api/test/seed-booking` +
`/api/test/charge-booking`. "The first class of this week" is usually in the
past now and `POST /api/bookings` answers 409 `class_already_started`.

The schedule page now loads `/api/my/persistent-bookings` (session-checked)
for a logged-in customer: a spec that blocks/deletes the customer BEFORE
opening `/schedule` gets logged out on page load, not on its own click
(see `session-invalidation.spec.ts` #277 — block after the page loaded).
