# Client Onboarding — Invite-Only, Passwordless, Permanent Sessions

**Date:** 2026-07-04
**Status:** approved by owner (chat, 2026-07-04)
**Goal:** The gym owner (solo operator) invites clients by email with one button.
Clients log in via a magic link, stay logged in permanently, install the PWA to
their home screen with minimal friction, and open the door from `/my-balance`.
No public self-registration. No passwords for customers.

## Business flow

1. Client gives their email at the reception desk.
2. Owner opens the client's card on `/staff` → fills email (existing edit-info
   form) → presses **"Poslat pozvanku"** (new button).
3. Client receives an email (unaccented Slovak) with a button-link.
4. Client taps the link → `/welcome?t=<token>` → logged in immediately, forever.
5. Welcome screen offers a big **install-to-home-screen** CTA (Android: native
   one-tap prompt; iOS: illustrated 2-step Share → Add to Home Screen guide).
6. Client lands on `/my-balance` with the door button.

**Recovery (new phone / cleared browser):** login page gains "Zadaj svoj email,
posleme ti prihlasovaci link". Works ONLY for existing customer accounts with a
stored email — never creates an account. Owner is not involved.

**Explicitly rejected during design** (do not re-propose):
- Password for customers (email link IS the auth; passwords admin/staff-only).
- Google OAuth sign-in button (magic link covers every email provider).
- Biometric/WebAuthn gate on app open (phone lock screen already covers it;
  owner explicitly rejected — no backlog ticket).
- Session expiry for customers (90d JWT rejected: "vela neštastnych ludi").

## Decisions (locked)

