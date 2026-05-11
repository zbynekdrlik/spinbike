# Door Self-Entry Design

**Date:** 2026-05-10
**Status:** Approved (brainstorm), pending implementation plan
**Issue:** https://github.com/zbynekdrlik/spinbike/issues/92
**One-line goal:** Let allowlisted customers tap a button in the PWA to remotely open the fitness front door via a Sonoff MINI-D smart relay, while billing them correctly (visit if monthly pass, single-entry charge otherwise).

---

## 1. Goals

1. Customer logs into the PWA on their phone, taps `Hold to open door` for 2 seconds, the front-door buzzer at the fitness center sounds for 3 seconds, the magnetic lock disengages, customer walks in.
2. The system writes the correct billing row to `transactions`:
   - First press today, customer has an active monthly pass → `action='visit'`, amount `0`.
   - First press today, no active pass → `action='charge'`, amount `-<single-entry price>`, credit decremented.
   - Second-or-later press today (regardless of pass) → `action='charge'`, amount `0`, `note='door: 2nd'` / `'3rd'` / ...
3. Customers are restricted to their own data (credit, recent transactions, monthly-pass status). They never see admin/staff features (reports, other users, settings).
4. Only customers the CEO has explicitly allowlisted may open the door. Default is **denied**.
5. Hardware failure must NOT charge the customer. No transaction row is written unless the door physically opens.

## 2. Non-goals

- Physical-presence verification (GPS, LAN check, QR scan, NFC) — explicitly chosen out of scope; the system trusts whoever holds a logged-in PWA session.
- Customer self-signup. CEO creates accounts manually (existing flow).
- Multi-door support. Single MINI-D, single buzzer, single front door.
- Door state read-back (e.g. "is door currently locked?"). Inching mode means the relay is always idle after 3s; no state to query.
- Replacing the legacy buzzer button on the reception phone. It still works in parallel for staff-driven entry.
- A separate audit-log table. The `transactions.note` column carries the audit trail.

## 3. User-facing decisions (locked during brainstorm)

| Decision | Choice | Reason |
|---|---|---|
| Presence proof | None (trust logged-in user) | Simplest. Allowlist + admin revocation covers lost-phone risk. |
| Hardware protocol | Rust-native eWeLink WebSocket client | Avoids Node/Python sidecar; keeps deploy to one binary. |
| Same-day re-entry | One real visit/charge per day; later presses logged with amount=0 + `note='door: Nth'` | Honest reports, no double-counting, full audit trail. |
| Empty wallet (no pass, no credit, no allow_debit) | Open door anyway, charge into debt | Trusted community model. |
| Allowlist | Per-user explicit boolean `allow_self_entry`, default `0`, admin-only writeable | Safe by default. |
| Rate limit | 1 press / 10s, hard 5 / minute per user; 30 / minute global | Stops runaway / abuse without bothering legit users. |
| Tap UX | Press-and-hold 2 seconds | Prevents accidental pocket-tap. |
| Door unlock duration | 3 seconds (configured ON THE DEVICE in eWeLink inching mode, not in server) | Long enough to push the door open. |
| Hardware fail | Return 503, no tx row written, banner "Door unavailable — ask reception" | User does not pay if door did not open. |
| Single-entry price | Existing `services` row already used at reception (no new config) | One source of truth. |
| Button placement | Big primary button on `/my-balance` page | Customer lands there after login. |
| Onboarding | CEO creates user in admin, delivers credentials manually | Existing flow. |
| eWeLink WS lifecycle | Persistent connection with auto-reconnect (exponential backoff 1→2→4→8→30s) | First press of the day is instant. |
| Same-day boundary | `date('now', 'localtime')` in Europe/Bratislava | Matches everywhere else in the codebase. |
| Rollout | ONE PR containing everything | Per project preference. |

## 4. Architecture

