---
paths:
  - "crates/spinbike-server/src/jobs/pass_renewal.rs"
  - "crates/spinbike-server/src/db/users.rs"
  - "crates/spinbike-server/src/routes/door.rs"
  - "crates/spinbike-server/src/jobs/charger.rs"
  - "crates/spinbike-server/src/routes/users.rs"
  - "crates/spinbike-server/tests/door_route.rs"
  - "spinbike-ui/src/pages/dashboard/edit_info_form.rs"
---

# Monthly-pass auto-renewal: per-user flag + daily contiguous job (#374)

**Owner decision #374 (2026-08-27) REPLACED the visit-triggered mechanism
(#365/#372) entirely.** Auto-renewal is now governed by ONE explicit rule — a
per-user opt-in flag — and is driven by the END of the previous month via a
daily job, never by the customer's next visit. The old `db::users::auto_renew_pass`
helper, its two #372 gates (recency ≤31 days + no-paid-visit-since), and its
calls in `routes/door.rs` + `jobs/charger.rs` are all GONE.

## The visit sites no longer renew — they charge again

`routes/door.rs` (a door press with no active pass) and `jobs/charger.rs` (a
class charge with no covering pass) fall back to the plain **single-entry /
Spinning charge** for an expired-pass customer — exactly as they did before
#365. Do NOT re-introduce any renewal at these sites: continuous renewal is the
daily job's job. The regression tests `first_of_day_expired_pass_charges_single_entry_no_renewal`
(`tests/door_route.rs`) and `charger_charges_spinning_for_expired_pass_no_renewal`
(`jobs/charger.rs`) pin this.

## The flag — `users.auto_renew_pass` (migration V28)

`INTEGER NOT NULL DEFAULT 0`. OFF for everyone by default; staff enables it per
customer. Toggled via the EXISTING `PUT /api/users/{id}` (`UpdateUserRequest.
auto_renew_pass`), guarded **staff-or-admin** (reuses `ErrorCode::StaffRequired`
— unlike `allow_self_entry`, which is admin-only). A customer may NEVER set it
on their own row (the gym auto-bills them). DB helper:
`db::users::update_user_auto_renew_pass`. Threaded through `UserRow`/
`UserRowWithPass` → `UserResponse` → UI `CardInfo`; the checkbox
("Automaticke predlzovanie permanentky") lives in `edit_info_form.rs`, visible
to staff AND admin for a CUSTOMER target only (mirrors `allow_self_entry`'s
customer-only + only-when-changed payload pattern).

## The daily job — `jobs::pass_renewal`

Scheduled at `DAILY_RUN_HOUR = 5` (Europe/Bratislava) via `jobs::spawn_daily_job`
+ a startup tick in `bin/server.rs` (`daily-job-scheduling.md`; before the 09:00
`notifications` job so a renewal suppresses that user's redundant "pass expiring"
push the same morning). `tick_as_of(pool, push, today)` selects candidates via
the canonical `user_active_pass` view (V18):

```
auto_renew_pass = 1  AND  deleted_at IS NULL  AND  blocked = 0
AND date(newest non-voided pass.valid_until) < today
```

The JOIN excludes users who never held a pass; the `< today` filter excludes
users whose pass still covers today (idempotency). Per candidate it calls the
money-write and fires a best-effort push.

## The money-write — `db::users::renew_expired_pass(pool, user_id, today)`

Returns `Some(Renewal)` or `None` (no pass history, or newest pass still covers
today — a defensive idempotency re-guard). Issues a pass row identical to a
manual `sell_pass` EXCEPT `staff_id = NULL` + `note = AUTO_RENEW_NOTE`
("auto-obnova") — the SAME machine distinguisher from a desk sale (#357, #328:
never classify by note text). Price = `round_cents(ABS(last pass amount))`,
rounded ONCE and reused for the credit debit + ledger row (`money-rounding.md`).
Debit runs even into NEGATIVE credit — NO credit gate (owner decision #374). A
0 € barter pass renews at 0 € (#342 — asserted in a named test, never "fixed").
Debit + insert run in ONE transaction.

### Continuity — `renewal_valid_until(last_valid_until, today)` (pure)

The schema stores only `valid_until`, and coverage everywhere (door, charger,
my_balance, view V18) is decided by `valid_until` alone — so continuity is
expressed purely through the new END date, never a `valid_from` column:

- **Lapse ≤ `CONTIGUITY_TOLERANCE_DAYS` (3):** CONTIGUOUS — the new pass starts
  the day AFTER the old one ended (`last_valid_until + 1 day`), `+ 1 month`. The
  customer keeps unbroken coverage bridging a small gap (weekend, short outage).
- **Bigger gap** (long lapse, or the flag flipped on a long-dead pass): starts
  FRESH from `today`, `+ 1 month`. NO back-dated months, NO retro debit — "a
  pass from now", not an invoice for the past.

`+ 1 month` is chrono `Months::new(1)` (clamps 31 Jan → 28/29 Feb). Anchor dates
are gym-local (`today_bratislava()` in prod) — never `chrono::Local`
(`bratislava-tz.md`; a source-invariant test pins it in `pass_renewal.rs`).

### Idempotency invariants (against the #372 bug class — 17 years of real data)

- Only expired-pass users are selected; a renewal moves `valid_until` to
  `>= today`, so a second run the same day (or a restart startup tick) renews
  that user again NEVER — **max one renewal per user per run**, never a chain of
  months, never a double debit.
- Deleted / blocked users skipped (no debit to a dead/blocked account). A
  flagged user with no pass history is skipped (no price, no continuity — the
  first sale is a manual desk sale).

## The push notification

After each renewal the job calls `notifications::send_to_subscriptions_for_user`
(extracted from `evaluate_reason`, behavior-preserving) — push-only, NO anti-spam
ledger and NO e-mail fallback (a renewal fires at most ~once/month per user, so
the event itself is the throttle). A user with no subscription simply gets
nothing, without error; a delivery failure NEVER rolls back the committed
renewal. Text (Slovak, UNACCENTED, `render_renewal_notification`):
`"Vasa permanentka bola predlzena do <DD.MM.RRRR>. Aktualny kredit: <X> EUR"`.

## Consequences to remember

- The auto-issued pass carries `valid_until`, so it already counts in the
  `passes_sold` KPI (`db/reports.rs`) with no extra work — same INSERT shape as
  a manual sale.
- The client `/my/balance` summary lights `.card-credit--negative` when
  `credit < 0` (E2E: `client-negative-balance-highlight.spec.ts`) — a renewal
  debit into negative surfaces there just like any other charge.
