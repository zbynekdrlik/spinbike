---
paths:
  - "crates/spinbike-server/src/db/**"
  - "crates/spinbike-server/src/jobs/**"
  - "crates/spinbike-server/src/routes/**"
  - "crates/spinbike-server/src/auth/**"
  - ".github/workflows/ci.yml"
---

# Prod + dev now live on the SpinBike VPS — access recipe (#350, 2026-08-12)

**Prod and dev are NOT on this machine (dev1).** Both moved to a dedicated
Hetzner VPS on 2026-08-12 (issue #350). Any command that touches the live
databases, the running services, their env files, or their logs needs to run
**on the VPS**, not in a local session on dev1.

## The ssh recipe

```bash
ssh -i ~/.ssh/spinbike_vps root@167.233.245.147 '<cmd>'
```

Run `sqlite3` / `systemctl` / `journalctl` / `sudo cat /etc/default/...`
yourself over that ssh connection — never ask the user to SSH in or paste
output for you. This is the ONE recipe every other playbook file below
points back to instead of repeating.

## Path / unit / port table (unchanged from before the move)

| What | Path / unit |
|---|---|
| Prod DB | `/opt/spinbike/prod/spinbike.db` |
| Dev DB | `/opt/spinbike/dev/spinbike-dev.db` |
| Prod env file | `/etc/default/spinbike-prod` |
| Dev env file | `/etc/default/spinbike-dev` |
| Prod backups | `/opt/spinbike/prod/backups/` |
| Prod service | `spinbike.service` (port 8080) |
| Dev service | `spinbike-dev.service` (port 8081) |
| Tunnel | `spinbike-tunnel.service` |
| Prod→dev nightly sync | `spinbike-sync-dev.timer` |
| Cloudflare tunnel config | `/home/newlevel/.cloudflared/config.yml` (on the VPS now — was `~/.cloudflared/config.yml` on dev1) |

## A session-side `curl 127.0.0.1:8080` / `:8081` reaches NOTHING

Those ports only exist on the VPS's own loopback. A `curl`/`sqlite3`/`cat`
run directly in a dev1 session against `127.0.0.1:808x` or
`/opt/spinbike/...` will silently fail or hang — it is not "prod is down",
it is the wrong host. Wrap it in the ssh recipe above, or run it from
inside an ssh session.

## SSH addresses the VPS by IP — there is no DNS name for the box

`spinbike.sk`, `www.spinbike.sk`, `spinbike.newlevel.media` and
`spinbike-dev.newlevel.media` all resolve to **Cloudflare** edge IPs
(`104.21.x` / `172.67.x`), not to the VPS. The app is delivered through the
Cloudflare Tunnel, so the box's real address appears in no DNS record at all,
and Cloudflare's proxy forwards only HTTP/HTTPS — `ssh spinbike.sk` knocks on
Cloudflare and is refused. Use the IP (or the `spinbike-vps` alias in dev1's
`~/.ssh/config`).

Consequence for credentials: **running the tunnel needs no Cloudflare API
access.** It was created once by an interactive `cloudflared tunnel login`
and since then only needs its credentials JSON. Do not assume tunnel access
implies DNS access — the two old tokens in `~/.secrets/cloudflare-account-*`
and `~/.secrets/cloudflare-newlevelmedia-admin` are DEAD (`Invalid API
Token`, checked 2026-08-12).

**The working DNS token is `~/.secrets/cloudflare-spinbike`** (also on the
VPS at the same path), scoped `Zone → DNS → Edit` on `spinbike.sk` only,
named `spinbike-dns · claude` in the Cloudflare UI. Read it from there
instead of asking the owner for another one.

### `/user/tokens/verify` LIES about a scoped token (#443)

A token scoped to one zone has no user-level permission, so
`GET /user/tokens/verify` answers **`401 Invalid API Token` for a perfectly
valid token**. Trusting that endpoint made this session report the owner's
correct token as broken three times, across four round trips, until he said
so outright. **Test a token against the resource it is FOR:**

```bash
# works  -> 200 and the zone object; this is the ONLY validity test
curl -s -H "Authorization: Bearer $(cat ~/.secrets/cloudflare-spinbike)" \
  "https://api.cloudflare.com/client/v4/zones?name=spinbike.sk"
```

Nor is length a test: this token is `xxxx_` + 48 chars (53 total), not the
40 that older documentation describes. **The API decides, not the shape.**

### Adding a record

`POST /zones/<zone_id>/dns_records` with `{"type":"A","name":"vps",
"content":"<ip>","ttl":1,"proxied":false}`. `proxied:false` is mandatory for
anything reached by SSH — Cloudflare's proxy forwards only HTTP/HTTPS. The
optional `comment` field is capped at **100 characters**; a longer one is a
flat 400.

`vps.spinbike.sk` already exists this way (created 2026-08-13, #350), so
`ssh root@vps.spinbike.sk` works and is preferred over the bare IP.

## Hand-minting a JWT against the live server

The claims struct is `crates/spinbike-core/src/auth.rs`. Two shapes bite:

- **`sub` is `i64` — a bare JSON number, not a string.** Nearly every JWT
  helper defaults `sub` to a string, and `serde` then refuses the payload.
- `role` must be exactly `"admin"` / `"staff"` / `"customer"` (lowercase).

`validate_token` collapses EVERY decode failure — bad signature, expired,
wrong field type, missing field — into the single message
`"Invalid or expired token"`, so a wrong-shape payload is indistinguishable
from a genuinely bad token. If a freshly minted token 401s, suspect the claim
types before suspecting the secret.

Read `JWT_SECRET` on the VPS (never print it): sign and call from a script
that runs there, so the value never crosses to this machine.

**Calling the PUBLIC host from a script gets `403 error code: 1010`** —
Cloudflare fingerprints non-browser clients and blocks `curl`/`urllib`
defaults. It looks exactly like an app outage. Either send a normal browser
`User-Agent`, or hit `http://127.0.0.1:8080` from ON the VPS. Full treatment
(and the DOM-verification counterpart) is in
`.claude/skills/prod-verification/SKILL.md` — this pointer exists because
that skill only loads when a session deliberately asks for it, and the 1010
has now cost two separate sessions (2026-07-24, 2026-08-12).

## Moving prod between hosts — the ordering and the proof

Learned doing #350; the same shape applies to any future host move.

1. **Stop the writers BEFORE snapshotting.** Tunnel first (traffic stops),
   then the app, and only THEN `sqlite3 ... ".backup"`. Snapshot-then-stop
   loses everything written in between; stop-then-snapshot cannot.
2. **`systemctl stop` is NOT `systemctl disable`.** The old host's units stay
   enabled and a reboot resurrects the whole stack: a second tunnel replica
   (Cloudflare load-balances between replicas, so real customers land on the
   STALE database) and a second eWeLink session — the two then kick each
   other every ~2s and door unlock breaks. Disable every unit on the old box
   the moment it stops being the live one.
3. **Prove no data was lost by checksum, not by counts.** Compare both sides
   with `SELECT id||'|'||...` piped through `sha256sum`, over ALL rows of
   `transactions` and `users` (credits included), plus per-table counts and
   max ids. Expect ONE legitimate difference: `login_tokens` shrinks, because
   `jobs::token_purge` deletes expired rows at startup (#119) — check the
   missing row's `expires_at` before treating it as loss.
4. **Verify reboot survival on the NEW box explicitly** (the owner has to
   approve the reboot). #350's test: 28s of downtime, then both instances,
   the tunnel, the sync timer and the Actions runner all returned unaided.

## The CI deploy runner is DIFFERENT — it needs NO ssh

The `spinbike-deploy` self-hosted Actions runner (workflow label
`[self-hosted, spinbike-deploy]` in `.github/workflows/ci.yml`) now runs
**on the VPS itself**, registered there as `spinbike-vps`, as user
`newlevel`. Its own job steps (`install -Dm755 ...`, `sudo -n systemctl
restart ...`, health checks) are genuinely LOCAL to that runner — they
execute ON the VPS already, so they need no ssh wrapper. Only a
human/agent SESSION (which runs on dev1) needs the ssh recipe above; the
CI runner's own steps never do.
