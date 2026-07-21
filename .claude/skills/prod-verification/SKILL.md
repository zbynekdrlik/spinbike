---
name: spinbike-prod-verification
description: >
  How to functionally verify a customer-facing feature LIVE on prod
  (spinbike.sk) without real customer credentials or PII — a synthetic
  throwaway account + a self-minted JWT, exercised through the REAL API,
  cleaned up after. Load before any post-deploy verification that needs
  a logged-in customer session on prod (not just a liveness/curl check).
triggers:
  - post-deploy verification
  - functional verification
  - synthetic customer
  - JWT
  - my/balance
  - my/bookings
  - verify on prod
---

# SpinBike Prod Functional Verification — Synthetic Customer Session

`post-deploy-verification.md` (global) requires exercising the actual
customer workflow, not just a liveness curl. SpinBike prod has NO customer
test fixtures (`SPINBIKE_TEST_MODE`-gated `/api/test/*` endpoints only exist
on dev/CI) and there is no real customer password to log in with. This is
the pattern that worked for #146/#147 — reusable for any future
customer-facing (`/my/*`) ticket.

## The recipe

1. **Prod and dev run LOCAL** (see project `CLAUDE.md`) — read the JWT
   secret straight off the running service, no SSH:
   ```bash
   systemctl cat spinbike.service   # shows EnvironmentFile=/etc/default/spinbike-prod
   sudo cat /etc/default/spinbike-prod   # JWT_SECRET=...
   ```
   Never print the raw secret into the transcript — redirect straight to a
   scratchpad file (`sudo cat ... | grep '^JWT_SECRET=' > /tmp/.../\.jwtsecret`)
   and read it back only inside the signing step. Delete the file when done.

2. **Insert a throwaway customer row directly into the prod SQLite DB**
   (`/opt/spinbike/prod/spinbike.db`) — a distinguishable name/email/card_code
   (`autopilot-verify-<issue-numbers>@spinbike.local`) makes it trivially
   greppable and impossible to confuse with a real customer:
   ```sql
   INSERT INTO users (email, name, role, credit, card_code, blocked, allow_debit, allow_self_entry)
   VALUES ('autopilot-verify-NNN@spinbike.local', 'AUTOPILOT VERIFY NNN', 'customer', 0.0, 'AUTOPILOT-VERIFY-NNN', 0, 0, 0);
   ```

3. **Mint a JWT matching `spinbike_core::auth::Claims`** exactly
   (`{sub, email, role, exp, iat}`, HS256, `jsonwebtoken::encode` with
   `Header::default()` — see `crates/spinbike-server/src/auth/mod.rs::create_token`).
   PyJWT is already installed; a short-lived token (an hour) is plenty:
   ```python
   import jwt, time
   claims = {"sub": USER_ID, "email": EMAIL, "role": "customer",
             "exp": int(time.time()) + 3600, "iat": int(time.time())}
   token = jwt.encode(claims, SECRET, algorithm="HS256")
   ```

4. **Drive the REAL production API** with the token (curl or Playwright) to
   set up the scenario — e.g. `POST /api/bookings` to book a REAL upcoming
   class occurrence (found via the public `GET /api/classes?from=&to=`, no
   auth needed), or a direct SQL `INSERT INTO transactions` for a movement
   the API has no test-mode shortcut for. Prefer the real API over raw SQL
   wherever a real endpoint exists — it exercises the actual code path.

