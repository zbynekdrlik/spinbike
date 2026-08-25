---
paths:
  - "crates/spinbike-server/src/routes/door.rs"
  - "crates/spinbike-server/src/jobs/charger.rs"
  - "crates/spinbike-server/src/db/users.rs"
  - "crates/spinbike-server/tests/door_route.rs"
---

# Expired monthly pass auto-renews on the next visit (#365)

**An expired-pass customer's next visit no longer bills a single entry — it
auto-renews the pass.** Owner decision #365 (variant b): when a customer whose
monthly pass has lapsed makes their NEXT real visit, instead of a one-off
single-entry charge they get a fresh monthly pass at the price of their LAST
one and their credit goes negative. This deliberately removed the old
single-entry charge the owner used to delete by hand after re-selling a pass.
There is NO daily job and NO per-user switch (a customer who stops coming never
accrues debt) — the logic lives at the two visit-charge sites.

## The shared helper — `db::users::auto_renew_pass`

Both charge sites call **one** helper (never duplicate the money-write):

```rust
db::users::auto_renew_pass(conn: &mut SqliteConnection, user_id, anchor_day: NaiveDate)
    -> Result<Option<f64>>
```

- Runs inside the CALLER's open transaction (`&mut *tx`) so pass-issue + the
  caller's visit row commit/roll back atomically.
- Resolves the last non-voided pass via the canonical `user_active_pass` view
  (V18) joined to `transactions.amount` for its own price — NEVER hand-rolled
  pass SQL. A VOIDED pass is excluded by the view → no renewal falls out for
  free. Price = `round_cents(ABS(amount))`, rounded ONCE (money-rounding.md).
- Issues a row identical to `sell_pass` EXCEPT **`staff_id = NULL`** +
  `note = 'auto-obnova'` (the constant `AUTO_RENEW_NOTE`). `staff_id NULL` is the
  MACHINE distinguisher from a desk sale (sell_pass always records the selling
  staff's id); the note is a human label only (#328). This is what stops the
  client app (#357) claiming staff recorded it.
- `valid_until = anchor_day + 1 month` (chrono `Months`, clamps 31 Jan → 28/29
  Feb). Anchor is the VISIT day: `today_bratislava()` at the door,
  the booking `date` in the charger (both gym-local — never `chrono::Local`).
- Returns `Ok(None)` when the customer NEVER held a pass → the caller keeps its
  existing single-entry / Spinning charge (the boundary implied by "za cenu
  poslednej predanej" — no previous price, nothing to renew at).
- A last pass of 0 € renews at 0 € — deliberate barter (#342), asserted in a
  named test, never "fixed".

## The two auto-renewal GATES — both must hold, both inside the helper (#372)

Owner decision (#372, prod incident 2026-08-25: a 2020 pass auto-renewed on a
door press). Auto-renewal is for a CONTINUING monthly customer only, so the
helper renews ONLY when BOTH gates pass (else `Ok(None)` → the caller's normal
single-entry / Spinning charge). Both live in `auto_renew_pass` so ONE place
covers door.rs AND charger.rs; both run BEFORE any write.

1. **Recency ≤ 31 days (INCLUSIVE).** Renew only if the last pass expired at
   most 31 days before the visit: `(anchor_day - valid_until).num_days() <= 31`.
   Exactly 31 → renews; 32 → does not. `anchor_day` is gym-local
   (`today_bratislava()` at the door, the booking date in the charger) and
   `valid_until` is a bare gym-local date, so it's a pure day-count — never
   `chrono::Local` (bratislava-tz.md).
2. **No paid class-visit since expiry.** If even one **paid class-visit** exists
   AFTER the pass expired, the customer switched to per-visit mode → no renewal.
   A **paid class-visit** uses the codebase-canonical shape (the same one
   `db/reports.rs` / `routes/payments.rs` count): `amount < 0` AND
   `valid_until IS NULL` AND `deleted_at IS NULL` AND a service whose
   `kind ∈ CLASS_VISIT_KINDS` (Fitness/Spinning, via `class_visit_filter_sql` —
   never `name_en`/`name_sk`, service-kind.md), recorded as EITHER a door
   single-entry `action='charge'` (`routes/door.rs`) OR a charger Spinning
   single-visit `action='visit'` (`jobs/charger.rs`) — so gate 2 matches
   `action IN ('charge','visit')`, NOT `action='charge'` alone. The owner's #372
   note said "action='charge'"; that misses the charger's `action='visit'` paid
   Spinning row, which is exactly a per-visit payment and MUST block — widened to
   the two-action form to match the owner's intent ("čo i len jeden platený
   vstup"). What does NOT count / does NOT block: a VOIDED row (`deleted_at`
   set), a non-class (bar/generic) charge, a €0 door/pass-covered `visit` row
   (`amount = 0`), and any class-visit BEFORE `valid_until`. "After expiry" is
   compared via the UTC instant of gym-local midnight of the day AFTER
   `valid_until` (`bratislava_day_range_utc(valid_until + 1).0`, formatted
   `%Y-%m-%d %H:%M:%S` like every other `created_at` bind) — never
   `date(created_at)` (UTC, ~2h off near midnight, the #205 bug class).

**The current entry never counts against itself**: both call sites insert the
triggering visit/charge only AFTER `auto_renew_pass` returns (door.rs: the
`match` precedes the INSERT; charger.rs: the INSERT follows the helper call).
0 € barter passes still auto-renew under the SAME two gates. Manual desk sale is
unaffected — the gates constrain only the AUTOMATIC renewal. Tests: helper-level
unit tests in `db/users.rs` (`auto_renew_skips_*` / `auto_renew_renews_*` /
`auto_renew_recency_boundary_is_inclusive_at_31_days` / `auto_renew_ignores_*`),
plus integration coverage at both call sites
(`first_of_day_pass_expired_over_31_days_ago_does_not_auto_renew` in
`tests/door_route.rs`, `charger_does_not_auto_renew_pass_expired_over_31_days_ago`
in `jobs/charger.rs`).

## Wiring at the two sites — don't double-debit

- **door.rs** (`n==0`, no active pass): `Some(new_credit)` → the door row becomes
  a €0 pass-covered `visit` (NOT a single-entry charge), `credit = new_credit`,
  `charged = true` (credit did drop by the pass price). `None` → single-entry
  charge unchanged.
- **charger.rs** (`has_pass == false`): a `(amount, renewed)` tuple. `renewed`
  gates the Spinning debit — `if !has_pass && !renewed` — so a renewed visit is
  €0 and the pass price (already debited inside the helper) is never charged
  twice. Three cases: active pass → €0 no debit; renewed → €0 no Spinning debit;
  never-held → Spinning charge unchanged.

## Consequences to remember when touching these files

- The auto-issued pass carries `valid_until`, so it already counts in the
  `passes_sold` KPI (`db/reports.rs`) with no extra work — there is a test
  pinning this (`auto_renewed_pass_counts_in_passes_sold_kpi`).
- The client `/my/balance` summary card lights `.card-credit--negative` when
  `credit < 0` (strict `<`, a €0 balance is settled) — mirrors the desk-side
  `.card-balance--negative` (#49). Don't re-implement the desk one.
- Door tests for consecutive presses still need DISTINCT users (ewelink-ack.md
  10s limit) — the auto-renewal tests each use a fresh `TestApp` + one press.
