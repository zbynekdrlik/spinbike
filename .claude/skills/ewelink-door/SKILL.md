# eWeLink Door Unlock (Sonoff MINI-D)

Load before touching `crates/spinbike-server/src/ewelink/*`, `routes/door.rs`,
the `/api/door/open` + `/api/door/health` endpoints, or door credentials.

## What it is

Allowlisted customers (or admin/staff, who bypass the `allow_self_entry`
gate) tap "hold-to-open" on `/my-balance`. Server presses a **Sonoff MINI-D**
Wi-Fi dry-contact relay over the eWeLink cloud WebSocket. The MINI-D drives
the legacy fitness-center door buzzer. Billing mirrors reception:
1st open/day = `visit` (or `charge -<single-entry>` if no pass), 2nd+ = `charge 0`;
trail lives in `transactions.note` (`door: 1st` / `door: 2nd` …).

## Hard-won integration gotchas (all verified live 2026-07, device `10028e311b`)

1. **v1 login response OMITS `error` on success.** `LoginResp.error` MUST be
   `#[serde(default)]` (0 = success) — otherwise every real login fails with
   `bad response: error decoding response body` in an infinite retry loop.

2. **The WS URL is DYNAMIC — never hard-code it.** The retired legacy
   `wss://{region}-dispa.coolkit.cc:8080/dispatch/app` now refuses TLS
   (`tlsv1 alert internal error`). Resolve per-connection:
   `GET https://{region}-dispa.coolkit.cc/dispatch/app` (bearer token) →
   `{"error":0,"domain":"eu-pconnectN.coolkit.cc","port":443}` →
   connect `wss://{domain}:{port}/api/ws`. See `resolve_dispatch_url`.

3. **MINI-D is a MULTI-OUTLET product — the actuation frame MUST use the
   `switches` array.** The single-channel `params:{"switch":"on"}` is
   SILENTLY DROPPED (no ack → door route times out 5 s → 503
   `hardware_unavailable`). Correct frame:
   `params:{"switches":[{"outlet":0,"switch":"on"}]}` → device acks `error:0`.
   Outlet 0 is the dry-contact relay.

4. **The 2 s pulse is HARDWARE-ENFORCED, not server-timed.** MINI-D **Inching
   Mode** (`pulses:[{outlet:0,pulse:"on",width:2000}]`, `swMode:2`) is set
   ONCE in the eWeLink phone app. The server ONLY ever sends `switch:"on"`;
   the device turns itself off after 2000 ms. A stuck/duplicate press can
   NOT leave the door held open — this is the CEO's explicit requirement.
   Don't add a server-side "off" press.

5. **ONE app-session per eWeLink account.** The cloud kicks the older WS with
   a `Bye` close frame whenever a second session (phone app, dev server, a
   probe) logs in with the same account → endless reconnect pingpong.
   Consequence: **EWELINK_ creds live ONLY on prod** (`/etc/default/spinbike-prod`),
   NEVER on dev — two servers on one account fight forever. If the phone app
   is logged in it also fights the server; log out of the app, OR (future)
   make a dedicated eWeLink account for the server and share the MINI-D to it.

## Config (prod only)

`/etc/default/spinbike-prod` (NOT git, NOT dev):
```
EWELINK_EMAIL=…        # the account the MINI-D is registered under
EWELINK_PASSWORD=…     # plain literal (systemd EnvironmentFile — no shell escaping)
EWELINK_DEVICE_ID=10028e311b
```
Any of the three empty/unset → module runs Disabled fast-path (`press()`
returns immediately; useful kill-switch). `APP_ID`/`APP_SECRET` are the public
sonoffLAN constants, hard-coded in `auth.rs`. Region defaults `eu`.

## Verify after any change

1. `journalctl -u spinbike -f | grep ewelink` → expect `WS connected + handshake ok`
   and NO repeating `peer sent close (Bye)` churn (churn = a second session on the account).
2. `GET /api/door/health` (admin/staff JWT) → `{"ewelink_ws":"connected","last_ack_ms_ago":…}`.
   `null` = never pressed; a number = last device ack age (proof the frame round-trips).
3. Live press: `POST /api/door/open` → `200 {"status":"opened"}`, health then shows
   `last_ack_ms_ago` small (~100–200 ms), a `transactions` row `note='door: Nth'`.
   Physical buzz confirmation is user-only (remote relay) — ask the person on site.

## Test the WS protocol without the server

`websocket-client` probe: login → resolve dispatch → `userOnline` → send an
`update` frame → print the ack. Lets you confirm a frame format against the
REAL device before changing Rust. (It steals the account's WS session briefly;
prod reconnects after.) Fire it with the switches-array params to actuate.
