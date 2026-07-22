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

**Door `single_entry` service = the SAME row as seeded 'Fitness' (V16 retag — NOT a separate service).** Migration V16 re-tags the seeded Fitness row's `kind` to `single_entry`, keeping `name_en='Fitness'` (`migrations.rs` ~656-660; confirmed on prod: services id=2 kind=single_entry name_en=Fitness). Consequence: every `service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))` filter (CLASS_VISIT_NAMES_EN) ALREADY includes all door self-entry transactions — do NOT "fix" such filters to add door entries (a 2026-07-21 ticket was mis-scoped on exactly this false premise; disproved empirically on the prod DB). Door Nth-press `charge amount=0` audit rows stay excluded by the OR-pattern.

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

## Lighter validation for a READ-ONLY predicate/VIEW migration (no row mutation)

The full stop-service snapshot dance above is for migrations that MUTATE rows or
run a backfill. A migration that only ADDS a VIEW or changes a query's
predicate (no `UPDATE`/`DELETE`, nothing that touches `dev`'s running service)
needs a cheaper but still real validation: run the OLD predicate and the NEW
predicate side-by-side as plain read-only `sqlite3` queries against the LIVE
prod DB and diff the results — no service stop needed since nothing is
written:

```bash
sqlite3 /opt/spinbike/prod/spinbike.db "
SELECT count(*) FROM <old predicate rows>
WHERE NOT (<new predicate>);
"   # 0 = the two predicates agree on every real row today
```

Also check the TIE-BREAK, not just set membership — for any query with a
"return the winner" comparator (a correlated subquery's `ORDER BY … LIMIT 1`,
a window function's `PARTITION BY`), confirm the winning row is IDENTICAL
between old and new for every group with more than one candidate row, not just
that both narrow to the same overall set. Two predicates can agree on "does
this row count" while still disagreeing on "which row wins" once ties are
possible. This caught nothing wrong in #159, but it is the check that WOULD
have caught it if the view's `ROW_NUMBER()` tie-break had drifted from the
old `ORDER BY valid_until DESC, id DESC` convention.

This is real evidence — CI's in-memory SQLite can't see production data
quirks — without touching the running service, so it's safe to do for ANY
predicate-only change, not just ones that pass the mutation trigger above.

## GOTCHA: a VIEW or TRIGGER referencing `services`/`transactions` breaks the V8/V11/V16 rebuild pattern

If a migration adds a `CREATE VIEW` or `CREATE TRIGGER` that references
`services` or `transactions` (e.g. V18's `user_active_pass` view, V20's
`enforce_active_pass_invariant` trigger), any FUTURE migration that needs the
established DROP-TABLE + CREATE-new + INSERT + RENAME rebuild pattern on
EITHER of those two tables (used by V8, V11, V16 to work around SQLite's
"can't ALTER to add a CHECK constraint") will fail with `no such table:
main.services` (or `main.transactions`) at the `ALTER TABLE … RENAME`
step — SQLite reparses EVERY dependent object's stored SQL during the rename
(views AND triggers alike), and at that instant the table doesn't exist yet
under its final name.

**Fix pattern:** `DROP VIEW <name>` / `DROP TRIGGER <name>` for every such
dependent object before the rebuild, re-run the exact `CREATE VIEW`/`CREATE
TRIGGER` statement(s) after the `RENAME`, all inside the same migration
transaction. Worked example:
`db::migrations::tests::v8_drop_rename_pattern_works_with_fk_child_rows`
(had to be updated when V18 landed, then again when V20 landed — it manually
re-simulates a future `services` rebuild and hit this exact failure both
times; now drops+recreates BOTH the view and the trigger).

## GOTCHA: adding a schema INVARIANT (CHECK/trigger) needs a grep sweep across THREE separate trees, not one

Unlike a column rename (covered above — one `TABLE\.COLUMN\b` grep across the
whole repo), a new value-level invariant (e.g. V20's "valid_until implies
charge+monthly_pass") is validated per-INSERT, so you must independently sweep
every `INSERT INTO <table>` site that could set the guarded column, in THREE
places that do NOT share one grep root: `crates/spinbike-server/src/`
(app code + unit tests), `crates/spinbike-server/tests/` (integration
tests — a SIBLING of `src/`, invisible to a `src/`-scoped grep and the exact
gap that cost a wasted CI cycle on #204), and `e2e/tests/*.spec.ts` (seeds
via `/api/test/seed-transactions` / `/api/test/seed-expired-pass`, which pass
through the real route so they're usually already compliant — but check).
`grep -rn "INSERT INTO <table>" <each root> --include=*.rs` (and `*.ts` for
e2e) separately; do not assume one root's sweep covers another.

## GOTCHA: a migration that ALTERs `schema_version` ITSELF can't be relied on inside its own INSERT, on a fresh install

`run_migrations` (db/mod.rs) applies `MIGRATIONS` in a single loop, in
version order, from whatever `current_version` already is. If a migration
adds a column to `schema_version` (V19's `ALTER TABLE schema_version ADD
COLUMN checksum TEXT`, #170), that column does NOT exist yet when EARLIER
migrations in the SAME fresh-install run reach their own
`INSERT INTO schema_version (...)` — V19 hasn't executed yet at that point,
even though it's later in the same `MIGRATIONS` array. Concretely: on a
brand-new DB (`current_version=0`), the loop applies v1, v2, ... v18 (each
inserting its own `schema_version` row) BEFORE it ever reaches v19's
`ALTER`. Any code that unconditionally writes to a not-yet-added column in
the per-migration INSERT will error with `no such column` on a fresh
install, even though the exact same code works fine on an UPGRADE (where
`current_version` is already ≥18 and the loop only runs v19).

**Fix pattern used for the checksum column:** don't try to write the new
column from inside the per-migration INSERT at all. Leave it NULL there
(nullable column, no special-casing needed), then add a SEPARATE pass
AFTER the whole apply loop that walks every migration in `MIGRATIONS` and
backfills/verifies the column — by that point every migration (including
the one that added the column) has been applied, so the column is
guaranteed to exist. This one AFTER-pass handles the fresh-install path
(all versions were NULL going in) and the incremental-upgrade path
(pre-existing versions had no column at all until this run's ALTER) with
the SAME code, no branching on which scenario you're in. Worked example:
`db::run_migrations`'s post-loop checksum loop, tested from BOTH angles —
`db::tests::fresh_db_backfills_checksum_for_every_migration` (full 1..=19
chain in one call) and `migrations::tests::
v19_checksum_backfills_on_genuine_upgrade_from_v18` (1..=18 pre-committed
via the `apply_sql_block` idiom, mirroring a REAL pre-upgrade database,
THEN `run_migrations` applies only v19) — an independent code review
caught that the first test alone doesn't actually exercise the real
production upgrade path, only the fresh-install shape of it.

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

## DB error type — the query layer returns `DbError`, not `anyhow` (#163)

The `db` query submodules (classes/users/transactions/settings/login_tokens/
persistent_bookings/reports) return `Result<T, DbError>` — the thiserror enum in
`crates/spinbike-server/src/db/error.rs`. Only `db/mod.rs`'s startup/infra fns
(`create_pool`, `create_memory_pool`, `run_migrations`) stay on `anyhow::Result`
(app boundary; the `Migration v{n} failed` context is load-bearing there).

- **`DbError::from(sqlx::Error)` classifies unique violations** into
  `DbError::UniqueViolation` (via `db_err.is_unique_violation()` — a provided
  `DatabaseError` trait method that is callable on the `dyn DatabaseError`
  receiver WITHOUT importing the trait). Variants: `UniqueViolation`, `NotFound`,
  `ClassFull`, transparent `DateParse`/`IntParse`/`Sqlx`.
- **Handling a db error in a route:** the generic path is
  `.map_err(internal_error)?` (`internal_error(e: impl Display)` takes a DbError
  for free and logs it → 500, body leaks nothing). To branch on a kind, use
  `matches!(e, crate::db::DbError::UniqueViolation)`.
- **GOTCHA — use `crate::db::DbError`, NOT `db::DbError`, in routes.** Route
  files alias `db` to a SUBMODULE (`use crate::db::classes as db;`), so
  `db::DbError` resolves to that submodule's private import (E0603 "private").
  The public re-export lives at `crate::db::DbError`.
- **When blanket-changing db error types, grep `sqlx::Result` too, not just
  `use anyhow`** — the layer was a mix; `create_user` was on `sqlx::Result`. And
  every non-sqlx `?` (chrono `parse_from_str`, `str::parse` for `ParseIntError`)
  needs a `#[from]` variant on `DbError` or it won't convert. `cargo fmt --all`
  collapses the `.await` / `?` split left after removing `.context(...)`.

## GOTCHA: never compare a DATE column against `datetime('now')` with `>`/`<` (#179, MONEY bug)

DATE columns like `transactions.valid_until` (monthly-pass expiry) are stored as
a bare `YYYY-MM-DD` (10 chars) — `routes/payments.rs::sell_pass` binds a
`chrono::NaiveDate`. SQLite compares TEXT **byte-wise**, and a 10-char bare date
is a PREFIX of the 19-char `datetime('now')` (`'2026-07-11'` vs
`'2026-07-11 20:22:47'`), so the shorter string sorts as **LESS**. Therefore
`valid_until > datetime('now')` is **FALSE on the pass's exact expiry day** — the
pass reads as already-expired on its own last valid day. This was a real
OVERCHARGE: the door route charged a single entry on a day the pass still
covered (customer's last paid day).

**Canonical form** — coerce BOTH sides with `date()` and use INCLUSIVE `>=` (a
pass is valid THROUGH its last day; the charger treats it that way):

```sql
date(valid_until) >= date('now')      -- ✅ inclusive, calendar-date compare
valid_until       >  datetime('now')  -- ❌ off-by-one on the expiry day
```

- Reference forms: `jobs/charger.rs` (the canonical inclusive compare, against
  the booking's date), `routes/door.rs` + `routes/users.rs::my_balance` (both
  fixed to match in #179).
- The "is this an active monthly pass?" predicate lives ONCE in the
  `user_active_pass` view (V18). Route any NEW pass check through the view —
  never hand-roll an Nth copy (#159 unified 6 sites, #179 finished the 7th).
- `db/users.rs::get_user_pass_valid_until` / `get_user_pass_tx` also wrap the
  view's `valid_until` in `date(...)` before decoding into `chrono::NaiveDate`,
  defending the decode against a hypothetical future full-datetime row.
- Day-boundary basis is the **gym's LOCAL day (Europe/Bratislava)**, RESOLVED by
  #205 (owner: a pass is valid THROUGH the whole of its last gym-local day).
  Do NOT use `date('now')` (UTC) or `date('now','localtime')` (server-OS zone,
  fragile) for a pass-expiry "today" — compute it in Rust via
  `crate::util::today_bratislava()` (named IANA `Europe/Bratislava` via
  `chrono-tz`, DST-correct, OS-TZ-independent) and pass it as a **bound `?`
  param**: `date(valid_until) >= ?`. Every "today"-relative pass check uses this
  ONE helper — `door.rs`, `my_balance.rs`, `payments.rs::log_visit`/`sell_pass`,
  `users.rs` days_remaining, and the charger `tick()` window
  (`util::now_bratislava()`). The charger's pass-vs-booking compare is EXEMPT:
  it compares `valid_until` against the booking's own bare calendar date (both
  already gym-local), no "today"/`date('now')` involved.

## GOTCHA: gym-local "today" — `NaiveDateTime::date()` and the UTC-CI-runner test flake (#205)

- **`NaiveDateTime` has `.date()`, NOT `.date_naive()`.** `date_naive()` is a
  `DateTime<Tz>` method. `today_bratislava()` first wrote
  `now_bratislava().date_naive()` (where `now_bratislava()` returns a
  `NaiveDateTime`) → clippy `E0599`, one wasted CI cycle. Use `.date()` on a
  `NaiveDateTime`; `.date_naive()` only on the tz-aware `DateTime<Tz>`.
- **CI Test/E2E jobs run on `ubuntu-latest` (UTC).** Any boundary test that
  seeds "today" via `chrono::Local::now()` or SQL `date('now')` (both = UTC on
  the runner) and then exercises a handler that now uses `today_bratislava()`
  will DIVERGE and FLAKE in the ~00:00–02:00 UTC window (Bratislava is already
  the next day). Fix: seed such tests via
  `spinbike_server::util::today_bratislava()` so test-today == handler-today on
  ANY runner TZ. Expired-YESTERDAY guards can stay `date('now','-1 day')` —
  robust, since Bratislava-today is always ≥ UTC-today.
- **This bites `chrono::Local::now()` on the DEV BOX too, invisibly, if the box
  itself runs `Europe/Bratislava`** (this repo's dev machine does) — a test
  using `chrono::Local` then PASSES locally (dev-box TZ happens to already
  match the handler's Bratislava anchor) and only flakes on the UTC CI runner,
  during the exact 00:00-02:00 Bratislava window. Confirmed live (#251):
  `attendance_counts_only_fitness_and_spinning_visits` passed every local run
  and failed on CI the moment a push landed in that window. **Never trust "it
  passed when I ran it locally" as proof a `chrono::Local`-seeded test is
  CI-safe** — grep for `chrono::Local::now()` in any test touching
  day-bucketed data and replace with `today_bratislava()`, regardless of
  whether it currently passes on your box.
- **The #239-242 "exhaustive" audit only grepped `spinbike-ui/` (frontend
  display parsers) — it MISSED the SERVER's own raw SQL bucketing.** #251
  found `db/reports.rs`'s `day_report`/`range_report` still doing
  `WHERE date(t.created_at) = ?` / `BETWEEN ? AND ?` (the exact anti-pattern
  the "bare-DATE vs UTC-INSTANT column" gotcha below describes) — a real,
  live business-impact bug (the Reports page under-counts attendance/cash-in
  for up to 2h after Bratislava midnight), never caught because nobody grepped
  `crates/spinbike-server/src/db/*.rs` for `date(` alongside the UI-side
  `parse_server_date(` sweep. **When auditing this bug class, grep BOTH
  halves of the codebase** — `grep -rn 'date(.*created_at\|date(.*_at)' crates/spinbike-server/src/db/` for the server's raw SQL, not just the UI's date parsers.
- **The bug class extends to E2E specs, not just Rust tests — same fix
  shape, TypeScript side.** A Playwright spec computing "today"/"tomorrow" via
  `new Date().toISOString().slice(0, 10)` (or `Date.now() + N * 3600_000`)
  disagrees with `today_bratislava()` in the same window, for the same
  reason. `e2e/tests/helpers.ts` now exports `bratislavaToday()` (Intl-based:
  `new Date().toLocaleDateString('en-CA', { timeZone: 'Europe/Bratislava' })`)
  and `bratislavaDateOffset(days)` (pure calendar-date arithmetic via
  `Date.UTC` on the broken-down Y/M/D — no UTC-instant-arithmetic ambiguity).
  Use these for ANY E2E assertion against a day/range-bucketed endpoint;
  never widen a UTC-instant margin (e.g. "+48h") as a substitute — that masks
  the symptom without fixing the actual date the test asserts against (see
  the `e2e-testing` skill's own entry for the live before/after example).

## GOTCHA: gym-local day boundary — bare-DATE column vs UTC-INSTANT column (#222)

Two DIFFERENT shapes, pick by the column's storage type:

- **Bare calendar-date column** (`bookings.date` = `'YYYY-MM-DD'`): bind
  `util::today_bratislava()` (a `NaiveDate`) and compare directly —
  `b.date >= ?`. A byte-wise 10-char vs 10-char compare is a correct calendar
  compare. NEVER `date('now')` (UTC) / `date('now','localtime')` (OS zone).
- **UTC-INSTANT column** (`transactions.created_at` = `datetime('now')` =
  `'YYYY-MM-DD HH:MM:SS'` UTC): do NOT hand-convert in SQL. `date(created_at,
  'localtime')` reads the fragile server OS zone; a bound `today_bratislava()`
  alone can't help because the LHS conversion is still OS-dependent. Instead
  compute the gym day's UTC-instant half-open RANGE in Rust —
  `util::bratislava_day_range_utc(day) -> (start, end)` (start = UTC instant of
  the day's Bratislava local midnight, end = next day's) — `.format("%Y-%m-%d
  %H:%M:%S")` both, and compare `created_at >= ? AND created_at < ?`. Both sides
  are 19-char space-ISO UTC, so byte-wise == chronological. DST-correct (offset
  from tzdata). Used at `door.rs` same-day count (MONEY-adjacent: `n==0` drives
  the charge path — a wrong boundary double-charges or skips a pass check) and
  `users.rs` stats month/year totals + monthly buckets (there the 12 month
  boundaries are computed as `bratislava_day_range_utc(month_first(y,m)).0` and
  a `CASE created_at < bound_i THEN label_i` bucketer replaces
  `strftime(...,'localtime')`).

**A wall-clock-derived rollover branch inside a handler is mutation-untestable
→ extract it.** `user_stats` derives `today` from the live clock, so its
December year-rollover branch (`if today.month()==12 { year+1 }`) is unreachable
in an integration test, and widening (not narrowing) the `this_month` window
leaves every seeded-in-current-month assertion unchanged → the `==12` / `year+1`
mutants SURVIVE (cargo-mutants shard failed). Fix: extract the pure logic
(`next_month_first(day)`) and unit-test every arm with fixed dates
(July→Aug, Nov→Dec, Dec→Jan-next-year). General rule: any date/time boundary
branch keyed off the live `now` must live in a pure `fn(today: NaiveDate)` with
direct fixed-date unit tests, or its boundary mutants are uncatchable.

**Validate a `'localtime'`→UTC-range migration is behavior-neutral on prod
read-only** (host OS TZ IS Europe/Bratislava, so old == new today): compare the
two predicates row-for-row with SQLite's `datetime(<local-str>, 'utc')` modifier
(treats its LHS as localtime, converts to UTC — same instant the Rust helper
computes on a Bratislava host). Zero disagreements + equal counts = neutral:
```bash
DB=/opt/spinbike/prod/spinbike.db
# every door row falls in the gym-day range of its OWN localtime date → 0
sqlite3 "$DB" "SELECT COUNT(*) FROM transactions WHERE note LIKE 'door:%' AND deleted_at IS NULL
  AND NOT (created_at >= datetime(date(created_at,'localtime')||' 00:00:00','utc')
           AND created_at < datetime(date(created_at,'localtime','+1 day')||' 00:00:00','utc'));"
# stats this-month: old strftime-localtime == new UTC range (equal counts)
sqlite3 "$DB" "SELECT
 (SELECT COUNT(*) FROM transactions WHERE deleted_at IS NULL
    AND strftime('%Y-%m',created_at,'localtime')=strftime('%Y-%m','now','localtime')),
 (SELECT COUNT(*) FROM transactions WHERE deleted_at IS NULL
    AND created_at >= datetime(strftime('%Y-%m-01',date('now','localtime'))||' 00:00:00','utc')
    AND created_at <  datetime(strftime('%Y-%m-01',date('now','localtime','+1 month'))||' 00:00:00','utc'));"
```
Pure SELECTs → no service stop needed (the read-only-predicate validation path).

## `#[derive(sqlx::FromRow)]` matches columns by NAME, not position (#164)

This codebase has zero `#[sqlx(rename = ...)]` attributes anywhere, so every
`FromRow` struct's default derive decodes each field via
`Row::try_get("field_name")` — a NAME lookup against the query's result set,
not a positional one. Proven from an existing query that predates #164:
`users_by_last_movement`'s `SELECT u.id, u.name, u.card_code,
u.allow_self_entry, MAX(t.created_at) AS last_movement_at` lists
`allow_self_entry` BEFORE the `last_movement_at` alias, while
`UserByMovementRow`'s field order is `id, name, card_code,
last_movement_at, allow_self_entry` — the mismatch already worked correctly
before #164, which only makes sense under name-based matching.

**Consequence when writing an explicit-column `SELECT`** (replacing a
`SELECT *`, or writing a new query into an existing `FromRow` struct):
column ORDER in the SQL does not need to match the struct's field
declaration order — only that every struct field has a same-named column
(or `AS alias`) somewhere in the result set. Missing one is a runtime
"column not found" decode error (loud, not a silent field-shift), which is
exactly the failure mode #164 hardened every `SELECT *` site against. When
converting one, cross-check the struct's field list against the table's
CREATE/ALTER migration history (a field can be added by a LATER migration
than the table's original `CREATE TABLE`), not just the original schema —
`UserRow`'s `deleted_at`/`allow_self_entry` and `bookings`'s
`charged_at`/`charge_transaction_id` were both added by later `ALTER TABLE`
statements.

## GOTCHA: a partial-upgrade test whose applied range includes a TRIGGER (or any internal-`;` block) must use `sqlx::raw_sql`, not `apply_sql_block`

`apply_sql_block` (the test helper) strips `-- comments` then splits on `;` and runs
each fragment — fine for single-statement migrations, but it MANGLES a
`CREATE TRIGGER ... BEGIN ...; ...; END;` body (V20's `enforce_active_pass_invariant`)
because the internal semicolons split the trigger into invalid fragments. The
v19-genuine-upgrade test dodged this by applying only `<= 18`. A test that must apply
THROUGH V20+ (e.g. V21's "preserve rows on genuine upgrade from v20") must mirror
`run_migrations` instead: per migration `pool.acquire()` → `PRAGMA foreign_keys=OFF`
→ `conn.begin()` → `sqlx::raw_sql(sql).execute(&mut *tx)` → INSERT schema_version →
commit → `PRAGMA foreign_keys=ON`. `raw_sql` hands the whole block to SQLite (trigger
bodies stay intact). Worked example: `db::migrations::tests::
v21_preserves_existing_rows_on_genuine_upgrade_from_v20`.

## login_tokens rebuild needs NO view/trigger dance (unlike a services/transactions rebuild)

V21 rebuilds `login_tokens` (widen the `purpose` CHECK to add `'code'` + add an
`attempts` column) via the V8/V11/V16 DROP-new + INSERT + RENAME pattern. Unlike a
`services`/`transactions` rebuild, NO view or trigger references `login_tokens`
(V18's `user_active_pass` + V20's trigger reference services/transactions only), and
no table holds an FK INTO it — so this rebuild needs no DROP-VIEW/TRIGGER step, just
the runner's PRAGMA foreign_keys=OFF. The INSERT lists every old column explicitly
and OMITS the new `attempts` so pre-existing rows adopt its DEFAULT 0.
