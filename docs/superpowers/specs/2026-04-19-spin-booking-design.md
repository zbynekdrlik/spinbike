# Spin Class Booking — Design Spec

**Date:** 2026-04-19
**Version introduced:** 0.6.0
**Supersedes:** Walk-in form in `staff_dashboard.rs` (still accessible, but card-driven flow becomes primary).

## Goal

Let staff book a **card owner** into any of the 4 weekly spin classes in one flow, support **persistent (recurring) bookings**, and charge **automatically 4 hours before class** (pass-free, or credit deducted — negative allowed).

## The 4 classes (seeded on deploy)

| Weekday | Time  | Instructor | Duration | Capacity |
|---------|-------|------------|----------|----------|
| Mon     | 18:00 | Stevo      | 60 min   | 19       |
| Tue     | 18:00 | Vlada      | 60 min   | 19       |
| Wed     | 18:00 | Stevo      | 60 min   | 19       |
| Thu     | 18:00 | Vlada      | 60 min   | 19       |

Seed is idempotent: skip if a template already exists for the same `(weekday, start_time)`.

## User-visible behavior

### Staff card page (`/staff`, after picking a card)

New **Upcoming classes** panel between the existing credit/pass block and the action row:

```
UPCOMING CLASSES
─────────────────────────────────────────────────────
Mon 20 Apr · Stevo · 18:00 · 12/19   [ BOOK ]
Tue 21 Apr · Vlada · 18:00 · 19/19   [ FULL ]
Wed 22 Apr · Stevo · 18:00 ·  5/19   [ AUTO — cancel this week ]
Thu 23 Apr · Vlada · 18:00 ·  8/19   [ BOOKED — cancel ]
Mon 27 Apr · Stevo · 18:00 ·  0/19   [ AUTO ]
…
```

- Shows the next **14 days** of classes (≈ 8 rows given 4/week).
- Button states:
  - `BOOK` — seat free, create one-off booking.
  - `BOOKED — cancel` — a one-off booking exists for this card.
  - `AUTO` — seat is held by a persistent booking; label doubles as the "cancel this week" button.
  - `FULL` — all 19 seats taken (not for this card).
  - `PAST` — class has already started (read-only).
- Cancel on a BOOKED row removes that one booking.
- Cancel this week on an AUTO row inserts a per-occurrence skip so the seat is freed; the persistent booking keeps generating future seats.

### Persistent booking toggles

Below the Upcoming classes panel:

```
PERSISTENT BOOKING (auto-book every week)
Mon – Stevo 18:00  [ ON ]
Tue – Vlada 18:00  [ OFF ]
Wed – Stevo 18:00  [ ON ]
Thu – Vlada 18:00  [ OFF ]
```

- Turning a toggle ON immediately materialises future bookings (next 14 days' worth — that's the visible window and also the work unit for the 4h charger).
- Turning OFF deletes future (start-time > now) bookings flagged `source = 'persistent'` for that (card, template) **only if `charged_at IS NULL`**. Already-charged or past bookings are preserved (money already moved, history stays intact).
- If an ON toggle can't materialise a booking because a class is already full, the persistent booking is still created but that specific occurrence shows `FULL` — it'll grab any future seat that opens up at the next materialisation sweep (once per day) or when the toggle is flipped.

### Customer schedule page (unchanged layout)

