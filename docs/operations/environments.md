# SpinBike Environments Runbook

Both environments run on the dedicated SpinBike Hetzner VPS
(`167.233.245.147`), NOT on this machine — migrated off dev1 on 2026-08-12
(#350). Reach the VPS with `ssh -i ~/.ssh/spinbike_vps root@167.233.245.147
'<cmd>'` and run every `systemctl`/`sqlite3`/`journalctl` command below over
that connection (see `.claude/rules/vps-access.md` for the canonical
recipe) — never ask the user to SSH in or paste output.

| Env  | URL                                 | Port | Service                | DB                                         | Branch |
|------|-------------------------------------|------|------------------------|--------------------------------------------|--------|
| prod | https://spinbike.newlevel.media     | 8080 | spinbike.service       | /opt/spinbike/prod/spinbike.db             | main   |
| dev  | https://spinbike-dev.newlevel.media | 8081 | spinbike-dev.service   | /opt/spinbike/dev/spinbike-dev.db          | dev    |

## One-time rollout

See `scripts/setup-environments.sh`. Run once as the `newlevel` user on the
runner machine — that machine is now the SpinBike VPS (ssh in per
`.claude/rules/vps-access.md` first):

```bash
./scripts/setup-environments.sh
```

Then do the Cloudflare steps manually (need Cloudflare auth). **Ordering
matters**: the tunnel must be restarted *after* the DNS CNAME exists,
otherwise the edge caches a "hostname exists but no WebSocket" state for
the new hostname and `/api/ws` upgrades return 400 until the next tunnel
restart.

Run this on the VPS (see `.claude/rules/vps-access.md`):

```bash
# 1. Create the DNS CNAME first
cloudflared tunnel route dns spinbike spinbike-dev.newlevel.media

# 2. Edit /home/newlevel/.cloudflared/config.yml (on the VPS, not this
#    machine) per deploy/cloudflared/config.yml.example

# 3. Restart the tunnel LAST so it re-registers with the edge against the
#    hostname that now exists in DNS
sudo systemctl restart spinbike-tunnel.service
```

### Adding another env hostname later

Same ordering. If you ever see `400 Bad Request` on `/api/ws` for a newly-
added hostname while regular HTTP works fine, fix with (run on the VPS —
see `.claude/rules/vps-access.md`):

```bash
sudo systemctl restart spinbike-tunnel.service
```

The edge's per-hostname WebSocket registration refreshes on connector
re-announcement. Deploy CI's `Wait for <env> site health` step now probes
the WS upgrade as part of readiness, so a stuck edge state will fail the
deploy loudly rather than poison post-deploy smoke tests.

## Daily operations

- Push to `dev` → CI auto-deploys to dev env. Safe to push broken code here.
- Merge `dev` → `main` → CI auto-deploys to prod env after running DB backup.
- Nightly at 03:00, `spinbike-sync-dev.timer` copies the prod DB over the dev
  DB so dev tests against realistic data.

## Service catalog

Services are managed via the admin UI (`/admin?tab=services`) and stored
in the `services` table with dual-language names (`name_sk`, `name_en`)
plus a stable `kind` enum (`generic` | `monthly_pass`). The `kind` is
read-only after create — admin can rename either language label freely
without breaking sell-pass / pass-banner detection.

### Seeded categories

V8 (2026-04-27) seeded three new generic services for snack / supplement /
activation-fee sales: **Občerstvenie / Refreshments**, **Doplnky výživy /
Supplements**, **Aktivácia karty / Card activation fee**. Their
`default_price` is `0.0` by design — legacy data shows snack/supplement
prices vary widely (€0.66 to €278), so there's no useful default. Staff
types the actual price each time. Admin can set a per-service default via
the services tab if a single price emerges.

### Migration runbook

For any new schema migration (V10+), see `docs/operations/migrations.md` —
the foreign-key handling around table-rebuild migrations has gotchas worth
reading before writing one.

## Inspecting service state

Run these on the VPS (see `.claude/rules/vps-access.md`):

```bash
systemctl status spinbike.service spinbike-dev.service
journalctl -u spinbike.service -n 100
journalctl -u spinbike-dev.service -n 100
```

## Backups

Pre-deploy snapshots live in `/opt/spinbike/prod/backups/` **on the VPS**
(see `.claude/rules/vps-access.md`) as `spinbike-YYYYMMDD-HHMMSS.db`. CI
keeps the last 10.

### Restore from backup

Run this on the VPS (see `.claude/rules/vps-access.md`):

```bash
sudo systemctl stop spinbike.service
sudo cp /opt/spinbike/prod/backups/spinbike-<ts>.db /opt/spinbike/prod/spinbike.db
sudo chown newlevel:newlevel /opt/spinbike/prod/spinbike.db
sudo systemctl start spinbike.service
```

## Secret rotation

Run this on the VPS (see `.claude/rules/vps-access.md`):

```bash
# Generate new secret
NEW=$(openssl rand -hex 32)
sudo sed -i "s|^JWT_SECRET=.*|JWT_SECRET=$NEW|" /etc/default/spinbike-prod
sudo systemctl restart spinbike.service
```

All existing user sessions invalidate on the next request — expected.

## Rollback (forward-fix)

There is no automated binary rollback. To revert a bad prod deploy:

1. Push a revert commit to `dev` (`git revert <sha>`).
2. Merge `dev` → `main`.
3. CI redeploys the reverted code to prod.

If prod DB was corrupted by the bad deploy, restore from backup (see above)
BEFORE redeploying, so migrations run against the clean snapshot.
