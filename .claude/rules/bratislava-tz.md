---
paths:
  - "crates/spinbike-server/src/util.rs"
  - "crates/spinbike-server/src/jobs/**"
  - "crates/spinbike-server/src/routes/**"
---

# Bratislava wall-clock — never `chrono::Local`, always the shared `util.rs` helpers

**Recurring bug class (#205, #222, #327, #330): a NEW production call site computing
"today"/"now" via `chrono::Local::now()` instead of the shared, tz-database-driven
`crate::util` helpers.** `chrono::Local` reads the SERVER PROCESS's OS/`TZ`
configuration. `deploy/systemd/spinbike.service` sets no `TZ=`, so on a
UTC-configured host `Local::now()` silently drifts from real Bratislava local time
by up to ~2h around midnight — every day-boundary decision (pass expiry, booking
weekday, edit-window cutoffs) computed from it is wrong for that window. Four
independent production sites have now shipped this exact bug (door.rs/
my_balance.rs/payments.rs/notifications.rs/charger.rs fixed under #205/#222;
materialiser.rs under #327; transactions.rs under #330) — grep
`chrono::Local|Local::now` across `crates/spinbike-server/src` (excluding
`#[cfg(test)]` blocks) before adding ANY new day/time-of-day boundary logic.

**Use instead:**
- `crate::util::today_bratislava() -> NaiveDate` — the gym-local calendar date.
- `crate::util::now_bratislava() -> NaiveDateTime` — the gym-local wall clock.
- `crate::util::bratislava_local_to_utc(local: NaiveDateTime) -> NaiveDateTime`
  (added #330) — convert an ARBITRARY Bratislava-local naive datetime (any
  time-of-day, not just midnight) to its UTC instant, resolving DST-fold
  ambiguity to the earliest match. This is now the SINGLE shared implementation
  — `util.rs`'s own `bratislava_local_midnight_utc` (private, day → UTC midnight)
  is a 3-line wrapper over it. **Before hand-rolling
  `.from_local_datetime(...).earliest()...` anywhere, check whether this helper
  already covers it** — that duplication (util.rs's midnight-only version vs.
  `routes/transactions.rs`'s inline arbitrary-time version) is exactly what #330
  found and fixed.
- `crate::util::bratislava_day_range_utc(day) -> (NaiveDateTime, NaiveDateTime)`
  — the half-open `[start, end)` UTC range for binding against a `created_at`
  UTC-instant column.

## Regression-testing this bug class when behavior can't be reproduced locally

**This dev box AND the CI runner both already run `TZ=Europe/Bratislava`
(`/etc/timezone`).** That means `Local::now()` and `today_bratislava()` AGREE at
test time regardless of which one the code under test calls — the actual
divergence only exists on the UTC-configured prod systemd unit, and there is no
way to force a genuinely different, deterministic OS TZ into a test process
without flakiness (setting `TZ` env var mid-test races other tests; picking a
real-time window where dates differ is time-of-day-dependent and flaky).

**The pattern used for #327/#330's regression tests instead: a source-level
string-invariant test**, not a behavioral one:

```rust
#[test]
fn some_fn_computes_today_via_shared_bratislava_helper() {
    let src = include_str!("this_file.rs"); // self-reference, compile-time embed
    let production_src = src
        .split("#[cfg(test)]\nmod tests {")
        .next()
        .expect("file always has content before its own test module marker");
    assert!(!production_src.contains("Local::now()"), "... (OS-TZ dependent)");
    assert!(production_src.contains("crate::util::today_bratislava()"), "...");
}
```

Splitting on the literal `#[cfg(test)]\nmod tests {` marker excludes the test
module's OWN source (including this assertion's string literals, and any
legitimate `Local::now()` use inside test helpers computing a reference date —
see `materialiser.rs`'s `sweep_skips_full_classes`, which now also uses
`today_bratislava()` for consistency, but didn't strictly need to) from the scan.
This is a genuine RED-before-GREEN test: it fails (assertion false) against the
current buggy source and passes once the call site is swapped — just verified at
the SOURCE level instead of by observing different runtime output, because the
runtime output genuinely can't differ in this environment. Reach for this
pattern specifically for "call site uses banned pattern X instead of required
helper Y" bugs where a real behavioral repro would need controlling the OS TZ.

## Tests must use the SAME shared helper as the production code they exercise (#336)

The bug class above isn't limited to PRODUCTION call sites — an integration
test (or `#[cfg(test)]` module) that computes its OWN expected date/time via
`chrono::Local::now()` while the endpoint under test computes "today" via
`today_bratislava()`/`now_bratislava()` has the SAME divergence risk, just one
level removed: the two only agree while the process's ambient `TZ` happens to
already be Europe/Bratislava. #336 was discovered exactly this way — CI run
`31439513457` on an unpinned UTC runner, `transactions_date.rs`'s 30-day-window
tests computed the target date via `chrono::Local::now().date_naive()` while
`payments.rs`'s `patch_created_at` validated it against `today_bratislava()`.
**Never write a NEW test that calls `chrono::Local::now()`/`Local::now()` to
derive a date/time it will compare against, or feed into, ANY endpoint that
itself uses the `crate::util` helpers — use `spinbike_server::util::
today_bratislava()`/`now_bratislava()` (integration tests, via the public
`pub mod util`) or `crate::util::...` (in-crate `#[cfg(test)]` modules)
instead, even when the site looks self-referential today.** A value that's
merely used to seed a fixture and then compared only against itself has no
CORRECTNESS risk from `chrono::Local` — but leaving it in test code is still
a misleading precedent for the next person copying the pattern, and #336
removed it everywhere for exactly that reason.

**CI runs the suite against BOTH zones now, deliberately.** The `test` job in
`ci.yml` pins `TZ: Europe/Bratislava` (490cdba) — the CI/prod-parity
baseline — while a second job, `test-tz-utc`, runs the IDENTICAL suite with
`TZ: UTC` (github-hosted `ubuntu-latest`'s own unpinned default) and is wired
into `e2e`'s `needs:` so it actually blocks deploy, not just an informational
job. If you ever regress a call site back to `chrono::Local`, `test-tz-utc`
catches it on the very next push instead of only in the ~1-2h/day window
where the two zones disagree on the calendar date. Do NOT remove either
job's `TZ` setting — they're deliberately testing two different things.
