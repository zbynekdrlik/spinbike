---
paths:
  - "crates/spinbike-server/src/db/**"
  - "crates/spinbike-server/src/jobs/**"
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

## The CI deploy runner is DIFFERENT — it needs NO ssh

The `spinbike-deploy` self-hosted Actions runner (workflow label
`[self-hosted, spinbike-deploy]` in `.github/workflows/ci.yml`) now runs
**on the VPS itself**, registered there as `spinbike-vps`, as user
`newlevel`. Its own job steps (`install -Dm755 ...`, `sudo -n systemctl
restart ...`, health checks) are genuinely LOCAL to that runner — they
execute ON the VPS already, so they need no ssh wrapper. Only a
human/agent SESSION (which runs on dev1) needs the ssh recipe above; the
CI runner's own steps never do.
