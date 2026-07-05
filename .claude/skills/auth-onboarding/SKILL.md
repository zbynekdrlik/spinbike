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
