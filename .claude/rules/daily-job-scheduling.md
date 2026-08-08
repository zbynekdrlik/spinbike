---
paths:
  - "crates/spinbike-server/src/bin/server.rs"
  - "crates/spinbike-server/src/jobs/**"
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
`jobs::spawn_daily_job(pool, hour, job_name, tick)` — any new daily (or otherwise
wall-clock-scheduled) job calls that helper from `bin/server.rs` instead of copying a
spawn block:**

```rust
spinbike_server::jobs::spawn_daily_job(
    pool.clone(),
    spinbike_server::jobs::<your_job>::DAILY_RUN_HOUR,
    "<your job's log name>",
    |pool| async move { spinbike_server::jobs::<your_job>::tick(&pool).await },
);
```

If your job's `tick` needs an extra dependency beyond `&SqlitePool` (like
`notifications::tick`'s `&PushHandle`), capture it in the closure instead of widening
`spawn_daily_job`'s own signature — see the `push notifications` call site in
`bin/server.rs` for the pattern (`move |pool| { let push = push.clone(); async move {
notifications::tick(&pool, &push).await } }`).

- Give the job its own `pub const DAILY_RUN_HOUR: u32` in its module (not a shared
  constant, and not owned by the helper) — pick an hour that doesn't collide with the
  OTHER daily jobs, so they never compete for the DB pool at the same instant.
  Currently: `notifications` = 9 (customer-visible reminders, mid-morning),
  `token_purge` = 4 (pure housekeeping, off-peak).
- `duration_until_next_bratislava_hour` (in `util.rs`) is the shared, already-tested
  helper `spawn_daily_job` itself calls — don't reinvent DST-safe scheduling per job.
- `spawn_daily_job`'s own sleep loop is NOT unit-testable (it's an infinite
  `tokio::spawn` future). Regression coverage instead pins the production
  `duration_until_next_bratislava_hour(now, DAILY_RUN_HOUR)` call at each job's own
  hour — see `token_purge.rs`'s `daily_run_hour_*` tests (single-job) and
  `jobs/mod.rs`'s own tests (parameterized over both current job hours) — before-target-
  hour and at/after-target-hour cases. A new job doesn't strictly need its own copy of
  these tests (the arithmetic is already covered generically), but adding one at your
  job's specific hour is cheap insurance if that hour is adjacent to another job's.
- Still run your job's `tick` once at startup, directly in `main()`, BEFORE the
  `spawn_daily_job` call — the helper only owns the recurring loop, not the startup
  tick, so the first observable log line appears right after boot instead of waiting
  up to 24h.
- `charger` (60s) and `materialiser` (60min) are sub-daily and deliberately keep
  plain `tokio::time::interval`, NOT `spawn_daily_job` — this rule (and the helper)
  is specifically about DAILY-or-longer cadences where restart-pinning + DST drift
  actually matters.
