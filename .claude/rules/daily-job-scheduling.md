---
paths:
  - "crates/spinbike-server/src/bin/server.rs"
  - "crates/spinbike-server/src/jobs/**"
  - "crates/spinbike-server/src/lib.rs"
---

# Daily background jobs — wall-clock-aligned, never `tokio::time::interval(86400s)`

Two spawn blocks in `bin/server.rs` had the SAME bug independently (`jobs::notifications`
fixed in #264, `jobs::token_purge` fixed in #297): a plain
`tokio::time::interval(Duration::from_secs(86400))` measures its first tick from
task-SPAWN time, i.e. server-RESTART time. Every subsequent tick then fires exactly
N\*86400s after that restart, pinning the "daily" job to whatever wall-clock moment
the process last happened to restart (e.g. 03:00 after an overnight deploy) — forever,
until the next restart re-pins it. It also drifts across Bratislava's two DST
transitions a year (23h/25h calendar days).

**#299 extracted the sleep-loop those two blocks duplicated into
`jobs::spawn_daily_job(pool, hour, job_name, unit, tick)` — any new daily (or otherwise
wall-clock-scheduled) job calls that helper from `bin/server.rs` instead of copying a
spawn block:**

```rust
spinbike_server::jobs::spawn_daily_job(
    pool.clone(),
    spinbike_server::jobs::<your_job>::DAILY_RUN_HOUR,
    "<your job's log name>",
    "<noun for what tick's Ok(n) counts, e.g. \"rows removed\" / \"sent\">",
    |pool| async move { spinbike_server::jobs::<your_job>::tick(&pool).await },
);
```

`job_name` is reused in EVERY log line (sleep/error), so keep it a plain identity
phrase; `unit` is used ONLY in the success line (`"{job_name}: {n} {unit}"`) — a
separate parameter exists specifically so the count-carrying line still says what `n`
counts (a review on #299 caught the first draft collapsing both jobs' bespoke
`"{n} rows removed"` / `"sent {n} notifications"` wording into a job-identity-only
string, silently losing that information).

If your job's `tick` needs an extra dependency beyond `&SqlitePool` (like
`notifications::tick`'s `&PushHandle`), capture it in the closure instead of widening
`spawn_daily_job`'s own signature — see the `push notifications` call site in
`bin/server.rs` for the pattern (`move |pool| { let push = push.clone(); async move {
notifications::tick(&pool, &push).await } }`).

- Give the job its own `pub const DAILY_RUN_HOUR: u32` in its module (not a shared
  constant, and not owned by the helper) — pick an hour that doesn't collide with the
  OTHER daily jobs, so they never compete for the DB pool at the same instant.
  Currently: `notifications` = 9 (customer-visible reminders, mid-morning),
  `token_purge` = 4 (pure housekeeping, off-peak), `pass_renewal` = 5 (#374 —
  contiguous monthly-pass auto-renewal; off-peak, and BEFORE `notifications`=9
  so a renewal suppresses that user's redundant "pass expiring" push).
- `duration_until_next_bratislava_hour` (in `util.rs`) is the shared, already-tested
  helper `spawn_daily_job` itself calls — don't reinvent DST-safe scheduling per job.
- The DELAY arithmetic is pinned per-hour in `token_purge.rs`'s `daily_run_hour_*`
  tests (single-job) and `jobs/mod.rs`'s own tests (parameterized over both current
  job hours) — before-target-hour and at/after-target-hour cases. A new job doesn't
  strictly need its own copy (the arithmetic is already covered generically), but
  adding one at your job's specific hour is cheap insurance if that hour is adjacent
  to another job's.
- `spawn_daily_job`'s own execution (sleep, then actually call `tick`, then log
  correctly) is covered by three tests in `jobs/mod.rs`, all built on a shared
  `spawn_and_drive_one_tick` helper: `spawn_daily_job_runs_tick_after_the_computed_delay`
  (proves the loop calls `tick` at all — a mutation run MISSED the whole function body
  replaced with `()` until this existed), and
  `spawn_daily_job_logs_info_when_tick_returns_a_positive_count` /
  `_does_not_log_info_when_tick_returns_zero` (prove the `Ok(n) if n > 0` log-gating
  guard is actually exercised both ways — 5 more mutants on that one guard were MISSED
  until these existed, since the first test always returns `Ok(0)`). All three use a
  paused virtual clock via **manual `tokio::time::pause()` called AFTER pool
  creation** — NOT `#[tokio::test(start_paused = true)]`, which was tried first and
  reverted: pausing before the pool exists lets tokio's auto-advance-when-idle race
  ahead of the pool's own connect-acquire deadline and fail with "pool timed out while
  waiting for an open connection" (tokio's `test-util` feature is still the
  dev-dependency enabling `time::pause`/`advance` — see `Cargo.toml`).
