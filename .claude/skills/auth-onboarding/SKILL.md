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
- **Redeem is atomic, and `invite`/`login` are re-redeemable within a 10-minute grace window after first use (#246):** `UPDATE login_tokens SET used_at=COALESCE(used_at, datetime('now')) WHERE token_hash=? AND expires_at > datetime('now') AND (used_at IS NULL OR used_at > datetime('now', ?grace)) AND purpose IN (...) RETURNING user_id`. `fetch_optional` → `Some(user_id)` for unexpired+right-purpose AND (unused OR used-within-`REDEEM_GRACE_SECS`=600s). `used_at` is `COALESCE`d — a grace reuse does NOT re-stamp it, so the window is pinned to the FIRST redeem, never reset/extended. This fixes the dominant iPhone double-open (a mail app's in-app webview redeems the link first, the real browser/installed PWA reopens the SAME link second) that used to dead-end on the second open. The 6-digit `code` purpose (#227) is untouched — `verify_code` is a separate function, strictly single-use, never calls `redeem()`.
- `expires_at` is computed in SQL (`datetime('now', ?)` with a `format!("{ttl_secs:+} seconds")` interval) so it uses the exact same clock/format the `expires_at > datetime('now')` comparison reads back; the grace interval (`format!("{:+} seconds", -REDEEM_GRACE_SECS)`) is interpolated the same way.
- **`purge_expired_and_used` (the daily housekeeping job, `jobs/token_purge.rs`, also runs once at server startup) must stay the exact logical negation of `redeem`'s validity check** — it deletes expired rows OR rows used-and-past-grace, never a row still inside its grace window. Missing this when the grace window was added would let a deploy landing mid-grace (between the webview open and the browser open) wipe the token out from under the second open.

## Endpoints

- `POST /api/users/{id}/invite` — admin/staff only (`can_manage_cards()`). 400 if the target has no email; **503 `mail_not_configured`** when the mail module is Disabled (dev has no SMTP → 503 is the correct, expected response); echoes `test_link` in `SMTP_TEST_MODE=capture`.
- `POST /api/auth/request-login-link` `{email}` — **public, ALWAYS returns 200 `{"status":"ok"}`** (no user enumeration). Sends only for an existing, non-blocked, `role=customer` account. Email-keyed `LoginLinkRateLimiter` (60 s/email + 10/min global). **The SMTP send is `tokio::spawn`'d off the response path** so an existing customer's response isn't measurably slower than a non-customer's — otherwise the latency is a timing side-channel that leaks membership. The token row is committed synchronously (durable), delivery is best-effort.
- `POST /api/auth/token-login` `{token}` — redeems an invite OR login token → the existing `AuthResponse` JWT. Rejects invalid/expired tokens AND blocked/deleted users (re-checked from the DB after redeem, since `get_user_by_id` does NOT filter `deleted_at`). A reuse WITHIN the 10-minute grace window (#246) succeeds again (fresh JWT, same account); a reuse PAST grace is rejected same as before.

## Permanent customer sessions (`auth/mod.rs::create_token`)

Role-based expiry: `Role::Customer` → ~100 years (`CUSTOMER_SESSION_SECS`), admin/staff → 90 days. `parse_role` maps any non-`admin`/`staff` DB role string to `Role::Customer`, so in practice only admin/staff get the 90-day tier. A permanent JWT is NOT revoked on block/delete — that's bounded because the security-critical routes (door, payments) re-check `blocked` from the DB at action time, and `token-login` re-checks blocked/deleted before issuing a session.

## Route authorization — use the role extractors, not inline guards (#160)

Authorization is enforced at the **extraction boundary**, not re-authored in each handler body. Three extractors live in `auth/mod.rs`:

- `AuthUser(pub Claims)` — authenticates the JWT only (no role check).
- `StaffUser(pub Claims)` — authenticates **and** rejects `403 staff_required` unless `role.is_staff_or_admin()` (`Admin | Staff`).
- `AdminUser(pub Claims)` — authenticates **and** rejects `403 admin_required` unless `role.is_admin()` (`Admin`).

**Adding a protected route:** take the tier extractor in the handler signature — `_: StaffUser` (or `StaffUser(claims)` if you need `claims.sub` for logging/audit), `_: AdminUser` for admin-only. Do **NOT** write `if !claims.role.can_*() { return Err(ApiError::Forbidden(..)) }` in the body — that copy-paste is exactly what #160 removed (all the staff-tier `can_manage_cards`/`can_process_payments`/`can_book_for_others`/`can_cancel_any_booking`/`can_cancel_class` predicates are the same `Admin|Staff` check; `can_manage_users`/`is_admin` are the same `Admin` check).

**Ownership-mixed guards STAY inline on `AuthUser`.** When a route is "staff **OR** the resource owner" (`!role.can_*() && claims.sub != id`), a pure role extractor can't see the resource id — keep the inline check and take `AuthUser(claims)`. The four such sites (`users::update_user`, `users::user_transactions`, `classes::create_booking`, `classes::cancel_booking`) carry an explanatory comment; follow that pattern for any new mixed guard.

**Gotchas:**
- The extractor rejection type is `axum::response::Response` (it composes `AuthUser`'s tuple-rejection with `ApiError::into_response()`); the 403 body is byte-identical to the old inline `ApiError::Forbidden(ErrorCode::StaffRequired|AdminRequired)` — existing 403 assertions on `error_code`/`error` stay green.
- The extractor runs **before** body (`Json<_>`) extraction, so a malformed body + non-staff token now yields `403` (not `422`) — more secure, and no test asserted the old order.
- The role predicates `is_staff_or_admin()`/`is_admin()` are unit-tested (incl. the `Role::Unknown` reject) in `spinbike-core/src/auth.rs`; tier behavior is locked end-to-end in `tests/api_error_codes.rs`. `Role::Unknown` is rejected by both extractors (JWTs are only ever minted with known roles).

## Public registration is removed (server side, #108)

`POST /api/auth/register` (route + handler + `RegisterRequest`) is gone. Accounts are created only via the desk add-person form (`POST /api/users`) or the test-seed fixture. The `/register` UI page + nav links are removed separately in **#112** (its POST 404s in the interim — acceptable for invite-only MVP). See `ci-deploy` skill for the "removed route → SPA fallback 200" testing gotcha and the `seed-account` E2E-seed replacement.

## UI: `/welcome` page + shared `LoginLinkForm` (#109)

- `spinbike-ui/src/pages/welcome.rs` redeems `?t=` via `POST /api/auth/token-login`, stores the session (`auth::set_auth`), and shows a CTA. It's **role-aware** (`staff`/`admin` → `/staff`, else → `/my/balance`) even though no admin-invite UI exists yet — the server places no role restriction on who can be invited/redeem a token, so the client has to handle it.
- The request-login-link email form (email input + submit + confirmation state) is `components::LoginLinkForm` — used by BOTH `/welcome`'s invalid-token fallback and the login page's customer section. Don't re-inline it a third time; extend the shared component.
- **Exception — page-context-specific copy stays OUT of the shared component (#151).** "Extend the shared component" applies to the FORM itself (fields, button, states). A piece of static text that references something only true on ONE of the two call sites does NOT belong in `LoginLinkForm` — e.g. #151's "this link is for customers; staff/admin use the password form **above**" hint is only positionally accurate on `/login` (which has a password form above it); `/welcome`'s invalid-token fallback renders `LoginLinkForm` with no password form anywhere on the page, so the same text there would be false. That hint lives directly in `login.rs`, between the `customer-login-heading` and `<LoginLinkForm />`. When adding page-adjacent copy near the shared form, ask "is this true on BOTH call sites?" — if not, it's page-local, not component-shared.
- **`api::post` vs `api::post_public`:** `api::post` has a global side effect — a 401 response while ANY token is stored in localStorage triggers `clear_auth()` + redirect to `/login` (it assumes 401 always means "the current session died"). This is WRONG for any public/pre-auth endpoint that can legitimately 401 for reasons unrelated to the browser's current session (token-login on an already-used magic link; a wrong password on `/api/auth/login` while a DIFFERENT valid session — e.g. a shared kiosk — happens to be stored). **Use `api::post_public` for every public auth endpoint**, not `api::post` — it skips both the `Authorization` header attachment and the 401-clears-session logic. This bit a real bug in #109 (caught by `welcome.spec.ts`'s "reuse the link" assertion, RED in CI) and the SAME class of bug was found in the pre-existing password-login call, fixed at the same time.
- **`use_query_map()` inside `Effect::new`: use `.get_untracked()` for a run-once-on-mount read, never `.get()`.** `query.get()` is a TRACKED reactive read — an effect that reads it stays subscribed to the query-map memo and can re-fire if that memo ever re-notifies while the page stays mounted. Before #246's grace window, a second fire re-POSTed the (now-used) token, got a 401, and flipped a just-logged-in user back to the invalid-link screen — that exact symptom is gone now (a reuse within grace just succeeds again), but a stray re-fire is still a wasted network round-trip and would still 401 once the grace window has passed. `get_untracked()` has no tracked dependency, so the effect runs exactly once (Leptos effects always run once immediately on creation regardless of tracked reads) and never re-fires.

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

## 6-digit email login code (#227) — the in-PWA login path

Third member of the `login_tokens` family (`purpose='code'`, migration V21), for the
iOS installed-PWA logged-out loop (magic links always re-open in Safari, never in
the home-screen app). `db/login_tokens.rs` + `routes/auth.rs`.

- **Per-user hash salt is MANDATORY for a low-entropy code.** 6 digits = only 1M
  values, so two users could be issued the same code → identical `token_hash` →
  UNIQUE-index collision on insert. `hash_code(user_id, code) =
  sha256("{user_id}:{code}")` (NOT `sha256(code)`) sidesteps it AND binds a code to
  its own account (no cross-account replay). `create_code` also DELETEs the user's
  prior `code` rows (not mark-used) so a rare same-user/same-value re-issue can't
  trip UNIQUE either.
- **`verify_code`** is one transaction: newest live `code` row → hash match → mark
  used (single-use) → `Some(uid)`; wrong → `attempts+1`, invalidate at
  `MAX_CODE_ATTEMPTS=5`; every miss → `Ok(None)` (uniform, no leak). The `attempts`
  column (added by V21) is the per-code brute-force cap.
- **Two limiters, distinct roles:** request-login-code REUSES
  `login_link_rate_limit` (same "send an email to this address" budget);
  code-login (VERIFY) gets its OWN `CodeLoginRateLimiter` (per-email 10/60s + global
  60/60s), keyed by the submitted email BEFORE any DB lookup so a 429 leaks no
  account existence. Verify throttle → 429 `too_many_requests`; every other failure
  → uniform 401 `invalid_or_expired_code`.
- **rand 0.10 gotcha (cost a CI Lint cycle):** `random_range` lives on
  `rand::RngExt`, NOT `rand::Rng` → `use rand::RngExt;` (mirror `ewelink/auth.rs`).
  `fill_bytes` still needs `rand::Rng`. Both traits are imported in `login_tokens.rs`.
- **Mutation-killability for async auth handlers (RECURRING — cost 2 CI cycles):**
  an operator (`||`/`&&`/`==`) inside a DB-bound async handler can SURVIVE the
  diff-scoped mutation gate when a DOWNSTREAM check masks it — code_login's
  customers-only gate `!= Customer || blocked` survived because a wrong-code test
  still 401'd via `verify_code`. Fix: EXTRACT the predicate into a PURE fn
  (`is_eligible_customer(role, blocked) -> bool`) and unit-test every boundary
  combo — unit tests are guaranteed-run and directly kill the operator mutant. Same
  for a content-composing fn (`login_code_email`): unit-test that the output
  CONTAINS the code + the key phrases (kills the junk-tuple return mutant). And
  REMOVE a redundant guard whose branches are behaviourally equivalent to the
  downstream result (`if email.is_empty() || code.is_empty()` when an empty email
  already misses `get_user_by_email` and an empty code never matches a hash) — it is
  an unkillable EQUIVALENT mutant.
- **A bare arithmetic TTL/window constant (`N * M`) ALWAYS needs its own literal-pin test (#246, cost 1 CI cycle).** `REDEEM_GRACE_SECS = 10 * 60` shipped with only behavioral tests (boundary redeems via direct `-601 seconds` backdating) — none of them pin the CONSTANT itself, so the mutation gate's `10 * 60 -> 10 + 60` (600 -> 70) mutant survived undetected; every boundary test still passed since they redefine "past grace" relative to whatever the constant happens to be. Fix: a one-line literal-equality test (`assert_eq!(REDEEM_GRACE_SECS, 600, ...)`), same pattern as the pre-existing `ttl_constants_are_exactly_14_days_and_24_hours`/`code_ttl_is_ten_minutes`. **Whenever you add a new `pub const ... = N * M;` TTL/window/limit constant to this module, add its literal-pin test in the SAME commit** — don't rely on the boundary-behavior tests to catch an arithmetic-operator mutant in the constant's own definition.
- **E2E code seam:** `POST /api/test/mint-login-code {email} -> {code}`
  (SPINBIKE_TEST_MODE-gated) returns a raw code so a Playwright spec can drive the
  real UI then enter a known-valid value (the public request endpoint never echoes
  it — no enumeration). UI: `CodeLoginForm` + `CustomerLoginMethods` toggle
  (default = email-link, so existing login-link/welcome selectors stay green).
