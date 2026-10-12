# Spinning — recurring bookings, day-before reminder, binding 09:00 charge (#382)

Owner decisions are recorded as `ROZHODNUTÉ` comments on issue #382 (2026-10-12). This spec is the single source for implementation.

## Context

~90 % of spinning customers come regularly and only report when they will NOT come. Spinning runs only outside summer and has fewer customers than the gym, and spinners rarely use the gym. The app must therefore not push spinning onto gym-only customers.

Schedule after #381 (live v0.15.0-dev.173): Mon/Wed 18:00 Stopa, Tue/Thu 18:30 Vlada, capacity 19. Bikes are NOT numbered (decision: capacity only).

Already in the code base:

- `persistent_bookings(user_id, template_id, created_at, ended_at)` and `jobs/materialiser.rs` materialise recurring bookings into `bookings` 14 days ahead, hourly (0 rows on prod; route `/api/users/{id}/persistent-bookings` exists).
- `jobs/charger.rs` charges every non-cancelled booking 4 h before start (charges reservations, not attendance; pass holders get a 0 € visit row).
- `cancel_booking` has no deadline and never refunds.
- `jobs/notifications.rs` provides daily push with e-mail fallback; the `push_notify_log` reason CHECK only allows `low_credit`/`pass_expiring`.

## Rules

### R1 — binding charge at 09:00 on class day
The charger charges a booking once `now >= 09:00 Europe/Bratislava on booking.date` (replacing the 4-hours-before rule). A booking created on the class day after 09:00 is charged immediately (on the next charger tick). Pass holders keep the existing 0 € visit row. Use the shared `crate::util` Bratislava helpers (`.claude/rules/bratislava-tz.md`), never `chrono::Local`.

### R2 — customer cancel deadline
A customer may cancel their own booking for any future date (a week ahead is fine) up to 09:00 on the class day. After that, `DELETE /api/bookings/{id}` by the owner returns 409 with a new `ErrorCode` (`CancelDeadlinePassed`, Slovak unaccented text such as "Odhlasit sa da len do 9:00 v den spinningu").

### R3 — staff cancel with refund
Staff/admin may cancel at any time. If the booking was already charged (`charged_at` set, non-zero amount), cancelling writes a separate reversing ledger row (positive amount, linked to the booking, staff id recorded) and credits the user. The refund is rounded once and reused for both the credit write and the ledger row (`.claude/rules/money-rounding.md`). A 0 € pass visit needs no refund row.

### R4 — no past bookings
`create_booking` rejects a date before today (Bratislava), or today when the class has already started, with 409 and a new ErrorCode.

### R5 — recurring self-service
A customer manages their own weekly slots through a new customer-scoped route (e.g. `GET/PUT /api/my/persistent-bookings`): the response lists the active templates with a `subscribed` flag; PUT sets the subscribed template set. Staff keep the existing per-user route. Ending a subscription cancels that user's FUTURE uncharged bookings created by it (`source='persistent'`). The materialiser must NEVER re-create a booking for a (user, template, date) that the user or staff cancelled, and must skip dates in `class_cancellations`. When a class is full, the materialiser skips that date for that user and logs a warning. Customer cancelling one date never ends the subscription.

### R6 — day-before reminder at 18:00
A new daily job at 18:00 Europe/Bratislava (`.claude/rules/daily-job-scheduling.md`) sends, for every active booking for tomorrow, a push (e-mail fallback when the user has no subscription), for example "Zajtra mas spinning o 18:30 (Vlada). Ak nemozes prist, odhlas sa do 9:00, inak sa kredit strhne." (unaccented). Dedupe per booking with a new `bookings.reminded_at` column, stamped only after a successful send. Do not reuse the per-(user, reason) `push_notify_log` cooldown. A startup tick must not double-send.

### R7 — spinner visibility
A user is a *spinner* when they have an active `persistent_bookings` row OR any spinning visit/booking within the last 365 days (spinning `transactions` via `services.kind`, see `.claude/rules/service-kind.md`, or non-cancelled `bookings`). 365, not 90, because spinning pauses over summer. Expose it as `is_spinner` on the customer's own profile/balance response.

- Spinner: the customer home (`/my/balance`) shows a "Moje spinningy" card at the TOP with the next bookings (date, time, instructor, cancel button respecting R2) and a link to manage regular slots (R5).
- Everyone: a discreet "Spinning" entry in the customer menu leading to the schedule/booking page (today customers have no menu link to `/schedule`).
- Non-spinners see no spinning card on home.

### R8 — staff view
The staff participant list marks recurring bookings ("pravidelne") and offers cancel (with refund when charged, R3). It keeps the #379 `.participant-chip` contrast.

## Data changes (incremental migrations only)
- `bookings.reminded_at TEXT NULL`.
- Whatever the refund linkage needs (e.g. a `refund_transaction_id` on `bookings`, or reuse a transaction note/type plus `staff_id`), never classified by note text.
- No change to `class_templates`/`instructors`.

## Testing
- Server tests with an injected "now" for R1 (before/after 09:00, booking created after 09:00), R2 (customer before/after deadline, a week ahead), R3 (refund row and credit, pass-holder 0 € no refund), R4, R5 (materialiser does not resurrect a cancelled date, respects class cancellations, full class), R6 (one reminder per booking, startup tick idempotent, e-mail fallback), R7 (365-day boundary, recurring flag).
- Playwright: customer subscribes to a weekly slot and sees the bookings on the home card; cancels one date; spinner vs non-spinner home; staff cancel with refund; console clean.
- Bug-class fixes inside this work follow RED-before-GREEN.

## Out of scope
- Bike numbers (owner: capacity only).
- Attendance tracking (no-shows still pay by design).
- Admin Templates name picker (noted on #382; may ride along if small).
