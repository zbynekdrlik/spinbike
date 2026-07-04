---
name: spinbike-db-migrations
description: >
  SpinBike database migration procedures, visit-definition SQL pattern, and
  prod-synced-dev-DB validation workflow. Load before writing any migration,
  backfill, or query that touches visit/charge/transaction counts.
triggers:
  - migration
  - backfill
  - schema change
  - visit count
  - last_visit
  - prod db
  - dev db sync
  - validate against real data
---

# SpinBike DB Migrations & Data Validation

## Visit definition (canonical SQL pattern)

A "visit" (návšteva) is the UNION — never just `action='visit'` alone:

```sql
(action = 'visit')
OR (action = 'charge' AND amount < 0 AND valid_until IS NULL)
```

The `charge` branch covers customers paying per-class from card credit (negative amount = credit decrement; `valid_until IS NULL` excludes monthly-pass purchases). Using only `action='visit'` under-counts visits and breaks "last visit" displays for per-class customers.

Apply in: server SQL subqueries/joins, tests (seed BOTH branches), specs/docs.

Reference implementation: `crates/spinbike-server/src/db/reports.rs` lines 72-73, 202-203.

## Migration planning — exhaustive grep before scope is locked

When dropping or renaming a column on a heavily-referenced table:

1. `grep -rn 'TABLE\.COLUMN\b' . --include='*.rs' --exclude-dir=target` across the whole repo
2. Include: jobs (`charger.rs`), integration tests (`tests/*.rs`), test fixtures (`#[cfg(test)]` blocks and `helpers/mod.rs`), WASM frontend, V-migration idempotency tests in `db/migrations.rs`
3. Add each match site to the plan task's "Files" block before writing the migration
4. Verify sign/format invariants against actual prod data before writing comparison logic (legacy data may use different formats: `MM/DD/YY` vs `YYYY-MM-DD`, positive vs negative amount conventions)

A planning miss here causes a cascade of runtime failures (prod code) or CI failures (tests).

## Validate migrations against prod-synced dev DB before merge

For any PR that adds a schema migration mutating existing rows, a backfill/data-fix subcommand, or changes how serialised data is written/read:

**CI green alone is NOT sufficient** — CI uses fresh in-memory SQLite that cannot catch real-data quirks.

Steps before sending the completion report:
1. Confirm sync is recent: `systemctl list-timers spinbike-sync-dev.timer`
2. Snapshot dev DB: `cp /opt/spinbike/dev/spinbike-dev.db /tmp/dev-pre-X.db`
3. `sudo systemctl stop spinbike-dev.service` (release WAL locks)
4. Run the migration/backfill against the dev DB
5. Spot-check counts and sample rows match expectations
6. If broken: restore snapshot, fix, re-run
7. Restart dev service and verify via UI/API

Both prod and dev run locally — no SSH needed. Run `sqlite3` / `systemctl` / `journalctl` directly via Bash.

## Dev CI must sync prod DB before install

The `deploy-dev` job in `.github/workflows/ci.yml` MUST sync prod → dev BEFORE installing the new binary:

```
sqlite3 /opt/spinbike/prod/spinbike.db ".backup /opt/spinbike/dev/spinbike-dev.db"
```

- Use `.backup` (SQLite online backup API) — never plain `cp` on WAL mode
- Stop dev service first, wipe `.db-wal` / `.db-shm`, then sync, then install, then `start` (not `restart`)
- Sync must be unconditional (every dev push), never conditional on workflow inputs
- Do NOT carry over WAL/SHM from a previous dev run

## Functionally verify an authenticated route on live dev/prod — no account passwords needed

For post-deploy verification of a backend route change (not a UI change),
you don't have — and shouldn't need — any real customer's password. Build
your own valid JWT locally instead of logging in:

1. Read the running service's `JWT_SECRET` from its env file (local, no SSH):
   `sudo -n cat /etc/default/spinbike-dev` / `spinbike-prod` (also has
   `EWELINK_*` for the door route — dev is intentionally unset, prod is real).
2. Insert a throwaway user row directly via `sqlite3` — give it an
   unmistakable email so cleanup is trivial and unambiguous, e.g.
   `autopilot-test-<issue#>-<case>@local.invalid`. Set exactly the columns
   your test case needs (`blocked`, `allow_self_entry`, `role`, `credit`, …).
3. Sign a token with `python3 -c 'import jwt; jwt.encode({...}, secret,
   algorithm="HS256")'` (PyJWT is preinstalled) — match the exact `Claims`
   shape (`sub`, `email`, `role`, `exp`, `iat`) from
   `crates/spinbike-core/src/auth.rs`. Route handlers that re-query the DB
   for role/flags (like door.rs) don't even care what `role` the JWT claims —
   only `sub` (the user id) matters for those.
4. `curl` the route directly on `127.0.0.1:<port>` (8081 dev / 8080 prod) with
   `Authorization: Bearer <token>` — no need to go through the public HTTPS
   domain or worry about CORS (CORS is browser-only, irrelevant to curl).
5. **Always clean up in the SAME session**: `DELETE FROM users WHERE email
   LIKE 'autopilot-test-%'` (and any transaction rows it created) before
   moving on. Verify the count is 0 afterward.

This is safe on PROD too for a REJECTION-path test (e.g. a blocked-user gate)
— by definition a working rejection never reaches a real side effect (relay
press, charge), so the worst case of a bug is the SAME risk as the bug you're
fixing, caught in a controlled way instead of by a real member.

## Local access paths (no SSH needed)

```
/opt/spinbike/prod/spinbike.db          # production SQLite
/opt/spinbike/dev/spinbike-dev.db       # dev SQLite (prod-synced)
/opt/spinbike/dev/spinbike-server       # deployed dev binary
systemctl status spinbike.service        # prod service
systemctl status spinbike-dev.service    # dev service
sudo journalctl -u <service>             # logs
```