```
┌─────────────────┐                                ┌──────────────────┐
│  PWA (Leptos)   │   POST /api/door/open          │ spinbike-server  │
│  /my-balance    │   Authorization: Bearer JWT    │   (Axum)         │
│  hold-2s button │ ─────────────────────────────► │                  │
└─────────────────┘                                │  ┌────────────┐  │
                                                   │  │ door route │  │
                                                   │  │  guards +  │  │
                                                   │  │  billing   │  │
                                                   │  └─────┬──────┘  │
                                                   │        │         │
                                                   │  ┌─────▼──────┐  │
                                                   │  │  ewelink   │  │
                                                   │  │  module    │  │
                                                   │  │  (WS task) │  │
                                                   │  └─────┬──────┘  │
                                                   └────────│─────────┘
                                                            │ WSS, persistent
                                                            ▼
                                            ┌──────────────────────────┐
                                            │  eWeLink cloud           │
                                            │  wss://{region}-         │
                                            │  dispa.coolkit.cc        │
                                            └────────────┬─────────────┘
                                                         │ push command
                                                         ▼
                                            ┌──────────────────────────┐
                                            │  Sonoff MINI-D @ fitness │
                                            │  Inching mode 3000ms     │
                                            │  Dry contact → buzzer    │
                                            │  → magnetic lock         │
                                            └──────────────────────────┘
```

A long-lived tokio background task owns the eWeLink WebSocket. The HTTP route hands it a `PressRequest` through an `mpsc` channel and awaits a `oneshot` ack with a 5-second timeout. The route holds the DB transaction OPEN while waiting; commits the tx row ONLY after a successful ack. If the timeout or any error fires, the DB transaction is rolled back and the route returns 503.

## 5. Data model

### 5.1 Schema change (migration v16)

Two coupled changes — one column, one CHECK-constraint widen + one row retag:

```sql
-- 1. Per-user opt-in flag for self-service door entry.
ALTER TABLE users ADD COLUMN allow_self_entry INTEGER NOT NULL DEFAULT 0;

-- 2. Widen services.kind CHECK to include 'single_entry', then retag the
--    seeded 'Fitness' row so it has a stable, name-independent handle.
--    SQLite cannot ALTER a CHECK in place, so this is the standard
--    create-new + copy + swap dance (same pattern as V13_USERS_REPLACE_CARDS).
--
--    services_new = same columns, CHECK widened to
--      kind IN ('generic', 'monthly_pass', 'single_entry').
--    INSERT INTO services_new SELECT id, kind, name_sk, name_en, ...
--      FROM services. Then:
UPDATE services_new
   SET kind = 'single_entry'
 WHERE name_sk IN ('Fitness') OR name_en IN ('Fitness');
--    DROP TABLE services; ALTER services_new RENAME TO services;
--    plus rebuilding indexes (partial unique on monthly_pass + any FKs).
```

The retag is idempotent (matches by name, sets kind to a fixed value). On fresh deployments where the row was already seeded, this is the only place the 'Fitness' row's kind changes. The partial unique index on `kind = 'monthly_pass'` is recreated unchanged.

No new tables, no new indexes (the door-route query filters by `user_id` which is already PK-indexed and `note LIKE 'door:%'` is fast enough on the small per-user row count).

### 5.2 Transaction rows written by the door route

| Scenario | `action` | `amount` | `service_id` | `valid_until` | `note` |
|---|---|---|---|---|---|
| 1st open today, active monthly pass | `visit` | `0` | `NULL` | `NULL` | `door: 1st` |
| 1st open today, no pass | `charge` | `-<single-entry price>` | `<single-entry service id>` | `NULL` | `door: 1st` |
| Nth open today (N ≥ 2) | `charge` | `0` | `NULL` | `NULL` | `door: 2nd` / `3rd` / `4th` / ... |

The visit-definition memo is preserved: `action='visit' OR (action='charge' AND amount<0 AND valid_until IS NULL)`. Zero-amount charges fall outside it, so reports continue to count exactly one visit per customer per day.

### 5.3 In-memory state (no DB)

- Per-user rate limit: `Arc<Mutex<HashMap<i64, VecDeque<Instant>>>>` inside the door route's shared state. Each entry holds the timestamps of the last presses; window math computes both the 10s and 5/min checks. Pruning is incremental.
- Global rate limit: `Arc<Mutex<VecDeque<Instant>>>`, 30/min cap.
- Both reset on server restart. Acceptable: the 5/min and 30/min caps are anti-abuse, not accounting.

### 5.4 Secrets (env vars, not DB)

| Var | Purpose |
|---|---|
| `EWELINK_EMAIL` | eWeLink account email |
| `EWELINK_PASSWORD` | eWeLink account password |
| `EWELINK_DEVICE_ID` | Paired MINI-D serial (from eWeLink phone app) |
| `EWELINK_REGION` | Optional cache (`eu` / `us` / `cn` / `as`); auto-discovered on first login if absent |

