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
