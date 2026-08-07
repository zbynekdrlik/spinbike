---
paths:
  - "crates/spinbike-server/src/routes/**"
  - "crates/spinbike-server/src/auth/**"
---

# Session invalidation — the `AuthUser`-self-lookup pattern (#268/#274/#277)

Any handler that takes `AuthUser(claims)` and acts on the CALLER's own
account (not a staff-driven action on a target user) must decide what
happens when `claims.sub` no longer represents a live account — the JWT
itself has no expiry shorter than its `exp` claim, so a blocked or
soft-deleted user can hold a perfectly valid, unexpired token indefinitely.

**Three independently-discovered manifestations of the SAME gap, so far:**

- `#268` — `/api/my/balance` (`my_balance.rs`) originally 404'd for a
  missing/deleted user, leaving the customer on a broken page instead of
  triggering the client's 401-clears-session redirect.
- `#274` — `/api/door/open` (`door.rs`) had its OWN, independent user
  lookup (never touched by #268) and returned 403 instead of 401 for the
  same missing/deleted/blocked cases.
- `#277` (fixed, folded in `#281`'s duplicate finding) —
  `create_booking`/`cancel_booking`/`my_bookings` (`classes.rs`) had NO
  user-row lookup at all, and `update_user` self-edit (`users.rs`) looked up
  the target row but only checked `deleted_at` (404, not 401) and never
  `blocked` at all. Fixed by calling the new shared helper below.

**The established contract (do NOT invent a new shape):**

- Missing (hard-deleted/bogus `sub`), soft-deleted (`deleted_at` set), or
  `blocked=1` → `Err(ApiError::Unauthorized(ErrorCode::SessionInvalid))`.
  The client (`spinbike-ui/src/api.rs`'s `handle_unauthorized`, and
  `door_button.rs`'s own inline 401 handling) already special-cases 401 by
  clearing storage and redirecting to `/login` — reuse the EXISTING
  `ErrorCode::SessionInvalid`, never invent a new code for this.
- A LIVE, non-blocked user who is merely disallowed by a business rule
  (e.g. `allow_self_entry=false`) stays whatever ordinary 403/409 the
  route already used — session-invalidation is orthogonal to permission
  checks, never conflate the two.
- If the SELECT already filters `WHERE ... AND deleted_at IS NULL`, a
  soft-deleted row silently falls into "row missing" and the two cases
  become indistinguishable — this is a trap, not a shortcut. Drop the
  filter, fetch `deleted_at` explicitly, and check it alongside `blocked`.

**When adding or reviewing ANY handler that takes `AuthUser`:** grep for
whether it reads the `users` table for `claims.sub` at all, and if so
whether that lookup checks `blocked`/`deleted_at`. If it reads `users` but
skips the check (or filters the row out via `deleted_at IS NULL` instead of
checking it), that is the exact #268/#274/#277 gap — file it (or fix it if
in-scope) rather than assuming a prior PR already covered it. Each of the
three sites above was independently missed because the OTHER two existed
and looked "already handled" from a distance.

**Consolidation — decided in #277: a shared helper, NOT the `AuthUser`
extractor.** `auth::require_live_session(pool, user_id) -> Result<(), ApiError>`
(`crates/spinbike-server/src/auth/mod.rs`) is the canonical check now — call
it immediately after extracting `AuthUser` in any handler that acts on the
CALLER's own account. `classes.rs`'s `create_booking`/`cancel_booking`/
`my_bookings` and `users.rs`'s `update_user` self-edit branch all call it.

`door.rs` (#274) and `my_balance.rs` (#268) were deliberately left on their
own pre-existing hand-rolled inline checks rather than migrated to the new
helper — both are correct and already tested; migrating them would only add
diff/re-test risk for zero behavior change. Write NEW session-invalidation
checks against the helper; a future cleanup pass MAY consolidate the two old
sites onto it too, but that is not required.

**Why NOT the `AuthUser` extractor** (weighed seriously in #277's design
step — see the issue's design comment for the full reasoning): `AuthUser`/
`StaffUser`/`AdminUser` back ~17 handler call sites across `auth.rs`,
`payments.rs`, `admin.rs`, `users.rs`, `door.rs`, `my_balance.rs`,
`classes.rs`. Moving the check into the extractor would apply it to all of
them at once (correctly closing the class of bug for every future endpoint
with zero chance of a missed call site) but is a much bigger blast radius
than any one ticket's named scope, and needs re-verifying every affected
call site's existing tests. Cost of the extra `SELECT` is negligible either
way (single SQLite file, sub-millisecond, low-traffic single-operator app) —
scope discipline, not cost, was the deciding factor. If a future ticket
wants the extractor-level guarantee for the other ~13 sites (payments,
admin, user management), that is a deliberate follow-up, not something to
fold silently into an unrelated fix.
