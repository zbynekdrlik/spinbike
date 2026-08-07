---
paths:
  - "crates/spinbike-server/src/routes/**"
---

# Session invalidation — the `AuthUser`-self-lookup pattern (#268/#274/#281)

Any handler that takes `AuthUser(claims)` and acts on the CALLER's own
account (not a staff-driven action on a target user) must decide what
happens when `claims.sub` no longer represents a live account — the JWT
itself has no expiry shorter than its `exp` claim, so a blocked or
soft-deleted user can hold a perfectly valid, unexpired token indefinitely.

**Two independently-discovered manifestations of the SAME gap, so far:**

- `#268` — `/api/my/balance` (`my_balance.rs`) originally 404'd for a
  missing/deleted user, leaving the customer on a broken page instead of
  triggering the client's 401-clears-session redirect.
- `#274` — `/api/door/open` (`door.rs`) had its OWN, independent user
  lookup (never touched by #268) and returned 403 instead of 401 for the
  same missing/deleted/blocked cases.
- `#281` (filed, not yet fixed) — `create_booking`/`cancel_booking`/
  `my_bookings` (`classes.rs`) never look up the `users` row for
  `claims.sub` AT ALL — no check whatsoever, so a blocked/deleted customer
  can keep booking/cancelling classes indefinitely.

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
checking it), that is the exact #268/#274/#281 gap — file it (or fix it if
in-scope) rather than assuming a prior PR already covered it. Each of the
three sites above was independently missed because the OTHER two existed
and looked "already handled" from a distance.

**Consolidation is an open design question, not yet decided.** Three
independent sites now hand-roll the identical `blocked || deleted_at.is_some()
→ 401` check. A shared helper (e.g. `require_live_session(pool, user_id) ->
Result<(), ApiError>`) would remove the duplication, but whoever picks up
#281 should treat that as its own deliberate design decision (does it fit
naturally into the `AuthUser` extractor itself, or stay a helper called from
each handler after its own SELECT?) — not bundled silently into a fix.
