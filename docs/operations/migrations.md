# SpinBike Schema Migrations Runbook

Migrations live in `crates/spinbike-server/src/db/migrations.rs`, registered in
the `MIGRATIONS` static. Each row is a `(version, description, sql)` tuple.
The runner applies pending migrations on server startup, in version order,
each inside a single transaction.

## Foreign-key enforcement during migrations

`run_migrations` (in `db/mod.rs`) toggles `PRAGMA foreign_keys = OFF` on
the migration's connection BEFORE `BEGIN`, runs the migration SQL,
COMMITs, and toggles `PRAGMA foreign_keys = ON` AFTER. This is required
for **table-rebuild migrations** that follow the SQLite-canonical
CREATE\_NEW + INSERT\_FROM\_OLD + DROP\_OLD + RENAME\_NEW pattern. V8
(`services_dual_lang_kind`) is the first such migration.

### Why not `PRAGMA defer_foreign_keys = TRUE`?

This was tried first and caught by tests. It does NOT work for the
table-rebuild pattern: SQLite registers the FK violation when `DROP TABLE`
implicitly deletes the parent rows, and the subsequent RENAME of the new
table to the old name does NOT clear the pending violation. The tx then
fails at COMMIT with `FOREIGN KEY constraint failed`.

`PRAGMA foreign_keys = OFF` is the only mechanism that works. It must be
issued **before** `BEGIN` (the pragma cannot be toggled inside an open
transaction), which is why the runner acquires a single connection and
toggles per-connection rather than per-transaction.

The `v8_drop_rename_pattern_works_with_fk_child_rows` test in
`db/migrations.rs` codifies this: it seeds a transaction with an FK ref,
runs the same CREATE/INSERT/DROP/RENAME pattern V8 uses, and asserts the
commit succeeds AND the FK ref still resolves afterwards.

### Writing a new table-rebuild migration

1. The runner already disables FK around every migration. No code change
   needed.
2. In your migration SQL, INSERT ... SELECT id, ... preserves the
   original ids so existing FK refs (in child tables) point to the same
   rows after the RENAME.
3. Add a unit test asserting the migration leaves a populated child
   table's FK refs intact (mirror the V8 test).

If your migration does NOT need FK toggle (simple ALTER TABLE,
CREATE INDEX, etc.), the toggle is a harmless no-op.

## Running migrations against a copy of prod

Both prod and dev databases live on this machine
(`/opt/spinbike/{prod,dev}/spinbike.db`). Before a release that ships a
new migration, dry-run against a fresh prod snapshot:

```bash
sudo sqlite3 /opt/spinbike/prod/spinbike.db ".backup /tmp/prod-snapshot.db"
sudo chown newlevel:newlevel /tmp/prod-snapshot.db

# Run the new binary against the snapshot — migrations apply on first
# pool open (cargo run, or ./target/.../spinbike-server with TARGET set).
DATABASE_PATH=/tmp/prod-snapshot.db ./target/release/spinbike-server &
SERVER_PID=$!
sleep 2; kill $SERVER_PID

# Verify schema_version updated
sqlite3 /tmp/prod-snapshot.db "SELECT MAX(version) FROM schema_version"

# Verify FK integrity
sqlite3 /tmp/prod-snapshot.db "PRAGMA foreign_key_check"
```

`PRAGMA foreign_key_check` returns no rows when every FK ref resolves to
a valid parent — the post-migration health check.

## Backups

Pre-deploy snapshots: `/opt/spinbike/prod/backups/spinbike-YYYYMMDD-HHMMSS.db`
(CI keeps the last 10).

Restore from backup if a migration corrupts prod:

```bash
sudo systemctl stop spinbike.service
sudo cp /opt/spinbike/prod/backups/spinbike-<ts>.db /opt/spinbike/prod/spinbike.db
sudo chown newlevel:newlevel /opt/spinbike/prod/spinbike.db
sudo systemctl start spinbike.service
```

The auto-rollback story is "redeploy a revert commit" (see
`environments.md` § Rollback). Migrations themselves have no automatic
rollback.

## Legacy data backfill

The `migrate-legacy --backfill --mdb-path X --target Y` subcommand walks
the legacy `.mdb` and sets `service_id` on transactions where currently
NULL. Idempotent (NULL-guard); marker column `legacy_backfilled = 1`
enables targeted rollback.

The backfill commits in batches of `BACKFILL_BATCH_SIZE` (1000) rows
rather than one giant transaction, so concurrent staff actions on prod
don't queue against a multi-minute writer lock. Each batch commit is
sub-second, leaving brief windows where other writers can interleave.

A targeted rollback after a botched backfill:

```sql
UPDATE transactions SET service_id = NULL WHERE legacy_backfilled = 1;
```

After ≥2 weeks of stable operation, a follow-up migration can drop the
`legacy_backfilled` column.