- Logged-in card holders keep the existing `BOOK / BOOKED / FULL / CANCELLED` buttons on `/`.
- New: on AUTO rows (seat held by their persistent booking), the button reads `AUTO — skip this week`. Click cancels that one occurrence, exactly like the staff flow.
- Persistent toggles are **staff-only** (customers don't self-subscribe — matches the "client calls to cancel" workflow).

## Payment rules

Single rule, applied by a background charger **4 hours before class start**:

| Card state at T−4h                      | Action                                                        |
|------------------------------------------|---------------------------------------------------------------|
| Has active monthly pass                  | Log a `visit` transaction (amount = 0), decrement pass visits |
| No pass, credit ≥ Spinning price         | Log `visit` (amount = service price), debit credit            |
| No pass, insufficient credit             | **Still charge.** Credit goes negative. Staff sees red balance next time. |

- `Spinning` service price comes from `services.default_price` (already admin-editable, currently €5). No new config.
- Bookings cancelled before T−4h move no money.
- Bookings cancelled **after** T−4h keep the already-logged transaction (visit was "used"). The cancel still frees the seat so it could be re-sold, but that's on staff.
- Idempotency: the charger marks each booking with `charged_at` + `charge_transaction_id`; the sweep is safe to run multiple times.

## Data model

### Existing tables — changes

`class_templates` — bump `capacity` default to 19 for new inserts only. The seed migration sets the 4 templates to 19; existing rows (if any) are not touched.

`bookings` — add columns:
- `source TEXT NOT NULL DEFAULT 'manual'` — `'manual'` or `'persistent'`
- `charged_at TIMESTAMP NULL`
- `charge_transaction_id INTEGER NULL REFERENCES transactions(id)`

`bookings` currently has `user_id` only. Add `card_id INTEGER NOT NULL REFERENCES cards(id)` and backfill each existing row from its user's card (`SELECT id FROM cards WHERE user_id = bookings.user_id LIMIT 1`). Keep `user_id` for backwards compatibility with the existing customer-schedule endpoints. Rationale: a booking is tied to a card (that's what gets charged), not an abstract user; legacy cards without a linked user can still be booked via the card.

### New table

```sql
CREATE TABLE persistent_bookings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    card_id         INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    template_id     INTEGER NOT NULL REFERENCES class_templates(id) ON DELETE CASCADE,
    created_at      TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ended_at        TIMESTAMP NULL,
    UNIQUE(card_id, template_id)  -- one persistent subscription per (card, class)
);
```

## API

All new routes under `/api` behind existing staff/customer auth as appropriate.

### Staff routes

- `GET  /api/cards/:id/upcoming-classes?days=14` → list of occurrences with per-row state (`free`, `booked`, `auto`, `full`, `past`) and a `booking_id` when applicable.
- `POST /api/bookings` — already exists; extend to accept `{ card_id, template_id, date }` and default `source = 'manual'`.
- `DELETE /api/bookings/:id` — already exists; no change.
- `POST /api/cards/:id/persistent-bookings` → `{ template_id }`. Creates row + materialises next 14 days. Returns list of created booking ids.
- `DELETE /api/cards/:id/persistent-bookings/:template_id` → sets `ended_at=now`, deletes future bookings with `source='persistent'` for that (card, template) whose start is in the future.
- `GET  /api/cards/:id/persistent-bookings` → list (for rendering toggles).

### Customer routes

- `DELETE /api/bookings/:id` — already allows a customer to cancel their own booking. Works for `source='persistent'` too, since cancelling one occurrence doesn't touch the `persistent_bookings` row.

## Background charger

- Implementation: a `tokio::spawn` loop on server startup that ticks every **60 seconds**.
- Each tick: `SELECT * FROM bookings WHERE cancelled_at IS NULL AND charged_at IS NULL AND datetime(date || ' ' || template.start_time, '-4 hours') <= datetime('now')` — join templates for the start time, left-join passes for active-pass lookup.
- For each row:
  - Active pass → insert `transactions` row with amount 0, action `visit`, link it via `charge_transaction_id`, decrement `passes.visits_remaining`.
  - No pass → insert `transactions` row with amount = `service.default_price` (name=`Spinning`), debit `cards.credit` (allowed to go negative), link via `charge_transaction_id`.
  - Set `bookings.charged_at = now`.
- Transaction per row (not per tick) so one failure doesn't block others.
- Config: interval is a `const`, not env-driven. Tests override with a shorter interval via a dedicated test hook.

## Persistent materialisation sweep

A second ticker (every **60 minutes**) that, for every active `persistent_bookings` row, ensures a `bookings` row exists for each template-weekday occurrence within the next 14 days whose start is in the future. Skips if capacity is full (row is not created; caught next sweep). Also runs immediately on server start.

Rationale: 14-day window covers the visible staff panel; hourly sweep picks up seats that free up after cancellations.

## Initial seed migration (V2)

