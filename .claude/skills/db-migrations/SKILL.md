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

## Local access paths (no SSH needed)

```
/opt/spinbike/prod/spinbike.db          # production SQLite
/opt/spinbike/dev/spinbike-dev.db       # dev SQLite (prod-synced)
/opt/spinbike/dev/spinbike-server       # deployed dev binary
systemctl status spinbike.service        # prod service
systemctl status spinbike-dev.service    # dev service
sudo journalctl -u <service>             # logs
```