5. **Verify via BOTH layers**: `curl` the JSON response (fast, exact field
   check) AND load the page in Playwright with the token injected into
   `localStorage` (`spinbike_token`, `spinbike_user`, `spinbike_lang` —
   same keys `loginViaAPI` in `e2e/tests/helpers.ts` sets) to confirm the
   actual rendered DOM. **Clear any stale service-worker registration
   first** (see `frontend-pwa` skill's post-deploy-verification gotcha) —
   a long-lived Playwright MCP profile can otherwise show a stale cached
   build.

6. **Clean up immediately after**: cancel/delete anything created through
   the real API (e.g. `DELETE /api/bookings/{id}` — reverts the capacity
   count correctly, unlike a raw SQL delete), then `DELETE FROM
   transactions`/`DELETE FROM users` directly for rows with no such
   endpoint. Verify zero rows remain for that synthetic user_id before
   moving on. Delete the scratchpad secret/token files too.

## Why this beats the alternatives

- **Real customer JWT (no password needed, since JWTs aren't validated
  against a live session store)** would work but touches real PII in a
  Playwright profile for zero benefit — avoid it.
- **Raw SQL for everything** (including the booking) skips the real
  `create_booking` business logic (capacity check, `ServerMsg::BookingUpdate`
  broadcast) — prefer the real endpoint whenever one exists.
- **Curl-only** verification never proves the DOM actually renders the
  enrichment (see `autonomous-verification.md` — liveness ≠ functional).

## Shortcut — verifying a pure ROLE-boundary check needs NO DB row at all

If the only thing under test is an extractor-level role gate (`StaffUser`/
`AdminUser` vs `AuthUser` — e.g. #175, confirming a customer JWT now gets 403
`staff_required` where it used to get 200), skip step 2 entirely. Those
extractors (`crates/spinbike-server/src/auth/mod.rs`) decide purely from the
JWT's own `role` claim — no DB lookup happens before the check. Mint the JWT
with `sub` set to any non-existent id (e.g. `999999999`) and the target
`role`, then curl straight away. Zero DB writes, zero cleanup. Only fall back
to the full synthetic-user-row recipe when the endpoint under test actually
reads the DB row (balance, bookings, name rendering, etc.).

## Verifying an ADMIN/STAFF-only page (not customer `/my/*`) — same recipe, mint an admin-role JWT (#232)

This recipe reads as customer-only, but it works identically for a
`/staff` (admin dashboard) flow — the shortcut above already establishes
WHY: `StaffUser`/`AdminUser` extractors decide purely from the JWT's `role`
claim, no DB lookup on the CALLER. So verifying an admin-only UI change
(e.g. the edit-user sheet) needs:

1. Mint a JWT with `role: "admin"` and any non-existent `sub` (no DB row
   for the caller at all — the shortcut applies to the ADMIN caller too).
2. If the change is only visible when acting ON a target user (e.g. an
   edit-info sheet), create ONE throwaway CUSTOMER row via the real
   `POST /api/users` endpoint (using the minted admin token) — this is
   the "target" the admin flow operates on, not the caller.
3. Inject `spinbike_token`/`spinbike_user`/`spinbike_lang` into
   `localStorage` (same keys as the customer recipe) and navigate to
   `/staff?card=<the throwaway card_code>` — the query param pre-selects
   the card, skipping the search step.
4. Clean up: `DELETE /api/users/{id}` only SOFT-deletes (sets
   `deleted_at`, #143's soft-delete-conflict flow needs this) — if you
   want ZERO rows left (not even a soft-deleted one), follow with a
   direct `sqlite3 ... "DELETE FROM users WHERE id=..."` after confirming
   it has no `transactions`/`bookings` rows attached.

## Gotchas

- **NEVER name a shell var `UID` (or `GID`/`PPID`/`EUID`) when capturing the
  synthetic user id.** `UID` is a bash READONLY (= the OS uid, `1000` here), so
  `UID=$(sqlite3 ... "SELECT id FROM users WHERE email=...")` **silently fails**
  ("readonly variable"), and every later `user_id=$UID` INSERT attaches to a
  dangling `user_id=1000` (no such user row → SQLite's default `PRAGMA
  foreign_keys=OFF` lets it through), NOT your synthetic user. Discovered on
  #168 verify — the movements/booking landed on a non-existent user 1000 and had
  to be re-inserted. Use a distinctive name (`SYN_ID`), and after inserting
  ALWAYS assert the rows are on the real synthetic id
  (`SELECT count(*) ... WHERE user_id=$SYN_ID`) before driving the API.
- **Booking a real class occurrence via raw SQL** needs a valid `template_id`
  (from `class_templates`) + a future `date` matching a real occurrence — get
  the id from `GET /api/classes?from=&to=` (public), e.g. template 1 recurs
  weekly. `bookings` has `source` NOT NULL (`'manual'`).
- **`monthly_pass_active_until` is NOT set just by inserting a `charge` tx with
  a future `valid_until`** — the balance API derives the active pass by its own
  rule, so that field can read `None` even with a valid pass-sale row. To verify
  the pass-EXPIRY date render (`tx_until_short` + `fmt_date_short`), the pass-sale
  MOVEMENT row's `do DD.MM.` suffix is enough — you don't need the pass banner.
