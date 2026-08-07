---
paths:
  - "crates/spinbike-server/src/db/users.rs"
  - "spinbike-ui/src/pages/dashboard/mod.rs"
  - "e2e/tests/dashboard.spec.ts"
  - "e2e/tests/negative-balance.spec.ts"
---

# Card search: ranking rule + row-vs-panel digit display (#290, #39 recurrence)

## A barcode-code match that ENDS WITH the typed digits must outrank one that merely CONTAINS them

`search_users` / `search_users_with_pass` (`crates/spinbike-server/src/db/
users.rs`) rank card_code matches before falling back to name-ASC ordering.
The real-world case is "staff scans/types the LAST few digits off a
barcode" — a TAIL match, not a prefix. The ranking used to special-case only
a PREFIX card_code match (`'query%'`); a card_code ending with the typed
digits (the common case) fell into the same catch-all "contains it
somewhere" bucket as an unrelated card whose code merely happens to include
the same digits mid-string, and ordering fell through to plain name ASC —
letting the WRONG card win the top slot.

**Fixed ranking (both db-layer search functions, identically):**

```sql
ORDER BY
  CASE
    WHEN u.card_code = ?                              THEN 0  -- exact
    WHEN u.card_code LIKE ? OR u.card_code LIKE ?      THEN 1  -- prefix OR suffix
    ELSE 2                                                      -- contains-elsewhere
  END,
  last_visit_at IS NULL, last_visit_at DESC,
  u.name IS NULL, u.name ASC, u.card_code ASC
```

When touching this ranking again, keep BOTH `search_users` and
`search_users_with_pass` in sync — they share the exact same bug shape in
the same file; only `search_users_with_pass` is reachable from a route
today, but `search_users` regresses silently if it drifts.

## The search-RESULT ROW shows only the LAST 4 DIGITS — by deliberate design

`spinbike-ui/src/pages/dashboard/mod.rs`'s search dropdown renders each
result's card code truncated to its last 4 digits (`…2345`), never the full
number. This is intentional (a shoulder-surfing/privacy consideration for
the staff desk), and `negative-balance.spec.ts` already relies on it.

**The FULL card number only appears in the action/detail panel, after
clicking a result.** A test asserting the complete card_code MUST assert it
against the panel (post-click), never against the search-result row itself
— a regression test that got this backwards (`dashboard.spec.ts`, fixed in
`36eae91`) looked like it was testing the ranking fix but was actually
asserting an invariant the row was never supposed to satisfy.