| Topic | Decision |
|---|---|
| Customer session | Permanent: JWT `exp = iat + 100 years` for `role=customer`. Admin/staff keep the current 90-day expiry (role-based in `create_token`). |
| Revocation | Blocked/deleted users are rejected server-side where it matters (door route per #106, token-login endpoint). Token-leak risk accepted for MVP (scope: door + own balance). |
| Invite token | 32 random bytes, base64url, sent only in the link. SHA-256 hex stored. Single-use. Expiry **14 days**. |
| Login-link token | Same mechanics, purpose `login`, expiry **24 hours**. |
| Email transport | SMTP via `lettre`, configured by env (`SMTP_HOST/PORT/USERNAME/PASSWORD/FROM`). Provider-agnostic; production will start with the gym's Gmail + app password (pure config, post-merge task). Missing env → mail module Disabled; invite endpoint returns 503 `mail_not_configured` (clear error for the admin, not silent). |
| E2E test seam | `SMTP_TEST_MODE=capture`: outbound mail is NOT sent; the triggering endpoint includes `"test_link": "<url>"` in its JSON response so Playwright can drive the full flow. Mirrors the existing `EWELINK_TEST_MODE` pattern. Never set in production. |
| Public registration | REMOVED — `POST /api/auth/register` deleted, `/register` UI page + navbar links deleted. Accounts are created only via the desk add-person form. |
| Magic link scope | Customers only. Admin/staff authenticate with password only (magic link must not weaken admin auth to email-account security). |
| Email language | Unaccented Slovak (project convention), plain text + minimal HTML button. |

## Architecture

### DB (new migration, next free version in `crates/spinbike-server/src/db/migrations.rs`)

```sql
CREATE TABLE IF NOT EXISTS login_tokens (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    token_hash  TEXT NOT NULL UNIQUE,          -- SHA-256 hex of raw token
    purpose     TEXT NOT NULL CHECK (purpose IN ('invite','login')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at  TEXT NOT NULL,
    used_at     TEXT
);
CREATE INDEX IF NOT EXISTS idx_login_tokens_user ON login_tokens(user_id);
```

### Server modules / endpoints

- `crates/spinbike-server/src/mail/mod.rs` (new): `MailHandle::spawn()`-style
  env-config struct mirroring `ewelink::EwelinkHandle` conventions
  (Disabled fast-path, capture test mode). `send(to, subject, text, html)`.
- `POST /api/users/{id}/invite` — admin/staff only. Requires target user to
  have a non-empty email. Creates `invite` token (14d), sends invite mail with
  `https://<host>/welcome?t=<raw>`. 503 if mail Disabled. Response includes
  `sent_to` + (test mode only) `test_link`.
- `POST /api/auth/request-login-link` — public, body `{email}`. ALWAYS returns
  200 `{"status":"ok"}` (no user enumeration), but only actually sends when the
  email belongs to an existing, non-blocked `customer`. Token purpose `login`
  (24h). Rate limit: reuse the door `RateLimiter` pattern — per-email 60 s
  between sends + global 10/min.
- `POST /api/auth/token-login` — body `{token}`. Validates hash + unexpired +
  unused + user not blocked/deleted; marks `used_at`; returns the existing
  `AuthResponse` (JWT per role-based expiry above).
- `create_token`: role-based expiry (customer → +100y, others → +90d as today).
- `POST /api/auth/register` + its route + `RegisterRequest`: deleted.

The base URL for links: env `PUBLIC_BASE_URL` (prod `https://spinbike.newlevel.media`),
required whenever mail is configured.

### UI (spinbike-ui)

- `/welcome` page: reads `?t=`, POSTs token-login, stores JWT via the existing
  auth-storage path, then shows welcome + install component + CTA to
  `/my-balance`. Invalid/expired token → friendly unaccented-SK message + the
  request-login-link email form inline.
- Login page: below the password form (kept for admin/staff), a customer
  section: email input + "Poslat prihlasovaci link" → request-login-link →
  confirmation state.
- `/register` page and all links to it: removed.
- Edit-info form (staff card panel): "Poslat pozvanku" button, enabled when the
  card's user has an email; POSTs invite; toast "Pozvanka odoslana" / error.
  (Ephemeral status only — no persisted last-sent display in MVP.)
- Install component (`components/install_prompt.rs`):
  - Android/Chromium: `index.html` inline script captures `beforeinstallprompt`
    into `window.__deferredInstallPrompt` (event has no typed web-sys binding);
    the component reads it via `js_sys::Reflect`, shows a big "Pridat na plochu"
    button that calls `.prompt()`; hides after `appinstalled` or when running in
    `display-mode: standalone`.
  - iOS Safari: no such event — detect (UA + not standalone) and render a
    2-step illustrated guide (Share icon → "Pridat na plochu").
  - Rendered on `/welcome` (primary) and `/my-balance` (until installed).
- Manifest: add PNG icons 192×192 + 512×512 (+`purpose: "maskable"` variants)
  generated from `favicon.svg`; keep the SVG entry. Required for Chromium
  install-prompt eligibility.

### Config (post-merge, ops — not code)

`/etc/default/spinbike-prod` gains: `SMTP_HOST=smtp.gmail.com`, `SMTP_PORT=587`,
`SMTP_USERNAME=<gym gmail>`, `SMTP_PASSWORD=<app password>`, `SMTP_FROM=...`,
`PUBLIC_BASE_URL=https://spinbike.newlevel.media`. Dev stays unconfigured
(Disabled) except CI E2E which sets `SMTP_TEST_MODE=capture`.

## Testing

- Unit/integration: token create/redeem (happy, expired, reused, blocked user,
  wrong purpose), role-based JWT expiry, mail Disabled 503, no-enumeration 200.
- Playwright E2E (with `SMTP_TEST_MODE=capture`): full invite → welcome →
  logged-in → my-balance flow using `test_link`; login-link recovery flow;
  register page returns 404 / register API gone; install component renders
  (assert button/guide presence — the native prompt itself can't fire headless).
- RED→GREEN commit order for anything fixing existing behavior (register
  removal is a feature change, not a bug fix).

## Tickets (implementation order / dependencies)

1. mail infra (no deps)
2. tokens + endpoints + permanent sessions + register API removal (needs 1)
3. `/welcome` + login-page email form (needs 2)
4. install component + manifest icons (needs 3 for placement on /welcome)
5. admin invite button (needs 2)
6. UI register-page removal (can ride with 3; separate ticket for clean scope)
7. #106 blocked-door gate (pre-existing, independent — do first or anytime)