If `EWELINK_DEVICE_ID` is empty / unset, the module starts in `Disabled` state and every press attempt returns `EwelinkError::Disabled` → route returns 503. Dev and CI environments run cleanly with no eWeLink configuration.

## 6. Door-open route — state machine

```
POST /api/door/open
Authorization: Bearer <jwt>
(no body)

  1. Extract user_id from JWT.
  2. SELECT allow_self_entry, role, credit FROM users
       WHERE id = ? AND deleted_at IS NULL.
       not found / deleted        → 403 "ask_reception"
       allow_self_entry = 0       → 403 "not_allowed"
                                     EXCEPT when role IN ('admin', 'staff')
                                     — they bypass the flag entirely (deviation
                                     from initial spec, see commit 0dfe85b).
                                     Customers still require the CEO to enable
                                     allow_self_entry. The original prompt's
                                     "users allowed by CEO config" is the
                                     per-customer toggle; admin/staff need
                                     no opt-in because they manage the place.
  3. Rate-limit check (per-user + global).
       exceeds                    → 429 "rate_limited"
  4. BEGIN DB TRANSACTION.
  5. SELECT COUNT(*) AS n FROM transactions
       WHERE user_id = ?
         AND note LIKE 'door:%'
         AND date(created_at, 'localtime') = date('now','localtime')
         AND deleted_at IS NULL.
  6. If n = 0:
       a. Check active monthly pass:
            SELECT 1 FROM transactions
              WHERE user_id = ?
                AND action='charge'
                AND valid_until > datetime('now')
                AND deleted_at IS NULL
              LIMIT 1.
       b. If pass active:
            row = {action='visit', amount=0, note='door: 1st'}
       c. Else:
            price = SELECT default_price FROM services WHERE kind='single_entry' AND active=1
            row = {action='charge', amount=-price, service_id=<single_entry>,
                   note='door: 1st'}
            UPDATE users SET credit = credit - price WHERE id = ?.
     Else (n >= 1):
       row = {action='charge', amount=0, note=format!("door: {ordinal(n+1)}")}
  7. INSERT INTO transactions (...) VALUES (...).
       Do NOT commit yet.
  8. ewelink_handle.press().await
       a. Ok within 5s    → COMMIT. 200 with payload below.
       b. Err or timeout  → ROLLBACK. 503 "hardware_unavailable".
  9. Record press timestamp into rate-limit state on Ok.
```

**Race note**: two simultaneous taps from the same user could read `n=0` twice and both try to write `door: 1st`. The 10-second per-user rate limit makes this practically impossible. We accept it; if it ever happens, the second insert is a regular zero-amount audit row showing the user got two `1st` entries — harmless, fully traceable.

**Response payload (200):**

```json
{
  "status": "opened",
  "reason": "ok",
  "new_credit": 27.50,
  "door_count_today": 1,
  "charged": false
}
```

**Response payload (4xx/5xx):**

```json
{
  "status": "rejected",
  "reason": "not_allowed" | "rate_limited" | "hardware_unavailable" | "auth"
}
```

`ordinal(n)` is a small helper that returns `"1st"`, `"2nd"`, `"3rd"`, `"4th"`, ..., capped at `"99th"`; defensively clamps higher values.

## 7. eWeLink client module

### 7.1 File layout

```
crates/spinbike-server/src/ewelink/
├── mod.rs       — public API: EwelinkHandle, spawn(), press()
├── auth.rs      — HMAC-SHA256 login, region discovery, token refresh
├── ws.rs        — WSS connect, ping/pong, push command, ack routing
├── crypto.rs    — AES-128-CBC fallback for older devices (MINI-D bypasses)
└── error.rs     — EwelinkError enum
```

### 7.2 Public API

```rust
pub struct EwelinkHandle {
    tx: mpsc::Sender<PressRequest>,
}

pub struct PressRequest {
    pub ack: tokio::sync::oneshot::Sender<Result<(), EwelinkError>>,
}

impl EwelinkHandle {
    /// Spawn the background WS task. Returns immediately.
    /// Reads EWELINK_* env vars; starts in Disabled if any required var is empty.
    pub fn spawn() -> Self;

    /// Send a press command. Resolves when the device acks (≤5s) or errors.
    pub async fn press(&self) -> Result<(), EwelinkError>;
}
```

### 7.3 Background-task lifecycle

