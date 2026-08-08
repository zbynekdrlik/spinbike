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

**Any new daily (or otherwise wall-clock-scheduled) job MUST use the sleep-loop
pattern instead — copy `jobs::notifications`'s or `jobs::token_purge`'s spawn block
verbatim:**

```rust
loop {
    let delay = spinbike_server::util::duration_until_next_bratislava_hour(
        spinbike_server::util::now_bratislava(),
        spinbike_server::jobs::<your_job>::DAILY_RUN_HOUR,
    );
    tokio::time::sleep(delay).await;
    match spinbike_server::jobs::<your_job>::tick(&pool).await { /* ... */ }
}
```

- Give the job its own `pub const DAILY_RUN_HOUR: u32` in its module (not a shared
  constant) — pick an hour that doesn't collide with the OTHER daily jobs, so they
  never compete for the DB pool at the same instant. Currently: `notifications` = 9
  (customer-visible reminders, mid-morning), `token_purge` = 4 (pure housekeeping,
  off-peak).
- `duration_until_next_bratislava_hour` (in `util.rs`) is the shared, already-tested
  helper — don't reinvent DST-safe scheduling per job.
- The spawn loop itself is NOT unit-testable (it's an infinite `tokio::spawn`
  future in `main()`). Regression coverage instead pins the production
  `duration_until_next_bratislava_hour(now, DAILY_RUN_HOUR)` call directly in the
  job's own test module (see `token_purge.rs`'s `daily_run_hour_*` tests) —
  before-target-hour and at/after-target-hour cases.
- `charger` (60s) and `materialiser` (60min) are sub-daily and deliberately keep
  plain `tokio::time::interval` — this rule is specifically about DAILY-or-longer
  cadences where restart-pinning + DST drift actually matters.