- Still run your job's `tick` once at startup, directly in `main()`, BEFORE the
  `spawn_daily_job` call — the helper only owns the recurring loop, not the startup
  tick, so the first observable log line appears right after boot instead of waiting
  up to 24h.
- `charger` (60s) and `materialiser` (60min) are sub-daily and deliberately keep
  plain `tokio::time::interval`, NOT `spawn_daily_job` — this rule (and the helper)
  is specifically about DAILY-or-longer cadences where restart-pinning + DST drift
  actually matters.
- **A generic function's `where` clause defined INSIDE `mod tests` needs its OWN
  `use`/fully-qualified path — it does NOT inherit the outer module's `use`.** A
  private `use sqlx::SqlitePool;` at the top of `jobs/mod.rs` is visible to
  `mod tests` via `super::SqlitePool`, but the BARE identifier `SqlitePool` in a
  test helper's own `where F: Fn(SqlitePool) -> Fut` does NOT resolve without an
  explicit import in `tests`'s own scope — this is an `E0425` compile error the
  `Lint` CI job catches (NOT `cargo fmt`, so it survives the Tier-0 local
  fmt-only check and costs a full CI cycle). Fully-qualify (`sqlx::SqlitePool`)
  instead of adding a redundant `use` when it's the only reference in scope.
- **A log-gating branch (`Ok(n) if n > 0 => tracing::info!(...)`) has NO
  observable side effect other than the log line itself** — a test that only
  proves `tick` ran (via a call counter) leaves the GUARD itself unmutated, since
  every mutant of `n > 0` (`true`/`false`/`n < 0`/`n == 0`/`n >= 0`) produces the
  identical "tick ran once" result. To kill those mutants, capture the actual
  `tracing` output (`capture_tracing_output` in `jobs/mod.rs`'s test module — an
  in-memory `MakeWriter` over `Arc<Mutex<Vec<u8>>>`, scoped via
  `tracing::subscriber::set_default`, no new dependency since `tracing-subscriber`
  is already a runtime dep) and assert the log line's PRESENCE/ABSENCE for at
  least two different counts (one that should log, one that shouldn't). Any
  future `Ok(n) if n > <threshold>`-shaped log-gating branch in this codebase
  needs the same treatment, not just a "did the function run" test.

## A job that reads route-owned in-memory state spawns in `start_server`, NOT `bin/server.rs` (#355)

The background jobs in `bin/server.rs` (`charger`/`materialiser`/`notifications`/
`token_purge`) build their OWN handles (`push`/`mail`) and a `pool.clone()`, deliberately
NOT reaching into `AppState`. That is fine for those jobs because a `PushHandle`/`MailHandle`
is env-derived and stateless — a second instance behaves identically to the one in
`AppState`.

**It is WRONG for a job that must read PER-INSTANCE in-memory state a route mutates.**
`EwelinkHandle::failed_presses` (the door-fault signal behind `is_faulty()`) is an
in-memory atomic that ONLY accumulates on the `AppState.ewelink` instance the door route
calls `press()` on. A separately-spawned `EwelinkHandle` in `main()` would carry its own,
always-zero counter and never see a fault. So the `door_health` alert loop (#355) is
spawned INSIDE `start_server` — after the `AppState` is built, before `with_state(state)`
moves it — cloning `state.ewelink` / `state.mail` / `state.pool`. Same rule applies to any
future job that reads a route-owned in-memory handle (the rate-limiters, ewelink, or any
new `AppState` field holding live state). It stays a plain `tokio::time::interval` there,
same sub-daily convention as `charger` (60s) — the WHERE (start_server, for the shared
instance) is the point, not the cadence.

**Corollary — in-memory dedup state matches the lifetime of the in-memory signal it
tracks.** The door-fault counter resets to 0 (healthy) on every restart, so the
`door_health` alert's "already alerted" dedup lives in-memory (in `DoorHealthMonitor`),
NOT a DB ledger. A DB row that outlived the process would, on the next post-restart tick,
see the reset counter as healthy and fire a FALSE recovery e-mail. Persist alert/dedup
state to the DB ONLY when the signal it tracks is itself durable.
