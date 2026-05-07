# V13 Orphan persistent_bookings Fix — Design

**Date:** 2026-05-06
**Issues:** [#70](https://github.com/zbynekdrlik/spinbike/issues/70), [#69](https://github.com/zbynekdrlik/spinbike/issues/69)
**Type:** Migration hardening + test coverage
**Status:** Approved

## Problem

`crates/spinbike-server/src/db/migrations.rs` step 7b of the V13 SQL block migrates `persistent_bookings.card_id → user_id` via:

```sql
INSERT INTO persistent_bookings_new (id, user_id, template_id, created_at, ended_at)
SELECT pb.id, c.user_id, pb.template_id, pb.created_at, pb.ended_at
FROM persistent_bookings pb
JOIN cards c ON c.id = pb.card_id;
```

The `INNER JOIN` silently drops any `persistent_bookings` row whose `card_id` does not resolve in `cards` (e.g., orphan from legacy import or external delete). Production currently has zero `persistent_bookings`, so the bug is dormant on prod, but a future fresh import could lose rows without warning.

A second, related gap: the existing test `migration_13_users_replace_cards_full_round_trip` only seeds well-linked rows. The synthetic `(deleted)` user fallback at step 5 (orphan transactions) is exercised only by ad-hoc dev-DB validation, not by the unit test (#69).

## Goal

Tighten V13 orphan handling so that no `transactions` or `persistent_bookings` row is silently dropped during the cards-to-users migration, regardless of seed-data shape, and prove it via the existing migration unit test.

## Frozen-rule justification

`database-migrations.md` forbids editing migrations that have already run on prod. V13 ran on prod with `schema_version=14` recorded; it will never re-run there. The proposed edit affects only the FRESH-DB code path (CI tests, future imports). A V15 reconcile step cannot recover already-dropped rows, so it would reduce to a purely performative pre-flight check. The inline edit is the only fix that actually closes the gap, and its blast radius is bounded to never-yet-executed code paths.

## Design

### Single-unit change

One coherent edit to `const V13_USERS_REPLACE_CARDS` and its companion test. No new migration step, no schema-version bump, no Rust API change.

### SQL change — step 5 (extend conditional creation of `(deleted)` user)

Step 5 currently inserts the synthetic user only when an orphan transaction exists. After the change, it also fires when an orphan `persistent_booking` exists, so step 7b can rely on the row being present.

```sql
INSERT INTO users (email, name, role)
SELECT NULL, '(deleted)', 'customer'
 WHERE EXISTS (SELECT 1 FROM transactions WHERE user_id IS NULL)
    OR EXISTS (SELECT 1 FROM persistent_bookings pb
                LEFT JOIN cards c ON c.id = pb.card_id
                WHERE c.user_id IS NULL);
```

### SQL change — step 7b (LEFT JOIN + COALESCE)

```sql
INSERT INTO persistent_bookings_new (id, user_id, template_id, created_at, ended_at)
SELECT pb.id,
       COALESCE(c.user_id,
                (SELECT id FROM users WHERE name = '(deleted)' ORDER BY id DESC LIMIT 1)),
       pb.template_id, pb.created_at, pb.ended_at
  FROM persistent_bookings pb
  LEFT JOIN cards c ON c.id = pb.card_id;
```

Outcomes:

| `pb.card_id` state | `c.user_id` after LEFT JOIN | result |
|---|---|---|
| valid → resolved cards row | non-null | `c.user_id` (unchanged behavior) |
| NULL | NULL | `(deleted)` user_id |
| points to non-existent cards row | NULL | `(deleted)` user_id |

### Safety: ordering invariant

Step 5 → step 6 → step 7 → step 7b → step 8. The `(deleted)` user is created in step 5 and survives until step 8 (which only drops the `cards` table). Step 7b's `SELECT id FROM users WHERE name = '(deleted)' ORDER BY id DESC LIMIT 1` correctly resolves it. The `ORDER BY id DESC LIMIT 1` mirrors the existing step-5 reference pattern (defensive against duplicate-named rows).

### Test changes

File: `crates/spinbike-server/src/db/migrations.rs`, function `migration_13_users_replace_cards_full_round_trip`.

**New seed rows (BEFORE `run_migrations`):**

1. Orphan transaction — `transactions(card_id=NULL, user_id=NULL, action='charge', amount=-5.0, …)`. Covers #69.
2. Orphan persistent_booking — `persistent_bookings(card_id=999, template_id=<existing>, …)` where `cards.id=999` does not exist in the seed. Covers #70.

**New assertions (AFTER `run_migrations`):**

```rust
// Synthetic user exists.
let deleted_user_id: i64 = sqlx::query_scalar(
    "SELECT id FROM users WHERE name = '(deleted)' ORDER BY id DESC LIMIT 1",
)
.fetch_one(&pool).await.unwrap();

// Orphan transaction maps to it.
let txn_user: i64 = sqlx::query_scalar(
    "SELECT user_id FROM transactions WHERE amount = -5.0 AND legacy_backfilled = 0",
)
.fetch_one(&pool).await.unwrap();
assert_eq!(txn_user, deleted_user_id);

// Orphan persistent_booking maps to it.
let pb_user: i64 = sqlx::query_scalar(
    "SELECT user_id FROM persistent_bookings WHERE id = <orphan_pb_id>",
)
.fetch_one(&pool).await.unwrap();
assert_eq!(pb_user, deleted_user_id);
```

The `(orphan_pb_id)` is captured from the seed `INSERT … RETURNING id` (or the autoincrement value).

### Mutation-pressure considerations

Surviving mutants the new test must kill:

- `COALESCE(c.user_id, deleted)` → `COALESCE(c.user_id, NULL)`: assertion fails because `pb.user_id` would be NULL violating NOT NULL constraint OR the assert_eq fails.
- `COALESCE(c.user_id, deleted)` → `c.user_id`: same — INSERT would fail at NOT NULL.
- `LEFT JOIN` → `JOIN` (revert): orphan PB row vanishes; the `SELECT user_id FROM persistent_bookings WHERE id = <orphan_pb_id>` query returns no rows; test panics on `fetch_one`.
- Step-5 `OR EXISTS (… persistent_bookings …)` → removed: with no orphan transaction, `(deleted)` user is never created; step-7b `SELECT id FROM users WHERE name = '(deleted)' …` returns NULL; INSERT fails NOT NULL. To force this branch, the seed needs the orphan PB without an orphan transaction — handled by giving the orphan transaction `legacy_backfilled=0` while leaving Bob's transaction `legacy_backfilled=1`, or by ensuring the test seeds orphan PB independently. Alternative mutant kill: the test can run a second sub-scenario with PB-only orphan, OR the assertion `users WHERE name='(deleted)'` count = 1 catches the empty case.

The single-test design covers all four mutants by including BOTH orphan rows in the same seed (the OR-EXISTS still flips on transactions branch alone, but kill-by-construction is acceptable here since cargo-mutants would surface any surviving mutant on next CI).

If the EXISTS-extension mutant survives in practice, add a second test `migration_13_orphan_pb_only_creates_deleted_user` that seeds ONLY an orphan PB (no orphan transaction) and asserts `users WHERE name='(deleted)'` count = 1. Defer until cargo-mutants reports the surviving mutant.

## Components touched

- `crates/spinbike-server/src/db/migrations.rs`: V13 SQL const (step 5 + step 7b), unit test seed + assertions.
- `VERSION`: bump 0.13.23 → 0.13.24, sync via `bash scripts/sync-version.sh`.

## Out of scope

- Editing any other migration step.
- Adding a defensive runtime check around `run_migrations` (rejected option B).
- Closing the issue without a fix (rejected option C).
- Touching `routes/`, `db/users.rs`, frontend, or E2E suites.

## Validation gates

1. `cargo fmt --all --check` locally.
2. CI green: lint, test (incl. new unit-test assertions), build WASM, E2E, mutation testing, deploy-dev, smoke-dev.
3. Mutation testing on the diff: zero surviving mutants for the changed lines.
4. PR mergeable + clean before reporting done.

Post-merge: prod deploy is a no-op for the migration (V13 already past). Verify only via `[data-testid="version"]` reading `v0.13.24` on `https://spinbike.newlevel.media`.

## Risk

Bounded. The edit changes one INSERT … SELECT in a code path that prod has already passed. No backward-compat shim, no schema change, no data migration. Worst case: an undiscovered cargo-mutants survivor — addressed by the defer-to-CI escape clause above.