```sql
-- 0002_seed_spin_classes.sql (idempotent)
INSERT INTO instructors (name, active)
SELECT 'Stevo', 1 WHERE NOT EXISTS (SELECT 1 FROM instructors WHERE name='Stevo');
INSERT INTO instructors (name, active)
SELECT 'Vlada', 1 WHERE NOT EXISTS (SELECT 1 FROM instructors WHERE name='Vlada');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 0, '18:00', 60, (SELECT id FROM instructors WHERE name='Stevo'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=0 AND start_time='18:00');
-- same for Tue(1,Vlada), Wed(2,Stevo), Thu(3,Vlada)
```

## Testing

### Unit (Rust, `cargo-mutants` applies)

- `persistent_booking_materialises_next_14_days` — create persistent, assert N `bookings` rows with `source='persistent'`.
- `persistent_booking_end_removes_future_only` — end it; rows in the past remain, rows in the future are gone.
- `persistent_booking_skips_full_class` — pre-fill class to 19 bookings, create persistent, assert that occurrence is NOT created; other dates still work.
- `skip_one_week_keeps_persistent` — cancel one occurrence, assert persistent row unchanged and next week still has a booking.
- `charger_free_when_pass_active` — active pass, run charger at T−4h, assert amount=0 and `visits_remaining` decremented.
- `charger_deducts_credit_without_pass` — no pass, credit=10, price=5, assert credit=5 after.
- `charger_allows_negative_credit` — no pass, credit=2, price=5, assert credit=-3 after.
- `charger_idempotent` — run twice, assert only one transaction created.
- `charger_skips_cancelled_bookings` — cancel then run, assert no charge.
- `capacity_still_enforced_at_19` — 20th manual booking rejected.

### Integration / route tests

- `POST /api/cards/:id/persistent-bookings` → 201 and returns created booking ids, upcoming-classes endpoint reflects AUTO state.
- `DELETE /api/cards/:id/persistent-bookings/:template_id` → future bookings gone, past preserved, toggle is OFF.
- `DELETE /api/bookings/:id` on a persistent occurrence → only that one gone.

### Playwright E2E (`e2e/tests/spin-booking.spec.ts`, new)

Given how fragile our E2E has been on week boundaries, the test uses fixed offsets: `today + 1..7` so it runs any day of the week.

1. **Staff books card for one class.** Search card → Upcoming classes panel appears → click BOOK on next Monday → row shows BOOKED → public schedule (logged out) shows the `class-spots` counter incremented.
2. **Staff flips persistent ON.** Toggle Mon ON → future Mondays show AUTO → database has `persistent_bookings` row (asserted via a read-only test endpoint gated by test-mode, or via a returned JSON from the POST).
3. **Staff skips one occurrence.** On an AUTO row, click "cancel this week" → that row goes back to BOOK → next Monday still AUTO.
4. **Staff flips persistent OFF.** Toggle Mon OFF → all future Mondays go back to BOOK.
5. **Capacity full.** Seed 19 bookings for a template+date (fixture endpoint) → staff sees FULL, button disabled.
6. **Clean browser console.** All tests assert `consoleMessages == []` per project rule.

Charger logic is **not** tested in Playwright (time-sensitive). It's covered by unit tests with injected clock.

## CI / deploy

- `ci-push-discipline`: `cargo fmt --all --check` before pushing.
- Version bump (0.5.0 → 0.6.0) is the first commit.
- Two GitHub workflows (lint, test, E2E, mutation, deploy) already in place; nothing new.
- Post-deploy verification: open `https://spinbike.newlevel.media/staff`, log in, pick a known card, confirm Upcoming classes panel renders and BOOK flow works. Check browser console is clean.

## Out of scope

- Customer-facing persistent booking toggle (staff-only as decided).
- Multi-week navigation on the public schedule page.
- Class end-date / series termination.
- Waitlist / overbooking.
- Per-class price override (everyone pays `Spinning` service price).
- Instructor names on class cards for the public schedule (that's separate visual polish).

## Risks

- **Charger running mid-migration.** Solved by starting the charger only after the migration runner completes (it already runs on startup, before route mounting).
- **Clock skew on the deploy host.** Accept risk — SQLite `datetime('now')` + system clock is authoritative; we don't sync with NTP here. A 30s skew at the 4h mark has no business impact.
- **Seed migration running on an install that hand-created `Stevo`/`Vlada` with different casing.** The `WHERE NOT EXISTS` is name-exact; worst case is a duplicate row. Acceptable, staff can merge via admin.
