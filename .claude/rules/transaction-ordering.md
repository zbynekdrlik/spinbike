---
paths:
  - "crates/spinbike-server/src/db/transactions.rs"
  - "crates/spinbike-server/src/routes/my_balance.rs"
  - "crates/spinbike-server/src/routes/payments.rs"
---

# `transactions.created_at` has only SECOND precision — `ORDER BY created_at DESC` alone is not deterministic

**#291** (a genuine backend bug, found root-causing a 3x-recurring CI
flake in `txn-note.spec.ts`): the `transactions` table's `created_at`
column defaults to `DEFAULT (datetime('now'))` (`db/migrations.rs`), and
SQLite's `datetime('now')` resolves at **second precision** — two
transactions created within the same wall-clock second get an
**identical** `created_at` string. This is routine, not exotic: a fresh
user's `createUniqueUser`-style initial-credit top-up immediately followed
by its first charge (in tests) or a fast-fingered staff member ringing up
two items back-to-back (in real use) both land in the same second often
enough to matter.

**Any `ORDER BY ... created_at DESC` with no secondary key gives SQLite no
tiebreaker for those rows — their relative order is UNSPECIFIED**, and was
observed flipping between separate CI runs (sometimes the older row
sorted first, sometimes the newer one did). Any code that assumes "first
row = most recently created" (a UI's `.first()` transaction, "most recent
movement", a duplicate-visit lookup picking the latest match) can silently
act on the WRONG row when a tie happens.

**The fix: `ORDER BY t.created_at DESC, t.id DESC`.** The table's `id` is
`INTEGER PRIMARY KEY AUTOINCREMENT` — SQLite guarantees it strictly
increasing and never reused, so it's a safe, deterministic tiebreaker that
always resolves to newest-first, matching the actual intent. All FOUR
currently-affected queries already carry this fix — if you add a NEW
`created_at`-ordered query against `transactions`, add the same `, t.id
DESC` (or the equivalent `, id DESC` when the table isn't aliased) from
the start:

- `db/transactions.rs`: `list_transactions_for_user`,
  `list_transactions_for_user_paginated` (both the cursor and
  no-cursor branches)
- `routes/my_balance.rs`: the customer-facing recent-transactions query
- `routes/payments.rs`: the same-day duplicate-visit lookup

**Testing this deterministically does NOT need real-time timing.**
`db/transactions.rs`'s `same_created_at_transactions_break_ties_by_id_newest_first`
test forces the tie via a raw SQL insert with an explicit `created_at`
literal (bypassing the `datetime('now')` default entirely) — reuse this
pattern for any new same-pattern test rather than trying to race two real
inserts within the same wall-clock second.

## Known dormant gap — the `before` pagination cursor does not account for the tiebreaker

`list_transactions_for_user_paginated`'s `before` cursor filters with
`WHERE t.created_at < ?` (created_at ONLY). If a page boundary ever fell
exactly on a same-second tie, the next page's filter would exclude the
tied row with the smaller `id` (which the `id DESC` ordering now places
SECOND within the tie) — silently dropping it instead of showing it on
either page. **Currently unreachable**: no caller passes `before` today
(`spinbike-ui/src/pages/dashboard/transactions_list.rs` always omits it),
and no E2E test exercises the cursor. If real pagination is ever wired up
here, the cursor needs to become a composite `(created_at, id)` key, not
just `created_at`.