```
spawn
  │
  ▼
read env → if device_id empty → enter Disabled (press() always errs)
  │
  ▼
┌─────────────────────┐
│ login (auth.rs)     │  ← refresh on 401 or token-expiry
│  POST /v2/user/login│
│  HMAC-SHA256(secret)│
│  → access_token,    │
│    region           │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│ open WSS            │
│  wss://{region}-    │
│  dispa.coolkit.cc   │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│ userOnline handshake│
│  → apikey, deviceid │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────────────────────────────┐
│ select! loop:                               │
│   - press req from mpsc                     │ on error: reconnect with
│     → send {action:update,                  │ exponential backoff
│        params:{switch:"on"}}                │ (1s, 2s, 4s, 8s, cap 30s)
│     → wait for matching ack                 │
│     → relay ack to oneshot                  │
│   - ws frame from peer (ping/pong/keepalive)│
│   - tokio interval: ping every 60s          │
└─────────────────────────────────────────────┘
```

### 7.4 Why MINI-D bypasses `crypto.rs`

Sonoff devices manufactured after 2022 (including MINI-D R4) use protocol v3 where the WSS connection itself provides TLS encryption and payloads are JSON plaintext. `crypto.rs` exists for fallback against older firmware but the active code path for MINI-D never invokes it.

### 7.5 Inching mode

Configured ONCE on the device via the Sonoff phone app: Inching → ON → 3000 ms. The server therefore only ever sends `{"switch": "on"}`; the device auto-OFFs after 3 seconds. The server never sends `"off"` — simplifies the command path and removes a class of "command sent but ack lost" bugs.

### 7.6 Error taxonomy

```rust
pub enum EwelinkError {
    Auth(String),         // 401 on /login, bad creds
    Network(String),      // WS dropped, DNS, TLS
    DeviceOffline,        // eWeLink says the device is unreachable
    DeviceTimeout,        // 5-second ack timeout
    BadResponse(String),  // malformed JSON
    Disabled,             // env vars missing → module disabled
}
```

## 8. Customer dashboard UI (`/my-balance`)

### 8.1 Layout

```
┌─────────────────────────────────────────────────┐
│  SpinBike            🇸🇰 SK ▼     [logout]       │  ← existing top nav
├─────────────────────────────────────────────────┤
│                                                 │
│              Hello, Štefan                      │
│                                                 │
│      ┌─────────────────────────────────┐        │
│      │  Credit                         │        │
│      │       € 27.50                   │        │
│      └─────────────────────────────────┘        │
│                                                 │
│      ┌─────────────────────────────────┐        │
│      │  Monthly pass                   │        │
│      │  Active until 2026-05-31        │        │  ← or "Not active"
│      └─────────────────────────────────┘        │
│                                                 │
│      ┌─────────────────────────────────┐        │
│      │       🔓 Hold to open door      │        │  ← hidden if
│      │       ━━━━━━━━━━━━━━━━━ 60%     │        │  ← allow_self_entry=0
│      └─────────────────────────────────┘        │
│                                                 │
│      Recent visits                              │
│      • 2026-05-10  visit  (pass)                │
│      • 2026-05-08  charge €-3.30                │
│      • 2026-05-06  visit  (pass)                │
│      • 2026-05-04  topup  €+50.00               │
│                                                 │
└─────────────────────────────────────────────────┘
```

### 8.2 Button state machine

| State | Visual | Triggered by |
|---|---|---|
| `idle` | "🔓 Hold to open door", solid primary color | default |
| `holding` | progress ring fills 0→100% over 2 seconds | `pointerdown` |
| `firing` | "Opening…" + spinner | hold reached 2 seconds, request in flight |
| `success` | green "✅ Door open — step in" for 3 s | 200 response |
| `error_503` | red "Door unavailable — ask reception" for 5 s | 503 response |
| `error_429` | gray "Wait a moment…" for 5 s | 429 response |
| `not_allowed` | gray "Ask reception for entry" tooltip (button visible but disabled) | `allow_self_entry=false` |
| `hidden` | not rendered at all | `allow_self_entry=false` AND role ≠ customer |

Cancel rules: `pointerup` / `pointerleave` / `pointercancel` before progress reaches 100% → reset to `idle`, RAF loop stopped, no request sent. Unified `pointer*` events cover phone and desktop.

### 8.3 Page routing

- Customer JWT lands on `/` → redirect to `/my-balance`. Staff/admin land on `/staff` as today (unchanged).
- Customer-facing routes: `/my-balance`, `/my-bookings`, `/schedule`. All other routes redirect to `/my-balance` or return 403, per existing role guards.

