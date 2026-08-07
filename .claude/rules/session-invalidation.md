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

`door.rs` (#274) and `my_balance.rs` (#268) are DELIBERATELY left on their
own pre-existing hand-rolled inline checks, permanently — not "for now".
`#284` (which fixed the two NEW gaps below) considered consolidating them
onto the shared helper and decided against it: both sites already fetch
`blocked`/`deleted_at` in the SAME query as the other columns they need and
already carry the identical 401 `session_invalid` contract, tested. Moving
them onto `require_live_session` has only two honest shapes — an extra
query before the existing one (opens a narrow TOCTOU window between the two
reads that doesn't exist today) or a genuinely redundant second check next
to the first (pure diff/re-test risk for zero behavior change) — and
neither is worth it for already-correct, already-tested code. Write NEW
session-invalidation checks against the helper; do not migrate these two.

`#284` also covers the two gaps two independent reviews found right after
#277 shipped: `user_transactions`'s self-view branch (`users.rs`) and
`install_token` (`auth.rs`) both used to accept a dead caller session —
both now call `require_live_session`, gated on the self-acting branch only
(same `if is_self { ... }` shape as `update_user`).

**Why NOT the `AuthUser` extractor** (weighed seriously in #277's design
step, re-weighed with MEASURED numbers in #284's — see either issue's
design comment for the full reasoning): only **9** handlers actually
destructure `AuthUser(claims): AuthUser` directly (`door::open`,
`auth::install_token`, `auth::me`, `classes::create_booking`,
`classes::cancel_booking`, `classes::my_bookings`, `users::update_user`,
`users::user_transactions`, `my_balance::my_balance`) — after #284, 8 of
the 9 call `require_live_session` or its hand-rolled equivalent (including
`my_balance`'s own pre-existing hand-rolled check, described above); only
`auth::me` doesn't (it never touches the DB, just echoes the JWT's own
claims back, so there's nothing for the check to protect). But `StaffUser`
and `AdminUser`
(`auth/mod.rs`) both internally call `AuthUser::from_request_parts` FIRST —
so moving the check INTO `AuthUser` would silently cascade to every
`StaffUser`/`AdminUser` handler too: measured **50** `StaffUser` + **15**
`AdminUser` call sites, ~73 handlers total, not the ~17 #277 estimated.
Most of those act on a TARGET user by path id (staff editing a customer,
admin managing cards) — gating on the CALLER's own liveness there is a
materially bigger, separate decision (should a blocked staff account be
locked out of every admin action mid-shift, not just self-acting ones?)
needing its own review of all ~73 call sites' tests. Cost of the extra
`SELECT` is negligible either way (single SQLite file, sub-millisecond,
low-traffic single-operator app) — scope discipline, not cost, is still the
deciding factor. If a future ticket wants the extractor-level guarantee for
the `StaffUser`/`AdminUser` cascade, that is a deliberate, much larger
follow-up, not something to fold silently into an unrelated fix.
