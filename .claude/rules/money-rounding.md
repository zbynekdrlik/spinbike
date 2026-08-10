---
paths:
  - "crates/spinbike-server/src/db/users.rs"
  - "crates/spinbike-server/src/routes/payments.rs"
  - "crates/spinbike-server/src/routes/users.rs"
  - "crates/spinbike-server/src/routes/door.rs"
  - "crates/spinbike-server/src/routes/admin.rs"
  - "crates/spinbike-server/src/routes/charger.rs"
---

# Money writes — round ONCE, reuse EVERYWHERE (#325/#326)

Every site that mutates `users.credit` or inserts into `transactions.amount`
must round the money value to cents **exactly once**, as close as possible
to where it enters the operation, and reuse that SAME rounded value for
every write derived from it (the SQL `UPDATE`, the ledger `INSERT`, and any
JSON response field). Two independent roundings of the same logical amount
— even if each one individually rounds correctly — can still disagree with
each other and leave `users.credit` and the `transactions` ledger out of
sync. That was the exact shape of both #325 (`topup_user` rounded the
credit delta via `db::update_credit` but inserted the RAW amount into the
ledger) and #326 (`door.rs`'s no-pass charge path rounded nothing at all,
anywhere on its path).

**The established pattern** (`db::users::round_cents(value) -> f64`,
`(value * 100.0).round() / 100.0`):

```rust
let amount = users::round_cents(body.amount); // ONCE, right after validation
// ... reuse `amount` for the SQL UPDATE, the ledger INSERT, and the response.
```

For SQL, prefer ALSO wrapping the UPDATE itself in `ROUND(credit ± ?, 2)`
(not just rounding the Rust-side operand) — defense-in-depth against float
drift that might already be sitting in a pre-existing DB value (e.g. a
`services.default_price` set by `admin.rs`'s `create_service`/
`update_service`, which write with no rounding guarantee).

**Call sites currently following this convention:** `payments.rs`
(`charge`/`storno`/`sell_pass`), `db::users::update_credit`, `users.rs`'s
`topup_user` (#325), `door.rs`'s no-pass single-entry charge (#326). Any
NEW money-mutating write site should follow the same shape — round once,
right after the value enters the function, reuse it everywhere.

## Before assuming an existing drift needs a backfill migration — CHECK PROD FIRST

When a rounding bug like this is found, don't assume `users.credit` /
`transactions.amount` already contain drifted values in prod — CHECK,
read-only, before proposing any backfill:

```bash
sqlite3 /opt/spinbike/prod/spinbike.db \
  "SELECT id, credit FROM users WHERE ABS(credit - ROUND(credit,2)) > 0.0001;"
sqlite3 /opt/spinbike/prod/spinbike.db \
  "SELECT id, amount FROM transactions WHERE ABS(amount - ROUND(amount,2)) > 0.0001 LIMIT 50;"
```

If both return zero rows, the bug is real but hasn't yet produced
observable drift — ship a forward-fix only, no migration needed, and say so
explicitly with the query output in the fix's design comment. Only if real
drifted rows exist do you escalate the finding (with the exact rows) rather
than silently "fixing" a live customer's balance.
