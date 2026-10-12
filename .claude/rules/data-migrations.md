---
paths:
  - "crates/spinbike-server/src/db/migrations.rs"
  - "crates/spinbike-server/src/db/instructors.rs"
  - "crates/spinbike-server/src/routes/admin.rs"
---

# Data migrations that rewrite seeded rows (V30, #381)

A migration that UPDATEs/DELETEs rows an earlier migration SEEDED (V30
renamed V6's instructor 1 "Stevo" -> "Stopa" and moved V6's Tue/Thu
templates to 18:30) silently breaks every test that ran the FULL chain and
asserted the old seed. Before writing one, grep the seeded values across
`crates/spinbike-server/src`, `crates/spinbike-server/tests` AND
`e2e/tests` (three separate roots — see the db-migrations skill).

- **Test an old migration's own seed at its own version**:
  `pool_migrated_to(N)` (in `migrations.rs` tests), never `run_migrations`
  then assert V-N's output. Re-running V-N's SQL for an idempotency check:
  `sqlx::raw_sql(MIGRATIONS.iter().find(|(v, _, _)| *v == N).unwrap().2)`.
- **Match rows by id AND a guard column** (`WHERE id = 2 AND weekday = 1`),
  then add a `MIGRATION_POSTCONDITIONS` entry (`SELECT COUNT(*) = 2 ...`) so
  a DB whose ids differ fails loudly instead of half-applying — and add the
  failure-path test (`v16_/v27_/v30_postcondition_fails_loudly_*`).
- **Deleting referenced-by-FK rows**: `class_templates.instructor_id`
  REFERENCES `instructors(id)` and every app pool runs `foreign_keys(true)`
  (the migration runner turns FKs OFF). Delete only rows nothing references
  (`WHERE id NOT IN (SELECT instructor_id ... WHERE instructor_id IS NOT
  NULL)` — the `IS NOT NULL` matters, a NULL in a `NOT IN` list matches
  nothing). Any route writing `instructor_id` must check existence first
  (`db::instructors::exists`) or an unknown id is an FK 500.
- **Dry-run on a prod COPY, no service stop**: on the VPS,
  `sqlite3 /opt/spinbike/prod/spinbike.db ".backup <scratch>/prod-copy.db"`,
  run the migration SQL from a file via `sqlite3 -init <file.sql> <copy> .quit`
  (BEGIN … postcondition SELECT … COMMIT; `PRAGMA foreign_key_check;`), read
  the result, then delete the copy (it holds customer PII).
