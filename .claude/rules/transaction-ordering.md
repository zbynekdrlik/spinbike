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

## Fixed — the `before` pagination cursor now carries the tiebreaker (#331)

`list_transactions_for_user_paginated`'s `before` cursor used to filter
with `WHERE t.created_at < ?` (created_at ONLY), which had no tiebreaker
against a same-second tie — this is now fixed. The cursor is parsed by the
shared `crate::db::reports::parse_before_cursor` (composite
`"<created_at>|<id>"` wire format, same helper `db/reports.rs`'s day/range
cursors already used) and the query filters on
`(t.created_at < ? OR (t.created_at = ? AND t.id < ?))`, matching the
`(created_at DESC, id DESC)` ordering exactly. A cursor with no `|`
(malformed, or the old pre-#331 plain-timestamp shape) parses as absent and
falls back to no cursor filter — same fallback `db/reports.rs` uses,
never a partial/garbage filter.

**Any NEW paginated query against `transactions` (or any other
`created_at`-ordered table) should reuse `parse_before_cursor` and this
same OR-predicate shape** rather than inventing a new cursor encoding — it
is now the ONE established pattern, used by `db/reports.rs`'s day/range
cursors AND `db/transactions.rs`'s user-history cursor.

Still true: no real caller passes `before` to this endpoint today
(`spinbike-ui/src/pages/dashboard/transactions_list.rs` always omits it),
so this remains dormant in production traffic — but the cursor CONTRACT is
now correct for whenever real pagination gets wired up here.

Test: `db/transactions.rs`'s
`paginated_before_cursor_composite_key_excludes_only_the_cursor_row_on_ties`
forces a 3-way same-second tie via raw SQL inserts and asserts the
composite cursor built from the newest tied row excludes exactly that row
on the next page (not the whole tied group, and not a duplicate of it) —
same "raw SQL insert with an explicit `created_at` literal" test pattern
as `same_created_at_transactions_break_ties_by_id_newest_first` above.
