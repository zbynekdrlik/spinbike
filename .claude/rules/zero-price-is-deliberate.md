---
paths:
  - "crates/spinbike-server/src/routes/payments.rs"
  - "crates/spinbike-server/src/routes/users.rs"
  - "crates/spinbike-server/src/routes/admin.rs"
  - "spinbike-ui/src/pages/dashboard/action_form.rs"
---

# A pass sold for 0 € is DELIBERATE — never "fix" it

**Owner decision, reaffirmed 2026-08-11. Do not re-open, do not re-ask, do not
file it as a bug.**

Selling a pass at price `0` is a supported business flow, not a validation gap.
It is how the owner issues a card to someone who has settled up **in kind**
(barter — work, goods, a service traded against gym access) rather than with
money. The zero is the honest ledger entry for "paid, but not in cash".

## What this means in code

- `sell-pass` accepts `price == 0` on BOTH the client and the server. That
  asymmetry with the plain top-up/charge branch (which rejects `0`) is
  **intended**, not an oversight. The two flows are not the same thing: a
  top-up of 0 € is always a typo, a pass of 0 € is a real transaction.
- Only a NEGATIVE price is rejected — a negative price would *credit* the
  customer on every visit (see `.claude/rules/money-rounding.md` and the
  `admin.rs` service-price boundary, #343).
- The same reasoning already applies to a service whose `default_price` is `0`
  (a deliberately free service). `admin.rs` rejects negative and permits zero
  for exactly this reason; `crates/spinbike-server/tests/admin_routes.rs` pins
  it with `create_service_with_zero_price_accepted` and
  `update_service_with_zero_price_accepted` — those tests exist to stop a
  future "tightening" from silently removing the capability, and one of them
  was added specifically to kill a surviving mutant that would have.

## Why this file exists

This decision was made in conversation, was never written down anywhere in the
repo, and so came back as a "bug" report (#342) with a question the owner had
already answered — which he found, reasonably, infuriating. A decision that
lives only in a chat transcript does not survive; it has to live next to the
code it governs.

**Before flagging any money-validation asymmetry as a bug, check this file and
`money-rounding.md` first.** An asymmetry between two money paths is not
automatically a defect — some of them encode a real business distinction.
