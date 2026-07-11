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
