# Rename `Mesačný preplatok` → `Mesačná permanentka` (Issue #50)

## Goal

Fix incorrect Slovak label on the monthly-pass service row. `preplatok` means *overpayment / leftover credit balance*, not a pass. The correct word is `permanentka` (feminine), so the adjective also flips: `Mesačná` not `Mesačný`. The rest of the UI already uses `permanentka` (e.g. row title `Predaj permanentky · do <date>`) — this fix removes the inconsistency on customer/CEO-facing copy.

## Architecture

A single `name_sk` value in the `services` table changes from `'Mesačný preplatok'` to `'Mesačná permanentka'`. No schema change, no transaction backfill (transactions reference the row by `service_id`; the label is read live from `services.name_sk`).

Two surfaces need to learn the new name:

1. **Existing production / dev DBs** — add a new idempotent migration step `V14_RENAME_MONTHLY_PASS_LABEL` that runs `UPDATE services SET name_sk='Mesačná permanentka' WHERE name_sk='Mesačný preplatok'`. Re-runs match zero rows → no-op.

2. **Fresh databases (CI / new install)** — the V8 seed at `migrations.rs:252` still inserts the old name. The new V14 step runs immediately after V8 in `run_migrations()`, so a fresh DB ends up with the corrected label after the full migration chain. **V8 itself is NOT edited** (per `database-migrations.md`: never modify migrations that have already run on production).

3. **E2E specs** — five Playwright test files contain the literal `'Mesačný preplatok'`. Update to the new name. Two regex matches keep their `Monthly pass` English alternative; only the Slovak branch swaps.

The `migrate_legacy` binary and `db/backfill.rs` referenced in the original issue body were deleted in PR #67 (V13 users-replace-cards), so those paths are no longer in scope.

## Code impact

| File | Change |
|---|---|
| `crates/spinbike-server/src/db/migrations.rs` | Append `(14, "rename monthly_pass label to Mesačná permanentka", V14_RENAME_MONTHLY_PASS_LABEL)` to MIGRATIONS array. Add `V14_RENAME_MONTHLY_PASS_LABEL` const with the UPDATE SQL. Update existing V8 test assertion at line 778 from `"Mesačný preplatok"` to `"Mesačná permanentka"` (the test runs `run_migrations` which now includes V14). Add new V14 unit test (see "Tests" below). |
| `crates/spinbike-server/src/db/migrations.rs:252` | **NOT EDITED.** V8 is a frozen migration. |
| `e2e/tests/post-backfill-history.spec.ts:28` | `'Mesačný preplatok'` → `'Mesačná permanentka'` |
| `e2e/tests/visit-button-feedback.spec.ts:35,99` | `'Mesačný preplatok'` → `'Mesačná permanentka'` (2 occurrences) |
| `e2e/tests/services-admin.spec.ts:60,64` | Comment + assertion: `'Mesačný preplatok'` → `'Mesačná permanentka'` |
| `e2e/tests/dashboard-button-layout.spec.ts:34` | Regex `/Monthly pass\|Mesačný preplatok/` → `/Monthly pass\|Mesačná permanentka/` |
| `e2e/tests/log-visit-class-only.spec.ts:45` | Same regex shape — Slovak alternative swaps. |

`migrations.rs:799` (`'Druhý preplatok'`) is **out of scope** — it's a fixture for the V8 unique-index test (`v8_only_one_monthly_pass_allowed`) that asserts a SECOND monthly_pass insert fails. The string is intentionally distinct from the canonical row.

## Tests

Two new test surfaces, both inside `crates/spinbike-server/src/db/migrations.rs`:

1. **Update existing V8 test** (`v8_seeds_monthly_pass_and_generic_services` or whichever name covers line 778): change the assertion to `"Mesačná permanentka"`. Justification: the test runs `run_migrations` (full chain), so post-V14 the value is the new name.

2. **New V14 test** `v14_renames_monthly_pass_label`: 
   - Run all migrations.
   - Assert `SELECT name_sk FROM services WHERE kind='monthly_pass'` returns `'Mesačná permanentka'`.
   - Assert other services (`Spinning`, `Fitness`, `Občerstvenie`, `Doplnky výživy`, `Aktivácia karty`) have unchanged `name_sk`.
   - Run `run_migrations` a second time → no error, value still `'Mesačná permanentka'` (idempotency).

Mutation testing pressure: the second test exists specifically to kill mutants that change the WHERE clause (e.g. `WHERE name_sk LIKE '%preplatok%'` → would also match `Druhý preplatok` if seeded — but our test asserts only the canonical row changes and the V8-fixture row is not part of the seeded baseline). The first assertion + idempotency assertion together kill UPDATE-removal and predicate-drop mutants.

The existing E2E specs already cover the full UI render path; their literals just need the new value.

## Migration sequence

```
V1..V13         (existing, frozen)
V14_RENAME_MONTHLY_PASS_LABEL
  UPDATE services SET name_sk='Mesačná permanentka' WHERE name_sk='Mesačný preplatok';
```

No PRAGMA toggles needed — this is a pure UPDATE on an existing column.

## Rollout

1. CI runs the full migration chain on a fresh in-memory DB → V14 matches the row inserted by V8 → updates to the new name.
2. Deploy-dev step `.backup`s prod → dev, then runs the new binary which auto-applies V14 against the prod-shape data.
3. Spot-check on dev: `sqlite3 dev.db "SELECT name_sk FROM services WHERE kind='monthly_pass'"` returns `Mesačná permanentka`.
4. After user merges to main and prod deploys, Playwright spot-check on `https://spinbike.newlevel.media/staff?card=<known-card-with-pass>` confirms both the row title (`Predaj permanentky · do <date>`) and the service-label cell read `permanentka`.

## Acceptance criteria

- [ ] V14 migration step appended to MIGRATIONS array, idempotent UPDATE
- [ ] V14 unit test asserts new name + other-services unchanged + double-run idempotency
- [ ] V8 test assertion updated to `"Mesačná permanentka"`
- [ ] All 6 E2E string-literal occurrences updated across 5 spec files
- [ ] CI green (lint, test, build, E2E, mutation testing, deploy-dev, smoke-dev)
- [ ] Dev DB after deploy: `services.name_sk` for `kind='monthly_pass'` reads `'Mesačná permanentka'`
- [ ] Prod DB after merge + deploy: same row reads `'Mesačná permanentka'`; Playwright spot-check on a real card history confirms label consistency with row title

## Out of scope

- `'Druhý preplatok'` test fixture at `migrations.rs:799` — distinct purpose, kept as-is
- Any other Slovak label review (the issue notes the broader pattern but explicitly tracks this one row only)