### 8.4 Recent visits

Reuse the existing transactions-list component (`transactions_list.rs`). Filter to `user_id = current_user`, last 20 rows. Door-related rows render with a small lock icon next to the date so the customer can recognise their entries at a glance.

### 8.5 i18n

All new strings keyed in `i18n.rs` (`door_button_idle`, `door_button_holding`, `door_button_firing`, `door_success`, `door_unavailable`, `door_rate_limited`, `door_not_allowed`, `door_lock_icon_aria`). Slovak strings written unaccented per project convention.

## 9. Admin allowlist toggle

### 9.1 Existing user-edit modal

```
┌────────────────────────────────────┐
│  Edit user: Štefan Sumerling  [×]  │
├────────────────────────────────────┤
│  Name        [Štefan Sumerling   ] │
│  Email       [stefan@…           ] │
│  Phone       [+421…              ] │
│  Card code   [AF-xyzkqlmn        ] │
│  Allow debit [✓]                   │
│  ────────────────────────────────  │
│  Allow self-entry  [☐]             │  ← new
│  (door open from PWA without staff)│  ← help text
│  ────────────────────────────────  │
│                                    │
│           [Cancel]   [Save]        │
└────────────────────────────────────┘
```

### 9.2 API

`PUT /api/users/:id` already exists. Request body gains optional field `allow_self_entry: Option<bool>`. The route updates the column **only when the caller has `role='admin'`**; staff can edit other fields but the server enforces a separate guard on this one field (returns 403 if a staff token tries to set it). Existing fields keep their current authorization.

### 9.3 Users-by-movement list

Add a small `🔓` badge next to user names where `allow_self_entry=true`. Pure visual indicator. No bulk toggle, no dedicated allowlist page (YAGNI).

## 10. Error handling, security, observability

### 10.1 Error response matrix

| Failure | HTTP | Client UX | Tx written? | Log level |
|---|---|---|---|---|
| Missing/expired JWT | 401 | redirect to `/login` | no | DEBUG |
| User not found / soft-deleted | 403 | "Ask reception" | no | WARN |
| Role ≠ customer | 403 | "Ask reception" | no | WARN |
| `allow_self_entry = false` | 403 | "Ask reception for entry" | no | INFO |
| Rate limit (10s / 5min per user, 30/min global) | 429 | "Wait a moment…" | no | INFO |
| eWeLink WS disconnected | 503 | "Door unavailable" | no | ERROR |
| eWeLink press timeout (5s) | 503 | "Door unavailable" | no | ERROR |
| eWeLink reports device offline | 503 | "Door unavailable" | no | ERROR |
| DB error during tx insert (post-press) | 500 | "Try again" | no (rolled back) | ERROR |
| Success | 200 | green "Door open" | yes | INFO |

### 10.2 Security

- JWT auth required (existing middleware).
- Customer JWTs cannot reach admin/staff endpoints — existing `require_staff` / `require_admin` middleware.
- `allow_self_entry` writeable only by admin (server-side guard inside `PUT /api/users/:id`).
- Secrets in env, never in DB, never logged. `tracing` env-filter masks `EWELINK_PASSWORD`.
- Per-user rate limit + global cap protect against credential-leak abuse.
- CSRF not applicable (Bearer token).
- Full audit trail in `transactions.note` — searchable forever.

### 10.3 Observability (per comprehensive-logging memo)

- `tracing::info!(?user_id, ?door_count_today, ?charged, ?new_credit, "door open success")` on success.
- `tracing::warn!(?user_id, ?reason, "door open rejected")` on every reject path.
- `tracing::error!(?err, ?user_id, "ewelink press failed")` on hardware fail.
- `tracing::error!(?err, ?ws_state, "ewelink ws disconnected")` on each WS drop, including reconnect-attempt counter.
- The eWeLink WS task emits `tracing::debug!` on every frame for 3 a.m. debugging.
- Axum `request_id` middleware propagates through the ewelink call → one grep correlates all log lines.

### 10.4 Health endpoint

`GET /api/door/health` (admin/staff only):

```json
{
  "ewelink_ws": "connected" | "disconnected" | "disabled",
  "last_ack_ms_ago": 123
}
```

Used by the admin "More" sheet to expose "Door system: ✅" at a glance. Used by external monitoring if desired.

