---
paths:
  - "crates/spinbike-server/src/ewelink/**"
  - "crates/spinbike-server/src/routes/door.rs"
---

# The eWeLink `sequence` is an echo token — two constraints, not one (#353)

`press_sequence` builds the `sequence` field sent with every unlock frame.
The cloud hands that value straight back and `handle_text_frame` matches the
ack by **exact string equality** against the `pending` map key. So the value
must satisfy BOTH:

1. **Unique** among currently-pending presses — two presses can be dispatched
   inside one wall-clock millisecond, and the second `pending.insert` would
   otherwise drop the first press's ack sender (#323).
2. **Small enough to survive the round-trip.** eWeLink's backend passes the
   field through a JSON number, and an IEEE-754 double is exact only to
   2^53 (~9.0e15).

**#323 satisfied only the first and took door unlock down for two days.** It
widened the value to `{now_ms}{counter:06}` — 19 digits, ~1.7e18 — so the
cloud echoed back a *different* number:

```
1700000000123000000  ->  1700000000123000064     (changed by 64)
1700000000123        ->  1700000000123           (exact)
```

Nothing matched, every press timed out after 5 s, and `door.rs` rolled the
customer's visit back **after the relay had already fired**: the door opened,
the screen showed an error, nothing was recorded.

The fix is a monotonic MILLISECOND allocator — `seq = max(now_ms, last + 1)`.
Normally just `now_ms` (13 digits); a press landing in a used millisecond
takes the next free one. It can only run ahead of the clock by as many
milliseconds as there were same-millisecond presses, so it stays 13 digits
forever.

**Before changing this value's shape, ask what the far end does with it.**
The #323 review did flag that the cloud's validation is undocumented and
chose a pure-digit form as the mitigation — but the hazard was never the
character set, it was the magnitude.

## Uniqueness-only tests do not pin this

Three mutants survived the first fix (`>`→`==`, `>`→`<`, `+1`→`-1`) because
the tests asserted only that two consecutive values differ. All three keep
two calls distinct: `==`/`<` abandon the clock and hand out 1, 2, 3…, and
`-1` walks backward and re-issues an already-used id on the **third** press.
Assert the allocator's exact values, not just that they differ.

## Telling WHEN the door broke (do this before theorising)

```bash
# per-day door entries — the outage is unmistakable
sqlite3 /opt/spinbike/prod/spinbike.db \
  "SELECT date(created_at) d, count(*) FROM transactions
   WHERE is_door_press=1 GROUP BY d ORDER BY d DESC LIMIT 14;"

# exact first failure + last success (run on the VPS, see vps-access.md)
journalctl -u spinbike --since "<date>" -o short-iso | \
  grep -E "door: hardware press failed|path=/api/door/open"
```

That dated both ends in minutes and disproved the obvious suspect (a host
move the day *after* the break). A regression that survives a deploy AND a
machine move is a data/protocol problem, not an environment one.

## `/api/door/health` is real but nobody watches it

It reported `{"ewelink_ws":"connected","last_ack_ms_ago":null}` — "not one
ack since startup", the exact symptom — for the entire two days. `connected`
alone means nothing: the WS write half works while the ack path is dead,
which is precisely how the door opens and still fails. Alerting on it is
#355; until that lands, check it by hand after any change near this code.

## Seeing presses and acks on prod

Prod runs `RUST_LOG=info,spinbike_server::ewelink=debug` via
`/etc/systemd/system/spinbike.service.d/trace.conf`, so every press logs
`press sent sequence=…` and `ack received sequence=… error_code=Some(0)`
(~180 ms apart when healthy). Raise that drop-in to `=trace` to also dump
raw frames — including the device's own `update` broadcast, which carries no
`sequence` and is NOT an ack.

Verify a fix with a real press as an **admin**: admin presses log a visit and
charge nothing, so no customer is billed by the test. The door does physically
open.

## `last_ack_ms_ago` alone cannot tell "unused" from "broken" (#355)

`GET /api/door/health` reported `{"ewelink_ws":"connected","last_ack_ms_ago":null}`
for the whole two days of #353. That is not a missing signal — it is an
**ambiguous** one: the ack clock is process-local and reset by every restart,
so `null` reads identically whether nobody has pressed since the restart or
every press since then has failed. A dashboard value nobody can act on is
the same as no value at all.

The missing half is the PRESS side. `EwelinkHandle` therefore also tracks
`last_press_ms` and `failed_presses` (a run reset by any success), and the
endpoint publishes the derived verdict `faulty` rather than leaving the
inference to the reader. **Any new health/diagnostic field should follow the
same rule: publish the verdict, not just the raw clocks.**

Two conventions worth keeping:

- **`FAULT_THRESHOLD` is 2, not 1.** One press can fail on a transient cloud
  hiccup and the customer just presses again; #353 failed every press for two
  days. One failure must not page anyone.
- **The `Disabled` fast-path records nothing.** A dev box with no
  `EWELINK_*` env vars, or a deliberate production kill switch, is a
  CONFIGURATION state — it must never accumulate failures and read as broken
  hardware.

### Testing two presses needs TWO users

`door_rate_limit` rejects a second press by the SAME user inside 10 s before
it ever reaches the relay, so a same-user pair produces one failure, not two.
Any test that needs consecutive presses must press as two different seeded
users (`app.admin_token` then `app.staff_token`, both with
`allow_self_entry = 1`). `TestApp::with_door_mode` cannot switch stub mode
mid-test either, so a failure→success transition needs two separate tests.
