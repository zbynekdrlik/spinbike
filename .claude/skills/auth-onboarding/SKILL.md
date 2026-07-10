---
name: spinbike-auth-onboarding
description: >
  SpinBike passwordless client onboarding — magic-link tokens, permanent
  customer sessions, the invite/login flows, and how public registration was
  removed. Load before touching auth (login_tokens, /api/auth/*, /welcome,
  the login page, the staff invite button, or the /register removal in #112).
triggers:
  - magic link
  - login_tokens
  - invite
  - token-login
  - request-login-link
  - passwordless
  - permanent session
  - register removal
  - welcome page
---

# SpinBike Auth / Client Onboarding

Design spec: `docs/superpowers/specs/2026-07-04-client-onboarding-design.md`. Onboarding is **invite-only + passwordless** — the owner invites a client by email, the client logs in via a magic link and stays logged in permanently. Customers never have passwords; admin/staff still authenticate with a password.

## Magic-link token model (`db/login_tokens.rs`, migration V17)

- One `login_tokens` row per issued link. The **raw** token (32 random bytes, base64url) is sent ONLY inside the emailed link; the DB stores ONLY its **SHA-256 hex** (`token_hash`). Never log the raw token.
- `purpose` is CHECK-constrained to `'invite'` (14-day onboarding) or `'login'` (24-hour recovery). TTL constants live in `login_tokens.rs` (`INVITE_TTL_SECS`, `LOGIN_TTL_SECS`).
- **Single-use redeem is one atomic statement:** `UPDATE login_tokens SET used_at=datetime('now') WHERE token_hash=? AND used_at IS NULL AND expires_at > datetime('now') AND purpose IN (...) RETURNING user_id`. `fetch_optional` → `Some(user_id)` only if valid+unused+unexpired+right-purpose. SQLite serializes writers, so two concurrent redemptions can't both win.
- `expires_at` is computed in SQL (`datetime('now', ?)` with a `format!("{ttl_secs:+} seconds")` interval) so it uses the exact same clock/format the `expires_at > datetime('now')` comparison reads back.

## Endpoints

- `POST /api/users/{id}/invite` — admin/staff only (`can_manage_cards()`). 400 if the target has no email; **503 `mail_not_configured`** when the mail module is Disabled (dev has no SMTP → 503 is the correct, expected response); echoes `test_link` in `SMTP_TEST_MODE=capture`.
- `POST /api/auth/request-login-link` `{email}` — **public, ALWAYS returns 200 `{"status":"ok"}`** (no user enumeration). Sends only for an existing, non-blocked, `role=customer` account. Email-keyed `LoginLinkRateLimiter` (60 s/email + 10/min global). **The SMTP send is `tokio::spawn`'d off the response path** so an existing customer's response isn't measurably slower than a non-customer's — otherwise the latency is a timing side-channel that leaks membership. The token row is committed synchronously (durable), delivery is best-effort.
- `POST /api/auth/token-login` `{token}` — redeems an invite OR login token → the existing `AuthResponse` JWT. Rejects invalid/expired/reused tokens AND blocked/deleted users (re-checked from the DB after redeem, since `get_user_by_id` does NOT filter `deleted_at`).

## Permanent customer sessions (`auth/mod.rs::create_token`)

Role-based expiry: `Role::Customer` → ~100 years (`CUSTOMER_SESSION_SECS`), admin/staff → 90 days. `parse_role` maps any non-`admin`/`staff` DB role string to `Role::Customer`, so in practice only admin/staff get the 90-day tier. A permanent JWT is NOT revoked on block/delete — that's bounded because the security-critical routes (door, payments) re-check `blocked` from the DB at action time, and `token-login` re-checks blocked/deleted before issuing a session.

## Public registration is removed (server side, #108)

`POST /api/auth/register` (route + handler + `RegisterRequest`) is gone. Accounts are created only via the desk add-person form (`POST /api/users`) or the test-seed fixture. The `/register` UI page + nav links are removed separately in **#112** (its POST 404s in the interim — acceptable for invite-only MVP). See `ci-deploy` skill for the "removed route → SPA fallback 200" testing gotcha and the `seed-account` E2E-seed replacement.

## UI: `/welcome` page + shared `LoginLinkForm` (#109)

- `spinbike-ui/src/pages/welcome.rs` redeems `?t=` via `POST /api/auth/token-login`, stores the session (`auth::set_auth`), and shows a CTA. It's **role-aware** (`staff`/`admin` → `/staff`, else → `/my/balance`) even though no admin-invite UI exists yet — the server places no role restriction on who can be invited/redeem a token, so the client has to handle it.
- The request-login-link email form (email input + submit + confirmation state) is `components::LoginLinkForm` — used by BOTH `/welcome`'s invalid-token fallback and the login page's customer section. Don't re-inline it a third time; extend the shared component.
- **`api::post` vs `api::post_public`:** `api::post` has a global side effect — a 401 response while ANY token is stored in localStorage triggers `clear_auth()` + redirect to `/login` (it assumes 401 always means "the current session died"). This is WRONG for any public/pre-auth endpoint that can legitimately 401 for reasons unrelated to the browser's current session (token-login on an already-used magic link; a wrong password on `/api/auth/login` while a DIFFERENT valid session — e.g. a shared kiosk — happens to be stored). **Use `api::post_public` for every public auth endpoint**, not `api::post` — it skips both the `Authorization` header attachment and the 401-clears-session logic. This bit a real bug in #109 (caught by `welcome.spec.ts`'s "reuse the link" assertion, RED in CI) and the SAME class of bug was found in the pre-existing password-login call, fixed at the same time.
- **`use_query_map()` inside `Effect::new`: use `.get_untracked()` for a run-once-on-mount read, never `.get()`.** `query.get()` is a TRACKED reactive read — an effect that reads it stays subscribed to the query-map memo and can re-fire if that memo ever re-notifies while the page stays mounted. For a single-use token redemption, a second fire re-POSTs the (now-used) token, gets a 401, and flips a just-logged-in user back to the invalid-link screen. `get_untracked()` has no tracked dependency, so the effect runs exactly once (Leptos effects always run once immediately on creation regardless of tracked reads) and never re-fires.

## Prod SMTP — Resend from `spinbike@spinbike.sk` (cutover 2026-07-07; was Gmail pilot)

Invites/login-links send on **prod** (dev stays Disabled → 503 `mail_not_configured`, which is correct). Config lives in `/etc/default/spinbike-prod` (the systemd `EnvironmentFile`, NOT git). Non-secret shape:

- `SMTP_HOST=smtp.resend.com`, `SMTP_PORT=587` — the mail module uses lettre `starttls_relay`, i.e. **STARTTLS on 587**. Port 465 (implicit TLS) is NOT supported — don't switch to it.
- `SMTP_USERNAME=resend` (the literal string), `SMTP_PASSWORD` = a **Resend API key** (`re_...`). The key lives ONLY in the prod env file — never git/dev/logs.
- `SMTP_FROM="SpinBike <spinbike@spinbike.sk>"`. Resend accepts a From only on a **verified** domain; no `SMTP_USERNAME`=From match requirement (unlike Gmail).
- `PUBLIC_BASE_URL=https://spinbike.sk` (read in `lib.rs`) builds the magic-link URL — the app's own domain (cutover from `spinbike.newlevel.media` 2026-07-08; the old host still resolves via the same Cloudflare tunnel, both serve the app, but new invite/login links use `spinbike.sk`).
- Startup proof: journal logs `mail: SMTP transport configured host=smtp.resend.com port=587`. A successful send logs `mail: sent to=<..>` + `invite: sent user_id=<N>` — fires only after lettre's `send()` returns Ok (Resend accepted / 250), real send-verification.
- **Verified 2026-07-07:** app invite → mail-tester scored **10/10, "properly authenticated"** (SPF+DKIM+DMARC pass), From `spinbike@spinbike.sk`. NO code change — the switch was purely these env vars + a restart.

### Resend domain setup (how the DNS + verification was done, for the next domain/env)

- Domain `spinbike.sk` added to Resend via API (`POST /domains`, region `eu-west-1`) → returns 3 records: **DKIM** `TXT resend._domainkey` (`p=…`), **SPF** `TXT send` (`v=spf1 include:amazonses.com ~all`), **return-path** `MX send` → `feedback-smtp.eu-west-1.amazonses.com`. All on the `send.`/`resend._domainkey` subdomains → **no conflict** with the domain's apex MX/SPF/DMARC (WebSupport's own mail).
- DNS is at **WebSupport**; records written via their **REST API** (`POST /v1/user/self/zone/spinbike.sk/record`, HMAC-SHA1 auth: Basic user=API key, pass=hex `HMAC-SHA1(secret, "{METHOD} {path} {unix_ts}")`, + a `Date` header). WebSupport API key = UUID, secret = 40-hex. Client script + creds are in the session scratchpad pattern; the WebSupport API key can be revoked after — the DNS records persist.
- **GOTCHA — do NOT hammer `POST /domains/:id/verify`.** Each call resets the domain to `pending` and starts a fresh SES DNS check; polling verify every ~90 s kept resetting it before SES finished, so it flapped verified↔pending for ~40 min and never settled. Fix: trigger verify **once**, then poll **read-only** (`GET /domains/:id` + a real send probe to `delivered@resend.dev`) — it settled within minutes. The real "is it usable" gate is a **200 from `POST /emails`**, not the status flag (which flaps during propagation).
- **Deploy discipline:** the domain-verified state can lag the API flag; flip prod to Resend only after a stable send probe (3× 200), and keep the previous Gmail env as a `.bak-<ts>` so a rollback is one `cp` + restart. Prod was kept on Gmail throughout the wait so invites never broke.

### Footgun: NEVER `source /etc/default/spinbike-prod` in bash

The file has at least one **unquoted, space-containing value** (the eWeLink password on its own line). systemd's `EnvironmentFile` parser takes the whole rest of the line literally, so the service reads it fine — but bash `source`/`.` word-splits it, runs a fragment as a command (`command not found`), backgrounds part as a bogus `KEY=val`, and can echo a secret fragment into the transcript. To read a value in a script, extract the single key instead:

```bash
# safe — one key, no shell parsing of the rest of the file:
JWT_SECRET=$(sed -n 's/^JWT_SECRET=//p' /etc/default/spinbike-prod)
# non-secret keys for inspection:
grep -E '^(PORT|DATABASE_PATH|PUBLIC_BASE_URL|SMTP_HOST|SMTP_PORT)=' /etc/default/spinbike-prod
```

Prod runs on **:8080**, DB `/opt/spinbike/prod/spinbike.db` (WAL); systemd `spinbike.service`. To act as admin on prod without a browser, mint a short-lived HS256 JWT in Python from `JWT_SECRET` (claims `{sub, email, role:"admin", iat, exp}`) — see git history / this session for the one-liner; keep exp ≤5 min and never print the token or secret.