## 11. Testing

### 11.1 Unit tests (Rust)

| Module | Tests |
|---|---|
| `ewelink::auth` | HMAC-SHA256 signature against fixed vectors; region URL routing for `eu`/`us`/`cn`/`as`; token-refresh on 401 |
| `ewelink::ws` | mock `tokio-tungstenite` server: connect → handshake → press → ack relays to oneshot; reconnect-with-backoff after drop; 60s ping/pong keepalive |
| `routes::door` | rate-limit window math; same-day count SQL returns correct N; price lookup from `services` row; tx row shape for each scenario |
| `db::users::allow_self_entry` | column round-trip; default 0; admin-only write enforced |

### 11.2 Integration tests (`tests/door_*.rs`)

The route tests use a stubbed `EwelinkHandle` (success / timeout / offline) injected through axum state. Seven scenarios from the state machine asserted end-to-end: tx row shape, credit deduction or not, response JSON, log emissions.

Migration v16 idempotency test: run the migration runner twice, expect no error, `allow_self_entry` column present on users, exactly one services row with `kind='single_entry'`, the partial unique index on `kind='monthly_pass'` still rejects duplicates.

### 11.3 E2E tests (`e2e/tests/door-open.spec.ts`)

The server exposes a test seam via `EWELINK_TEST_MODE=success|timeout|offline` env var. When set, `EwelinkHandle::press()` returns the configured result after a 100 ms delay. CI sets `success` for the happy-path spec and flips to `timeout` / `offline` via the test-fixtures route inside the failure specs. NO real eWeLink cloud is touched in CI.

Specs:

- **Happy path** — customer login → land on `/my-balance` → see credit + pass + button → hold 2 s → assert button transitions (`idle → holding → firing → success`) → green "Door open" banner → recent-visits list shows new `door: 1st` row.
- **allow_self_entry=false** — button hidden / "Ask reception" tooltip.
- **Rate limit** — five quick presses (test seam bypasses the 2s hold), sixth shows "Wait a moment…".
- **Hardware fail** — `EWELINK_TEST_MODE=offline` → red banner, no new tx row.
- **Admin path** — admin login → open user-edit modal → toggle `allow_self_entry` → save → customer view updates (poll API).
- **Customer view scoping** — customer JWT requests `/staff`, `/admin`, `/reports`, `/settings` → all redirect or return 403.

Every spec ends with `assertCleanConsole(messages)`.

### 11.4 Mutation testing

Existing server `cargo-mutants --in-diff` covers the new route + ewelink module. Target ≥80% mutation score on the new code surface.

## 12. Rollout

ONE PR contains migration v16 + ewelink module + door route + admin toggle + customer button + i18n + E2E.

Order of operations on merge:

1. Merge to `main`.
2. CI deploys server binary to the fitness machine; migration v16 runs on startup.
3. Pair the MINI-D once via the Sonoff phone app (one-time, ~5 min). Set Inching = ON, 3000 ms.
4. Set `EWELINK_EMAIL`, `EWELINK_PASSWORD`, `EWELINK_DEVICE_ID` (and optionally `EWELINK_REGION`) in the server's secrets file; restart the spinbike-server service.
5. Health endpoint shows `ewelink_ws: connected`.
6. CEO logs in as themselves on phone, opens `/door`, holds the button at the front door (no `allow_self_entry` toggle needed for admin/staff — they bypass the flag, per commit `0dfe85b`).
7. CEO toggles `allow_self_entry=true` for the first batch of trusted customers via the admin user-edit modal.

If the eWeLink integration ever breaks: clear `EWELINK_DEVICE_ID`, restart server. Module enters `Disabled`. All `/api/door/open` calls return 503; PWA shows "Door unavailable"; staff still operates the buzzer manually from the reception phone. No DB rollback needed.

## 13. Open questions / future work

- Email notification to CEO when a customer enters debt (`credit < 0` after a charge). Out of scope; existing negative-balance report covers this asynchronously.
- Hold-to-open accessibility for users who cannot hold a button (motor impairments). Future: add a long-press alternative such as double-tap. Not in MVP.
- Multi-door / multi-relay support. The `EwelinkHandle` API is intentionally singular today; extending to a map of `device_id → handle` is a small change later.
- Automatic detection that the MINI-D's inching mode has been altered (e.g. someone reconfigured it via the Sonoff app and forgot). Not detectable through the WS API; rely on smoke tests.
