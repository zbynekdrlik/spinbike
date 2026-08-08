# Autopilot Log

Terse per-issue log of autonomous work cycles: issue #, commit SHAs, RED→GREEN
test names, decisions, and the shared PR #. Newest entries at the top.

## 2026-08-08 — #297 token_purge daily interval: wall-clock-aligned, not uptime-relative (PR #298, dev.134)

- **Root cause:** `bin/server.rs`'s `login_tokens purge: daily` spawn block used
  `tokio::time::interval(Duration::from_secs(86400))`, which measures its first
  real tick from task-spawn (= server-restart) time — same defect class #264
  fixed for `jobs::notifications`.
- **Fix (`bd36400`):** copied #264's sleep-loop pattern —
  `util::duration_until_next_bratislava_hour(now_bratislava(), DAILY_RUN_HOUR)`
  recomputed every cycle. New `jobs::token_purge::DAILY_RUN_HOUR = 4`
  (off-peak, offset from notifications' 09:00 so the two daily jobs never
  contend for the DB pool at once) — a technical placement decision, no owner
  input needed.
- **Tests:** `token_purge.rs::daily_run_hour_before_target_is_later_today` /
  `daily_run_hour_at_or_after_target_rolls_to_tomorrow` — the spawn loop itself
  isn't unit-testable, so these pin the production scheduling call
  (`duration_until_next_bratislava_hour` + the real `DAILY_RUN_HOUR` constant)
  as regression coverage, mirroring `util.rs`'s existing generic-helper tests.

## 2026-08-08 — #264 PWA push notifications: low credit + expiring pass (PR #296, dev.133)

- **Feature:** daily job `jobs::notifications::tick`, two reasons
  (`low_credit`, `pass_expiring`), migrations V23–V25 (`push_subscriptions`,
  `push_notify_log` + `sent_count`), `sw.js` gained `push` +
  `notificationclick` listeners.
- **Owner decisions** recorded on the issue before implementation: low-credit
  push requires `credit <= 3.30` **AND** last top-up `>= 20.00 EUR` (excludes
  one-off single-entry customers; boundary is `>=`, "last top-up" = the single
  most recent credit-increasing transaction, not a sum); cadence = one tick
  per day, `NOTIFY_COOLDOWN_DAYS = 7`, `MAX_NOTIFICATIONS_PER_EPISODE = 2`,
  re-armed when the condition clears.
- **Adversarial review** produced 10 findings; fixed in-PR: SSRF hardening of
  `/api/push/subscribe` (require `https://` + host allowlist), VAPID key
  length validation (a wrong-length key panicked inside `generic-array`), send
  timeout + pruning of `Failed` subscriptions, per-user loop changed from
  `?`-abort to log-and-continue, `tracing` instrumentation, session-
  invalidation regression tests for subscribe/unsubscribe.
- **Fallout fix `8e51039`:** the SSRF guard made `PushToggle` legitimately 401
  in two `session-invalidation.spec.ts` repros — fixed by adding
  `/api/push/config` to their `allow4xxFor` list (per
  `.claude/rules/e2e-console-check.md`, match `msg.location().url`, never
  `msg.text()`).
- **Spun off as #297** (`chore(server): token_purge's daily interval is
  uptime-relative, not wall-clock-aligned`) — the same pattern #264 fixed for
  the push job.
- **Prod verification** (0.15.0-dev.133, live prod DB + journal): synthetic
  customer with a 25 EUR last top-up and 1.00 EUR credit was selected and a
  real VAPID-signed POST was emitted (`push: notified user_id=… reason=
  "low_credit"`, `sent=1`); a second same-day tick sent nothing (ledger
  untouched, no second network attempt); lowering the last top-up to 5.00 EUR
  and clearing the ledger made the job skip the user entirely. All synthetic
  rows deleted afterwards.
- Testing gotcha now in `.claude/rules/push-notifications.md`: `push::send()`
  does no host validation (the allowlist is only in the subscribe route) and
  the anti-spam ledger is stamped only on a real 2xx — so proving the cooldown
  needs a local mock endpoint returning 201, not a failed send.

## 2026-08-07 — #278 4xx console-filter scoping + #282 install-token mint-guard race + #291 transaction ordering tiebreaker (PR #292 + follow-up PR #293, dev.130)

- **#278 (`9ca6d33` [red] → `758ccb2` [green] → `b34f9f2`/`71f48dc` CI fixes):**
  `setupConsoleCheck`'s blanket 4xx filter made `assertCleanConsole` vacuous
  for any spec whose flow legitimately triggers a 4xx. Made opt-in per test
  via `allow4xxFor`. First fix matched `allow4xxFor` against `msg.text()` —
  broke CI (23/218 tests) because Chromium's "Failed to load resource"
  message never carries the URL in its text. Root-caused via a standalone
  Playwright script: the URL lives in `msg.location().url`. Also allow-listed
  `/api/metrics/launch` (the standalone-launch beacon, rate-limited, fires on
  every `setIosStandalone()` test) — a second, unrelated 429 leak.
- **#282 (`963f21f` [red] → `330ba5e` [green] → `8225f32` [red] →
  `7ee41f9` [green]):** install-token mint guard was a check-then-act race
  (sessionStorage-only check, async mint POST in between) — fixed with a
  module-level `AtomicBool` single-flight guard. Code review then found the
  success path never verified the sessionStorage WRITE actually landed — a
  silent persist failure (private browsing/quota) left the guard
  permanently claimed. `confirm_mint_or_release` now re-checks and releases
  on a failed persist. Verified LIVE on prod (spinbike.sk): exactly 1 POST
  `/api/auth/install-token` across initial load + same-tab reload, same
  token both times.
- **#291 (`cad318d` insufficient fix → `cb5cbd4` [red] → `448b354` [green]
  → `c7184ac` same-pattern sweep):** `txn-note.spec.ts`'s void test flaked 3x
  on CI (dev run 31197701928, main run 31203717764, dev run 31204778238).
  First fix (wait for the DELETE response) was real but insufficient — the
  actual root cause, found via a failure's DOM-snapshot artifact: SQLite's
  `datetime('now')` default has only second precision, so a fresh user's
  initial-credit top-up and its first charge can tie on `created_at`, and
  `ORDER BY t.created_at DESC` had no tiebreaker — same-second rows sorted
  in an unspecified order, so the void sometimes landed on the wrong row.
  Fixed with `, t.id DESC` on all 4 affected queries across
  `db/transactions.rs`, `routes/my_balance.rs`, and `routes/payments.rs`
  (same table, same bug class, swept proactively).
- Both PRs: #292 (the original 2-issue batch + review fixes) and #293 (the
  post-merge #291 follow-up, since #292 had already merged when #291
  recurred on main). Deployed + verified: v0.15.0-dev.130 on both dev and
  prod, main CI green (E2E + all 8 mutation shards), prod Deploy + Smoke
  green.

## 2026-08-07 — #288 pagination flake + #39 recurrence: barcode-tail search ranking bug (same PR #290 batch, dev.127)

- **Why:** discovered mid-PR while hardening PR #290 (the #286+#287+#289
  batch above) — a shared-DB CI flake (#288) and a real recurrence of #39's
  collision class, this time surfacing an actual PRODUCT ranking bug, not
  just test-fixture pollution. All commits ride the same `dev` branch/PR
  #290 as #286/#287/#289; no separate PR.
- **#288 fix (`1794e11`):** `users-by-movement.spec.ts`'s "Show more" loop
  capped at 5 clicks (250 rows) hunting a seeded row by text match. As the
  shared single-worker CI E2E DB accumulates more no-movement users across
  the 215-test run, the seeded row's rank drifts deeper and the fixed cap
  becomes insufficient. The test already computes the row's exact rank via
  an API call before touching the UI — now it clicks "Show more" exactly
  `floor(rank / PAGE_SIZE)` times instead of guessing with a fixed bound.
  Root-cause investigation on PR #290 also REFUTED the had_token/add_auth
  changes and `action_form.rs` as causes (both unchanged by this diff).
- **#39 recurrence, E2E side (`c876103` [red] → `a048216` [green]):**
  `dashboard.spec.ts`'s staff search does an unanchored
  `search_text LIKE '%1001%'` and several tests trusted `.first()` for a
  short digit query. `category-revenue.spec.ts`'s own `Date.now()`-based
  card_code substring-collided with `'1001'` on CI run 31178634087
  ("CR Reports…" outranked "Jana Testova") — the exact #39 mechanism
  (closed May via `f289ac7`'s letters-only suffix helper) recurring because
  the fix never fully propagated. Producer-side (root cause, matches #39's
  own fix): extracted `uniqueLetterSuffix()` (pure a-z, zero digits — no
  digit-substring collision possible by construction) into `helpers.ts`,
  migrated 13 spec files off `Date.now()`-based suffixes onto it. Consumer-
  side (defense in depth): `dashboard.spec.ts`'s 3 affected tests now scope
  the result locator by `{ hasText: <expected name> }` instead of raw
  `.first()` position. Alternative rejected: consumer-side-only hardening
  (issue #39's own "Option 4") — cheaper but leaves all 13 producers free to
  manufacture the next collision; doing both retires the class.
- **#39 recurrence, SERVER side (`5fc07a0` [red] → `22477a7` [green]):**
  the E2E hardening above only patches test fixtures — investigation found
  the SAME bug shape live in `search_users`/`search_users_with_pass`
  (`crates/spinbike-server/src/db/users.rs`): only a card_code PREFIX match
  was special-cased; a card_code ending with the typed digits (a barcode
  TAIL — the common real-world case, "type the last digits off a card")
  fell into the same catch-all bucket as an unrelated card merely
  CONTAINING those digits, and ordering fell through to plain name ASC.
  Reproduces CI run 31189869886's "AAA Polluter" outranking "Jana Testova".
  Fix adds an exact-match bucket (0), then a combined prefix-OR-suffix
  "boundary match" bucket (1), above the generic contains-elsewhere bucket
  (2) — applied identically to both db-layer search functions (only
  `search_users_with_pass` is reachable from a route today; `search_users`
  shares the identical bug shape in the same file).
- **Follow-up test-assertion fix (`36eae91`):** the new regression test
  (`dashboard.spec.ts:48`) initially asserted the full 8-digit card number
  against the search-RESULT row before any click — that row intentionally
  renders only the last 4 digits by design (also relied on by
  `negative-balance.spec.ts`); the full number only appears in the action
  panel after clicking. Confirmed via the raw CI log that the locator
  correctly found Jana's row (name-scoped, immune to the ranking bug) — a
  misplaced assertion, not a second product defect. User decision (PR #290
  comment thread): fix the assertion, keep the row's tail-only rendering
  and the ranking fix untouched.
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.127) post-deploy:**
  two synthetic customers — card `88012345` (ends with `12345`) vs card
  `9912345000` (merely contains `12345`) — via both the local-origin API
  (`/api/users/search?q=12345`) and the real staff dashboard DOM: the
  tail-matching card ranks first in both, and clicking it opens the correct
  card's detail panel (full number `88012345`). Both synthetic rows deleted
  after (0 remaining).
- **Commits (dev, same PR #290 as the batch above):** `1794e11` → `c876103`
  [red] → `a048216` [green] → `5fc07a0` [red] → `22477a7` [green] →
  `36eae91`. PR [#290](https://github.com/zbynekdrlik/spinbike/pull/290)
  (Closes #286, Closes #287, Closes #289 — #288 and #39 closed separately,
  no `Closes` keyword since they predate/ride this same PR), merged
  `9978611`. Main CI run
  [31193314710](https://github.com/zbynekdrlik/spinbike/actions/runs/31193314710)
  green incl. E2E, Deploy (prod), Smoke (prod).

## 2026-08-07 — #286 + #287 + #289: had_token race fix + shared sessionStorage helper + backfilled log entry (dev.127)

- **Why:** all three surfaced during `/code-review` of PR #285 (the #284 +
  #275 batch): #286 and #287 as review findings on `api.rs`/`auth.rs`/
  `install_prompt.rs`, #289 because that same batch's autopilot worker died
  mid-run right after the merge and never wrote its own log entry (see the
  entry directly below this one, backfilled by this batch).
- **#286 root cause:** `api.rs`'s `handle_unauthorized(status, had_token)`
  was called at all 9 `api::` call sites with
  `had_token = get_token().is_some()` re-evaluated AFTER the response had
  already resolved (`.await`'d), not the token that was actually attached
  to THAT specific request. A concurrent `clear_auth()` (Logout click, a
  second tab) landing between send and 401-resolve made the re-read return
  `false`, silently skipping `mark_session_expired()`/`clear_auth()`/the
  `/login` redirect — the customer was left on the page with a generic
  inline error instead of the #275 notice.
- **#286 fix:** `add_auth()` now returns `(RequestBuilder, bool)`, the bool
  captured AT BUILD TIME from the same `get_token()` call that decides
  whether to attach the header. Every call site destructures
  `let (req, had_token) = add_auth(...)` and passes THAT into
  `handle_unauthorized`. `post_public`/`post_public_coded` untouched (never
  call `add_auth`).
- **#286 RED→GREEN:** `c9b52ef` [red] — new Playwright E2E in
  `session-invalidation.spec.ts` (`Concurrent logout during an in-flight
  request still shows the notice`): `page.route` intercepts
  `**/api/my/balance`, clears `localStorage` INSIDE the interception (before
  fulfilling 401) — deterministic, not timing-dependent — reproduces the
  exact ordering the bug depends on → `71ebe19` [green] — the `api.rs` fix,
  makes it pass.
- **#287:** pure DRY refactor riding on top of #286's now-race-free code —
  extracted the sessionStorage get/set JS interop `auth.rs`'s one-shot
  `mark_session_expired`/`take_session_expired_flag` (#275) and
  `install_prompt.rs`'s get/set-only `stored_install_token`/
  `store_install_token` mint guard (#261) each hand-rolled independently
  into a new `spinbike-ui/src/storage.rs` (`flag_set`/`flag_take` one-shot,
  `cache_get`/`cache_set` cache — mirrors the existing `platform.rs`
  precedent). Both call sites became thin wrappers; no behavior change,
  existing #275/#261 test suites cover it unchanged. `467bbad`.
- **#289:** the entry directly below this one — the #284 + #275 batch's
  worker died after its merge (stream watchdog "no progress for 600s")
  before writing its own log entry; backfilled here from the facts already
  verified and posted on #289's issue body.
- **PR [#290](https://github.com/zbynekdrlik/spinbike/pull/290)** (Closes
  #286, Closes #287, Closes #289).

## 2026-08-07 — #284 + #275: last two dead-session gaps + session-expired notice (dev.126)

- **Why:** `#284` — two independent reviews found `users::user_transactions`
  (self-view branch) and `auth::install_token` still accepted a dead caller
  session (missing/soft-deleted/blocked `sub` with a still-valid JWT) — the
  same gap class as #268/#274/#277, two more `AuthUser` call sites beyond the
  four #277 already fixed. `#275` — `handle_unauthorized`'s hard
  `set_href("/login")` destroyed the session-expired message before it could
  render, so a stale-session redirect landed on a bare login form with no
  explanation.
- **#284 fix:** both handlers now call the shared `require_live_session`
  helper (`auth/mod.rs`), same `if is_self { ... }` shape as `update_user`'s
  existing self-edit branch — 401 `session_invalid` instead of a silent
  success (`user_transactions`) or a 500 FK-constraint crash
  (`install_token`, missing-caller case). Consolidating `door.rs`/
  `my_balance.rs`'s pre-existing hand-rolled checks onto the shared helper,
  and moving the check into the `AuthUser` extractor itself, were both
  evaluated and rejected with measured (not estimated) call-site counts —
  written up in `.claude/rules/session-invalidation.md` and the issue's own
  design comments.
- **#275 fix:** a one-shot `sessionStorage` flag (`spinbike_session_expired`,
  same pattern as the existing `sb_install_token` install-prompt guard)
  carries the reason across the hard navigation; `LoginPage` reads + clears
  it on mount. Never fires on an ordinary wrong-password login (that path
  never had a token to begin with).
- **RED→GREEN:** server integration tests (per invalid state + a live-
  customer positive control per endpoint) for #284, Playwright E2E
  (`session-invalidation.spec.ts`, the #275 describe block) for #275 — both
  test-first, same PR.
- **Verified LIVE on prod BEFORE any code** (STEP 0, both issues) and again
  post-merge: `GET /api/users/{self}/transactions` and
  `POST /api/auth/install-token` both 401 `session_invalid` for blocked/
  soft-deleted/ghost-sub tokens, 200 for a live customer
  (`http://127.0.0.1:8080`). Browser flow confirmed the notice
  ("Tvoje prihlasenie uz nie je platne, prihlas sa znova.") renders once on
  the `/login` redirect, storage is cleared, it is NOT re-shown on reload,
  and a wrong-password login shows only the unrelated "Tento ucet pouziva
  ine prihlasenie".
- **Frontend storage keys** (worth writing down): `spinbike_token`,
  `spinbike_user`, `spinbike_session_expired` in `spinbike-ui/src/auth.rs`.
  Customer routes are `/my/balance` + `/my/bookings` — there is no bare
  `/my`.
- **PR [#285](https://github.com/zbynekdrlik/spinbike/pull/285)** (Closes
  #284, Closes #275), merged `dde3ad2`. Main run 31171571417 went green
  after ONE rerun of a flaky `users-by-movement.spec.ts` (filed as #288).
- **Note:** this entry was backfilled by the #286+#287+#289 batch above —
  the original worker for this batch died mid-run right after the merge
  (stream watchdog: "no progress for 600s") and never reached this step.

## 2026-08-07 — #274: door/open 401 session_invalid for missing/deleted/blocked user (dev.123)

- **Why:** filed as a review finding on #268/#273 — `/api/door/open` has its
  own user-lookup precondition, separate from `/api/my/balance`, and #268
  scoped itself to `my_balance.rs` only. STEP 0 re-confirmed live against
  prod BEFORE implementing (synthetic rows 616-619 + self-minted JWTs,
  `http://127.0.0.1:8080`, all cleaned up after): soft-deleted, blocked, and
  a bogus `sub` all returned 403 (`not_allowed`/`blocked`); the live
  `allow_self_entry=0` case correctly returned 403 and had to stay that way.
- **Root cause:** the precondition SELECT filtered `deleted_at IS NULL` (a
  soft-deleted row silently fell into the "missing row" branch) and both the
  missing-row and blocked branches hand-built a `403
  {"status":"rejected","reason":...}` tuple instead of going through the
  shared `ApiError` type — so a dead JWT never triggered the client's
  existing 401-clears-session handling (`door_button.rs`'s `post_door_open`
  already special-cased 401, it was just unreachable).
- **Fix:** dropped the `deleted_at IS NULL` filter, fetched `deleted_at`
  alongside `blocked`, folded missing-row + `blocked || deleted_at.is_some()`
  into one `Err(ApiError::Unauthorized(ErrorCode::SessionInvalid))` — checked
  in the same place (before the `allow_self_entry` role-bypass logic) the old
  standalone blocked check sat, preserving the `#106` invariant. Reused the
  existing `ErrorCode::SessionInvalid` — no new `ErrorCode` variant needed.
- **RED→GREEN:** `2370251` [red] — 2 new tests (missing user, soft-deleted
  user) + updated the 3 existing `#106` blocked-user tests to the new 401
  contract (justified in that same commit — the old 403 assertions tested
  the exact status code this fix intentionally changes) → `b850a54` [green]
  — the `door.rs` fix, makes all 5 pass.
- **Review follow-up (`96f9bd4`):** the deep `requesting-code-review` pass
  (0 Critical, 0 Important) flagged two logically-redundant-but-untested
  branch combinations — blocked customer with `allow_self_entry` at its
  default 0, and a customer both blocked AND soft-deleted at once. Added
  both as pure coverage (not a RED/GREEN pair — the code already passed).
- **PR [#280](https://github.com/zbynekdrlik/spinbike/pull/280)** (Closes
  #274), merged `dfba2fc`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.123 — DOM matches,
  SW unregistered + `spinbike-v3` cache cleared first, 0 console errors):**
  re-ran the full STEP-0 matrix against the deployed build with a fresh set
  of synthetic rows (620-623) — missing/soft-deleted/blocked all now return
  `401 {"error_code":"session_invalid"}`; the live `allow_self_entry=0` case
  is unchanged at `403 not_allowed`; the live `allow_self_entry=1` happy path
  is unchanged at `200 opened`. All synthetic `users`/`transactions` rows and
  JWT-secret scratch files deleted after (verified 0 remaining both runs).

## 2026-08-06 — #261: round-5 belt-and-braces iOS install auto-login (dev.118)

- **Why:** round 4 (#258, dev.113) shipped the whole feature on ONE undocumented iOS behavior (re-reading a runtime-swapped `<link rel="manifest">` at "Add to Home Screen" time) plus a 24h single-use credential — and it never worked. Prod evidence re-confirmed at STEP 0 of this ticket: `install-token: minted user_id=465` logged TWICE (no re-entrancy guard), both `login_tokens` rows `purpose='login'`, `expires_at` 24h out, `used_at` still NULL, no `token-login` line ever followed — the launch URL was never opened.
- **Design (decided in the issue, implemented verbatim):** a 3-leg ladder so no single guessed iOS behavior can sink it — (1) manifest `start_url` swap (kept), (2) NEW `history.replaceState` arms the address bar directly on `/welcome` AND `/my/balance`, (3) the already-built in-app 6-digit-code login as the guaranteed fallback (untouched). The credential itself changes shape: new `purpose='install'` token, 365-day TTL, genuinely MULTI-USE (no `used_at` consumption — new `revoked_at`/`last_used_at` columns instead), capped at 5 live tokens/user with oldest-eviction on mint #6.
- **Migration V22** (`db/migrations.rs`): widens `login_tokens.purpose` CHECK to add `'install'`, adds `last_used_at`/`revoked_at` — same DROP-new+INSERT+RENAME rebuild pattern as V8/V11/V16/V21, no view/trigger dance needed (nothing references `login_tokens`).
- **`db/login_tokens.rs`:** `create_install_token` (365d TTL, cap-eviction, wrapped in ONE transaction after a review follow-up flagged the un-transacted count→evict→insert as a concurrent-mint race), `redeem_install` (single atomic `UPDATE...RETURNING`, touches only `last_used_at`, never `used_at`), `revoke_user_install_tokens`.
- **`routes/auth.rs`:** `token_login` now tries the install-redeem path FIRST (falls through unchanged to invite/login on `None`), rate-limited on TWO keys sharing one limiter — the token's hash AND (review follow-up) the caller's best-effort client IP, since a per-token-only limit is trivially evaded by flooding with distinct garbage tokens. New staff-only `POST /api/users/{id}/revoke-install-tokens` ("Odhlasit zariadenia" admin action, `dashboard/revoke_install_button.rs`).
- **`routes/manifest.rs`:** `Cache-Control: no-store` on every `/manifest.json` response (token-bearing or not) — updated the stale #212 characterization test (`static_files.rs`) that asserted the OLD no-header behavior.
- **`install_prompt.rs`:** `sessionStorage` mint guard (`sb_install_token`) fixes the actual prod double-mint (component mounts on two pages, each used to re-mint unconditionally); `arm_install_credential` swaps the manifest link AND calls `history.replaceState` — added the `History` web-sys feature.
- **RED→GREEN (regression-test-first):** `3075407` [red] (install token still 24h/single-use via an interim stub, new test fails on both counts) → `0cfad99` [green] (real 365-day/multi-use logic, same test passes). Full ladder (mint guard, rate limit, revoke, cache-control) shipped with same-PR tests per the feature-tests-mandatory rule, not strict RED-first.
- **Mutation testing caught a real gap:** shard 5/8 found `deleted_at.is_some() || blocked` survived a `||`→`&&` flip in the NEW install branch — no test exercised a blocked-but-not-deleted user redeeming an install token. Fixed with `install_token_blocked_user_rejected`; all 8 shards green after.
- **Independent code review (dispatched before merge)** found 2 Important gaps (0 Critical): the count→evict→insert mint sequence wasn't transaction-wrapped, and the redeem rate limiter was token-hash-only (evadable via many distinct garbage tokens). Both fixed in `60655e3` with a locking test (`token_login_throttles_many_distinct_garbage_tokens_from_one_source`).
- **E2E:** `install-prompt.spec.ts` (sessionStorage dedup across a reload, both legs armed with the same token, manifest Cache-Control), `welcome.spec.ts` (dead token without session → recovery screen, the SAME install token signing in on a second simulated launch), `revoke-install-tokens.spec.ts` (admin button round-trip + staff-only UI surface).
- **Commits (dev, in order):** `2531caa` bump → `3075407` test[red] → `0cfad99` fix[green] → `0139003` feat (routes+manifest+client) → `116603b` feat (admin UI) → `1008812` test(e2e) → `95d0c15`/`6893dfb`/`796a1c1` CI-cycle fixes (clippy type_complexity, stale cache-control test, mutation survivor) → `60655e3` review follow-up (tx-wrapped mint + IP rate limit). PR [#271](https://github.com/zbynekdrlik/spinbike/pull/271) (Closes #261), merged `83ecb43`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.118 — DOM matches `/api/version`):** synthetic customer (id 606, cleaned up after) at a 390×844 iPhone-UA context, stale SW/cache cleared first. Both legs armed identically to the design: manifest link `href` and the address bar (post-`replaceState`) both carried the exact same install token, `GET` on the swapped manifest returned `Cache-Control: no-store` + the correct `start_url`. The actual prod failure mode is now proven fixed: the SAME baked launch URL, opened from TWO SEPARATE fresh/empty-storage contexts (simulating two real launches), signed the app in both times — `spinbike_token` present, `/welcome` → CTA → `/my/balance` rendered, zero console errors either time. Leg 3 (dead/unknown token, no session) landed on the recovery screen leading with the code form, as designed. All synthetic `users`/`login_tokens` rows deleted after (verified 0 remaining); scratch JWT-secret/token files deleted.
- **UNVERIFIED (honest, per the issue's own acceptance criterion):** the physical iOS "Add to Home Screen" gesture itself — no physical iPhone available, not simulable in Playwright. The CEO confirms with the next real client; #260's diagnostics (already shipped, same trail: `request_log` middleware + the `/api/metrics/launch` beacon) make that attempt measurable instead of a fifth guess if it fails again.

## 2026-07-24 — #255: revenue breakdown by category in money stats (dev.112)

- **Why:** CEO wants to see how much each service earns per period on the Reports page ("kolko sa utratilo za doplnky vyzivy" was the trigger), and confirmed (design already settled in the issue) a FULL per-active-service breakdown — Doplnky vyzivy ("Supplements", `services.id=5`, already seeded) is one row among all active services, not its own single KPI tile.
- **No migration:** `services.name_sk`/`name_en`/`active` and the Supplements/Refreshments/Card-activation-fee seed rows already existed (V8, `migrations.rs:277-310`); `transactions.service_id` already categorizes every sale.
- **`db/reports.rs`:** new shared `category_revenue_between(pool, start_str, end_str)` — `SUM(-amount)` for `action='charge' AND amount<0` per active service, LEFT JOIN (every active service appears, 0.0 with no sales), all period/soft-delete filters correctly in the `ON` clause (not `WHERE` — the classic LEFT-JOIN correctness trap, verified by an independent deep-review pass). Same `bratislava_day_range_utc` half-open UTC range the KPI aggregate already uses (#251) — never a raw `date(created_at)`. Wired into both `day_report`/`range_report`, both grew from a 3-tuple to `(KpiSummary, Vec<CategoryRevenue>, Vec<ReportEvent>, bool)` — all 6 existing call sites (routes + 4 test destructurings) updated.
- **UI:** new `spinbike-ui/src/pages/reports/category_revenue.rs` component below `KpiCards`, reusing the shared `.group`/`.list-row` primitive (same as `filters_bar.rs` on this page) rather than a bespoke style. testids `category-revenue`/`category-revenue-row`/`category-revenue-total`. Two new unaccented-Slovak i18n keys; category names themselves come straight from the DB (`name_sk`/`name_en`, which DO carry diacritics — intentional, not a bug).
- **Tests:** one discriminating server-side fixture (`db/reports.rs`) proves the sum, the visit/topup exclusions, the €0-pass-sale-contributes-0 case, full LEFT JOIN presence, and DESC sort — independently against both `day_report` and `range_report`. One delta-based E2E spec (`category-revenue.spec.ts`, mirrors `reports-attendance.spec.ts`'s shared-DB-safe pattern) drives the real `/reports` UI.
- **Two independent review passes, both clean:** `/review` — 0🔴0🟡0🔵. A dispatched deep-review subagent (`superpowers:requesting-code-review`) — 0 Critical, 0 Important, 4 Minor (none blocking): (1) the E2E commit message's audit claim ("no service activation elsewhere in the suite") slightly overstates itself — `desk-ux.spec.ts` DOES toggle a service's `active` flag off/on in a `try/finally`, harmless in practice since CI runs `workers:1` (serial) and the new test never asserts an absolute active-service count, only deltas + `rows.length >= 2`; (2) the plan said "reuse the kpi-card look", the impl reused `.group`/`.list-row` instead — arguably the better structural fit for a variable-length list, noted as a deviation but not a defect; (3) no covering index on `transactions(service_id, created_at)` — pre-existing condition (the KPI query has the same gap), non-issue at current MVP scale; (4) "Spolu" sums already-rounded per-row `f64` values client-side rather than computing the total server-side from unrounded values — no visible cent-level risk given the row count.
- **Commits:** `d9b0c57` (bump 0.15.0-dev.112) → `b3e065f` (server+core: query + types + tests) → `6442a49` (UI: component + i18n) → `74f1216` (e2e test), all on `dev`. PR [#256](https://github.com/zbynekdrlik/spinbike/pull/256) (Closes #255), merged `555a3f5`. Main CI run [30079475335](https://github.com/zbynekdrlik/spinbike/actions/runs/30079475335) green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.112):** DOM version == `/api/version`, stale SW registration cleared first, 0 console errors on `/reports`. Functional: synthetic customer (id 598) + synthetic staff actor (id 599, needed only to satisfy `transactions.staff_id`'s FK — `POST /api/users` with `initial_credit>0` failed with a `FOREIGN KEY constraint failed` 500 the first time, because the minted admin JWT's `sub` had no real row; fixed by inserting one throwaway staff row and re-minting against its real id) charged via the REAL `POST /api/payments/charge` on Supplements (+4.20 €) and Fitness (+3.00 €). Before/after `category_revenue` delta matched exactly; live DOM rendered `Fitness 21.00 €`, `Doplnky výživy 4.20 €`, 4 other active services at `0.00 €`, `Spolu 25.20 €`, sorted DESC. Independently cross-checked with a raw `sqlite3` query against `/opt/spinbike/prod/spinbike.db` using the same SQL and the same computed Bratislava-day UTC bounds — identical numbers. All synthetic transactions/users/login_tokens rows deleted after (verified 0 remaining); scratchpad JWT secret/token files deleted.
- **Playbook gotcha worth keeping in mind for future prod verification:** `transactions.staff_id INTEGER REFERENCES users(id)` is a REAL foreign key on this prod DB (contrary to an older skill note assuming `PRAGMA foreign_keys` is off) — any minted JWT used to drive a real write endpoint (charge, topup, `POST /api/users` with initial credit) needs its `sub` to be a REAL existing user row, not an arbitrary large fake id. The admin-only-page shortcut ("mint a JWT with a non-existent `sub`, no DB row needed") only holds for READ-only / role-boundary checks — the moment the flow WRITES a `transactions` row as that actor, insert a real (synthetic) user row first.

## 2026-07-23 — #253: welcome.rs module doc fixed to match current Invalid-branch behavior (dev.111)

- **Why:** doc comment (lines ~1-6) still claimed the invalid/expired/missing
  token fallback rendered "a friendly message plus the shared `LoginLinkForm`"
  — stale since the #247/#230 code-login work replaced that fallback with
  `CustomerLoginMethods`. Found by the post-merge deep review of PR #250.
- **Fix**: rewrote the doc sentence to say the Invalid branch renders the
  shared `CustomerLoginMethods` (code-first per #247, both methods reachable
  via toggle). Docs-only, no behavior change — `[no-test: doc-comment-only
  change, no logic]`.
- **Commit**: `docs(ui): fix stale welcome.rs module header ...` (solo PR).

## 2026-07-21 — #232: edit-user invite stays in-sheet, renamed save+send (dev.100)

- **Why:** "Poslat pozvanku" closed the edit sheet unconditionally on BOTH
  invite success AND error, with no in-sheet confirmation the whole form
  (email + `allow_self_entry`) had already been saved — operator had to
  reopen the sheet just to tick the checkbox and Save again.
- **Fix** (`edit_info_form.rs`): button renamed `send_invite` key text →
  "Ulozit a poslat pozvanku"/"Save & send invite" (#141 semantics). Invite
  success AND error both keep the sheet open now — success shows a new
  in-sheet green alert (`invite_sent_in_sheet`, dead `invite_sent` key
  removed), error routes to the existing in-sheet `save_err` red alert
  instead of the shared dashboard channel.
- **Deferred-flush architecture:** the saved `CardInfo` is stashed in an
  OUTER-scope `StoredValue<Option<CardInfo>>` (stashed right after the SAVE
  step commits, before the invite call — covers both invite outcomes) and
  flushed to `set_selected` by a SINGLE `Effect` watching `show`'s
  true→false transition centrally (mirrors the existing false→true refresh
  Effect) — NOT enumerated per close button. A code-review pass caught a
  real 4th close path the first (enumerated) version missed:
  `card_panel.rs`'s own "Edit info" toggle button bypasses EditInfoForm's
  Cancel/backdrop entirely. Stash cleared at the top of every fresh
  save/invite action so a plain Save can't race the Effect's re-flush.
- **Tests:** `invite-button.spec.ts` — two pre-existing tests deliberately
  flipped (sheet used to close on invite, now stays open — justified in the
  commit message per regression-test-first's "flip the test" allowance);
  new invite-ERROR-path test; new combined email+checkbox+reopen test; new
  regression test for the 4th close path (keyboard-activated toggle button,
  since the Sheet's backdrop blocks a real mouse click on it — `.focus()` +
  `Enter` instead of `.click()`). Collateral fix in `door-open.spec.ts`
  (`hasText: 'Save'` now also matched the renamed invite button).
- **Commits:** `78a3af1` (bump) · `3f0cdfb` (impl) · `668435c` (test flip) ·
  `15dfb54`/`3734dc9`/`b0edd36`/`da82c41`/`b22c5eb` (review-driven fixes:
  CI collateral breakage, stash-before-invite-call, centralized flush
  Effect, clippy collapsible_if, keyboard-reachable test), on `dev`. PR
  [#233](https://github.com/zbynekdrlik/spinbike/pull/233), merged
  `94b465b`. Main CI green incl. Deploy (prod) + Smoke (prod). #232
  auto-closed.
- **Review:** two independent passes (self-review + a fresh-eyes
  `superpowers:requesting-code-review` deep pass) each found one real bug
  before merge — the stash-before-invite-call ordering, and the 4th
  close-path bypass — both fixed and re-verified green before merging.
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.100):** DOM version
  matches deployed, stale SW cleared first. Minted an admin-role JWT
  (no DB row needed — `AdminUser` extractor checks only the JWT claim),
  created ONE throwaway customer via the real `POST /api/users`, opened
  `/staff?card=<code>`, confirmed the edit sheet renders the renamed
  "Save & send invite" button + the `allow_self_entry` row, zero console
  errors. Full invite-flow already covered by CI Playwright, so no invite
  was actually sent live. Cleaned up: soft-delete via the real API +
  hard-delete via SQL (the endpoint only soft-deletes), zero rows left.

## 2026-07-16 — #225 + #226: apple-touch-icon + iOS install guide v2 (bundled)

- **Issues:** [#225](https://github.com/zbynekdrlik/spinbike/issues/225) —
  no `apple-touch-icon`, so iOS "Add to Home Screen" used a page-screenshot
  thumbnail. [#226](https://github.com/zbynekdrlik/spinbike/issues/226) —
  the iOS install guide was 2 lines of plain emoji text, and in-app browsers
  (Instagram/Facebook/etc.) have no A2HS surface at all so the guide was
  actively misleading there. Both STILL_VALID, bundle-safe (no schema/API/
  security overlap, <300 LoC each) → one PR.
- **Version:** bump `7ea006a` (0.15.0-dev.93 → .94).
- **#225** (`2b2bcee`) — new `spinbike-ui/apple-touch-icon.png` (180x180,
  opaque `#15151a`, `convert -flatten` from icon-512.png; `git add -f` past
  the root `.gitignore`'s `*.png` rule) + `<link rel="apple-touch-icon">` +
  `apple-mobile-web-app-title` meta + Trunk copy-file directive in
  `index.html`. E2E: new assertion in the manifest-eligibility spec.
- **#226** (`81dfafe`) — reworked `install_prompt.rs`'s iOS branch: inline
  SVG glyphs (share icon, plus-square icon) replace emoji, numbered steps,
  a share-sheet scroll hint, a permanent footer fallback hint; new
  `PromptKind::IosWebview` UA-sniffs known in-app-browser markers (FBAN/
  FBAV/FB_IAB/Instagram/Line///GSA/) and swaps the A2HS steps for an
  "open in Safari" instruction + copy-URL button
  (`navigator.clipboard.writeText` via `js_sys::Reflect`, silent-degrade).
  Preserved the iPadOS-13 `MacIntel`+`maxTouchPoints>1` disambiguator
  (`c51b1ff`) untouched. New i18n keys (Sk unaccented + En). E2E: extended
  the Safari-guide test (SVG icons + hints) + new webview describe block.
- **Two-round parallel review before merge** (per-diff-scoped, not the full
  repo): round 1 (correctness + cleanup angles, 2 parallel agents) found 6
  issues — fixed in `1b458ab` (domain-agnostic footer hint; the
  `navigator.clipboard.writeText()` call moved to fire SYNCHRONOUSLY in the
  click handler with only the returned `Promise` awaited in `spawn_local`,
  since some WebKit builds only honor Clipboard-API user-activation on a
  synchronous dispatch; deduped the UA-fetch between `is_ios_ua`/
  `is_ios_webview_ua` into one shared `user_agent()` call; 3 dead-CSS/class
  cleanups). Round 2 (`superpowers:requesting-code-review` deep pass) caught
  a real bug the first round missed: `InstallPrompt` also mounts on
  `/welcome?t=<token>` right after a magic-link redemption, and that page
  never strips `?t=` from the address bar — the copy-URL button was reading
  raw `location.href`, so it would hand a webview user their own
  already-spent token. Fixed in `ac2b486`: copy `location.origin +
  location.pathname` instead, with a new E2E regression test.
- **CI:** dev green across all 3 pushes (Lint, Test, Build WASM (UI), Test
  (UI), all 8 mutation shards, E2E, Deploy+Smoke (dev)). PR
  [#229](https://github.com/zbynekdrlik/spinbike/pull/229) — body `Closes
  #225` + `Closes #226` on separate lines — merged `4374a04`. Main CI green
  incl. Deploy (prod) + Smoke (prod).
- **Deployed + verified LIVE on `https://spinbike.sk` (v0.15.0-dev.94):**
  `apple-touch-icon.png` 200 `image/png`, link+meta tags present in served
  HTML, DOM version matches `/api/version`, 0 console errors (stale SW
  registration cleared first). Downloaded the live wasm bundle and grepped
  for the exact new/fixed strings (all 5 new i18n keys, the webview UA
  markers, the SVG path data, AND `origin`/`pathname` — proving the
  query-string fix is the bytes actually deployed, not the pre-fix `href`
  version). Synthetic customer session (id 584, cleaned up after) confirmed
  `/my/balance` renders correctly with `InstallPrompt` mounted (renders
  nothing on desktop Chromium UA, as expected — matches CI's own "desktop:
  neither surface renders" case) with 0 console errors. The iOS-specific UA
  branches (Safari guide, webview guide, iPadOS disambiguator) could not be
  re-driven live via the Playwright MCP browser (no `addInitScript`-
  equivalent to override `navigator.userAgent` before the WASM module
  boots) — CI's E2E suite already drove all of them against this exact
  byte-identical deployed build.
- **Playbook:** noted (via this log, not yet folded into
  `frontend-pwa/SKILL.md`) two new gotchas: the synchronous-Clipboard-write
  timing requirement, and the shared-`user_agent()`-fetched-once pattern.

## 2026-07-12 — #167 (tokio-tungstenite sub-item, 3/3): bump 0.24 → 0.30 — CLOSES #167

- **Issue:** [#167](https://github.com/zbynekdrlik/spinbike/issues/167) —
  dependency-currency epic. FINAL of 3 sub-items (rand + leptos already
  merged) → this PR carried `Closes #167`; issue now CLOSED.
- **Validated first:** re-derived the pins live (workspace `Cargo.toml`
  `tokio-tungstenite = "0.24"`), confirmed latest stable = **0.30.0** via
  crates.io API, and read the tungstenite changelog 0.24→0.30 — the ONLY
  breaking change touching this code is 0.26's Message payload overhaul
  (`Message::Text` → `Utf8Bytes`, `Binary/Ping/Pong` → `Bytes`). Features
  (`rustls-tls-native-roots`, `connect`) unchanged in 0.30; no
  connect_async/TLS/crypto-provider change.
- **Change:** pin `0.24`→`0.30`; three outbound `Message::Text(<String>)`
  sites in `ewelink/ws.rs` (userOnline, door update-press, keepalive ping)
  wrapped `.into()`; two mock-server test sites in `tests/ewelink_ws.rs`
  same. Received text derefs to `&str` (unchanged); Ping→Pong echo
  `Bytes→Bytes` (unchanged). `Cargo.lock` refreshed via
  `cargo update -p tokio-tungstenite@0.24.0 --precise 0.30.0` (metadata-only).
- **Unrelated compile break fixed (see ci-deploy skill):** the bump dropped
  the last transitive consumer of rand 0.8, which was silently enabling
  `rand_core 0.6/getrandom` via feature-unification → `auth/mod.rs`'s
  `argon2::password_hash::rand_core::OsRng` stopped resolving (E0432).
  Fixed by declaring `password-hash = { features = ["getrandom"] }` on the
  server explicitly. Existing `password_hash_and_verify()` test proves it.
- **No RED→GREEN** — not a bug fix, zero behavior change; existing
  `ewelink_ws.rs` round-trip tests (green) prove wire semantics unchanged.
- **Commits:** `a24485c` (bump dev.86), `970fab5` (tungstenite + tests +
  lock), `812c8d9` (password-hash getrandom fix). PR
  [#220](https://github.com/zbynekdrlik/spinbike/pull/220), merged
  `2905590`. Dev + main CI green incl. all 8 mutation shards, E2E,
  Deploy+Smoke (prod).
- **Review:** one focused senior inline pass (tiny mechanical diff, CI
  covered the real risk) — 0 🔴 0 🟡 0 🔵.
- **Verified LIVE on prod (v0.15.0-dev.86, `https://spinbike.sk`):** version
  DOM = backend `/api/version` = deployed, 0 console errors. **Real door
  actuation:** new binary's log shows `ewelink: WS connected + handshake ok`
  (tungstenite 0.30 TLS handshake + userOnline round-trip OK); synthetic
  staff `POST /api/door/open` → `200 {"status":"opened"}`, `/api/door/health`
  `last_ack_ms_ago` null→2033 — device `error:0` ack received+parsed on 0.30.
  Physical Sonoff relay buzz is user-only-observable; cloud ack is the proof.
  Synthetic user + tx cleaned up.
- **Playbook:** added the "dep bump drops a transitively-provided Cargo
  FEATURE → breaks an unrelated module" gotcha + the safe prod door-actuation
  test recipe to `ci-deploy/SKILL.md`.

## 2026-07-12 — #167 (leptos sub-item): bump leptos 0.7 → 0.8

- **Issue:** [#167](https://github.com/zbynekdrlik/spinbike/issues/167) —
  dependency-currency epic (rand, tokio-tungstenite, leptos all behind). SOLO
  PR for the leptos sub-item only — issue stays OPEN (tokio-tungstenite still
  to follow); PR body used "Part of #167", never "Closes #167".
- **Validated first:** grep-confirmed zero `server_fn`/`#[server]`/
  `ServerFnError`/`leptos_axum` usage anywhere (CSR-only frontend, separate
  Axum backend) — leptos 0.8's breaking changes don't apply here.
- **Change:** `spinbike-ui/Cargo.toml` `leptos`/`leptos_router` `"0.7"` →
  `"0.8"` (resolves 0.8.20 / 0.8.14), `Cargo.lock` regenerated via
  `cargo metadata` (resolution-only, no build). One clippy fix:
  `#[allow(dead_code)]` on `class_card.rs`'s discarded booking-response `id`
  field (0.8's stricter transitive toolchain caught a latent dead field 0.7
  never flagged) — matched the existing idiom already used at 3 sibling
  call sites (`upcoming_classes.rs`, `staff_dashboard.rs`,
  `persistent_toggles.rs`).
- **No RED→GREEN test pair** — not a bug fix, zero behavior change (same
  reactive idioms). Used the `[no-test:]` push-gate bypass (Gate 1: `.rs`
  changed, no test diff) citing the full E2E suite as the real regression
  gate for a framework bump, per dispatch instructions.
- **CI:** dev push green — Build WASM (UI), Test (UI), full E2E suite, all
  8 mutation shards, Deploy+Smoke (dev) all passed on commit `2ec4be8`. PR
  [#219](https://github.com/zbynekdrlik/spinbike/pull/219), merged
  `fd293d4a`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Review:** one focused senior pass (correctness/security/perf/
  maintainability/style + deep requirements/hidden-breakage lens) since the
  diff was tiny (4 functional lines) and CI already covered the real risk
  (compile + full E2E) — 0 🔴 0 🟡 0 🔵 on `/review`; 0 🔴 0 🟡 1 🔵 on
  `requesting-code-review` (non-blocking commit-message precision nit,
  outside the code diff).
- **Deployed + verified LIVE on `https://spinbike.sk` (v0.15.0-dev.84):**
  root schedule page renders `ClassCard` (the exact modified component),
  version DOM matches `/api/version`, 0 console errors; clicked a day-picker
  button — Resource-driven re-render worked; synthetic customer session
  (`autopilot-verify-167leptos@spinbike.local`, cleaned up after) confirmed
  `/my/balance`'s data-fetch pipeline renders correctly and the global
  `RwSignal<Lang>` context toggle (EN→SK) propagated live across nav + body
  + footer with 0 console errors.
- **Playbook:** added the "major dep bump can surface a latent clippy lint
  unrelated to the crate's own API surface" gotcha to `ci-deploy/SKILL.md`
  (continuing the #167 rand-sub-item entry) — the `#[allow(dead_code)]`
  fix + where to find the established idiom for future dep-bump sub-items.

## 2026-07-12 — #204: enforce the active-pass invariant at the schema level (V20 trigger)

- **Issue:** [#204](https://github.com/zbynekdrlik/spinbike/issues/204) —
  split out of #179; the "which transaction row counts as a monthly pass"
  predicate (`action='charge' AND service kind='monthly_pass' AND valid_until
  IS NOT NULL`, canonicalized by V18's `user_active_pass` view) was
  application-level only. Ticket-validated STILL_VALID: 0/4671 live prod rows
  violate the invariant (confirmed #178/#179), so this is defence-in-depth,
  not a live-bug fix.
- **Version:** bump `d553ab3` (0.15.0-dev.81 → .82).
- **Migration V20:** `CREATE TRIGGER enforce_active_pass_invariant BEFORE
  INSERT ON transactions WHEN NEW.valid_until IS NOT NULL` — `RAISE(ABORT,…)`
  unless `action='charge'` AND `service_id` resolves to `kind='monthly_pass'`.
  Standalone DDL (no table rebuild needed, unlike V8/V11/V16's CHECK-add
  dance). INSERT-only: confirmed no code path UPDATEs `action`/`service_id`
  post-insert, and `patch_valid_until` only re-dates an already-qualifying row.
- **Tests (RED→GREEN):** `db::migrations::tests::v20_enforces_active_pass_invariant`
  — 5 cases (bad action, bad service, NULL service_id all rejected; the
  legitimate pass shape and a plain valid_until-NULL row both accepted). RED
  `710d00d` → GREEN `2782ead`.
- **Test-seed fixes (assertions unchanged, setup only):** 3 unit tests
  (`v12_normalizes_every_legacy_pattern`, `v18_user_active_pass_view_is_canonical`,
  `db::transactions::transaction_stores_and_retrieves_valid_until`) drop the
  trigger for a deliberately-legacy/invalid seed or swap a hardcoded service
  id for the real monthly_pass id. `v8_drop_rename_pattern_works_with_fk_child_rows`
  now also drops+recreates the V20 trigger around its simulated `services`
  rebuild (`6ea145b`) — same class as the pre-existing V18 view requirement.
  4 integration-test seeds in `crates/spinbike-server/tests/` (a sibling of
  `src/`, missed by the first grep sweep and the cause of a second failed CI
  run) were non-compliant (`reports.rs`, `transactions_routes.rs` x2 left
  `service_id` NULL; `users_delete.rs` used `action='topup'`) — fixed `c27283d`.
- **Review:** deep `superpowers:requesting-code-review` senior pass (Opus,
  base `63bed25`..head `c27283d`) — 0 Critical, 0 Important, 2 Minor/
  informational (both explicitly out-of-scope, no fix needed) — plus a fast
  `/review` pass on the PR diff, also clean.
- **CI:** dev push green (all jobs incl. all 8 mutation shards, E2E, Deploy
  (dev), Smoke (dev)). PR [#218](https://github.com/zbynekdrlik/spinbike/pull/218),
  merged `7889546`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Deployed + verified LIVE on `https://spinbike.sk` (v0.15.0-dev.82):**
  DOM version matches `/api/version`, 0 console errors. Confirmed the trigger
  exists on the real prod DB (`sqlite_master` query) and 0 rows violate the
  invariant. Functionally exercised the ONE legitimate write path for real —
  synthetic staff+customer accounts (`autopilot-verify-204-*@spinbike.local`),
  minted JWTs, drove `POST /api/payments/sell-pass` through the live prod
  API (transaction id 91989, `action='charge'`, `service kind='monthly_pass'`,
  `valid_until` set) — proving the trigger accepts the real shape post-deploy.
  All synthetic rows + secret/token scratch files deleted after.
- **Playbook:** `.claude/skills/db-migrations/SKILL.md` — generalized the
  "VIEW referencing services/transactions breaks the rebuild pattern" gotcha
  to cover triggers too, and added a new gotcha documenting that a
  schema-invariant grep sweep must independently cover `src/`, `tests/`
  (a sibling dir invisible to a `src/`-scoped grep — the actual gap hit
  here), and `e2e/*.spec.ts`. Committed as a small dev-only follow-up after
  #204's own PR had already merged (version bump `9ae14fc` → 0.15.0-dev.83,
  docs commit on top; both pushed at `f20d223`, dev CI green) — no separate
  PR to main yet; rides the next ticket's PR.

## 2026-07-12 — #212: sw.js edge-cached by Cloudflare for 4h

- **Issue:** [#212](https://github.com/zbynekdrlik/spinbike/issues/212) — found
  during #208's post-deploy verification. Ticket-validated STILL_VALID +
  confirmed LIVE right before work started (`curl spinbike.sk/sw.js`:
  `cf-cache-status: HIT`, `age` growing toward 14400).
- **Version:** bump `14c6a54` (0.15.0-dev.78 → .79).
- **Tests (RED→GREEN):** `crates/spinbike-server/tests/static_files.rs::sw_js_gets_no_cache_control_for_revalidation`.
  RED `8c60d96` (verified locally, scoped bypass per `ci-deploy/SKILL.md`'s
  allowance for a bug-fix ticket: `left=None, right=Some("no-cache")`) → GREEN
  `f747cdb` (all 6 tests pass). Two characterization tests added alongside
  (`hashed_asset_still_gets_long_cache_immutable_header`,
  `manifest_json_gets_no_explicit_cache_control`) guarding existing behavior.
- **Fix:** `static_handler` (`routes/static_files.rs`) special-cases `sw.js`
  (`else if` sibling of the `assets/` branch) → `Cache-Control: no-cache`.
  CI's placeholder-dist step (3 call sites: `ci.yml` test + mutation-test
  jobs, `mutation-full.yml`) extended to also create `sw.js`/`assets/`/
  `manifest.json` placeholders so the tests can exercise real `Asset::get`.
- **Second root-cause layer found + fixed LIVE (not in git — CDN config):**
  the origin fix alone did NOT stop edge caching — both Cloudflare zones
  (`spinbike.sk`, `newlevel.media`) are Free plan with a fixed
  `browser_cache_ttl=14400` and no "respect origin headers" toggle
  (Enterprise-only). Added a Cache Rule (Rulesets API — legacy Page Rules
  endpoint rejects account-owned tokens, code 1011) bypassing cache for
  `/sw.js` on both zones, via a temporary scoped API token (revoked after
  use). Documented on the issue (comment) + `frontend-pwa/SKILL.md` (full
  recipe + both zone IDs).
- **Review:** self-review across correctness/removed-behavior/cross-file/
  reuse/altitude/conventions angles (small diff, ~9 LoC real logic change) +
  deep `superpowers:requesting-code-review` dispatch (base `dd5a282`..head
  `35674f8`) — 0 🔴 0 🟡, one Minor (3x-duplicated CI placeholder step, noted
  as acceptable for one extra file, not blocking).
- **CI:** dev push green (all jobs incl. all 8 mutation shards, E2E, Deploy
  (dev), Smoke (dev)). PR [#215](https://github.com/zbynekdrlik/spinbike/pull/215),
  merged `ec7384d`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Deployed + verified LIVE on `https://spinbike.sk` (v0.15.0-dev.79 then
  .80):** DOM version matches `/api/version`; cleared a stale SW registration
  in the long-lived Playwright profile first (per the existing gotcha below),
  0 console errors; `curl`/in-page `fetch` both confirm `cache-control:
  no-cache` + `cf-cache-status: DYNAMIC` (never `HIT`) on `/sw.js`.
- **Playbook:** rewrote the `frontend-pwa/SKILL.md` #212 section from "known
  issue, fix direction" into "fixed — two layers, here's the recipe", with
  the Cache Rules API recipe + both zone IDs. Follow-up docs-only PR
  [#216](https://github.com/zbynekdrlik/spinbike/pull/216) (version bump
  `c004f2e` → 0.15.0-dev.80, merged `fc00ef4`) since #215 had already merged
  by the time the write-up was ready.

## 2026-07-11 — #165: split routes/users.rs by concern

- **Issue:** [#165](https://github.com/zbynekdrlik/spinbike/issues/165) —
  1105-LoC `routes/users.rs` tangled staff-CRUD, customer self-service
  (`my_balance`), and magic-link invite onboarding. Ticket-validated
  STILL_VALID with a fully settled design in the issue comments (rescope: keep
  `user_transactions`/`user_stats` in users.rs — thematic mismatch with
  transactions.rs/reports.rs). Solo PR, pure reorg → auto-merge.
- **Version:** bump `289aae0` (0.15.0-dev.72 → 0.15.0-dev.73).
- **Work** (`0b4a4ec`) — NEW `routes/my_balance.rs` (142 LoC): `my_balance`
  handler + `BalanceResponse`/`RecentTx` moved verbatim (`GET
  /api/my/balance`, `AuthUser`). EXTEND `routes/auth.rs` (+97 LoC):
  `invite_user`/`invite_email`/`InviteResponse` moved beside the existing
  magic-link machinery (`POST /api/users/{id}/invite`, `StaffUser`); only
  content change is `db::` → `users::` (same fn). `routes/users.rs` shrinks to
  881 LoC (staff-CRUD + `user_transactions`/`user_stats`, per the rescope).
  Pure refactor — no RED/GREEN pair; `[no-test: pure refactor, code moved
  between files with zero behavior/URL/JSON-shape change]` marker commit
  (`9fe2e6a`) bypasses the push gate (no test file needed changes, deep
  code-review confirmed every moved handler byte-identical to its original).
- **PR:** [#209](https://github.com/zbynekdrlik/spinbike/pull/209) — CI green
  incl. all 8 diff-scoped mutation shards (moved logic re-mutated fresh in
  its new location per the ci-deploy skill's gotcha — zero survivors), E2E,
  dev deploy+smoke. `/review` + `/requesting-code-review` both clean (0🔴 0🟡
  0🔵 — reviewer diffed every moved item against the pre-move original and
  found zero drift). Merged `17f0a0f`. Main CI green, prod deploy+smoke green.
- **Post-deploy verify:** synthetic customer (id 576) + staff JWT (per
  `prod-verification` skill) confirmed `GET /api/my/balance` (200, correct
  shape) and `POST /api/users/{id}/invite` (200) both work through their new
  file locations on live `spinbike.sk`. Playwright DOM read of `/my/balance`
  showed the credit/recent-activity render correctly, version footer
  `v0.15.0-dev.73`, zero console errors. Synthetic user/transaction/
  login_tokens rows cleaned up after.

## 2026-07-11 — #168: consolidate duplicated UI date parse/format helpers

- **Issue:** [#168](https://github.com/zbynekdrlik/spinbike/issues/168) —
  ISO-date parsing and the `DD.MM.YYYY` renderer were re-derived across the UI.
  Ticket-validated STILL_VALID (rescope comment on the issue: real surface was
  ~10 sites / 8 files, not the 4 cited; #146 had even added a new
  `parse_booking_date` instance). Solo PR, no schema/API/security → auto-merge.
- **Version:** bump `32cfa88` (0.15.0-dev.52 → 0.15.0-dev.53).
- **Work** (`7fbce66`) — new `spinbike-ui/src/dates.rs` (registered in `lib.rs`):
  `parse_server_date` (my_balance's trim+split_whitespace+split('T')+`%Y-%m-%d`,
  a safe superset of all 6 inline ISO parsers) + `format_ddmmyyyy`
  (`d.format("%d.%m.%Y")`, the shared digit renderer). 6 parse sites now route
  through `parse_server_date` (my_balance's `parse_pass_date`/`parse_visit_date`
  deleted; my_bookings `parse_booking_date` deleted; staff_dashboard,
  negative_balance_list, card_panel, persistent_toggles). Both render sites
  (`i18n::fmt_date` Sk arm + `relative_date::format_date`) delegate to
  `format_ddmmyyyy`.
- **PRESERVED (DO-NOT-MERGE, would be bugs):** `date_input::parse_user_date`
  (9-format lenient interactive parser) untouched; `relative_date::format_date`
  stays locale-INDEPENDENT (always DD.MM.YYYY, even En staff) — shares only the
  digits; `i18n::fmt_date_short` untouched.
- **Latent bug fixed:** `delete_user.rs` pass-expiry warning hard-coded
  `.format("%d.%m.%Y")`, bypassing `lang` → now `i18n::fmt_date(d, lang.get())`
  (En staff no longer forced to DD.MM.YYYY). Also routed `transactions_list`'s
  inline UTC→Bratislava parse through the now-`pub` `i18n::parse_to_local`.
- **Tests:** no bug-fix RED→GREEN mandate (refactor); behavior-preservation is
  the net — added 8 `#[wasm_bindgen_test]` in `dates.rs` (bare/space/T/whitespace/
  garbage parse, zero-pad + two-digit format, roundtrip). Existing relative_date
  combined-format + i18n datetime tests + E2E stayed green.
- **Gotcha:** UI crate has NO mutation gate (`mutation-ui` job intentionally
  absent, #47) — so a new UI module is not mutation-tested (unlike the #166
  server-crate case). `wasm-pack test --node` needs `#[wasm_bindgen_test]`, not
  plain `#[test]`.
- **Deploy:** merge `953a3351` → main CI green → prod v0.15.0-dev.53. Live
  Playwright verify (synthetic customer 575, cleaned up): /my/balance movements
  `11.07./10.07./09.07.` + pass expiry `do 11.08.`, /my/bookings `13.07.`,
  version DOM `v0.15.0-dev.53`, 0 console errors.
- **PR:** [#194](https://github.com/zbynekdrlik/spinbike/pull/194).

## 2026-07-11 — #146 + #147: bundled batch — bookings/movements enrichment

- **Issues:** [#146](https://github.com/zbynekdrlik/spinbike/issues/146) —
  `/my/bookings` rendered `"Class #<internal template id> — <ISO date>"`,
  meaningless to a customer. [#147](https://github.com/zbynekdrlik/spinbike/issues/147) —
  `/my/balance` movements didn't name the service a movement was for, even
  though the admin transactions list already does. Both ticket-validated
  STILL_VALID against current `dev` (grepped: no service join on the
  `my_balance` query, `format!("Class #{template_id} — {date}")` still
  literally in `my_bookings.rs`); pure read-enrichment, no schema change,
  zero file overlap → bundled one PR per the batch gate.
- **Version:** bump `26f2d81` (0.15.0-dev.46 → 0.15.0-dev.47).
- **#147** (`f502ecf`) — `my_balance`'s inline recent-transactions query
  gained `LEFT JOIN services s ON s.id = t.service_id` (same pattern as
  `db::transactions::list_transactions_for_user_paginated`, used by the
  admin view); `RecentTx` gained `service_name_sk`/`service_name_en`.
  Frontend renders it via a `service_label(lang)` helper. Falls back to
  showing nothing when the movement has no linked service (a plain
  top-up).
- **#146** (`cdb1c95`) — `db::classes::list_user_bookings` now JOINs
  `class_templates` + `instructors` (mirroring how
  `list_upcoming_for_user` resolves `instructor_name`), returning
  `start_time` + `instructor_name`. Frontend drops the raw
  `template_id`/ISO date and renders `fmt_date_short(date, lang)` +
  start time as the title, instructor as the sub-line — mirroring
  `UpcomingClasses`'s layout. Spin-only app, so no class name needed.
- **Review-driven refactor** (`2605f29`, two independent parallel
  passes — a 3-angle finder fan-out before merge, then a full
  `requesting-code-review` deep pass, both clean after): `RecentTx` now
  derives `sqlx::FromRow` (column-name matched) instead of an
  8-field manual tuple destructure; split a NEW `MyBookingResponse`
  (start_time + instructor_name) off the shared `BookingResponse`
  instead of bolting always-null fields onto the type `create_booking`'s
  echo response also uses — same reasoning as the `_coded` API variant
  pattern from #145; extracted the Sk/En service-name pick into a
  shared `i18n::service_label` helper used by BOTH the admin
  `TxnInfo::service_label` and the new customer `RecentTx::service_label`
  (was duplicated); `my_bookings.rs`'s instructor sub-line now renders
  via `Option<impl IntoView>.map(...)` (confirmed via Leptos's own docs:
  renders nothing on `None`) instead of a match with a dummy empty
  `<span>`.
- **Tests:** `classes_routes.rs` — extended
  `my_bookings_returns_user_bookings` (asserts `start_time="17:00"`,
  `instructor_name` null) + new `my_bookings_includes_instructor_name`
  (V6-seeded Monday-18:00-Stevo template, asserts both fields).
  `users_routes.rs` — new `my_balance_recent_includes_service_name`
  (charges against the seeded Spinning service, asserts
  `service_name_sk`/`service_name_en`) +
  `my_balance_recent_service_name_null_for_topup` (a plain top-up
  degrades to `null`, not an error). E2E: new `e2e/tests/my-bookings.spec.ts`
  (discovers the real `template_id` via the public `/api/classes`
  endpoint, books, asserts the row shows `"18:00"` + `"Stevo"` and
  NEITHER `"Class #"` NOR a raw `\d{4}-\d{2}-\d{2}` ISO date); extended
  `e2e/tests/my-balance-movements.spec.ts` (both EN and SK describe
  blocks now assert `"Spinning"` + `"Monthly pass"`/`"Mesačná
  permanentka"` render on the movement rows).
- **PR:** [#191](https://github.com/zbynekdrlik/spinbike/pull/191) —
  merged `c7c974c`. CI on `dev` green (incl. all 8 mutation-testing
  shards) both before and after the review-driven refactor commit; main
  CI green, `Deploy (prod)` + `Smoke (prod)` both passed.
- **Deployed:** v0.15.0-dev.47, confirmed on `https://spinbike.sk`. Live
  functional verification used a synthetic throwaway customer (own user
  row, cleaned up after — same pattern as the #109 cycle): booked the
  REAL Monday-18:00-Stevo occurrence via `POST /api/bookings` and seeded
  one real `charge` transaction against the real Spinning service via
  direct SQL, then read both `/api/my/bookings` and `/api/my/balance`
  AND the live rendered DOM (Playwright, stale-SW cleared first).
  `/my/bookings` row showed `"13.07. 18:00"` / `"Stevo"` (no `"Class #"`,
  no raw ISO date). `/my/balance` showed `"Výdaj z kreditu"` /
  `"11.07. · Spinning"` / `"-5.00"`. DOM version label matched
  `v0.15.0-dev.47` on both pages. 0 real console errors (only the known
  #188 wasm-bindgen deprecation warning + an unauthenticated-navigation
  401, both pre-existing/filtered). Synthetic user, transaction, and
  booking all deleted after verification (booking cancelled via the real
  `DELETE /api/bookings/{id}` API, user+transaction rows removed
  directly).

## 2026-07-11 — #145: localize customer error banners via error_code

- **Issue:** [#145](https://github.com/zbynekdrlik/spinbike/issues/145) —
  customer-facing error/alert banners rendered raw English (e.g. a
  Slovak customer mistyping a password on `/login` saw "Invalid email or
  password"). Ticket-validated PARTIAL: the backend prerequisite (a
  machine-readable `error_code` on every API error body) had already
  landed via #158/PR #181 (same-day architecture-review split), so this
  cycle rescoped to frontend-only: read `error_code`, map to Slovak.
- **Version:** bump `d1be15f` (0.15.0-dev.44 → 0.15.0-dev.45).
- **RED** (`e322f17`) — new `e2e/tests/auth.spec.ts` test (separate
  describe, no forced language — a fresh browser context defaults to
  Slovak via `i18n::get_saved_lang()`) asserting a wrong-password login
  shows `"Nespravny email alebo heslo"`, not raw English. Confirmed
  failing on CI (`Received: "Invalid email or password"`), 163/164 other
  E2E tests unaffected. Run:
  https://github.com/zbynekdrlik/spinbike/actions/runs/29135592859
- **GREEN** (`d4bd0d4`) — `api.rs` gained additive `get_coded`/
  `post_public_coded`/`delete_coded` (alongside the untouched originals
  — ~62 other call sites in the app unaffected) returning a new
  `CodedError{code, message}`; `error_code` parsed defensively (raw
  string first, then matched into `ErrorCode` — an unrecognized code
  degrades to `None` rather than failing the whole body parse).
  `i18n.rs` gained `error_code_key()` — an exhaustive match (same
  pattern as `tx_label_key`) mapping ONLY the 6 codes a customer can
  hit at the 5 scoped render sites (`invalid_credentials`,
  `oauth_account`, `booking_not_found`, `booking_not_owned`,
  `user_not_found`, `internal`); every other code (staff_required,
  conflict codes, etc.) resolves to `None` on purpose, falling back to
  the server's raw English — staff/admin errors are unchanged, out of
  this ticket's scope. Also localized the two generic hardcoded
  fallbacks ("Session expired, redirecting to login..." / "Request
  failed (HTTP {status})") via `i18n::get_saved_lang()` (api.rs has no
  reactive `Lang` context). Wired the 5 render sites (`login.rs`,
  `my_balance.rs`, `my_bookings.rs` x2, `door.rs`,
  `login_link_form.rs`) — error signals switched `String` →
  `Option<CodedError>`, localized at render time via each page's own
  reactive `Lang` signal. CI all green (Lint, Test, Test (UI), Build
  WASM, E2E 164/164, all 8 Mutation Testing shards, Deploy (dev), Smoke
  (dev)). Run:
  https://github.com/zbynekdrlik/spinbike/actions/runs/29135842463
- **Decision:** `oauth_account` fires whenever `password_hash` is NULL
  (login.rs's password form against a passwordless account) — this app
  has no actual third-party OAuth button wired into the UI today (the
  code is legacy/forward-looking scaffolding in
  `crates/spinbike-server/src/auth/oauth.rs`), so a specific provider
  name would be misleading. Used a deliberately generic Slovak message
  ("Tento ucet pouziva ine prihlasenie") rather than naming an unused
  provider — documented inline in both `oauth.rs`'s call site comment
  and `i18n.rs`.
- **Review:** inline self-review (10-angle checklist: line-by-line,
  removed-behavior, cross-file callers, Rust pitfalls, wrapper
  correctness, reuse/simplification/efficiency/altitude, CLAUDE.md
  conventions) — 0 findings requiring a fix. Cross-checked the
  `frontend-pwa` skill's gotchas (JS interop, UA sniffing, shared
  status-signal split, sheet occlusion, disposal-ordering) — none apply
  to this diff (no Sheet, no JS interop, no component disposal in the
  changed handlers).
- **PR:** [#190](https://github.com/zbynekdrlik/spinbike/pull/190),
  merged `dac34ed`. Main CI green (Lint, Test, Test (UI), Build WASM,
  E2E, Deploy (prod), Smoke (prod); Version Bump Check + Mutation
  Testing correctly skipped on the main push).
- **Live verification:** cleared stale SW/caches on the long-lived
  Playwright MCP profile first (`frontend-pwa` skill gotcha), then on
  `https://spinbike.sk/login` (default Slovak, no forced language)
  submitted a wrong password — banner showed
  `"Nespravny email alebo heslo"` live on prod. Only console message
  was the expected benign `401` fetch noise (the E2E harness's own
  filtered pattern) — zero real console errors.
- **Deployed:** v0.15.0-dev.45, confirmed on `https://spinbike.sk` DOM
  version label.

## 2026-07-11 — #152: login-link button missing loading feedback

- **Issue:** [#152](https://github.com/zbynekdrlik/spinbike/issues/152) —
  the customer login-link submit button on `/login` gave no visible signal
  while a request was in flight (subtle disabled/opacity change on a
  low-contrast `btn--ghost`); prod logs showed duplicate sends ~2.5 min
  apart for the same email, consistent with users retrying. A
  ticket-validator disproved the original "reactive double-submit"
  hypothesis live (a real double-click already fires exactly one request,
  guarded by `disabled=move || loading.get()`) — the real cause was
  missing loading feedback.
- **Version:** bumped `454d57e` (0.15.0-dev.42 → 0.15.0-dev.43).
- **RED** (`cf738a0`) — new `e2e/tests/login-link.spec.ts` test asserting
  the button shows `"Sending..."` within 1s of a click (well before an
  artificial 500ms response delay) and that a rapid double-click still
  fires exactly one `POST /api/auth/request-login-link`. Confirmed failing
  on CI (button stuck on `"Send login link"`), all 162 other E2E tests
  passed. Run: https://github.com/zbynekdrlik/spinbike/actions/runs/29133430958
- **GREEN** (`4b070a4`) — `login_link_form.rs` now swaps to a new
  `sending_login_link` i18n key while `loading` is true, mirroring the
  sibling staff-login button's existing loading-text pattern (`login.rs`).
  Also added a defensive `appearance: none; -webkit-appearance: none;`
  reset to the `.btn` base rule for the issue's reported iOS text-
  misalignment symptom — not reproducible in Chromium, shipped as an
  honestly-labeled unverified defensive fix, not a confirmed repro+fix. CI
  all green (Lint, Test, Test (UI), Build WASM, E2E, all 8 Mutation
  Testing shards, Deploy (dev), Smoke (dev)). Run:
  https://github.com/zbynekdrlik/spinbike/actions/runs/29133707069
- **Review:** two-stage (`/review` + `superpowers:requesting-code-review`
  via a dispatched general-purpose reviewer scoped to `454d57e..4b070a4`)
  both found 0 🔴 0 🟡 0 🔵 — only optional-only notes (the double-click
  assertion re-verifies already-proven disabled-guard behavior; the
  speculative CSS fix could have been its own commit). No fixes required.
- **PR:** [#189](https://github.com/zbynekdrlik/spinbike/pull/189), merged
  `fc71ff5`. Main CI green (Lint, Test, Test (UI), Build WASM, E2E,
  Supply-Chain Advisories, Deploy (prod), Smoke (prod)).
- **Post-deploy verify gotcha:** this session's long-lived Playwright
  browser profile had a stale service-worker registration from earlier
  test cycles, showing `v0.15.0-dev.30` on the DOM even though
  `/api/version` already served `v0.15.0-dev.43`. Current `sw.js` (network-
  first for `/`, `.html`, `sw.js`, `manifest.json`; cache-first only for
  Trunk's hashed immutable assets; `CACHE_NAME = 'spinbike-v2'`) is already
  the correct fix for this — the stale read was this specific persistent
  test profile carrying an old SW instance, not a deploy bug (confirmed:
  `navigator.serviceWorker.getRegistrations()` + unregister + cache clear +
  reload immediately showed the correct `v0.15.0-dev.43`). **Playbook
  takeaway: before trusting a DOM version read in a long-lived Playwright
  MCP session, unregister any stale service worker + clear caches first**
  — Smoke (prod) CI itself is unaffected since it uses a fresh browser
  context per run.
- **Live verify:** on `https://spinbike.sk/login` (fresh SW state), a real
  click showed the button as `"Odosielam..."` + `disabled=true` within one
  animation frame (caught via a synchronous in-page `requestAnimationFrame`
  poll, since the real round-trip is fast enough that a full MCP
  screenshot round-trip missed the transient state), then swapped to the
  success alert. 0 console errors/warnings on `/login` (the one console
  warning seen — the wasm-bindgen deprecated-init-params message — is the
  pre-existing, already-filed #188, unrelated to this fix).
- **Deployed:** v0.15.0-dev.43, confirmed on `https://spinbike.sk` — DOM
  `"Verzia aplikacie"` == `/api/version` == `v0.15.0-dev.43` (after
  clearing the stale test-profile SW above). prod `spinbike.service`
  active.

## 2026-07-10 — #169 + #171 + #173 + #176: bundled dead-code cleanup batch

- **Issues:** [#169](https://github.com/zbynekdrlik/spinbike/issues/169) —
  51 dead i18n translation keys in `spinbike-ui/src/i18n.rs` (legacy
  card-management cluster, stranded CZK-named keys, unused full weekday
  names, unused service filters, assorted orphans). [#171](https://github.com/zbynekdrlik/spinbike/issues/171) —
  18 dead CSS selectors in `spinbike-ui/style.css`. [#173](https://github.com/zbynekdrlik/spinbike/issues/173) —
  swap an untyped `Reflect`/`Function::call1` JS-interop trick for the typed
  `web_sys::Window::match_media` binding in `install_prompt.rs`. [#176](https://github.com/zbynekdrlik/spinbike/issues/176) —
  remove the dead `Role::can_manage_templates()` method + its lone test
  assertion. All four `/architecture-check`-filed (Opus 4.8, 2026-07-10),
  each independently re-verified STILL_VALID before implementation; bundled
  as one PR since all four are independent, disjoint-file, sub-300-LoC
  cleanups (bundling gate).
- **Version:** bump `39a6246` (0.15.0-dev.40 → 0.15.0-dev.41).
- **#169** (`88f4511`) — re-grepped all 51 named keys individually across
  `spinbike-ui/src/` before deletion (zero hits beyond each key's own
  `m.insert()` line); also dropped the now-empty "Day names (long)" comment
  header left behind by the 7 weekday-name deletions.
- **#171** (`343a17d`) — re-grepped all 18 named selectors; the `.data-table`
  selector was combined with the still-live bare `table` element selector
  across 5 compound rule blocks (`table, .data-table { ... }`) — surgically
  removed only the `.data-table` arm, kept `table` (confirmed live via 4
  `<table>` elements in `admin.rs`). Re-verifying `txn-amount`'s rule
  surfaced a second, previously-unflagged dead selector sharing the same
  rule (`.txn-row--voided .amount` — bare `.amount`, never emitted by
  `my_balance.rs`'s `amount_class`, which only ever produces
  `list-row__amount(--pos|--neg)`) — removed together as a same-rule,
  same-file cleanup.
- **#173** (`c1fa2cd`) — added `MediaQueryList` to `spinbike-ui/Cargo.toml`'s
  web-sys features; `is_standalone()` now calls `Window::match_media(...)`
  directly. Left `navigator.standalone` and `__deferredInstallPrompt`
  untouched (no stable web-sys binding for either, per the frontend-pwa
  skill's documented exception).
- **#176** (`326d8f1`) — grepped `can_manage_templates` repo-wide: only the
  definition + its own test assertion, zero production callers (template
  routes gate on `can_manage_users()`).
- **Push-gate gotcha:** the pre-push hook's Gate 1 ("feature code changed,
  no test files") fires on pure dead-code-deletion cleanups too, not just
  bug fixes — bypassed with a documented `[no-test: ...]` marker commit
  (`78aff46`, folded into a genuine playbook update to `.claude/skills/
  ci-deploy/SKILL.md` documenting the gotcha for future cycles).
- **Deep-review fixes** (`7c6e5bf`) — `requesting-code-review` pass (base
  `8623c1e`, head `78aff46`) found 0 🔴 0 🟡, 2 🔵 minor: `install_prompt.rs`
  fetched the window twice (once via `window_value()`, again via
  `web_sys::window()` for `match_media`) — fixed to fetch once and reuse;
  and the just-added SKILL.md gotcha about the `[no-test: ...]` bypass
  needing one physical line was stale versus the hook's actual current
  behavior (it flattens newlines before matching) — corrected.
- **PR:** [#187](https://github.com/zbynekdrlik/spinbike/pull/187), merged
  `614c619`. CI green throughout (Lint, Test, Test (UI), Build WASM (UI),
  E2E, all 8 Mutation Testing shards, Deploy (dev), Smoke (dev) on the dev
  pushes; Deploy (prod), Smoke (prod) on the main merge).
- **Follow-up filed:** [#188](https://github.com/zbynekdrlik/spinbike/issues/188) —
  pre-existing Trunk/wasm-bindgen console warning ("deprecated parameters
  for the initialization function"), unrelated to this PR (`index.html`'s
  wasm-loader directive last touched by an earlier, unrelated commit),
  found during post-deploy console verification.
- **Playbook:** `.claude/skills/ci-deploy/SKILL.md` gained the "dead-code
  cleanup batch trips Gate 1" gotcha (with the review-fix correction to
  the pre-existing "one physical line" `[no-test: ...]` note).
- **Deployed:** v0.15.0-dev.41, confirmed on `https://spinbike.sk` — DOM
  `[data-testid="version"]` == `/api/version` == `v0.15.0-dev.41`. 0
  console errors/warnings on fresh navigations to `/` and `/login` (an
  earlier `all:true` console dump showed stale messages from a prior
  browser context, not from these navigations — confirmed by re-checking
  with the default since-last-navigation scope). No `???` render artifacts
  on either page (`document.body.innerText.includes('???') === false`).

## 2026-07-10 — #161 + #162: prod-router fixture-route regression test + cargo-deny gate

- **Issues:** [#161](https://github.com/zbynekdrlik/spinbike/issues/161) —
  no test ever exercised the production router build path to prove the
  unauthenticated, arbitrary-role `/api/test/*` fixtures (`seed_account`
  accepts a caller-supplied `role`, no auth guard) are unreachable when
  `SPINBIKE_TEST_MODE` is unset. [#162](https://github.com/zbynekdrlik/spinbike/issues/162) —
  zero supply-chain advisory tooling existed anywhere in the repo. Both
  validated STILL_VALID, bundled (independent, disjoint-file changes).
- **Version:** bump `7df557b` (0.15.0-dev.38 → 0.15.0-dev.39), synced
  Cargo.toml/spinbike-ui/Cargo.toml + regenerated Cargo.lock
  (`cargo metadata`, resolution-only).
- **#161** (`42f7271`) — `production_router_does_not_expose_test_fixtures`
  in `crates/spinbike-server/src/lib.rs`: builds the router with NO
  `test_fixtures` merge, sends an anonymous `role="admin"` exploit payload
  to `seed_account`, asserts no DB row is created + never the handler's
  201; asserts the other 4 fixture routes never return JSON. Router
  fallback returns 200/HTML (SPA) for unmatched paths, not 404 (matches
  `tests/static_files.rs::unknown_spa_route_also_serves_index_html`) — so
  assertions target the removed capability, not a status code. Posted the
  404-vs-200 finding to the issue before implementing.
- **#162** (`c4da6bd`) — `deny.toml` ([advisories] only) + new
  `Supply-Chain Advisories` CI job (`EmbarkStudios/cargo-deny-action@v2`,
  `check advisories`). First run surfaced 2 REAL advisories beyond the
  already-known allowlisted RSA one: RUSTSEC-2026-0190 (anyhow, unsound
  `downcast_mut`) and RUSTSEC-2026-0097 (rand 0.8.5, unsound with a custom
  logger) — fixed via `cargo update --precise` (`be813f0`).
- **Review-driven round 2** (`c324902`) — an independent review pass
  caught that `rand` resolved to a SECOND Cargo.lock instance (0.9.2,
  reachable via axum's `ws` feature → tokio-tungstenite 0.28 →
  tungstenite 0.28) that cargo-deny's own scan silently did NOT flag,
  even though the advisory's raw `patched` ranges prove it's vulnerable.
  Fixed (`cargo update -p rand@0.9.2 --precise 0.9.3`); filed
  [#185](https://github.com/zbynekdrlik/spinbike/issues/185) to track the
  apparent cargo-deny detection gap itself.
- **Review-driven round 3, Critical** (`ec5917f`) — the deep
  `requesting-code-review` pass found #161's test hand-copied
  `start_server`'s router-building logic instead of sharing it — a
  regression that inverted/deleted the real gate inside `start_server`
  would NOT have been caught. Fixed by extracting a shared
  `build_router(test_mode)` function called by both `start_server()` and
  the test; also added `supply-chain-audit` to `e2e`'s `needs:` so a real
  advisory finding actually blocks deploy (was previously racing it in
  parallel, per the same review's Important finding).
- **PR:** [#184](https://github.com/zbynekdrlik/spinbike/pull/184), merged
  `822d519`. CI green throughout (Test Integrity, Version Bump Check,
  Supply-Chain Advisories, Lint, Test, Test (UI), Build WASM, E2E,
  Mutation Testing 8/8 shards, Deploy, Smoke) on every push.
- **Playbook:** `.claude/skills/ci-deploy/SKILL.md` gained a cargo-deny
  section (the `cargo update --precise` disambiguation pattern, the
  "don't trust cargo-deny's silence on a second same-named resolution —
  cross-check the raw advisory + `cargo tree` yourself" lesson) and a
  secret-scan-hook false-positive workaround (test literals, Cargo.lock
  checksum diffs).
- **Deployed:** v0.15.0-dev.39, confirmed on `https://spinbike.sk` — DOM
  `[data-testid="version"]` == `/api/version` == `v0.15.0-dev.39`, 0
  console errors/warnings. Live functional verification: `POST
  /api/test/seed-account` with an anonymous `role="admin"` exploit payload
  against real prod returns `200` HTML (SPA fallback), not a created
  account — same behavior the new test proves.

- #159 Unify "active monthly pass" behind one canonical query — the charger's
  copy omitted `deleted_at IS NULL` (`MAX(date(valid_until))`, no service/action
  filter), so a VOIDED pass still read as active there: zero-amount visit
  written, credit debit skipped, a real money defect (free visit) disagreeing
  with what `my_balance` showed the same customer. 6 sites re-implemented the
  predicate with 3 incompatible definitions. Fix: migration V18 adds a
  canonical SQL view `user_active_pass(user_id, pass_tx_id, valid_until)` —
  per user, the latest non-voided `monthly_pass` charge. All 6 named sites
  (`jobs/charger.rs::tick_as_of`, `routes/users.rs::my_balance`,
  `db/users.rs::get_user_pass_valid_until`/`get_user_pass_tx`/
  `list_all_users_with_pass`/`search_users_with_pass`/`list_negative_balance`)
  now resolve through it. Version bump `88f853e` (0.15.0-dev.33 →
  0.15.0-dev.34). RED `5fcbdea`
  (`crates/spinbike-server/src/jobs/charger.rs::charger_charges_when_pass_is_voided`
  — confirmed FAILED via a scoped local `cargo test` run, Tier-0 bypass
  justified as TDD debugging: amount=0, credit undebited) → GREEN `45da86d`
  (charger switched to the view + structured logging of the pass decision;
  4 test seeds fixed to carry the real monthly_pass service id since the
  canonical predicate now requires it: `db/users.rs`
  `pass_valid_until_returns_max_across_multiple_passes`,
  `pass_validity_ignores_soft_deleted_pass`,
  `list_negative_balance_returns_only_negatives_sorted`; `tests/users_routes.rs`
  `negative_balance_endpoint_round_trips_pass_field` — confirmed PASSED, plus
  the full `jobs::charger`/`db::migrations`/`db::users` unit suites and
  `tests/monthly_pass`/`users_routes`/`door_route`/`reports`/
  `transactions_note`/`transactions_routes`/`users_delete` integration suites,
  `cargo fmt --all --check`, `cargo check --workspace`, `cargo clippy
  --workspace --all-targets -- -D warnings`) → docs `4c6556f` (10-angle
  `/code-review` fan-out found: 3 stale doc comments describing the
  pre-migration subquery mechanism, fixed; the migration's "behaviour-
  preserving" claim corrected to cite the empirical validation rather than
  assert an unconditional guarantee). **Live prod-data validation** (this
  repo's own db-migrations skill mandate — CI-green alone isn't sufficient
  for a query-semantics change): 0/4671 `valid_until` rows diverge between the
  old and new predicate, 0 tie-break "latest pass" winner mismatches across
  every multi-pass user, the 6 customers holding a voided-but-future-dated
  pass independently confirmed with zero pending charger-window bookings, and
  post-deploy a direct query proved the view can never structurally resolve to
  a voided transaction (`0` rows) — plus for those same 6 customers, each
  turned out to also hold a LATER legitimate non-voided pass (staff had
  voided a bad charge and immediately reissued), so the view correctly
  resolved to the newer valid pass, not the voided one. Followup filed
  [#179](https://github.com/zbynekdrlik/spinbike/issues/179) — genuinely
  out-of-scope hardening the review surfaced: `routes/door.rs` still hand-
  rolls its own 7th copy of the predicate (currently semantically identical,
  found by 5 independent review angles, but an architecture-drift risk); a
  pre-existing (byte-identical before this PR) date-vs-datetime boundary bug
  where `my_balance`/`door.rs` treat a pass's expiry day as already expired
  while the charger's inclusive semantics still cover it — 0 customers
  currently at that exact boundary; plus two minor robustness notes
  (`get_user_pass_valid_until`/`get_user_pass_tx` lack the charger's `date()`
  coercion defense; the "valid_until implies monthly_pass" invariant is
  application-level, not schema-enforced). PR
  [#178](https://github.com/zbynekdrlik/spinbike/pull/178), merged `e5eec78`.
  Deployed v0.15.0-dev.34, confirmed on `https://spinbike.sk`: DOM
  `[data-testid="version"]` == `/api/version` == `v0.15.0-dev.34`, 0 console
  errors, `schema_version` row 18 present, prod service active (clean
  restart).

- #157 `resolve_jwt_secret` fail-open → fail-closed (booted with the public
  `dev-secret-change-in-production` default when `JWT_SECRET` was unset/empty
  and not in test mode; forgeable HS256 admin JWT). Worker resumed mid-flight
  from a durable state left by a prior worker that died before the GREEN
  commit: version bump `8cbd412` and RED tests `0b8d990`
  (`crates/spinbike-server/src/lib.rs:167+`, 5 tests) were already on `dev`;
  `bin/server.rs` was already wired to call `resolve_jwt_secret(...)?`. This
  cycle only wrote GREEN `056b218` (flip the match arm: `Some(non-empty)` →
  configured value; unset/empty + `test_mode` → dev default; unset/empty +
  `!test_mode` → `Err`). PR [#177](https://github.com/zbynekdrlik/spinbike/pull/177),
  merged `6e3097c`. Deploy safety pre-confirmed by supervisor (both
  `/etc/default/spinbike-dev` and `/etc/default/spinbike-prod` already set
  `JWT_SECRET`) — prod (`spinbike.service`) restarted clean via the merge's
  CI deploy job, no boot failure. Verified live on `https://spinbike.sk`:
  DOM `[data-testid="version"]` reads `v0.15.0-dev.32`, 0 console errors.

- #133 data-testid on local form-validation error divs (distinguishable from shared dashboard error channel) — commit `0f35565`, PR [#137](https://github.com/zbynekdrlik/spinbike/pull/137), v0.15.0-dev.25 (merge SHA unknowable at commit time since this line ships inside the same PR it documents — see the PR page).

---

## 2026-07-05 — #98: typed Role migration (UserResponse + UserInfo)

- #98 typed Role migration (UserResponse + UserInfo) — PR #135 merged 78d5168d, prod+dev v0.15.0-dev.22, wire-compat via green E2E role-gating (supervisor-completed after worker Monitor-death; logged retroactively here since the worker that implemented #98 died before writing its own log entry).

## 2026-07-05 — #122: spinbike-ui fmt+clippy CI gate

- #122 spinbike-ui fmt+clippy CI gate — added `cargo fmt --manifest-path spinbike-ui/Cargo.toml` + `cargo clippy --manifest-path spinbike-ui/Cargo.toml --target wasm32-unknown-unknown -- -D warnings` to the `build-wasm` CI job (already had the wasm32 target + a spinbike-ui-scoped rust-cache); pre-fixed the one predicted clippy hit (`ActivityFeed` 8 props, `too_many_arguments`, scoped `#[allow]`). Enabling clippy for the first time on this workspace then surfaced 44 real pre-existing warnings across 19 files — fixed all of them mechanically in commit `f675f5d`, applying clippy's own suggested rewrites verbatim (zero behavior change): `view!{}.into_any()` → `().into_any()` (unit-arg), `X.clone()` → `X` for Copy `Callback<T>` (+ 2 now-redundant `let x = x;` self-rebinds removed), nested if/match collapsed via Rust-2024 let-chains, `*d = *d - X` → `*d -= X` compound-assign, unnecessary `as u32` cast removal (`get_full_year()` already returns `u32`), one dead `let kind = ...` removed, one `wasm_bindgen::prelude::*` import cfg-gated to its use site, one redundant closure → bare fn ref. Reviewed clean by 3 parallel targeted agents (Callback-Copy/closure-capture semantics, control-flow-collapsing correctness, CI-config+misc) plus a deep `requesting-code-review` pass — all green, CI green (lint/fmt/clippy/test/test-ui/build-wasm/e2e/mutation/deploy-dev/smoke-dev). PR [#136](https://github.com/zbynekdrlik/spinbike/pull/136), v0.15.0-dev.23 (merge commit SHA unknowable at commit time since this line ships inside the same PR it documents — see the PR page).

## 2026-07-05 — #126: dashboard errors rendered in the green success alert

- **Issue:** [#126](https://github.com/zbynekdrlik/spinbike/issues/126) — the dashboard's `set_msg` channel (green `.alert-success`) was overloaded for BOTH success and error text in `block_button.rs`/`edit_info_form.rs`/`transactions_list.rs`, so a failed block/save/invite/void could read as a confirmation. Validated STILL_VALID (grepped the current code, confirmed `err`/`set_err` existed but wasn't wired to these 3 components).
- **Version:** bump `eab6aeb` (0.15.0-dev.19 → dev.20).
- **Round 1 (the ask):** threaded `set_err` from `mod.rs` through `card_panel.rs` to `block_button.rs`, `edit_info_form.rs` (save + both invite-error branches, incl. the `mail_not_configured` 503), `transactions_list.rs` (which only ever used the channel for errors, so it now takes `set_err` directly). `action_form.rs`/`add_person_form.rs` untouched (own local error signal). RED `1e6cc33` → GREEN `99c2b6f`. New file `e2e/tests/dashboard-error-alert.spec.ts`.
- **Round 2 (fan-out code review found a stacking bug):** splitting one signal into two independent ones meant neither cleared the other — a stale red error could survive alongside/past a fresh green success, and closing the panel only cleared `msg`. Point-fixed: clear both at the start of every action (block/save/invite/void/note-save), plus `clear_selection`/`pick_card`/the search-debounce effect in `mod.rs` made symmetric. RED `a5906e2` → GREEN `5891312`.
- **Round 3 (deep `requesting-code-review` pass found the point-fixes were incomplete):** `DeleteUserSheet`'s `on_saved` closed the panel via a bare `set_selected.set(None)`, bypassing `clear_selection` entirely (Critical). Also flagged that `ActionForm`'s own successes (top-up/charge/visit-log, writing the SHARED `set_msg`) and a `TransactionsList` refetch could still leave a stale alert — whack-a-mole point-fixing would never fully close this. Fixed the DeleteUserSheet gap directly, and added a **structural mutual-exclusion `Effect`** in `mod.rs`: whichever of `msg`/`err` just changed to non-empty clears the other, for ANY writer (including `action_form.rs`, which stayed untouched). RED `23357b6` → GREEN `4c8f476`. A 3rd, final targeted-verification pass confirmed the effect converges (no infinite loop) and the RED/GREEN test diffs never weakened an assertion.
- **PR:** [#132](https://github.com/zbynekdrlik/spinbike/pull/132), merged `95e813f`. 4 Playwright tests total in the new file.
- **Deployed:** v0.15.0-dev.20, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` — DOM version == `/api/version` on both (had to clear the Playwright session's own stale service-worker cache first — see the ci-deploy skill's documented gotcha), 0 console errors/warnings on both. Functionally verified LIVE on dev with a synthetic staff JWT (own throwaway user row, cleaned up after, incl. the `login_tokens` row the invite attempt created): triggered the real `mail_not_configured` 503 via "Send invite" on a synthetic customer — rendered in `.alert-error` (red), not `.alert-success`; then clicked Save — the stale red alert was cleared and only the fresh green "Saved" showed (no stacking).
- **Filed:** [#133](https://github.com/zbynekdrlik/spinbike/issues/133) — pre-existing, explicitly out-of-scope observation: `action_form.rs`/the sheet components' own LOCAL error divs share the identical `.alert.alert-error` CSS class as the shared channel, so two error boxes (one local, one shared) could show at once in different DOM locations. Low priority, not a regression from this PR.

## 2026-07-05 — #117 + #120: kill preload integrity console warning + e2e @types/node

- **Issues:** [#117](https://github.com/zbynekdrlik/spinbike/issues/117) — Chromium logs "The `integrity` attribute is currently ignored for preload destinations..." (crbug.com/981419) on every page load, from Trunk's default `data-integrity=sha384` on the `rust` link; [#120](https://github.com/zbynekdrlik/spinbike/issues/120) — `e2e/` had no `@types/node`, so local `npx tsc --noEmit` errored on `process` (CI has no tsc step — local-dev-quality gap only). Both validated STILL_VALID + bundle-safe (disjoint files, tiny).
- **Version:** bump `34dbcd1` (0.15.0-dev.13 → dev.14).
- **#117:** `spinbike-ui/index.html:19` — added `data-integrity="none"` to the `rust` link, stopping Trunk from stamping `integrity=` on the JS modulepreload + WASM preload hints (CSS link's own `integrity` untouched — SRI still honored there). Removed the 3 `e2e/tests/helpers.ts` console-filter lines that were whitelisting this exact warning, so the ~50 existing `assertCleanConsole` specs become the permanent regression guard.
- **#120:** `e2e/package.json` + `package-lock.json` — added `@types/node@^20` (matches CI's `actions/setup-node node-version: 20`); `e2e/tsconfig.json` — added `"types": ["node"]`. `npx tsc --noEmit` now exits 0.
- **Commits:** `34dbcd1` (version) → `edacf27` (#117) → `259470d` (#120).
- **PR:** [#128](https://github.com/zbynekdrlik/spinbike/pull/128), merged `0c310ec`.
- **Verification:** CI E2E job green WITH the filter removed = proof the warning is gone at the source. Live post-deploy: fresh `browser_navigate` (default scope, no `all: true`) to both `spinbike-dev.newlevel.media` and `spinbike.newlevel.media` shows 0 console errors and the integrity warning gone (dev retains only the pre-existing, already-filtered wasm-bindgen deprecation warning — unrelated, tracked separately). DOM version `v0.15.0-dev.14` matches `/api/version` on both.
- **Playbook gotcha found:** `browser_console_messages({ all: true })` returns the WHOLE persistent MCP session's history, not just the current page — cross-checking with `all: true` initially looked like the warning was still present because it surfaced 14 old messages from unrelated past-ticket navigations. Documented in the ci-deploy skill's live-verification section.

## 2026-07-05 — #111 + #112: staff invite button + remove public registration

- **Issues:** [#111](https://github.com/zbynekdrlik/spinbike/issues/111) — "Poslat pozvanku" button in the staff edit-info form; [#112](https://github.com/zbynekdrlik/spinbike/issues/112) — remove the public `/register` page + all links (invite-only accounts). Bundled: disjoint files except `i18n.rs` (#111 adds keys, #112 removes different keys).
- **Blocker before starting:** dev→main already had an ORPHANED, fully-green, unmerged PR #124 (install-prompt iPad fix, itself a fast-follow on #110/#123's own worker-death) — a prior worker died mid-CI-monitor again. Finished monitoring its CI and merged it first (unblocks the one-PR-per-head/base slot), then re-bumped the version (0.15.0-dev.12 → dev.13) before starting #111/#112. See the new ci-deploy skill section.
- **Version:** bump `70443e0` (0.15.0-dev.12 → 0.15.0-dev.13).
- **#111:** `spinbike-ui/src/pages/dashboard/edit_info_form.rs` — new `saved_email` signal (init from `card.email`, updated on save-success AND by the refresh-on-reopen Effect), a "Poslat pozvanku" button gated on it, `POST /api/users/{id}/invite` (existing, #108), 503 `mail_not_configured` → Slovak message.
- **#112:** deleted `RegisterPage`/`RegisterReq` (`login.rs`), the `/register` `<Route>` (`router.rs`), the navbar register link (`nav.rs`), and 5 now-dead i18n keys.
- **Review (2 rounds, both before merge):** an 8-angle parallel finder pass caught 3 real bugs — `saved_email` never re-synced by the refresh Effect (Cancel-then-reopen went stale), the invite button wasn't gated on the save form's own `loading`, and the sheet stayed open after an invite so the confirmation was hidden behind the sheet's own full-viewport backdrop blur (verified against `style.css`: `.sheet-backdrop` is `z-index: 200`, the message div has none). Fixed in `464af3d`. A deep second pass (`superpowers:requesting-code-review`) then caught that the fix only closed the race in ONE direction — Save/Cancel/backdrop-close weren't gated on `invite_loading` either, risking the exact disposed-reactive-scope bug the Sheet component's own doc comment already references (#89). Fixed in `e18f2ff`, plus the 2 regression tests the review flagged as missing.
- **Commits:** `70443e0` (version) → `164636f` (#111 feature) → `aeaafb6` (#112 removal) → `464af3d` (review-round-1 fixes) → `e18f2ff` (review-round-2 fixes + regression tests).
- **PR:** [#125](https://github.com/zbynekdrlik/spinbike/pull/125), merged `c76aaf9`.
- **Follow-up filed:** [#126](https://github.com/zbynekdrlik/spinbike/issues/126) — pre-existing, cross-cutting: the dashboard's `set_msg` has no error-styling variant (errors render in the green `.alert-success` box) across 5 files, found by the first review round but out of scope for this PR. [#127](https://github.com/zbynekdrlik/spinbike/issues/127) — the invite endpoint's `503 mail_not_configured` logs a browser console error (intrinsic browser behavior for any 5xx fetch response) on a real deployment with mail Disabled; CI can never catch this since the shared E2E server always forces `SMTP_TEST_MODE=capture`.
- **Live verification gotcha:** a stale service-worker cache in the verification browser session initially showed the OLD version/register-link even though the deploy had succeeded (`curl /api/version` already showed the new version) — unregistering the SW + clearing caches revealed the true, correct state. No CI-seed admin account exists on the real dev/prod DBs, so verification used a synthetic staff row + a self-signed JWT (same technique as #106) to drive the authenticated flow, then cleaned it up. Both gotchas documented in the ci-deploy skill.
- **Deployed:** v0.15.0-dev.13, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` — DOM version matches `/api/version` on both, `/register` renders "Stranka nenajdena" (router fallback, not the old form) on both, the invite button is live and its 503-mapped Slovak message displays correctly (verified live on dev, where mail is genuinely Disabled), 0 console errors on both (1 pre-existing filtered SRI-preload warning).

## 2026-07-05 — #110: install-to-home-screen prompt + manifest PNG icons

- **Issue:** [#110](https://github.com/zbynekdrlik/spinbike/issues/110) — `components::InstallPrompt` (Chromium/Android `beforeinstallprompt` capture-and-replay via `js_sys::Reflect`, no typed web-sys binding existed for it; iOS Safari static 2-step Share guide), manifest PNG icons rasterized from `favicon.svg`, mounted on `/welcome` Success block + `/my/balance`.
- **Version:** bump `2239840` (0.15.0-dev.10 → 0.15.0-dev.11).
- **Commits (PR #123):** `2239840` (version) → `0ad1cea` (feature: component + icons + i18n + E2E) → `050c769` (fix: `test.use({...devices['iPhone 13']})` inside a `describe` forces a new worker via `defaultBrowserType: 'webkit'` — scope to context-option fields only).
- **Coordination gap (read before trusting "supervisor completed"):** the worker's own CI-wait + independent-review-agent wait ran long; the supervisor concluded the worker had died (`Monitor-death`) and completed the merge itself on `050c769` — **before** the worker's independent review agent returned. The review agent (dispatched before the premature merge) then found a real bug in already-merged/deployed code. Lesson: a worker doing a genuine multi-stage wait (CI + independent review) can look dead to the supervisor; if you're re-dispatched onto a ticket that's already closed, check `dev` for unmerged commits ahead of `main` before assuming there's nothing left to do.
- **Post-merge review finding, shipped as a fast-follow (PR #124, no separate issue — the fix was already fully implemented+tested, not deferred):** `is_ios_ua()` only substring-matched `"iPhone"`/`"iPad"` in `navigator.userAgent`. Since iPadOS 13, Safari defaults to "Request Desktop Website" — a real iPad reports as a plain Mac (`Macintosh; Intel Mac OS X...`) with **no** `"iPad"` substring, so the install guide never rendered on a stock-configured iPad. Fix: standard disambiguator `navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1` (a genuine Mac has no touch points). New E2E coverage: iPadOS-spoofed-UA renders the guide; a real-Mac UA (`maxTouchPoints: 0`) renders neither surface (no false positive).
- **Version:** bump `fa6a093` (0.15.0-dev.11 → 0.15.0-dev.12).
- **Review:** self + one independent `general-purpose` review agent covering JS-interop correctness, `detect_kind()` precedence, double-fire protection, `/welcome` mount-timing race, E2E test isolation, UA-emulation correctness, and CSS/theme consistency — only the iPadOS gap was real; everything else confirmed correct.
- **Mutation gate:** diff-scoped `cargo-mutants` — 0 survivors on both PR #123 and PR #124.
- **PRs:** [#123](https://github.com/zbynekdrlik/spinbike/pull/123) merged `674f0c13` (closed #110); [#124](https://github.com/zbynekdrlik/spinbike/pull/124) merged `586531b` (follow-up fix, no issue reference — already-done work, not deferred).
- **Deployed:** v0.15.0-dev.12, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` DOM version labels + `/api/version`; manifest.json + all 4 PNG icons (`icon-192.png`, `icon-512.png`, `icon-192-maskable.png`, `icon-512-maskable.png`) live 200 with `image/png` content-type on both; live dev `/login` console log confirmed the real browser fires `beforeinstallprompt` and our script's `preventDefault()` correctly suppresses the native banner.

## 2026-07-05 — #109: /welcome magic-link page + customer login-link form

- **Issue:** [#109](https://github.com/zbynekdrlik/spinbike/issues/109) — client-facing UI for the onboarding flow (#107 mail + #108 tokens/auth, both merged/live). Validated still valid before work (mail + token endpoints confirmed live via `crates/spinbike-server/src/routes/auth.rs`; no `/welcome` route or customer login-link section existed yet).
- **Version:** bump `82bd4bd` (0.15.0-dev.9 → 0.15.0-dev.10).
- **New page** `spinbike-ui/src/pages/welcome.rs`: reads `?t=` via `use_query_map().get_untracked()` (untracked deliberately — a tracked read risked re-redeeming an already-used token on any query-map re-notify), redeems via `POST /api/auth/token-login`, stores session, shows welcome + role-aware CTA (`staff`/`admin` → `/staff`, else → `/my/balance` — the server places no role restriction on invite/login tokens, so this had to be handled even though no admin-invite UI exists yet). Invalid/expired/reused/missing token → friendly unaccented-SK message + the shared `LoginLinkForm`.
- **Login page** (`pages/login.rs`): new customer section below the existing password form — email + "Poslat prihlasovaci link" → `POST /api/auth/request-login-link` → confirmation state. Password form's `on_submit` also switched `api::post` → `api::post_public`.
- **New `api::post_public`** (`api.rs`) — like `post` but skips `add_auth` and the "401 while a token exists ⇒ session expired, clear + redirect to /login" handling. Real bug this fixed: redeeming an already-used magic link legitimately 401s from token-login; with plain `post` that 401 was wiping a DIFFERENT, still-valid session the browser happened to hold (caught by `welcome.spec.ts`'s own "reuse the link" assertion in CI, RED on the first push). Same class of bug existed in the pre-existing password-login call — fixed there too.
- **CI:** E2E server launch was missing `SMTP_TEST_MODE=capture` (mail stayed Disabled → invite 503, no `test_link`) — added it.
- **Review-driven refactor** (two independent finder passes + a deep second pass, all before merge): extracted the request-login-link form (duplicated ~120 lines between login.rs and welcome.rs) into `spinbike-ui/src/components/login_link_form.rs`; welcome.rs now deserializes the token-login response straight into `auth::AuthData` instead of duplicating `AuthResp`/`UserInfoResp`; fixed 5 E2E call sites that assumed a single `type="email"`/`button[type=submit]` on `/login` (now two — added `passwordLoginForm(page)` in `helpers.ts`, scoped by "the form containing a password input" rather than DOM order); corrected a CI comment that overclaimed `request-login-link` also echoes `test_link` (only `invite` does, by design — no-enumeration). Deep pass caught a `cargo fmt` violation in `i18n.rs` invisible to the project's own pre-push check, because `spinbike-ui` is a separate cargo workspace excluded from the root `Cargo.toml` — fixed inline, filed [#122](https://github.com/zbynekdrlik/spinbike/issues/122) for the CI gap itself (not bundled — clippy has apparently never run against spinbike-ui, unknown blast radius).
- **Tests:** `e2e/tests/welcome.spec.ts` (invite → token-login → `/my/balance` with door area visible; reused link → invalid + email form; missing token → invalid state) + `e2e/tests/login-link.spec.ts` (confirmation state, incl. no-enumeration for an unknown email). Clean console asserted throughout.
- **Commits:** `82bd4bd` (version) → `978e3cc` (feature) → `996fde7` (fix: selector disambiguation + `post_public` for the reuse bug — CI RED→GREEN) → `8eadc90` (review refactor: shared component, `AuthData` reuse, role-aware CTA, `get_untracked`, `passwordLoginForm`) → `433f044` (fmt fix + #122).
- **PR:** [#121](https://github.com/zbynekdrlik/spinbike/pull/121), merged `74895ec`.
- **Follow-up filed:** [#120](https://github.com/zbynekdrlik/spinbike/issues/120) — missing `@types/node` (pre-existing, unrelated `tsc --noEmit` noise); [#122](https://github.com/zbynekdrlik/spinbike/issues/122) — root CI lint never covers the separate `spinbike-ui` workspace.
- **Deployed:** v0.15.0-dev.10, confirmed on `https://spinbike.newlevel.media` DOM version label == `/api/version`; `/welcome` (no token) and `/login`'s customer section both verified live — rendered correctly, login-link form submit round-tripped to a real 200 + confirmation state, 0 console errors (1 pre-existing filtered SRI-preload warning, unrelated).

## 2026-07-04 — #108: magic-link auth + permanent customer sessions + remove register API

- **Issue:** [#108](https://github.com/zbynekdrlik/spinbike/issues/108) — passwordless invite-only client onboarding + recovery, permanent customer sessions, and removal of the public register API (server side; UI is #109/#111/#112). Validated live before work: no `login_tokens` table existed and `POST /api/auth/register` still worked.
- **Version:** bump `55d21ea` (0.15.0-dev.8 → 0.15.0-dev.9).
- **Migration V17** (`db/migrations.rs`) — `login_tokens` (SHA-256 `token_hash` UNIQUE, purpose CHECK('invite','login'), single-use `used_at`) + index. New `db/login_tokens.rs`: 32-byte base64url raw token (link-only), atomic `UPDATE ... RETURNING user_id` redeem. Tests: `v17_creates_login_tokens_table_with_expected_columns`, `v17_purpose_check_rejects_unknown_value`, `v17_token_hash_is_unique`, `v17_is_idempotent`; module `create_then_redeem_returns_user_id_once`, `reused_token_is_rejected`, `expired_token_is_rejected`, `wrong_purpose_is_rejected_by_scoping`, `ttl_constants_are_exactly_14_days_and_24_hours`.
- **Role-based JWT expiry** (`auth/mod.rs::create_token`) — customer → ~100y, admin/staff → 90d; split test into `jwt_expiry_customer_is_100_years` + `jwt_expiry_admin_and_staff_are_90_days`.
- **Endpoints:** `POST /api/users/{id}/invite` (admin/staff, 503 when mail Disabled, `test_link` in capture mode), `POST /api/auth/request-login-link` (public, uniform-200 no-enumeration, email-keyed rate limiter, **SMTP send `tokio::spawn`'d off the response path** to close a timing side-channel), `POST /api/auth/token-login` (redeem invite/login → JWT, rejects blocked/deleted). Locking tests in `tests/auth_routes.rs`.
- **Register removed** — route+handler+`RegisterRequest` deleted. **Gotcha:** unmatched `/api/*` falls through to the SPA static fallback → 200 index.html, NOT a router 404 — asserted the removed *capability* (no 201, no JWT, no account). Register was also the E2E seed mechanism (`global-setup.ts` + `door-open.spec`) → replaced with a `SPINBIKE_TEST_MODE`-gated `POST /api/test/seed-account` fixture; `auth.spec.ts` register-flow tests reworked (logout now bootstraps via login).
- **Commits:** `55d21ea` (version) → `b8d17c9` (feature) → `0ca6b35` (V17 table-list test) → `3ed4591`/`58de271` (register-removal behavioral assert + fmt) → `d2d7950` (review fixes: per_email prune, spawn-send, doc, coverage) → `643f529` (kill 13 mutation survivors: TTL literals, retain window > decision window, seed-account tests) → `6c8d566` (playbook).
- **Review:** two independent Opus passes (general-purpose deep + `/review` 5-dimension). Deep pass raised a 🔴 for the still-live frontend register CTA — **out of this diff, the map-mandated #112 (verified it covers UI register removal)**; 🟡 unbounded `per_email` map + 🔵 timing side-channel + 🔵 misleading comment all fixed in `d2d7950`.
- **Mutation gate:** diff-scoped `cargo-mutants` found 13 survivors on the first pass (105 mutants, ~70 min); all killed in `643f529` — key lesson: a memory-prune window must be WIDER than the decision window or the boundary mutant is masked (widened `LoginLinkRateLimiter` retain to 120s vs the 60s decision).
- **PR:** [#118](https://github.com/zbynekdrlik/spinbike/pull/118), merged `627c115`.
- **Follow-up filed:** [#119](https://github.com/zbynekdrlik/spinbike/issues/119) — periodic purge of used/expired `login_tokens` rows.
- **Playbook:** new `.claude/skills/auth-onboarding/SKILL.md` + router line; `ci-deploy` skill gained the SPA-fallback-200, seed-account, and mutation-gate learnings.
- **Deployed:** v0.15.0-dev.9, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` DOM version labels; `login_tokens` table + schema_version 17 present on both DBs; register creates no account on live prod.

## 2026-07-04 — #106: door blocked-users gate

- **Issue:** [#106](https://github.com/zbynekdrlik/spinbike/issues/106) — `POST /api/door/open` never checked `users.blocked`; a blocked customer with `allow_self_entry=1` (or a blocked admin/staff, who bypass the `allow_self_entry` gate entirely) could still actuate the relay and get billed.
- **Fix:** added `blocked` to the door precondition SELECT, rejected with `403 {"status":"rejected","reason":"blocked"}` before the allow_self_entry role bypass, the rate limiter, the tx insert, and the relay press — for every role.
- **Commits:** version bump `791bbf0` (0.15.0-dev.6 → 0.15.0-dev.7) → RED `683f540` (`crates/spinbike-server/tests/door_route.rs::blocked_customer_with_allow_self_entry_is_rejected` + `blocked_admin_is_rejected_despite_role_bypass` + `blocked_staff_is_rejected_despite_role_bypass`) → GREEN `4046370` (`crates/spinbike-server/src/routes/door.rs`) → polish `a9abf23` (review-feedback: health-endpoint assertion symmetry + reason-tag comment).
- **Decision (yours to make, per issue text, already settled — not re-asked):** blocked-means-blocked for ALL roles including admin/staff, even though admin/staff still bypass `allow_self_entry`. Reason tag `"blocked"` reuses door.rs's own `{"status":"rejected","reason":"<tag>"}` envelope (matches its existing `not_allowed`/`rate_limited` shape and users.rs's 403 precedent) rather than payments.rs's `{"error": "User is blocked"}` + 409 shape — documented inline in door.rs so a future reader doesn't "fix" the intentional inconsistency.
- **Review:** two independent passes (general-purpose + `superpowers:requesting-code-review`), both 0 Critical / 0 Important; 2 Minor items addressed in the polish commit before merge.
- **Live verification:** synthetic test users (created + JWT-signed + cleaned up, zero real customer data touched) on BOTH dev and prod confirmed blocked-customer, blocked-admin → 403 `blocked`, zero tx rows, `last_ack_ms_ago` stayed null (relay never pressed); unblocked control customer on dev correctly passed the gate and reached the relay call (503 `hardware_unavailable` — expected, dev has no eWeLink creds by design).
- **PR:** [#114](https://github.com/zbynekdrlik/spinbike/pull/114), merged `68d37b3`.
- **Follow-up filed:** [#113](https://github.com/zbynekdrlik/spinbike/issues/113) — pre-existing Trunk-generated console warning (preload `integrity` attribute), unrelated to this fix, found during post-deploy verification.
- **Deployed:** v0.15.0-dev.7, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` DOM version labels.
- #110 install-to-home-screen prompt + manifest PNG icons — PR #123 merged 674f0c13, main CI 28724973486 green, prod+dev v0.15.0-dev.11, 4 PNG icons live 200 (supervisor completed merge after worker Monitor-death)

## 2026-07-05 — #102 + #103: bound mutation gate + eliminate push/PR double-fire (bundled)

- **Issues:** [#102](https://github.com/zbynekdrlik/spinbike/issues/102) — `mutation-test` job mutated the whole tree on every PR with a 240-min cap (diff-scoping itself had already shipped in a prior PR, `f232216`, Apr 2026 — this ticket was rescoped to the remaining timeout/flags/full-sweep work). [#103](https://github.com/zbynekdrlik/spinbike/issues/103) — every `dev` commit fired TWO CI runs (push + pull_request, confirmed ~7 same-headSha pairs in the last 16 runs). Bundled into one PR because both edit the same `ci.yml` job block and two sequential PRs would collide.
- **Version:** bump `65e2105` (0.15.0-dev.15 → 0.15.0-dev.16).
- **#103 fix** (`0c350c2`) — dropped the `pull_request:` trigger entirely (kept `push: [main, dev]`); re-scoped concurrency group from event-scoped to `${{ github.workflow }}-${{ github.ref }}` with plain `cancel-in-progress: true` (identical semantics to the old `${{ github.event_name == 'push' }}` now that only push events remain — verified by an independent review pass, no new risk; deploy-dev/deploy-prod keep their own `cancel-in-progress:false`). Re-gated `mutation-test` and `check-version-bump` (both were `if: pull_request`) to `if: push && ref==dev`; pinned the mutation diff base to `origin/main` (`github.base_ref` is empty on a push event).
- **#102 fix** (`e64e66c`) — `mutation-test` timeout 240→20 (hard cap), added `--baseline=skip` (the `test` job already proves the baseline green in the same run) + `--test-tool=nextest` (+ installed `cargo-nextest`). New `.github/workflows/mutation-full.yml`: `workflow_dispatch`-only, `runs-on: ubuntu-latest`, 8-way sharded (0-indexed `--shard k/8`) full-tree sweep; survivors/timeouts → `gh issue create --label test-quality`, job fails only on a genuine cargo-mutants tooling failure (exit 1/4), never on findings (exit 2/3).
- **Review-fix** (`38a798f`, gated by push discipline — required a fresh `[no-test]` marker commit `2a2ef20` since the bypass only shields the LATEST commit) — two independent Agent code-review passes on the diff found: `mutation-full.yml`'s `--baseline=skip` meant a broken/non-building tree at dispatch time returned exit 0 ("0 viable mutants tested" reads as success) instead of exit 4, silently filing nothing — unlike the PR gate (which has `needs: test` proving the baseline green in the SAME run), the standalone sweep has no such guarantee. Dropped `--baseline=skip` from the full sweep only (cost: one redundant baseline run per shard, acceptable for the unbounded on-demand job). Also fixed a cosmetic header-jamming bug in the survivors-issue body (command substitution ate the trailing newline).
- **Gate note:** `[no-test: ...]` bypass used twice (commits `056718f`, `2a2ef20`) — CI-workflow-YAML-only change, no product logic, Gate 2's bug-fix heuristic fires on the `Closes #N` commit-body text alone.
- **PR:** [#130](https://github.com/zbynekdrlik/spinbike/pull/130), merged `9a8b339`.
- **Post-merge single-fire proof:** the PR's own dev-push run (`28729893592`) already ran `check-version-bump`/`mutation-test`/`deploy-dev` on push-to-dev with no companion pull_request run (`count=1` for that SHA before AND after opening the PR). The main-push merge run (`28730057115`) then correctly SKIPPED `check-version-bump`/`mutation-test`/`deploy-dev`/`smoke-dev` and ran `deploy-prod`/`smoke-prod` — exactly the designed split.
- **Playbook:** none needed — the CI-config gotchas (orphan-PR check, fmt-twice, mutation diff-scoping context) were already documented in `.claude/skills/ci-deploy/SKILL.md` before this ticket; no new reusable procedure emerged beyond what's captured here.
- **Deployed:** v0.15.0-dev.16, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` DOM version labels == `/api/version`, 0 console errors/warnings on both.

## 2026-07-05 — #119: daily purge of used/expired login_tokens

- **Issue:** [#119](https://github.com/zbynekdrlik/spinbike/issues/119) — `login_tokens` (magic-link tokens, #108/V17) only ever grew; nothing deleted used/expired rows. Validated STILL_VALID (clean mechanical slot-in, no schema change, DML only).
- **Version:** bump `78a8bec` (0.15.0-dev.17 → 0.15.0-dev.18).
- **Feature** (`b9d362f`) — `db::login_tokens::purge_expired_and_used()` (`DELETE ... WHERE used_at IS NOT NULL OR expires_at <= datetime('now')`, returns rows removed), `jobs::token_purge::tick()` wrapper mirroring `jobs::charger`/`jobs::materialiser`, registered in `jobs/mod.rs`, wired into `bin/server.rs` as a startup-once run + daily (86400s) scheduled tick alongside the existing charger (60s)/materialiser (3600s) spawns. Tests: seed used+expired+live rows, assert purge removes exactly the dead ones and the live token still redeems afterward; plus a no-op case.
- **CI fix** (`9784768`) — first push failed `Lint`/clippy: `needless_question_mark` on `Ok(inner(pool).await?)` in a same-Result-type wrapper (clippy is CI-only here, never caught locally — see playbook).
- **Deep-review fixes** (`500a67b`) — an independent Agent code-review pass (base `9a8b339`, head `9784768`) found 3 Minor findings, no Critical/Important, all fixed: (1) the purge predicate's `expires_at < now` was one instant off from being the exact negation of `redeem()`'s `expires_at > now` validity check — switched to `<=` so the two are mutually exclusive by construction (never a security/data-loss issue either way, just an exactness gap); (2) `token_purge::tick` now returns `usize` (cast from the DB layer's native `u64`) to match `charger::tick`/`materialiser::sweep`'s signatures.
- **PR:** [#131](https://github.com/zbynekdrlik/spinbike/pull/131), merged `6e69d50`. CI green: Lint, Test, Test (UI), E2E, Mutation Testing, Deploy (dev), Smoke (dev) all passed both before and after the review-fix push.
- **Playbook:** updated `.claude/skills/ci-deploy/SKILL.md` — added the `needless_question_mark`-on-a-thin-wrapper gotcha (CI-only visible since local clippy is banned) and the "negate a predicate literally, strict-vs-non-strict boundary matters" gotcha from the review-fix.
- **Deployed:** v0.15.0-dev.18, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` DOM version labels == `/api/version`, 0 console errors/warnings on both. Both services restarted cleanly at deploy (no panics/errors in journal); `login_tokens` table is empty on both dev and prod (0 rows), so the startup purge correctly logged nothing (`Ok(_) => {}` silent path, n=0) — functional correctness is covered by the unit tests.

## 2026-07-10 — #158: typed HTTP API error layer with machine-readable codes

- **Issue:** [#158](https://github.com/zbynekdrlik/spinbike/issues/158) — 86 ad-hoc `json!({"error":"..."})` bodies / 57 `Result<Json<T>, (StatusCode, Json<Value>)>` handler signatures across routes/, no machine-readable `error_code` anywhere in the HTTP surface. Backend root of #145 (customer error banners are raw English). Ticket-validated STILL_VALID (re-grepped: 86 json! sites, 38 tuple-return signatures, 0 `error_code` hits in routes/).
- **Version:** bump `b7c39dd` (0.15.0-dev.35 → 0.15.0-dev.36).
- **Feature** (`f28f8f7`) — `spinbike_core::errors::ErrorCode` (28 snake_case variants, serde `rename_all`, `message()` table) in core so the Leptos UI can later match on it (#145 stays the UI-mapping ticket). `spinbike_server::error::ApiError` (`Unauthorized`/`Forbidden`/`NotFound`/`Conflict{code,message,extra}`/`BadRequest`/`ServiceUnavailable`/`Internal`) implementing `IntoResponse`, body `{"error_code":..., "error":...}` — additive, `"error"` string kept for the 13+ tests/UI that read it. Migrated all 57 handler signatures + 86 error sites via a scripted regex transform (idempotent, aborted on any unmapped tuple — caught one extra site, `users.rs` "User already deleted", added as `UserAlreadyDeleted`). Unified the three-way "Staff access required"/"Staff only"/"Only staff can book for other users" onto one `staff_required` code. Preserved the two `conflict_name`/`conflict_card` extra-field sites (create_user always; update_user only for staff/admin — anti-enumeration guard intact). `internal_error()`/`bad_request()` now return `ApiError` so the ~110 `.map_err(internal_error)?` sites were untouched. `door::open()` keeps its distinct `{status,reason}` terminal-client contract (Ok arm, unaffected); `test_fixtures.rs` keeps its own `(StatusCode, String)` test-only shape.
- **New tests:** exhaustive `ErrorCode` code/message table (`spinbike-core`), `ApiError` status/body mapping incl. conflict-extra flattening (`spinbike-server/src/error.rs`), end-to-end `tests/api_error_codes.rs` contract test (forbidden/unauthorized/bad-request `error_code` assertions).
- **CI gotcha — mutation gate budget overrun on a wide mechanical refactor.** 57 signature changes + 86 error sites pushed `cargo mutants --in-diff` to 236 mutants (≈88% of the whole tree, per a local `--list` check) — the diff-scoped 20-min PR gate isn't sized for a refactor this wide and hit its hard cap (cancelled). Per `mutation-testing.md` (fix the setup, never bump the timeout): sharded the PR gate 8 ways (`--shard k/8`, `fail-fast: false`, mirroring `mutation-full.yml`'s proven split) + added `[profile.mutants]` (inherits test, debug off) via `.cargo/mutants.toml` for faster per-mutant builds (`6e72ecc`). ~30 mutants/shard, ~9 min worst-case shard — comfortably inside the cap. Surfaced ONE genuine survivor: `users.rs:353` `chain.contains("UNIQUE") || chain.contains("unique")` in the `create_user` DB-unique-violation fallback had no test reaching it (the pre-check filters `deleted_at IS NULL` but the `email UNIQUE` index covers soft-deleted rows too — `delete_user` only sets `deleted_at`, never nulls the email). Added a test that creates a user, soft-deletes it, re-creates with the same email — reaches the DB-fallback arm, kills the `||`→`&&` mutant (`fa93ae8`).
- **Review:** two independent Opus adversarial passes (correctness/semantic-equivalence + security/completeness) on the full diff — 0 🔴 0 🟡 both. Three intentional documented message unifications (staff_required, service_not_found capitalization) confirmed test/UI-safe by grep. Anti-enumeration guard on update_user's email-conflict verified intact. One pre-existing observation (get_settings has no role gate) — already tracked as #175, not a new finding.
- **PR:** [#181](https://github.com/zbynekdrlik/spinbike/pull/181), merged `c469b92`. CI green: Lint, Test, Test (UI), Build WASM, E2E, all 8 Mutation Testing shards, Deploy (dev), Smoke (dev) on dev; Deploy (prod), Smoke (prod) on main (Version Bump Check + Mutation Testing correctly skipped on the main push).
- **Playbook:** see `.claude/skills/ci-deploy/SKILL.md` update — the mutation-gate-budget-overrun-on-wide-refactor gotcha and the sharding fix.
- **Deployed:** v0.15.0-dev.36, confirmed on `https://spinbike.sk` — DOM version label matches, 0 console errors/warnings on initial load. Functional verification: `POST /api/auth/login` with wrong credentials on the LIVE prod site returned `{"error":"Invalid email or password","error_code":"invalid_credentials"}` — the new contract confirmed working end-to-end on production.

## 2026-07-11 — #166 extract one shared SlidingWindowLimiter (door + login-link)

- **Issue:** #166 (arch-review 🔵) — door::RateLimiter (i64) and auth::LoginLinkRateLimiter (String) hand-rolled the same sliding-window pattern into two AppState Mutex maps; door's per_user map also never evicted emptied entries (latent leak).
- **Validated STILL_VALID** against current dev: both structs + the two Arc<Mutex> AppState fields present as described; door `entry().or_default()` never removes emptied keys, login has the `retain(<120s)` guard — the drift is real.
- **Implementation:** new `src/rate_limit.rs::SlidingWindowLimiter<K>` — per-key `HashMap<K,{hits:VecDeque<Instant>, last:Instant}>` + global `VecDeque<Instant>`, driven by a flat `RateLimitConfig`. door::RateLimiter / auth::LoginLinkRateLimiter become ~8-line typed wrappers delegating to it (i64 by value / &str by ref) → every route call site, AppState field and existing test unchanged (only 3× `rl.per_email.len()` → `rl.tracked_keys()`). Commits `0472fdc` (version), `a377f8a` (refactor).
- **Byte-identical** on every tested decision (all 13 door + 12 login rate-limit tests green, unmodified): same 10s/5-per-60s/30-per-60s door caps, 60s login interval, 10-per-60s global cap, `too_fast`/`per_user_cap`/`global_cap` tags, strict-`>` boundaries. Check order preserved (prune global → evict quiet keys → global cap → per-key min-gap → per-key cap → record); a rejected hit never creates a key.
- **Design calls** (byte-identical over the issue's literal shorthand, noted on the issue): (1) login `per_key_max = None`, NOT a literal cap of 1 — a real `len>=1` cap would fire at the exact 60s boundary and wrongly reject `login_link_allowed_at_exactly_60s_boundary`; login has no per-key cap in the original. (2) Separate `key_memory` horizon + `last` field: door memory==window (60s, key drops when hits expire → closes the leak); login memory 120s > 60s decision window (key observable past decision, matching the old `retain(<120s)` — the wider-window-for-observable-boundary rule already in ci-deploy skill).
- **Mutation:** all 8 shards green. The relocated logic is fully new-diff → mutated fresh; the door/login tests kill it through the wrappers, but the eviction `!hits.is_empty() || last<memory` `||→&&` mutant needed a NEW direct test (`key_retained_between_decision_and_memory_window`). Added 5 shared-limiter tests total.
- **PR:** [#193](https://github.com/zbynekdrlik/spinbike/pull/193), merged `83a9d85`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Deployed:** v0.15.0-dev.51, confirmed on `https://spinbike.sk` — DOM `v0.15.0-dev.51` == backend `/api/version` == deployed; 0 console errors/warnings. Internal refactor (no user-visible surface); rate-limit behaviour covered by the green unit + E2E suite, prod login endpoint not spammed.
- **Playbook:** `.claude/skills/ci-deploy/SKILL.md` — added the "moving already-tested logic into a NEW module re-exposes ALL of it to `--in-diff`" gotcha (wrapper delegation keeps behaviour tests reaching it; add direct tests for keep/drop or `&&`/`||` branches the moved tests miss).

## 2026-07-11 — #175 gate GET /api/admin/settings behind StaffUser

- **Issue:** [#175](https://github.com/zbynekdrlik/spinbike/issues/175) (arch-review 🔵, filed by `/architecture-check` on #158's diff) — `get_settings` bound `AuthUser(_claims): AuthUser` and discarded the claims, the only admin GET handler without a role gate (siblings `list_templates`/`list_instructors`/`list_services` all use `StaffUser`). Ticket-validated STILL_VALID by direct code read (admin.rs:509-524 matched the issue exactly).
- **Version:** bump `e5038d1` (0.15.0-dev.54 → 0.15.0-dev.55).
- **Test (RED) `d5484a9`:** `settings_get_forbidden_for_customer` — customer JWT on `GET /api/admin/settings` asserts 403 `error_code=staff_required`; deterministically RED on current dev by static proof (`AuthUser::from_request_parts` in `auth/mod.rs` does zero role check, so any authenticated caller got 200) rather than an actual failing CI run — local `cargo test` is banned in this repo (CI-only), and the trigger is 100% mechanical (bare extractor swap), so a real RED CI cycle would only re-confirm what the extractor code already proves. Added `settings_get_allowed_for_staff` alongside.
- **Fix (GREEN) `4051566`:** `get_settings` extractor `AuthUser(_claims): AuthUser` → `_: StaffUser`, matching every sibling admin GET exactly. Dropped now-unused `AuthUser` from the file's `use` statement (grep-confirmed zero remaining references). `update_setting` (write path) untouched — stays `AdminUser`, the read/write asymmetry is intentional and now documented in a doc-comment.
- **Review:** both `/code-review` (self, xhigh) and an independent `/requesting-code-review` subagent pass found 0 🔴 0 🟡 0 🔵. The subagent additionally grepped every remaining bare-`AuthUser` handler repo-wide (`auth::me`, class booking/cancel, `door::open`, user update/balance/transactions) — all need `claims.sub` for an ownership check a pure role extractor can't express, confirming #175 was the last stray sensitive-GET-on-bare-auth handler.
- **CI:** dev push green incl. all 8 mutation-testing shards, E2E, Deploy (dev) + Smoke (dev). PR [#195](https://github.com/zbynekdrlik/spinbike/pull/195), merged `f182d9e`. Main CI green (Deploy (prod) + Smoke (prod); Version Bump Check + Mutation Testing correctly skipped on main).
- **Deployed:** v0.15.0-dev.55, confirmed on `https://spinbike.sk` — DOM version matches `/api/version`, 0 console errors/warnings. Functional prod verification: minted a customer-role JWT directly (no DB row needed — `StaffUser`/`AdminUser` decide purely from the JWT's own `role` claim, no DB lookup) → `GET /api/admin/settings` returned 403 `staff_required`; a staff-role JWT returned 200 with the real settings rows. Zero DB writes, zero cleanup needed.
- **Playbook:** `.claude/skills/prod-verification/SKILL.md` — added a shortcut noting pure JWT-role-boundary checks need no synthetic DB row at all (skip straight to minting the JWT). `.claude/skills/ci-deploy/SKILL.md` router entry in `CLAUDE.md` broadened to also trigger on "monitoring a CI run" (the foreground-poll sandbox-block gotcha was already documented there but the router didn't point at it for that case).

## 2026-07-11 — #163 typed DbError at the DB query boundary

- **Issue:** [#163](https://github.com/zbynekdrlik/spinbike/issues/163) (arch-review 🔵) — the db query submodules returned `anyhow::Result`, erasing the error type so route handlers string-matched the chain (`format!("{e:#}").contains("UNIQUE")`) to spot a unique violation. Validated STILL_VALID: all 8 db files still imported anyhow; the two string-match sites (routes/users.rs:349, routes/test_fixtures.rs:181) present as described. (The issue's #143 rationale is false per its own adversarial note, but the anti-pattern is real and independent — dispatch scoped #143 OUT.)
- **Design:** new `db/error.rs::DbError` (thiserror) — `UniqueViolation` (classified from `sqlx::Error` in a MANUAL `From<sqlx::Error>` via `db_err.is_unique_violation()`, the same detector routes/admin.rs uses), `NotFound`, `ClassFull`, transparent `DateParse(#[from] chrono::ParseError)` / `IntParse(#[from] std::num::ParseIntError)` (the two non-sqlx `?` sites: classes list_upcoming date parse, settings get_bike_count), transparent `Sqlx(sqlx::Error)` catch-all. `pub type Result<T> = Result<T, DbError>` alias glob-imported per submodule.
- **Migrated:** ~57 query fns across classes/users/transactions/settings/login_tokens/persistent_bookings/reports → `Result<T, DbError>`. Dropped all **42** per-query `.context()` strings (variant + wrapped sqlx::Error + the `internal_error` route-boundary `tracing::error!` carry the signal). `create_user` was the ONE fn on `sqlx::Result` (not anyhow) — also switched to the alias so callers see DbError.
- **Kept on anyhow (deliberate):** `db/mod.rs` create_pool / create_memory_pool / run_migrations — the startup/app boundary (issue's "keep anyhow at bin/main"), where the 7 `.context("Migration v{n} failed")` messages are load-bearing and no caller matches their kind. No `From<DbError> for ApiError` added — `internal_error(impl Display)` accepts DbError for free and UniqueViolation maps to a DIFFERENT ErrorCode per site, so a blanket From wouldn't help.
- **Callers rewired (3):** routes/users.rs + routes/test_fixtures.rs `contains("UNIQUE")` → `matches!(e, crate::db::DbError::UniqueViolation)`; routes/classes.rs `contains("full")` → `matches!(e, crate::db::DbError::ClassFull)` (echoes `e.to_string()` = "Class is full", byte-identical body). NOTE: routes alias `db` to a *submodule* (`use crate::db::classes as db`), so the typed match must use the `crate::db::DbError` re-export, not `db::DbError` (E0603).
- **Tests:** not a bug fix (pure refactor) — all existing green (capacity_enforcement still asserts `.to_string().contains("full")` ✓). Added: unique-violation→UniqueViolation + non-unique→Sqlx (error.rs), duplicate-insert→UniqueViolation end-to-end + missing-row update→NotFound (users.rs), Display stability pins.
- **CI:** 3 dev round-trips (clippy `-D warnings`: (1) E0603 db::DbError private + chrono ParseError From; (2) create_user sqlx::Result E0308). Final dev push green incl. all 8 mutation shards + E2E + Deploy(dev) + Smoke(dev). PR [#198](https://github.com/zbynekdrlik/spinbike/pull/198), merged `4cfcc51`. Main CI green incl. Deploy(prod) + Smoke(prod).
- **Deployed:** v0.15.0-dev.59, confirmed on `https://spinbike.sk` — DOM `[data-testid=version]` = `v0.15.0-dev.59` == `/api/version` == deployed; 0 console errors/warnings; spinbike.service (prod) active. Internal refactor (no user-visible surface); the 409/500 error paths covered by the green unit + E2E + mutation suite, prod not polluted with junk data.

## 2026-07-11 — #196 tautological E2E assertion + #185 cargo-deny unsound scope (bundled)

- **Issues:** [#196](https://github.com/zbynekdrlik/spinbike/issues/196) (bug) — `expect(autoCount).toBeGreaterThanOrEqual(0)` in `spin-booking.spec.ts` can never fail. [#185](https://github.com/zbynekdrlik/spinbike/issues/185) (architecture-review) — `deny.toml`'s `[advisories]` never set `unsound`, defaulting to `Scope::Workspace` (direct deps only), silently excluding transitive-only vulnerable resolutions. Both validated STILL_VALID by direct code read (spin-booking.spec.ts:167 matched exactly; deny.toml had no `unsound` key). Bundle-safe: independent files, well under the LoC gate, no schema/API/security-boundary work.
- **#196 fix:** capture the `template_id` from the toggle just turned off (`persistent-toggle-{tid}` → strip prefix) and assert the auto-cancel row count scoped to `auto-cancel-{tid}-` is exactly `0` — the deterministic effect of server-side `end_persistent` (cancels every future/uncharged/persistent-source booking for that (user_id, template_id); the materialiser only re-creates rows for ACTIVE subscriptions). Verified against actual frontend source (`persistent_toggles.rs:91`, `upcoming_classes.rs:60`) that `tid` is a numeric ID, so the prefix-match (with trailing hyphen) can't collide across templates (e.g. tid `5` vs `50`). This genuinely strengthens coverage — the old assertion could have passed even if `end_persistent` silently did nothing.
- **#185 fix:** one line, `unsound = "all"` under `[advisories]` in `deny.toml`. Verified against current `Cargo.lock` that both `rand` resolutions (0.8.6, 0.9.3) are already patched, so the tightened scope stays green — proven empirically by the green Supply-Chain-Advisories job, not just reasoned.
- **Neither is a classic bug fix** (test-quality + CI-config hardening) — no RED→GREEN commit-order mandate applied. For #196, the old tautological assertion COULD have passed on broken behavior (that was the point of the bug report); the new assertion is a genuine regression guard going forward, confirmed executing successfully against real server behavior in CI (not merely typechecked).
- **Version:** bump `bee841b` (0.15.0-dev.60 → 0.15.0-dev.61).
- **Commits:** `0c4e5d6` (#196), `31cb3da` (#185), both on top of the version bump.
- **Review:** `/review` + a deep manual second pass (angles: correctness, removed-behavior, cross-file, testid-format cross-check against frontend source, test-ordering/isolation via playwright.config.ts) — 0 🔴 0 🟡 0 🔵. Diff small enough (2 files, ~40 LoC) that a full 10-agent fan-out was skipped in favor of a thorough single pass plus empirical CI confirmation, per the fan-out right-sizing rule.
- **CI:** dev push green incl. Test Integrity, Version Bump Check, Supply-Chain Advisories, Lint, Test, Build WASM (UI), Test (UI), **E2E Tests**, all 8 mutation-testing shards, Deploy (dev), Smoke (dev). PR [#199](https://github.com/zbynekdrlik/spinbike/pull/199), merged `4869a90`. Main CI green incl. Deploy (prod) + Smoke (prod) (Version Bump Check + Mutation Testing correctly skipped on main).
- **Deployed:** v0.15.0-dev.61, confirmed on `https://spinbike.sk` — DOM "Verzia aplikácie" = `v0.15.0-dev.61` == `/api/version` == deployed; 0 console errors/warnings; spinbike.service (prod) active. No user-visible change (test + CI-config only).

## 2026-07-11 — #170 checksum verification for the hand-rolled migration runner

- **Issue:** [#170](https://github.com/zbynekdrlik/spinbike/issues/170) (architecture-review) — `run_migrations` (db/mod.rs) tracked applied migrations by integer `version` only; editing an already-applied migration's SQL const after it shipped was silently skipped, never detected. Validated STILL_VALID by direct code read (mod.rs:100-101 `if version <= current_version { continue; }`, no checksum anywhere).
- **Fix:** V19 migration adds a nullable `schema_version.checksum` column; a post-apply-loop pass walks every `MIGRATIONS` entry, backfilling a NULL checksum from the current SQL const's SHA-256 (`db::sha256_hex`, extracted + shared with `login_tokens::hash_token`) or returning `Err` (server refuses to boot) on a mismatch.
- **Tests:** 7 new — hash-primitive correctness against an independently-computed SHA-256 (not self-referential), fresh-DB backfill, NULL-row backfill-on-rerun, tampered-checksum loud failure (core regression guard), and — added after an independent-reviewer subagent flagged the gap — a genuine v18→v19 upgrade-path test (`apply_sql_block` + manual INSERT, schema_version literally lacking the `checksum` column, not just NULL) distinct from the fresh-install-only coverage of the others.
- **Review:** self-review (10-angle reasoning) + one dispatched independent-reviewer subagent. Found + fixed: (1) `migration_checksum` byte-for-byte duplicated `login_tokens::hash_token` — extracted `db::sha256_hex`; (2) the backfill/verify pass had zero logging on any branch — added `info!`/`error!`/`debug!`; (3) missing genuine-upgrade-path test coverage (above); (4) 2 mutation-testing survivors (`migration_checksum -> String::new()` / `-> "xyzzy".into()`) because every self-written test computed its own "expected" via the same function — fixed with an independently-computed fixture.
- **Version:** bump `b5cb7da` (0.15.0-dev.66 → .67, ticket work), `4b98e56` (.67 → .68, post-merge playbook commit).
- **Commits:** `04e3cfe` (feat), `2e82e68` (kill mutation survivors), `42d9df5` (dedup + logging), `b539abe` (upgrade-path test), on `dev`; `2eafc96` (playbook) + `4b98e56` (version) rode along after merge.
- **CI:** every dev push green incl. all 8 mutation-testing shards, E2E, Deploy (dev), Smoke (dev). PR [#203](https://github.com/zbynekdrlik/spinbike/pull/203), merged `fd0bfd4`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Deployed:** v0.15.0-dev.67, confirmed on `https://spinbike.sk` — DOM version = `v0.15.0-dev.67`, 0 console errors, spinbike.service (prod) active. Direct read-only `sqlite3` query against `/opt/spinbike/prod/spinbike.db` confirmed `schema_version` has 19 rows, all with non-null 64-char SHA-256 checksums (backfill ran cleanly on the real prod data, not just in-memory test DBs).

## 2026-07-11 — #179 finish the active-pass unification: door.rs's 7th copy + inclusive expiry-day boundary (MONEY)

- **Issue:** [#179](https://github.com/zbynekdrlik/spinbike/issues/179) (bug) — `routes/door.rs` still hand-rolled the "active monthly pass" predicate (the un-migrated 7th site after #159 unified 6) AND used the wrong date boundary `valid_until > datetime('now')`. Validated STILL_VALID by direct code read (door.rs:221-229, users.rs:1009-1011 both exactly as filed; charger.rs:64-79 the canonical inclusive form).
- **Root cause (real overcharge):** `sell_pass` writes `valid_until` as a bare `YYYY-MM-DD`; SQLite's byte-wise TEXT compare makes the 10-char bare date a PREFIX of the 19-char `datetime('now')`, so it sorts LESS → `>` is FALSE on the pass's exact expiry day → the door charged a single entry on a day the pass still covered.
- **Fix:** door.rs pass check now queries the canonical `user_active_pass` view (V18) with `date(valid_until) >= date('now')`, mirroring `jobs/charger.rs` exactly (inclusive last day). `my_balance` (display-only) got the same inclusive predicate. `db/users.rs::get_user_pass_valid_until`/`get_user_pass_tx` wrap `date(valid_until)` (defensive decode). Behaviour change is one-directional — EXTENDS coverage by the pass's own paid-through day; never newly undercharges.
- **Tests (RED→GREEN, money-consequential):** RED `2b49e2e` — `first_of_day_pass_expiring_today_grants_entry_without_charge` (door_route.rs) seeds `valid_until = date('now')`, asserts charged=false + credit untouched + visit row (fails on old `>` code); plus `my_balance_shows_pass_active_on_its_expiry_day`. GREEN `4617d98`. Permissiveness guards (expired-yesterday → charged/null). All 8 mutation shards green (no surviving diff mutants).
- **Version:** bump `075d0bc` (0.15.0-dev.68 → .69).
- **Review:** inline two-pass (`/review` + deep `/requesting-code-review`) — 0 🔴 0 🟡 0 🔵. Small, focused diff (3 src files + 2 test files).
- **CI:** dev push green incl. all 8 mutation shards, E2E, Deploy (dev), Smoke (dev). PR [#206](https://github.com/zbynekdrlik/spinbike/pull/206), merged `f522b58`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Deployed:** v0.15.0-dev.69, confirmed on `https://spinbike.sk` — DOM version = `v0.15.0-dev.69` == `/api/version`, 0 console errors, spinbike.service (prod) active. **Read-only prod validation** (`/opt/spinbike/prod/spinbike.db`): 0 rows where old-granted-but-new-denies (fix never revokes access — one-directional confirmed), 0 non-bare-date `valid_until` rows (all 10 chars), 0 users at the exact boundary today (zero live disruption); boundary semantics proven on prod's own SQLite (`date('now')>=date('now')`=1, `date('now','-1 day')>=date('now')`=0).
- **Follow-ups filed:** [#204](https://github.com/zbynekdrlik/spinbike/issues/204) (schema-level CHECK/trigger for the active-pass invariant — needs a transactions rebuild), [#205](https://github.com/zbynekdrlik/spinbike/issues/205) (needs-decision — pass-expiry day boundary UTC vs gym-LOCAL Europe/Bratislava).
- **Playbook:** added the bare-DATE-vs-`datetime('now')` TEXT-prefix gotcha + canonical inclusive `date(x) >= date('now')` form to `.claude/skills/db-migrations/SKILL.md`.

## 2026-07-11 — #201 give `.form-help` hint text actual CSS styling

- **Issue:** [#201](https://github.com/zbynekdrlik/spinbike/issues/201) — `.form-help` (used at 3 sites: `edit_info_form.rs:529,560`, `login.rs:148` the #151 hint) had ZERO CSS rules anywhere in `style.css`; hint text rendered with only the browser default `<small>` styling. Validated STILL_VALID by direct grep (still zero rules, still 3 unstyled usages).
- **Fix:** one `.form-help` block added to the FORMS section of `style.css`, right after `.form-group label` — `display: block; margin-top: var(--s-1); font-size: var(--fs-xs); color: var(--text-muted);`, matching the existing label spacing pattern and reusing the already-theme-aware `--text-muted` token. No Rust/JS changes needed.
- **Test:** not a bug fix in the RED/GREEN sense (pure styling) — extended the existing #151 E2E test (`e2e/tests/login-link.spec.ts`) with a computed-style assertion (`fontSize === '12px'`, `color !== bodyColor`) proving the CSS actually applies, not just that the class exists in the DOM.
- **Version:** bump `128559d` (0.15.0-dev.70 → .71, ticket work); `.71 → .72` rides in this same post-merge playbook commit.
- **Commits:** `128559d` (version), `75751bb` (fix + test), on `dev`.
- **Review:** inline `/review` + deep `/requesting-code-review` subagent (base `9e40f73`..head `75751bb`) — 0 🔴 0 🟡 0 🔵 (one Minor note on the exact-match `fontSize` assertion, explicitly characterized by the reviewer as a legitimate design choice, not a defect).
- **CI:** dev push green (all jobs incl. 8 mutation shards, E2E, Deploy (dev), Smoke (dev)). PR [#207](https://github.com/zbynekdrlik/spinbike/pull/207), merged `1836d68`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Deployed:** v0.15.0-dev.71, confirmed on `https://spinbike.sk/login` — DOM version = `v0.15.0-dev.71` == `/api/version`, hint computed style `fontSize: 12px`, `color: rgb(84,90,103)` (light-theme `--text-muted`) vs body `rgb(20,22,27)`, 0 console errors. Required clearing a stale service-worker registration first (pre-existing gotcha, see below).
- **Found during verification (filed, NOT fixed here — genuinely out of scope):** [#208](https://github.com/zbynekdrlik/spinbike/issues/208) — `sw.js`'s `isVolatile()` only matches the exact root `/` and `*.html`, not SPA client-side routes (`/login`, `/dashboard`, `/welcome`), so those routes get cache-first'd FOREVER — reproduced live (DOM stuck on `v0.15.0-dev.65` while prod served `.71`, confirmed via `caches.open('spinbike-v2').then(c=>c.keys())` showing `/login`'s HTML in the permanent cache-first store). Needs a real design decision on the fix heuristic (Content-Type sniffing vs URL-shape vs route allowlist) — its own ticket.
- **Playbook:** corrected the existing `frontend-pwa/SKILL.md` post-deploy-verification entry (it previously claimed the SW's network-first handling was a complete fix — it isn't, per #208) and added the CLAUDE.md router trigger "post-deploy DOM checks" → `frontend-pwa` (the existing gotcha wasn't discoverable from a plain post-deploy verification task, costing a re-derivation this cycle).

## 2026-07-12 — #208 service worker stops cache-first-pinning SPA route HTML (stale-deploy fix)

- **Issue:** [#208](https://github.com/zbynekdrlik/spinbike/issues/208) (bug) — `sw.js`'s `isVolatile()` only network-first'd `/`, `*.html`, `/sw.js`, `/manifest.json`; every extension-less SPA route (`/login`, `/dashboard`, `/my/balance`, `/welcome`) fell into the cache-first branch and got pinned forever. A later deploy's HTML then referenced content-hashed JS/WASM that no longer existed → 404 (the old path has a `.`, missing both `Asset::get` and the dotless SPA fallback in `static_files.rs`). Validated STILL_VALID: buggy `isVolatile` present verbatim, `static_files.rs:36` SPA fallback confirmed dotless-only.
- **Fix (two PRs — the second corrected a defect found in live post-deploy verification of the first):**
  - PR [#210](https://github.com/zbynekdrlik/spinbike/pull/210) (merged `0c60853`, deployed v0.15.0-dev.74): replaced the URL-shape `isVolatile` with `/api|/ws` bypass + `/assets/`→cache-first + everything-else→network-first, bumped `CACHE_NAME` v2→v3 (activate purges the poisoned per-route caches).
  - **Post-deploy verification on live prod found a regression** in PR #210: this app's Trunk bundle is served at the **ROOT** (`/spinbike-ui-<hash>.js`, `_bg.wasm`), NOT under `/assets/` (`/assets/…` 404s; the 2.4 MB WASM has NO cache-control), so the immutable bundle fell into network-first → re-downloaded on every hard navigation. PR [#211](https://github.com/zbynekdrlik/spinbike/pull/211) (merged `444e1b9`, deployed v0.15.0-dev.75) fixes it by routing on `request.mode === 'navigate'` (the canonical SW discriminator — navigations network-first, everything else incl. the root bundle cache-first). Self-adapts to any route AND any asset path; keeps the `text/html` guard as defence-in-depth.
- **Tests (RED→GREEN):** `e2e/tests/sw-cache.spec.ts` loads the REAL `spinbike-ui/sw.js` into a mocked ServiceWorker scope (`self`/`caches`/`fetch` via `vm`) and drives synthetic FetchEvents (deterministic, server-independent — a real-browser SW test can't force a mid-run "deploy"). RED `3e9a721`/`73c478f` → GREEN `ab019b3`/`22b7484`. Proven across all THREE sw.js generations via a Node harness: original (T1/T2/T7 fail = #208 pin + no bump), shipped `/assets/`-only (T3 "root bundle cache-first" FAILS = the regression), mode-based (7/7 pass). The `root-level bundle cache-first` assertion is the one that catches the #210 regression.
- **Version:** bumps `b393c36` (.73→.74), `0c1268f` (.74→.75).
- **Review:** inline (dispatch: review INLINE) — both PRs 0 🔴 0 🟡 0 🔵.
- **CI:** both dev pushes + both main runs green — all jobs incl. 8 mutation shards, E2E (the new spec ran through real Playwright), Deploy (prod) + Smoke (prod).
- **Deployed + verified LIVE on `https://spinbike.sk` (v0.15.0-dev.75):** DOM version `v0.15.0-dev.75` == `/api/version`; SW controls page, cache = `spinbike-v3` (v2 purged); **#208 core proven live** — injected a poisoned STALE HTML doc into `spinbike-v3` under `/dashboard`, navigated there, SW served the FRESH app (v0.15.0-dev.75, no stale marker) via network-first, poisoned entry did not survive. 0 console errors. bundle-cache-first refinement proven by the 7/7 unit suite on the real artifact + origin serving the mode-based script (cache-busted `/sw.js` shows `request.mode`).
- **Found during verification (filed, NOT fixed here — distinct layer/root cause):** [#212](https://github.com/zbynekdrlik/spinbike/issues/212) — Cloudflare edge-caches `/sw.js` for 4h (`cf-cache-status: HIT`, `max-age=14400`), delaying SW-script updates after deploy (HTML/manifest are `DYNAMIC`, unaffected; hashed JS is immutable-by-hash so harmless). Fix direction: serve `/sw.js` with `Cache-Control: no-cache` from `static_files.rs`. dev.75 SW therefore reaches existing users within CF's ≤4h TTL, then permanently.
- **Playbook:** updated `frontend-pwa/SKILL.md` — the #208 gotcha is now the FIXED reference pattern (navigation-mode routing + root-served-bundle note + the CF `/sw.js` edge-cache caveat → #212).

## 2026-07-12 — #164 replace SELECT * with explicit column lists (rescoped mechanical hardening)

- **Issue:** [#164](https://github.com/zbynekdrlik/spinbike/issues/164) (architecture-review, severity blue) — originally proposed adopting sqlx's `query!`/`query_as!` compile-time macros + a committed `.sqlx` metadata cache. **Rescoped by the supervisor before work started** (durable issue comment): the macro workflow needs a live `DATABASE_URL` or a regenerated-and-committed `.sqlx` cache on every query edit, which this project has zero scaffolding for and which conflicts with its Tier-0 no-local-build policy. Accepted scope instead: replace all real `SELECT *` sites with explicit column lists on the SAME runtime `sqlx::query`/`query_as` calls, mirroring the codebase's own pre-existing `list_upcoming_for_user` pattern. Not a bug fix — zero-behavior-change mechanical hardening, validated STILL_VALID (0 compile-time macros in use, 15 real `SELECT *` call sites confirmed by grep).
- **Fix:** 15 sites converted — `db/users.rs` x7 (`backfill_search_text`, `get_user_by_email`, `get_user_by_id`, `get_user_by_oauth`, `list_users`, `search_users`, `get_user_by_card_code`), `db/classes.rs` x3 (`list_active_templates`, `list_all_templates`, `list_bookings_for_class`), `routes/admin.rs::update_template`, `routes/classes.rs::cancel_booking`, `routes/payments.rs` x3 (`charge`/`storno`/`sell_pass`). Left alone: `db/error.rs`'s intentional `SELECT * FROM does_not_exist` error test, and a doc comment (reworded to stay accurate, then tightened again per review — see below).
- **Key fact established:** `#[derive(sqlx::FromRow)]` (no `#[sqlx(rename)]` anywhere in this codebase) matches columns by NAME, not position — proven from the pre-existing `users_by_last_movement` query, whose column order already differed from its struct's field order. Column order in a new explicit `SELECT` therefore never matters, only completeness. Documented in `db-migrations/SKILL.md`.
- **Tests:** no RED→GREEN mandate (not a bug fix), but two new spot-check regression-guard tests added (`get_user_by_id_decodes_every_column`, `list_bookings_for_class_decodes_every_column`) round-tripping every struct field — including `created_by`, which no prior test read back through `BookingRow` itself.
- **Commits:** `2e5f2be` (version bump), `5f864fc` (refactor + tests — combined into ONE commit deliberately, see gotcha below), `3cf0152` (review-nit doc fix), on `dev`.
- **Gotcha (documented in `ci-deploy/SKILL.md`):** the pre-push hook's Gate 2 bug-fix heuristic fires on `Closes #N` in a commit BODY regardless of subject prefix. Since this change genuinely isn't a bug fix but the PR still needed to auto-close #164, the commit used `refactor(db):` + `Ref #164` (not `Closes`) and the real `Closes #164` was placed only in the PR body — GitHub still closes the issue on merge, and the hook never sees it. Avoided the `[no-test:]` bypass entirely since real tests exist.
- **Review:** 3 parallel finder angles (correctness/bind-order/missed-sites, cleanup/conventions, fresh-eyes gap sweep) + deep `superpowers:requesting-code-review` senior pass (base `89e3f11`..head `5f864fc`) — 0 🔴 0 🟡, one Minor (doc-comment precision on `BookingRow`'s intentionally-partial column set) fixed same-session in `3cf0152` since it was inside the diff.
- **CI:** dev push green (all jobs incl. all 8 mutation shards at 100% diff-scoped kill, E2E, Deploy (dev), Smoke (dev)). PR [#213](https://github.com/zbynekdrlik/spinbike/pull/213), merged `63f8e89`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Deployed + verified LIVE on `https://spinbike.sk` (v0.15.0-dev.77):** service active, DOM version `v0.15.0-dev.77` == `/api/version`, login page renders, 0 console errors/warnings (2 benign info-level messages: PWA install-banner notice, autocomplete hint — both pre-existing, unrelated).
- **Playbook:** added the `FromRow` name-based-matching fact to `db-migrations/SKILL.md` (with the migration-history cross-check reminder — a struct field can be added by a LATER `ALTER TABLE`, not just the original `CREATE TABLE`), and the `Closes #N`-in-PR-body-only technique to `ci-deploy/SKILL.md` as an alternative to the `[no-test:]` bypass when a commit has real tests but isn't a bug fix.

## 2026-07-12 — #143: soft-deleted email reuse → resolvable 409 + restore/free-email (dev.88)

- **Bug:** reusing an email held by a SOFT-DELETED account 500'd (PUT) / generic-409'd (POST): `get_user_by_email` filters `deleted_at IS NULL` but `users.email TEXT UNIQUE` counts archived rows, so the collision check missed the holder and the raw INSERT/UPDATE hit the constraint. Confirmed live on prod (5 soft-deleted rows still held emails, incl. #569).
- **Owner decision (settled on the issue):** not just a block — a clear message + explicit resolution (restore old account OR free its email). Implemented exactly that.
- **Backend:** new `ErrorCode::EmailBelongsToDeletedAccount`; `db::get_user_by_email_including_deleted`; create+update pre-check → structured 409 (`conflict_id`/`conflict_name`/`conflict_deleted_at`) for a soft-deleted holder, existing `EmailConflict` (staff-only leak, customer generic) for a live one; `update_user_info` UNIQUE→409 safety net (also covers the parallel card_code-vs-archived case); staff-gated `POST /api/users/{id}/restore` + `/free-email` (free-email REFUSES an active account). Comprehensive logging.
- **Frontend:** `DeletedEmailConflictDialog` (new `dashboard/deleted_email_conflict.rs`) — names the archived account + offers Obnovit ucet / Uvolnit email (free-email → auto-retry the original create/update). Wired into add-person + edit-info via a shared `api::post_json` (rich `ApiError` carrying `code`/`conflict_id`/`conflict_deleted_at`) and `ApiError::deleted_email_conflict()`.
- **Tests:** RED `2c161f7` → GREEN `bcf4ae0`. Integration (`tests/users_email_conflict_resolution.rs`): PUT 409-not-500, create structured 409, restore + free-email happy/403/404, refuse-active, retry-after-free succeeds; updated `api_error_codes.rs` fallback test to the new structured code. DB unit tests for the 3 new query/mutation fns. Playwright E2E (`deleted-email-conflict.spec.ts`) drives the dialog + free-email resolution.
- **Commits:** `62c8b79` (bump) · `2c161f7` (RED) · `bcf4ae0` (GREEN) · `f039138` (clippy fix), on `dev`. PR [#221](https://github.com/zbynekdrlik/spinbike/pull/221), merged `65ea4a7`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Gotcha (documented in `frontend-pwa/SKILL.md`):** `Build WASM (UI)` runs `clippy --target wasm32 -D warnings`; `Test (UI)` (wasm-pack) passing does NOT mean it passes. `clippy::collapsible_if` on a nested `if` cost one CI cycle — collapse to a let-chain. Tier-0 can't run clippy locally, so write clippy-clean UI Rust the first time.
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.88):** service active, DOM version `v0.15.0-dev.88` == `/api/version`. Full dialog flow exercised with a SYNTHETIC staff+soft-deleted pair via Playwright: structured 409 → dialog naming the account + date → "Free the email" → auto-retry → "Person added"; DB confirmed old row's email freed + still archived, new row created. All synthetic rows deleted after; secrets scrubbed. (The lone console "error" is the browser's benign 409 network-status log — the app handles it via the dialog; E2E `assertCleanConsole` passes in CI.)

## 2026-07-12 — #205: pass-expiry day boundary = gym-LOCAL day (Europe/Bratislava), not UTC (dev.90)

- **Owner decision (settled on #205):** a monthly pass is valid THROUGH the whole of its last calendar day AT THE GYM. The inclusive `date(valid_until) >= date('now')` checks (#179) used SQLite's `date('now')` = **UTC**, up to 2h off from the gym's day near local midnight.
- **Fix — explicit named-tz, NOT OS `'localtime'`/`chrono::Local`:** new `util::now_bratislava() -> NaiveDateTime` / `today_bratislava() -> NaiveDate` (`Utc::now().with_timezone(&chrono_tz::Europe::Bratislava)`, DST-correct via tzdata, independent of the server OS/`TZ`). Single source of the gym-local day, used at every pass day-boundary site as a **bound SQL param** (never a `date('now')` literal): `door.rs` + `my_balance.rs` (`date(valid_until) >= ?`), `payments.rs` `log_visit` + `sell_pass`, `users.rs` days_remaining ×2, `charger.rs` `tick()` window (`now_bratislava()`).
- **Charger pass check UNCHANGED (confirmed, not assumed):** it compares `valid_until` against the **booking's own bare calendar date** — both already gym-local calendar dates, no `date('now')`/`now` involved — so the boundary never touched it. Only the "today"-relative checks needed the helper.
- **GOTCHA:** `NaiveDateTime` has `.date()`, NOT `.date_naive()` (that's a `DateTime<Tz>` method). `today_bratislava()` first wrote `.date_naive()` → clippy `E0599`, one wasted CI cycle. `date_naive()` is only for the tz-aware `DateTime<Tz>`; on a `NaiveDateTime` use `.date()`.
- **CI-runner-TZ flake trap:** the Test/E2E jobs run on `ubuntu-latest` (**UTC**). The #179 boundary tests seeded "today" via `Local::now()` / SQL `date('now')` (= UTC on CI) and flow through the now-Bratislava handlers → they diverge & flake in the ~00:00-02:00 UTC window. Re-seeded every such test (`door_route.rs`, `monthly_pass.rs`, `users_routes.rs`) on `spinbike_server::util::today_bratislava()` so test-today == handler-today on ANY runner TZ. The expired-YESTERDAY guards stay `date('now','-1 day')` (robust — Bratislava is always ≥ UTC).
- **Tests:** deterministic Bratislava-conversion unit tests (winter CET rollover + summer CEST DST offset) in `util.rs`; new `tests/pass_expiry_local_day.rs` — DB-level regression proving the shared pass-active predicate honors a **caller-supplied** gym-local "today" inclusively, using a fixed 2020 date impossible under old `date('now')`. Not a strict RED→GREEN (design-consistency change, #205 is `question`-labeled, behavior-neutral today) — tests assert the NEW semantics.
- **Commits:** `72f66ca` (bump) · `6b70dd3` (impl+tests) · `505e408` (clippy `.date()` fix), on `dev`. PR [#223](https://github.com/zbynekdrlik/spinbike/pull/223), merged `8d1d31e`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.90):** service active, DOM version `v0.15.0-dev.90` == `/api/version`, 0 console errors. **Behavior-neutral on real prod data today:** 41 active passes under UTC `date('now')` == 41 under gym-local (`date('now','localtime')`, = Bratislava on the prod host). **3 real passes expire exactly today** — active under both bases now, and the fix keeps them valid through Bratislava midnight tonight instead of flipping at 02:00 CEST (UTC midnight) — the exact correction. Follow-up **#222** filed for the remaining OS-TZ-dependent day-boundary sites (upcoming-classes list, door same-day press count) — separate features, not pass-expiry.

## 2026-07-12 — #222: remaining day-boundary sites → gym-local (Europe/Bratislava) (dev.92)

- **Scope (the 3 sites #205 left out, behavior-neutral on the Bratislava-OS prod host today):** upcoming-classes / my-bookings list, door same-day re-entry count, 12-month stats chart. All the `date('now')`/`'localtime'`/`chrono::Local` day-boundary keys.
- **Two shapes (see db-migrations skill #222 gotcha):** bare-DATE column (`bookings.date`) → bind `today_bratislava()`, `b.date >= ?`. UTC-INSTANT column (`created_at`) → compare against the gym day's UTC-instant RANGE via NEW `util::bratislava_day_range_utc(day) -> (start,end)` (start = UTC instant of Bratislava local midnight, DST-correct via tzdata), `.format("%Y-%m-%d %H:%M:%S")` both, `created_at >= ? AND created_at < ?`. NEVER `date(created_at,'localtime')` (fragile OS zone).
- **Door (MONEY-adjacent):** `n==0` (no prior press today) drives the pass-check/charge path — the UTC-range boundary prevents a same-gym-day double charge (or a skipped pass check) across an OS/UTC rollover. `db::classes::list_user_bookings` + `routes::upcoming_classes` + `list_upcoming_for_user`'s `now` → gym-local. `users.rs` stats: month/year totals + monthly `CASE created_at < bound_i THEN label_i` bucketer key off Rust-computed gym-local UTC boundaries.
- **Mutation trap (cost 1 CI cycle):** `user_stats` derives `today` from the live clock, so its December year-rollover branch is unreachable in an integration test and widening `this_month` leaves seeded assertions unchanged → the `==12`/`year+1` mutants survived shard 5/8. Fix: extract `next_month_first(day)` (pure) + fixed-date unit tests (Jul→Aug, Nov→Dec, Dec→Jan-next-year). General rule now in the db-migrations skill.
- **Tests:** `bratislava_day_range_utc` unit tests (winter CET+1 / summer CEST+2 / 24h span); new `tests/local_day_boundary.rs` (door same-day count + bookings filter, deterministic via fixed 2020/2026 dates incl. the double-charge-guard case); `cards_stats.rs` re-based (created_at seeds → real UTC, month/year labels → `now_bratislava()`, the #205 CI-TZ flake). Not strict RED→GREEN (design-consistency change like #205, behavior-neutral today; tests assert the NEW semantics).
- **Commits:** `27b80d6` (bump) · `ff35e60` (impl+tests) · `0c6678f` (mutation-kill: extract `next_month_first`), on `dev`. PR [#224](https://github.com/zbynekdrlik/spinbike/pull/224), merged `6457610`. Main CI green incl. Deploy (prod) + Smoke (prod). #222 auto-closed.
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.92):** DOM version `v0.15.0-dev.92` == `/api/version`, service active, 0 console errors (1 benign wasm-bindgen init deprecation, pre-existing). **Behavior-neutral on real prod data (host OS = Europe/Bratislava):** door predicate 0 mismatches over all 8 door rows; stats this-month old `strftime('localtime')` == new UTC-range (488 == 488); bookings upcoming old(UTC) == new(gym) (0 == 0). Proven via SQLite's `datetime(<local>, 'utc')` modifier — recipe in the db-migrations skill.

## 2026-07-16 — #227: 6-digit email login code — closes the iOS installed-PWA logged-out loop (dev.96)

- **Why:** on iOS a home-screen web app has storage partitioned from Safari and a magic link always re-opens in Safari, so a link can never complete login INSIDE the installed app (the "install je pain" loop). Fix = a login method that completes entirely in-app: a short numeric code the user types.
- **Migration V21** (`login_tokens` rebuild): widen `purpose` CHECK to `('invite','login','code')` + add `attempts INTEGER NOT NULL DEFAULT 0`. Table-rebuild pattern (V16 shape) since SQLite can't ALTER a CHECK. **login_tokens is referenced by NO view/trigger** (V18 view + V20 trigger reference services/transactions only), so the rebuild needs no DROP-VIEW/TRIGGER dance. Preserves existing rows (genuine-upgrade test uses `raw_sql` per-migration — `apply_sql_block`'s `;`-split mangles V20's trigger body).
- **Token layer** (`db/login_tokens.rs`): `generate_code` (crypto 6-digit, zero-padded); `hash_code(user_id, code)` = SHA-256 of `"{user_id}:{code}"` — **per-user salt** avoids the `token_hash` UNIQUE collision two users sharing a 6-digit value would cause, and binds a code to its own account; `create_code` DELETEs the user's prior code rows (invalidates prior unused + frees the UNIQUE slot); `verify_code` transactional single-use redeem + `attempts+1`, invalidate at `MAX_CODE_ATTEMPTS=5`. All failure modes → `Ok(None)` (uniform, no leak). `CODE_TTL_SECS=600`.
- **Endpoints** (`routes/auth.rs`): `POST /api/auth/request-login-code` mirrors request-login-link (always 200, customers-only send, **reuses** `login_link_rate_limit` — same email-send budget, `tokio::spawn` SMTP off the response path = same timing side-channel guard). `POST /api/auth/code-login` → NEW `CodeLoginRateLimiter` (per-email 10/60s + global 60/60s) keyed by submitted email BEFORE any DB lookup (429 `too_many_requests` leaks no account existence); every other failure → uniform 401 `invalid_or_expired_code`. Success → permanent customer JWT. New `ErrorCode::{InvalidOrExpiredCode, TooManyRequests}` + `ApiError::TooManyRequests`→429.
- **UI**: `CodeLoginForm` (email → Poslat kod → `inputmode=numeric autocomplete=one-time-code` input → code-login → role-aware redirect) + `CustomerLoginMethods` toggle (email-link vs code, **default link** so existing login-link/welcome E2E + habitual flow unchanged), on login page customer section + /welcome invalid fallback. `post_public(_coded)` both endpoints. Sk/En unaccented i18n + localized banners.
- **E2E seam**: test-fixture `POST /api/test/mint-login-code` (SPINBIKE_TEST_MODE-gated) returns a raw code so `code-login.spec.ts` enters a known-valid value after driving the real UI. Public request-login-code never echoes the code (no enumeration).
- **Tests:** V21 migration (5) + token layer unit (11: generate/hash-per-user/create-supersede/verify happy+wrong+5-invalidate+4-then-correct+expired+no-code+scoped) + `error.rs` 429 arm + `errors.rs` code table + rate-limiter unit (4) + `auth_routes.rs` integration (11: enumeration identical-200, non-customer, blocked, code creation, happy+single-use, wrong x5 invalidate, expired, unknown-email, blocked, 429). Playwright `code-login.spec.ts` (happy via mint + wrong-code banner). Feature (not bug) → tests same PR, not strict RED→GREEN.

## 2026-07-16 — #228: install-first flow — welcome note, standalone login ordering, email guidance (dev.98)

- **Why:** #227 gave iOS clients a way to log in inside the installed app (a code), but nothing TAUGHT the extra step — the install guide never warned "you'll be asked to log in once more", and the invite/login/code emails carried zero install guidance. Package finisher for #225/#226/#227.
- **Shared platform module (`spinbike-ui/src/platform.rs`, new):** extracted `is_standalone()`/`is_ios_ua()`/`user_agent()`/`get_prop()`/`window_value()` out of `components::install_prompt` (`pub(crate)`, byte-identical logic, no behavior drift) so `CustomerLoginMethods` (`code_login_form.rs`) can reuse the same iPadOS-desktop-UA disambiguator without duplicating the `Reflect`-based JS interop. New composite `is_ios_standalone() = is_standalone() && is_ios_ua(&user_agent())`.
- **Login-method reorder (#228 item 2):** `CustomerLoginMethods`'s initial toggle state (previously always `"link"`) is now `if platform::is_ios_standalone() { "code" } else { "link" }`, computed ONCE at component setup (same non-reactive-detect-once pattern as `InstallPrompt::detect_kind()`) — a magic link is a dead end when already installed+standalone on iOS. Android/Chromium standalone explicitly UNCHANGED (shares storage with the browser, no reorder) — both the code and a dedicated E2E test (`navigator.standalone=true` + Android UA) prove this, not just the test.
- **`/welcome` iOS post-install note (#228 item 1):** page-LOCAL copy (per `.claude/skills/auth-onboarding/SKILL.md` #151 rule — never baked into the shared `CustomerLoginMethods`/`InstallPrompt` components), gated on `platform::is_ios_ua(&platform::user_agent())` computed once at mount in `welcome.rs`, rendered under `<InstallPrompt/>` in the `Success` state only. Android shows nothing extra.
- **Email guidance (#228 item 3):** new shared `append_ios_install_hint(text, html) -> (String, String)` in `routes/auth.rs`, called once at the end of ALL THREE composer functions (`login_link_email`, `login_code_email`, `invite_email`) — the validator's post-#227 read of the codebase showed all three exist and should all carry the note, not just the two the original issue text named. Unaccented Slovak: "Mas iPhone? Po prihlaseni si pridaj SpinBike na plochu (navod ti ukazeme) a v appke sa prihlas kodom."
- **Tests:** new `spinbike-ui/src/i18n.rs` key `welcome_ios_post_install_note` (Sk+En, unaccented Sk). New server unit tests locking the appended iOS section on `login_code_email` (extended existing test) + new `login_link_email_carries_the_link_and_ios_hint` + `invite_email_carries_the_link_and_ios_hint` (both also regression-guard the link itself survives the new trailing-section append). New Playwright coverage: `code-login.spec.ts` — 2 new `describe` blocks (iOS-standalone leads-with-code on `/login` AND `/welcome` invalid fallback, plain-Safari-tab and Android-standalone stay-with-link); `welcome.spec.ts` — iOS success shows the note, Android does not. New shared `setIosStandalone(page)` helper in `e2e/tests/helpers.ts` (`Object.defineProperty(navigator, 'standalone', {get:()=>true})`, combined with `devices['iPhone 13']` / a raw Android UA `test.use` context — matches the existing iPadOS-UA-override pattern already in `install-prompt.spec.ts`). Feature (not bug) → tests same PR, not strict RED→GREEN.
- **Review:** two independent-agent passes (a fresh-eyes read + the `superpowers:requesting-code-review` deep pass) both returned clean — 0 Critical, 0 Important; two cosmetic Minor notes (the code-email also getting the hint; a second UA/Reflect round-trip per mount) explicitly assessed as non-blocking / not worth a shared memo for two call sites.
- **Commits:** `91f9095` (bump) · `2684ae7` (impl+tests), on `dev`. PR [#231](https://github.com/zbynekdrlik/spinbike/pull/231), merged `82eae81`. Main CI green incl. Deploy (prod) + Smoke (prod). #228 auto-closed.
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.98):** DOM version `v0.15.0-dev.98` == `/api/version` == deployed server binary/wasm, 0 console errors, stale service-worker registration cleared first. **Standalone+iOS override (live, real deployed bytes, client-side SPA route change — not just a fresh page load) → "Login with a code" tab leads on `/login`.** **Standalone+Android override → "Email link" still leads (unaffected).** Live wasm bundle `strings` grep: `welcome_ios_post_install_note`'s Sk text FOUND. Live prod server binary (`/opt/spinbike/prod/spinbike-server`, restarted at deploy) `strings` grep: the exact `append_ios_install_hint` text FOUND verbatim in both plain-text and `<p>`-wrapped HTML form — proves the deployed email-composer bytes, without needing a live SMTP round-trip (already proven end-to-end via the `SMTP_TEST_MODE=capture` integration tests in CI).

## 2026-07-21 — #234 + #235: duplicate-visit warning + last-visit in quick-search (dev.101, dev.102 follow-up)

- **RESUME note:** this batch was implemented by a prior worker (commits `41338e8` #234, `5723620` #235, already on `dev`) which then died when CI run 29815174191 failed on `E2E Tests`. This cycle resumed from that pushed state — did NOT redo the implementation, only root-caused and fixed the CI failure, then drove the full PR→merge→deploy→verify cycle.
- **#234:** `log_visit` now warns instead of silently duplicating — 409 `already_visited_today` (with `last_entry_at` + `source: door|manual`) when the user already has a same-day class-visit event (canonical UNION: `action='visit'` OR a per-class pay-as-you-go `charge` with `amount<0 AND valid_until IS NULL`, scoped to `CLASS_VISIT_NAMES_EN` = Fitness+Spinning). Additive `force: true` bypasses it for a genuine second visit in a day. UI: in-form confirm ("Add anyway"/"Cancel") naming the time + source.
- **#235:** quick-search results + the card panel header now show each client's last visit (relative bucket via existing `format_last_visit`/`relative_date` helpers), highlighted with `.visited-today` (danger+bold) when it was today — display-only, no schema/API change.
- **CI-failure root cause:** the new #234 guard broke an EXISTING e2e spec, `reports-attendance.spec.ts` — it seeds paid Fitness + paid Spinning charges for one synthetic user (both already count as a same-day class-visit event under the new canonical definition), then calls `log-visit` twice more for the SAME user/day to prove the attendance KPI sums every class-visit event. The guard fired on the very FIRST `log-visit` call (not the second), because the preceding charges already satisfied its condition. Fix: `postLogVisit(..., force=true)` on those two calls — the guard's own documented legitimate case (a genuine second/third visit in a day), not a guard bypass. Audited every other e2e `log-visit` call site (`grep -rln`); none double-log for the same user/day.
- **Commits:** `1b2f755` (CI fix — force:true), `770bbaf` (bump 0.15.0-dev.102 + new e2e-testing playbook skill), on `dev`. PR [#236](https://github.com/zbynekdrlik/spinbike/pull/236) (Closes #234, #235), merged `d36940f`. Follow-up docs-only PR [#237](https://github.com/zbynekdrlik/spinbike/pull/237) (playbook update — playbook-review ran after #236 already merged), merged `09d233f`. Both main CI runs green incl. Deploy (prod) + Smoke (prod).
- **Playbook:** new `.claude/skills/e2e-testing/SKILL.md` — a new 4xx/409 validation guard on any endpoint can silently break an EXISTING e2e spec that assumed happy-path (a PRIOR seeded transaction, not just the exact repeated call, can satisfy the guard's condition); audit every e2e call site (`grep -rln`) before pushing, fix at the call site with the guard's documented bypass, never weaken the new guard. Router line added to `CLAUDE.md`.
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.102):** service active, DOM version == `/api/version`. Functional verify via a synthetic throwaway customer (id 590, deleted after) + a real-admin-id JWT (SQLite FK enforced — a non-existent `sub` 500s with `FOREIGN KEY constraint failed` on any write touching `staff_id`, so the admin-shortcut needs a REAL admin user id, not an arbitrary one): sold a real pass via `/api/payments/sell-pass`, first `log-visit` 200, second `log-visit` (no force) → 409 `already_visited_today`, `force:true` → 200; quick-search + card panel both showed `"Last visit: today"` with `visited-today` class. All synthetic rows + scratchpad JWT secret/token files deleted after.

## 2026-07-21 — Review follow-up to #236 (#234/#235): Bratislava-local date + force-button double-tap guard (dev.104, no VERSION bump — already ahead)

- **Trigger:** two VERIFIED post-merge code-review findings from PR #236, no GitHub issue — fix-now path, referenced as "review follow-up to PR #236 (#234/#235)" in commits/PR.
- **Finding 1 (bug, Important):** `last_visit_at` (a `MAX(created_at)` UTC instant) fed through `dates::parse_server_date` (raw UTC date token, no tz conversion) at the #235 "today" highlight call sites — `dashboard/mod.rs` search-dropdown row + `card_panel.rs` card-title — instead of the Bratislava-LOCAL date. A visit logged 00:00-02:00 local time rendered "vcera" (yesterday), unhighlighted, in exactly the window the #234 anti-duplicate signal must fire. Server-side guard (`bratislava_day_range_utc`) was already correct — display-only bug. Fix: new `dates::parse_server_date_local` (delegates to the existing DST-aware `i18n::parse_to_local(...).map(|dt| dt.date_naive())`); both call sites switched to it.
- **Finding 2 (bug, Minor):** the "Pridat aj tak" force-retry confirm button (`action_form.rs`) had NO re-entry guard (only the `disabled=move || loading.get()` DOM binding, which can lag a fast double-tap) — unlike the primary visit button, which already guards via `if loading.get_untracked() { return; }` before calling `do_log_visit`. Fix: moved the guard to the TOP of `do_log_visit` itself (the shared choke point for both callers), covering both current callers + any future one from one place.
- **RED→GREEN commit order (regression-test-first):** `e34f31a` test[red] (dates.rs stub + failing test) → `70cdfe9` fix[green] (real conversion + both call sites) → `236705e` test(e2e)[red] (double-click e2e, mirrors the existing `visit-button-feedback.spec.ts` guard-test technique — `.click()` then `.click({force:true})` for a deterministic, non-flaky repaint-lag simulation) → `c681e08` fix[green] (the guard). A deep independent code-review pass (dispatched subagent, scoped diff `d13d284..c681e08`) then requested one addition — `703d500` test: `parse_server_date_local` garbage/empty-input `None` symmetry test (same-file, trivial, done immediately per `complete-planned-work.md`'s follow-up gate, not deferred).
- **Follow-ups filed, NOT fixed here (genuinely outside this PR's named scope):** `relative_date::today_local()` still derives "today" from the browser/system clock, not an explicit Europe/Bratislava computation (correct today only because every staff device runs Bratislava TZ; widening touches 9+ call sites) → [#239](https://github.com/zbynekdrlik/spinbike/issues/239). The identical UTC-instant-vs-local-day bug at a THIRD, un-flagged call site, `negative_balance_list.rs`'s last-visit relative text → [#240](https://github.com/zbynekdrlik/spinbike/issues/240). The deep-review pass additionally found the SAME bug class on the CUSTOMER-facing `/my/balance` recent-transactions date (`my_balance.rs:151`, same `transactions.created_at` column) — arguably higher-visibility than the two fixed staff-only sites, but outside this PR's diff → [#242](https://github.com/zbynekdrlik/spinbike/issues/242) (also notes a lower-priority 4th instance in `deleted_email_conflict.rs`).
- **Commits:** `e34f31a`, `70cdfe9`, `236705e`, `c681e08`, `703d500` (review-requested addition), on `dev`. PR [#241](https://github.com/zbynekdrlik/spinbike/pull/241) (no Closes — no issue), merged `5a320de`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.104):** DOM version == `/api/version`, 0 console errors, stale SW registration cleared first. Synthetic customer (id 591, real pass sold + a real Fitness visit logged via `/api/payments/sell-pass` + `/api/payments/log-visit`, both real endpoints) → search-dropdown row rendered `class="search-result-last-visit visited-today"` text `"Posledna navsteva: dnes"`; card panel rendered `class="card-title__last-visit visited-today"` text `"Posledna navsteva: 21.07.2026 (dnes)"`. All synthetic rows (transactions + user, soft-delete via API then hard-delete via SQL) + scratchpad JWT secret/token files deleted after.

## 2026-07-21 — #239 + #240 + #242: today_local() Bratislava anchor + last-visit/tx-date UTC-instant fixes (dev.106)

- **Trigger:** the three follow-up issues filed by the #241 review (see the previous entry) — same UTC-instant-vs-Bratislava-local-day bug class as #236/#241, bundled into ONE batch PR per the bundling gate (each <300 LoC, same root cause, no schema/API/security cross-cut).
- **#239:** `relative_date::today_local()` derived "today" from `chrono::Local::now()` (the BROWSER's clock) — correct only because every staff device happens to run Bratislava TZ. Extracted a testable `today_from_utc(instant: DateTime<Utc>) -> NaiveDate` core (pinned-instant so tests don't depend on host TZ), anchored to `chrono_tz::Europe::Bratislava` (same pattern as `i18n::parse_to_local`/`dates::parse_server_date_local`). `today_local()` keeps its zero-arg signature — all 13 existing call sites untouched.
- **#240:** `negative_balance_list.rs`'s `format_optional_date` (last-visit relative bucket on the negative-balance list) — third call site of the SAME `last_visit_at` UTC-instant bug #241 fixed at two other sites — swapped `parse_server_date` → `parse_server_date_local`.
- **#242:** `my_balance.rs`'s customer-facing recent-transactions date label AND `deleted_email_conflict.rs`'s deletion-date body text both swapped to `parse_server_date_local`. Both inline computations were extracted into testable functions (`format_tx_date_label`, `body_text_for`) first (no prior test module in either file).
- **RED→GREEN commit order (regression-test-first), one pair per issue:** `0b2b962`→`616d885` (#239), `3e672e6`→`c3a7bc9` (#240), `e6ef1e9`→`2c4c87e` (#242). Each RED test pins the exact 00:00-02:00 Bratislava boundary instant (`2026-07-20 22:30:00` UTC = `2026-07-21 00:30` CEST) used throughout `dates.rs`'s existing #236/#241 tests.
- **GOTCHA (new, playbook-worthy):** `spinbike-ui` is a SEPARATE cargo workspace with its OWN rustfmt invocation in CI (`Build WASM (UI)` job runs `cargo fmt --manifest-path spinbike-ui/Cargo.toml --all -- --check`) — the project's documented pre-push check, root-level `cargo fmt --all --check`, does NOT cover it (root workspace excludes `spinbike-ui/`). First push failed CI on this exact gap (one multi-arg `assert_eq!` line unformatted in the new `relative_date.rs` test); fixed with one follow-up commit (`cf383cd`) running `cargo fmt --manifest-path spinbike-ui/Cargo.toml --all` (no `--check`) to auto-fix, then re-pushed. See `.claude/skills/ci-deploy/SKILL.md` for the codified fix — pre-push checks for THIS repo must run `cargo fmt --all --check` AND `cargo fmt --manifest-path spinbike-ui/Cargo.toml --all -- --check`, not just the root one.
- **UTC-instant-vs-local-day bug class is now COMPLETE**: the #241-fix-time grep audit found 5 total instances (`dashboard/mod.rs`, `card_panel.rs` fixed by #241; `negative_balance_list.rs`, `my_balance.rs`, `deleted_email_conflict.rs` fixed by this PR) plus the root `today_local()` clock source (#239). No remaining `dates::parse_server_date(` call site takes a UTC-instant field — the rest (`my_bookings.rs`, `my_balance.rs`'s `monthly_pass_active_until`/`valid_until`, `staff_dashboard.rs`, `persistent_toggles.rs`) are genuine calendar-date fields (booking slot dates, pass expiry, weekday schedules), correctly left on the raw parser.
- **Commits:** `a6917da` (bump 0.15.0-dev.106) → 6 RED/GREEN pairs → `cf383cd` (UI-workspace fmt fix), all on `dev`. PR [#244](https://github.com/zbynekdrlik/spinbike/pull/244) (Closes #239, #240, #242), merged `c36ed57`. Main CI green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.106):** DOM version == `/api/version`, stale SW cleared first, 0 console errors on `/staff` and `/my/balance`. Synthetic customer (id 592, negative credit + one real `visit` transaction inserted directly — no test-mode endpoint needed for a bare visit row — deleted after) via a minted admin JWT (no DB row needed, role-only extractor) on `/staff`: negative-balance row rendered `"AUTOPILOT VERIFY 239-240-242 (Posledna navsteva: dnes)"` for a visit logged seconds earlier — proves #239+#240 together (today_local() and parse_server_date_local agree). Via a minted customer JWT on `/my/balance`: recent-transactions row rendered `"21.07. · Spinning"` (today's date) — proves #242. Full 00:00-02:00 midnight-boundary behavior is proven by the unit regression tests in CI (RED/GREEN above), not faked live. All synthetic rows + scratchpad JWT secret/token files deleted after.

## 2026-07-22 — #246: magic-link 10-min grace re-redeem (dev.108)

- **Why:** an iPhone mail app opens an invite/login link in its own in-app webview FIRST (redeeming it), then the user's real browser or the installed PWA re-opens the SAME link (`use_query_map` effect on `/welcome`) and used to hit the now-used token → dead end, "invalid link" loop, no visible reason.
- **Fix (`db/login_tokens.rs`):** `redeem()`'s SQL now accepts a reuse when `used_at IS NULL OR used_at > datetime('now', '-600 seconds')` (new `REDEEM_GRACE_SECS = 10 * 60`), for `invite`/`login` purposes only — `used_at` is `COALESCE`d so a grace reuse does NOT re-stamp/extend the window; it stays pinned to the FIRST redeem. `purge_expired_and_used` kept as the exact logical negation (never purges a row still inside its grace). The 6-digit `code` purpose (#227, `verify_code`) is untouched — strictly single-use, separate function.
- **RED→GREEN commit order:** `db37c2a` (test, red — reuse-within-grace must succeed) → `1aeb69e` (fix, green). Plus-same-PR: `8cae26b` (e2e — reused invite link within grace succeeds), `062dfb2` (integration-level grace re-redeem coverage), `1494088` (mutation-kill: a bare `10 * 60` arithmetic constant needs its own literal-pin test — the `10*60→10+60` mutant survived the boundary-behavior tests since they redefine "past grace" relative to whatever the constant is), `de662b2` (playbook doc). Two review follow-ups: `6925888` (fix a stale single-use claim in the V17 migration comment) + `259e343` (log grace-window reuse distinctly from a fresh redeem, for observability).
- **Commits:** `f8f9158` (bump 0.15.0-dev.108) → the pairs above, all on `dev`. PR [#249](https://github.com/zbynekdrlik/spinbike/pull/249) (Closes #246), merged `ef33779`. Main CI run [29958210651](https://github.com/zbynekdrlik/spinbike/actions/runs/29958210651) green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.108):** DOM version == `/api/version`, stale SW cleared first. Functional: a synthetic customer (id 596) + a real `login_tokens` row (purpose `login`, hand-hashed raw token, `expires_at` +1h) redeemed via the REAL `POST /api/auth/token-login` endpoint TWICE with the identical raw token — both calls returned `200` with a fresh JWT (the exact webview-then-browser double-open this ticket fixes). Synthetic user + token rows deleted after; no JWT secret needed (real API + real token row, no minted JWT for this one).

## 2026-07-23 — #247 + #248: welcome invalid-screen leads with code + in-app-browser banner on Android/`/login`, plus #251 discovered mid-flight (dev.109)

- **#247:** the `/welcome` invalid-token screen (used/expired link, or opened in the wrong app) used to lead with the SAME email-link method that just failed. `CustomerLoginMethods` gained an optional `lead_code` prop; `/welcome`'s Invalid branch passes `lead_code=true` so the 6-digit code (works regardless of platform or why the link died) always leads there — the login page's customer section is unaffected (still platform-aware per #228). Rewrote `welcome_invalid_message` to explain in plain language why the link likely died and recommend the code.
- **#248 (RESCOPED at batch assembly, ticket-validator):** the iOS in-app-browser slice already shipped in #229/#226 — `detect_kind()` gated the whole webview check behind an iOS-only UA test, so an Android in-app browser (same Facebook/Instagram/Messenger apps, same missing-A2HS problem) got zero guidance, and `/login` had no webview detection mounted at all. Extracted the marker list into `platform::is_in_app_browser_ua` (adds the Android WebView UA token `"; wv)"` + `"Messenger"` to the existing six iOS markers), lifted the check out of the iOS-only gate in `detect_kind()`, and extracted the webview markup into a new self-contained `InAppBrowserBanner` component (renders nothing when not applicable) — reused as `InstallPrompt`'s own webview branch (now on EITHER platform) AND mounted standalone on `/login` (webview guidance only, no full A2HS prompt there). Copy branches on platform (Safari vs. "a browser (Chrome)"). Renamed testid `install-prompt-ios-webview` → `install-prompt-webview`.
- **Review fix (same PR, before merge):** `InAppBrowserBanner`'s markup dropped the `install-prompt--ios` class (correctly — an Android banner isn't "ios"), but that class was ALSO the only source of the shared card look (background/padding/shadow) the old combined-class webview branch relied on — a real visual regression. Fixed by giving `.install-prompt--webview` its own copy of that styling in `style.css`.
- **#251 — discovered mid-flight (not a named ticket), fixed in the same PR because it was blocking CI:** CI run 29960845093 failed 4 unrelated E2E tests, all inside the 00:00-02:00 Bratislava-local window (a UTC CI runner landed right in it). Root cause: `db/reports.rs`'s `day_report`/`range_report` bucketed transactions via SQLite's `date(created_at)` — the raw UTC calendar date on a UTC-instant column — instead of the Bratislava-local gym day, the exact bug class #239/#240/#242/#246 already fixed everywhere else, missed at the Reports page's own queries. Real business-impact bug (the Reports page under-counts attendance/cash-in and hides fresh transactions for up to 2 hours after Bratislava midnight), not just a test artifact. Filed as #251, fixed with the established `bratislava_day_range_utc` range pattern (4 SQL sites: day/range × events/KPI). Chased the fix through THREE more layers the same bug class had corrupted: the pre-existing Rust unit test's own `chrono::Local::now()` "today" (passed locally on this Bratislava-TZ dev box, failed on the UTC CI runner), all 7 `chrono::Local::now()` "today" derivations in `tests/reports.rs` (same fix, `spinbike_server::util::today_bratislava()`), and `reports-attendance.spec.ts`'s own client-side UTC-based "today"/"tomorrow" (new shared `bratislavaToday()`/`bratislavaDateOffset()` E2E helpers, Intl-based, mirroring the server's anchor — superseded an initial `+48h margin` band-aid that only masked the symptom).
- **RED→GREEN commit order (#251):** `aec3201` (test, red) → `8d11229` (fix, green) → `45e4ac6` (fix: the reports.rs test suite's own today) → `a898f55` (fix: e2e margin, interim) → `247ed21` (fix: proper Bratislava-anchored e2e helper, superseding the margin). Mutation gate (all 8 shards) passed clean on the new server-side diff.
- **Commits:** `b6990e6` (bump 0.15.0-dev.109 + #246 docs tail, prepped by the predecessor worker) → `2790d7b` (#247 impl+test) → `37268d7` (#248 impl+test) → `3e8bf1a` (CSS review-fix) → the #251 chain above, all on `dev`. PR [#250](https://github.com/zbynekdrlik/spinbike/pull/250) (Closes #247, #248, #251), merged `0e12534`. Main CI run [29964898214](https://github.com/zbynekdrlik/spinbike/actions/runs/29964898214) green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.109):** DOM version == `/api/version`, stale SW cleared first. `/welcome` (no token, invalid state): `login-method-code` `aria-selected="true"`, `code-login-email-form` present, `login-link-form` absent, message text matches the new copy — proves #247. `/login` with the default (non-webview) UA: no `install-prompt-webview` banner — negative case confirmed live. Positive case (Android/iOS webview UA + the exact deployed bytes) confirmed via the live WASM bundle's `strings`: `install-prompt-webview`, `install_prompt_webview_title_other`, `"; wv)"`, `"Messenger"`, `code-login-email-form`, `"Gmaile"` all FOUND — the same UA-context-driven behavior is proven end-to-end by CI's real-Chromium E2E suite (`install-prompt.spec.ts`'s new Android/iOS-webview-on-`/login` describe blocks), which passed on this exact merged code.

## 2026-07-24 — #258 (iOS home-screen app opens already logged in)

- **#258 (feature, solo PR):** the installed iOS home-screen app now opens ALREADY SIGNED IN on first launch (CEO onboarding incident, user-approved design). A one-time **install token** — a plain `purpose='login'` magic-link token minted for the caller's OWN session (NO migration, NO new token kind, NO security-model change) — is embedded in the manifest's `start_url` via a dynamic `/manifest.json?it=<token>` handler; iOS reads `start_url` at "Add to Home Screen" time, so the first icon tap lands on `/welcome?t=...` and completes token-login. Code login (#227) stays as the fallback.
  - **Server:** `POST /api/auth/install-token` (`AuthUser`, any role) mints for `claims.sub` ONLY — never a caller-supplied id. `GET /manifest.json` handler (`routes/manifest.rs`), registered BEFORE the static fallback: no `it` → verbatim; `it=<raw>` → same JSON with only `start_url` swapped (serde_json, no string-splice; token never validated at serve → no oracle). Body compiled in via `include_str!` (deterministic under CI placeholder dist). `token-login` gained an optional `source` label (install vs email-link, journal only).
  - **UI:** `welcome.rs` session short-circuit (stored session → role home, no re-redeem — needed for repeat opens whose start_url carries the spent token, also stops #246 grace burn; forwards `?src` as `source`). `InstallPrompt` on iOS + logged in mints an install token and repoints `<link rel=manifest>` at `?it=<token>` (typed `HtmlLinkElement::set_href`, iOS only, silent no-op on failure). Copy: iOS footer hint now says Safari AND Chrome; welcome post-install note + email hint state the new auto-login truth (code = fallback), their unit tests updated together.
  - **KEY audit finding (see e2e-testing skill):** `loginViaAPI(admin)` STORES the admin session in localStorage, so the 3 welcome invite specs were NOT logged-out when redeeming the invite — the short-circuit would send them home. Fixed by clearing `spinbike_token`/`spinbike_user` before the logged-out navigation (established pattern from `code-login.spec.ts`).
  - **Tests:** server unit (`manifest.rs`: verbatim/swap start_url pin) + integration (`manifest_route.rs`: route precedence + content type; `auth_routes.rs`: install-token 200 self / 401 anon / redeemable). E2E: (a) simulated first launch, (b) session short-circuit, (c) iOS manifest link swap + GET, (d) non-iOS unchanged.
  - **Commits:** `89544bf` (bump 0.15.0-dev.114) → `a871471` (feat+tests) → `3990748` (i18n Slovak gender fix, review self-catch). PR [#259](https://github.com/zbynekdrlik/spinbike/pull/259), merged `dd5f2fd`. Main CI run 30090520261 green incl. Deploy (prod) + Smoke (prod). `/requesting-code-review` deep pass: 0 findings.
  - **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.114, stale SW cleared first):** DOM version == deployed. Via local origin `http://127.0.0.1:8080` (Cloudflare 1010 blocks scripted API on the public host) with a synthetic customer: install-token mint 200 (×2 distinct) / 401 unauth, manifest verbatim `start_url=/` vs swapped `start_url=/welcome?t=<tok>&src=install` with icons preserved, install redemption 200 as the customer. Via real browser: opening the install `start_url` logged-out → `welcome-success` + session stored (first-launch chain); reopening it WITH the session → `/my/balance` (short-circuit, no invalid); 0 console errors. Synthetic customer + tokens hard-deleted after.
  - **UNVERIFIED-on-device:** the physical iPhone "Add to Home Screen" tap can't be simulated — CEO/client validates. If iOS ever ignores the manifest-`start_url` trick, the client falls into the unchanged code-login fallback (nothing regresses).

## 2026-08-06 — #262 + #263: door hold 2s→1s, "preplatok"→"permanentka" wording fix (bundled batch, dev.115)

- **#262 (bug/UX, CEO request):** door-unlock hold shortened from 2s to 1s. `HOLD_DURATION_MS` (`spinbike-ui/src/components/door_button.rs`) 2000.0 → 1000.0; doc comments + `door_button_idle` SK label ("drz 2s" → "drz 1s") updated to match. Accidental-tap guard preserved (1s is still a deliberate hold).
- **#263 (bug, i18n):** "Mesacny preplatok" (wrong — means overpayment/refund) → "Mesacna permanentka" (correct — pass) in both `monthly_pass_active_until`/`monthly_pass_not_active` (`spinbike-ui/src/i18n.rs`), matching the term already used in `edit_pass_date`. English strings unchanged (never wrong). No other "preplatok" occurrence existed anywhere in `spinbike-ui/src/` (confirmed by full grep); the historical `migrations.rs` occurrences are an already-applied, immutable prior fix (#50) for a different string (`services.name_sk`) — untouched.
- **RED→GREEN commit order:** `e458d67` (test, red — door e2e timing) → `082d4e1` (fix, green — #262) → `80c7525` (test, red — new `my-balance-pass-wording.spec.ts`) → `86db3d6` (fix, green — #263).
- **CI-blocking dependency fix discovered mid-flight (not a named ticket, fixed in the same push per the bundling gate — resolution-only, no application code touched):** `event-listener` 5.4.1 → 5.4.2 (`429d36e`), clearing RUSTSEC-2026-0221 (a real unsound Send/Sync advisory, reachable transitively via `httpmock`/`sqlx-core` dev-deps). `cargo update -p event-listener` only, `Cargo.lock`-only diff (also drops the now-unused `concurrent-queue` sub-dep).
- **Test-infrastructure bug found + fixed by CI (not an app bug):** the new `my-balance-pass-wording.spec.ts`'s `loginAsCustomerSk` helper called `localStorage.clear()` before any navigation in the "no active pass" test (no prior admin `loginViaAPI` call to establish an origin first, unlike every other spec's convention) → `SecurityError` on `about:blank`. Fixed in `774513d` (`page.goto('/')` before touching storage) — same convention `loginViaAPI` itself uses internally.
- **Gotcha for future specs:** if a test's FIRST page action is a customer-session switch (no prior admin login establishing a real origin), a `loginAsCustomerSk`-style helper MUST navigate before touching `localStorage` — don't assume a document is already loaded on a fresh Playwright context.
- **Commits (dev):** `e458d67` → `082d4e1` → `80c7525` → `86db3d6` → `429d36e` → `774513d`, all on top of the pre-existing unpushed `b2ae3bf` (docs/#258 playbook + version bump to 0.15.0-dev.115, rode along per dispatch). PR [#265](https://github.com/zbynekdrlik/spinbike/pull/265) (Closes #262, #263), merged `2f3f45b`. Deep review (`superpowers:requesting-code-review`, independent subagent against `b2ae3bf..774513d`): 0 🔴 0 🟡 0 🔵 blocking findings.
- **Main CI run:** [31079340181](https://github.com/zbynekdrlik/spinbike/actions/runs/31079340181) — all jobs green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.115, stale SW cleared first):** DOM version == `/api/version`. Two synthetic customers (`prod-verification` recipe, `allow_self_entry=true`): no-active-pass account's `/my/balance` shows `card-pass` text "Nemas aktivnu mesacnu permanentku" (no "preplatok") AND the door button reads "Otvorit dvere - drz 1s"; active-pass account (real `charge`/`monthly_pass` transaction inserted, `valid_until` 30d out) shows "Mesacna permanentka platna do 05.09." — both states confirmed correct, 0 console errors on either navigation. Both synthetic users + their transaction hard-deleted after. The 1s-vs-2s HOLD TIMING behavior itself is proven by CI's real-Chromium E2E suite (`door-open.spec.ts`), which ran against this exact merged code and passed — not independently re-driven live (same precedent as the #247/#248 entry above).

## 2026-08-06 — #260 + #266: request logging + launch beacon, door quick-tap guard test (bundled batch, dev.116)

- **#260 (diag):** the server logged zero request paths/User-Agents — round 4 of the iOS install auto-login fix (#258) failed and was undiagnosable (three hypotheses fit the same two-line evidence identically). Added:
  - `spinbike_core::redact::redact_query` (`crates/spinbike-core/src/redact.rs`) — new shared pure fn, used by BOTH server and frontend, truncating `t=`/`it=`/`code=` query VALUES to 8 chars + `...`; everything else passes through unchanged. Traced the real token generators (`db/login_tokens.rs`'s base64 `URL_SAFE_NO_PAD` raw token, the 6-digit login code) — neither ever emits a literal `&`/`=`, so the raw-string split (no percent-decoding, no query parser) is exact for the real shape.
  - `spinbike_server::request_log::middleware` (`crates/spinbike-server/src/request_log.rs`) — an `axum::middleware::from_fn`, deliberately NOT `tower_http::TraceLayer` (its default span logs the raw un-redacted URI). Wired into `build_router` UNDER `CatchPanicLayer` (added last → stays outermost, so a panic anywhere including inside the logger is still caught). Logs method/path/redacted-query/status/User-Agent; never bodies.
  - `POST /api/metrics/launch` (`crates/spinbike-server/src/routes/metrics.rs`) — new unauthenticated route, rate-limited per client IP. **Gotcha:** the app sits directly behind a Cloudflare tunnel straight to `127.0.0.1:8080` (no local nginx/caddy, no `ConnectInfo` wiring) — the TCP peer address is never the real client, so the key is derived from `Cf-Connecting-Ip` (set by Cloudflare on every proxied request) falling back to the first `X-Forwarded-For` hop, then a shared `"unknown"` bucket. New `AppState.launch_rate_limit` field — **4 construction sites** needed updating: `lib.rs` ×3 (start_server + 2 test blocks), `routes/version.rs`, `tests/helpers/mod.rs`.
  - Frontend beacon (`spinbike-ui/src/launch_beacon.rs`), fired from `main()` on boot when `platform::is_standalone()` (reused existing helper) — fire-and-forget POST carrying path/query_redacted/had_token/had_session/src.
  - **Review-fix (same PR, before merge):** reviewer flagged the `launch` handler trusted the client's own `query_redacted` field verbatim with no server-side re-application — a real gap since the endpoint is public/unauthenticated. Fixed by calling `redact_query()` again server-side before logging (idempotent on already-redacted text, catches a regressed/hand-crafted client). **Verified LIVE**: sent a raw unredacted `t=FULLUNREDACTEDTOKENVALUEZZZZ999888` value in the beacon body against prod — journal shows `query=t=FULLUNRE...`, proving the defense actually fires in production, not just in unit tests.
- **#266 (test):** the door-hold guard (`HOLD_DURATION_MS=1000.0` in `door_button.rs`) had NO test proving a quick tap does NOT open the door — every existing case in `door-open.spec.ts` holds 1200ms. New case: pointerdown → wait 300ms (well under threshold) → pointerup, asserts via `page.on('request')` that zero `/api/door/open` POSTs fired, no banner, progress reset to `0%`; the SAME test then holds past the threshold right after to prove the boundary is real (release-just-before does nothing, hold-just-past opens), not that the button is simply broken.
  - **Local-build-ban gotcha:** could not empirically confirm "lowering `HOLD_DURATION_MS` toward 0 makes the test fail" — this project bans local `trunk build`/`cargo test`, and Playwright only runs via the self-hosted CI runner. Traced `on_pointer_down`'s async tick loop by hand instead (polls every 16ms, fires `post_door_open()` once `elapsed >= HOLD_DURATION_MS` independent of whether `pointerup` already happened) — a genuinely deterministic, non-flaky structural proof, independently re-verified by the review subagent.
- **Secret-scanner gotcha:** the redaction tests' synthetic long alphanumeric test-token literals (`"abcdefghijklmnopqrstuvwxyz0123456789"` etc.) trip `block-sensitive-staging.sh`'s high-entropy heuristic even though they're obviously not real credentials — bypassed with a **trailing shell comment on the actual `git commit` command line** (`# airuleset:secret-ok <reason>`), NOT inside the commit message heredoc — the hook reads the raw Bash tool_input, not the resulting git log.
- **Design-gate gotcha:** `block-commit-without-design.sh` resolves the "ticket" needing a design comment from the CURRENT branch's OPEN PR NUMBER (not just `#N` refs in the commit message text) — a review-driven follow-up commit made mid-PR needed its own `gh issue comment <PR-number>` (PRs and issues share GitHub's numbering; `gh issue comment` works fine against a PR number) even though the commit message itself only mentioned the original issue (#260).
- **Commits (dev):** `da255e7` (#260 impl+tests) → `e5c9bdf` (#266 test) → `59d52e8` (review-fix: server-side re-redaction). PR [#269](https://github.com/zbynekdrlik/spinbike/pull/269) (Closes #260, #266), merged `596746a`. Review subagent (`Explore`, independent, against the full diff): 1 Important (fixed), 2 Minor (acknowledged, non-blocking — panic-path requests get no request-log line since the log call is only reached after `next.run()` returns normally; the `Cf-Connecting-Ip` trust is an operational assumption this diff can't itself enforce).
- **Main CI run:** [31086117169](https://github.com/zbynekdrlik/spinbike/actions/runs/31086117169) — all jobs green incl. Deploy (prod) + Smoke (prod). No version bump needed (dev was already at `0.15.0-dev.116`, strictly ahead of main's `0.15.0-dev.115`, from two prior unpushed commits riding this push by design).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.116, DOM `Verzia aplikacie` == `/api/version`):** via local origin `http://127.0.0.1:8080` (Cloudflare 1010 blocks scripted API on the public host): `GET /manifest.json?it=THISISALONGTESTTOKENVALUE...` → journal line `path=/manifest.json query=it=THISISAL... status=200 user_agent=...` (full token absent, only 8-char prefix); `POST /api/metrics/launch` → 204, journal `standalone launch beacon path=... query=... had_token=true ...`; the server-side re-redaction re-verification above. 0 console errors on `/login`.

## 2026-08-06 — #267: version label out of the floating overlay into normal flow (solo, dev.117)

- **The complaint (user, verbatim intent):** "na pwa appke je napravo dole hnusny sivy obdlznik ktory prepisuje appku, podla vsetkeho si tak nejak umiestnil verziu ale to je najhnusnejsia vec co som kedy videl". Measured before the fix on a 390x844 phone viewport: `.app-version` was `position: fixed; bottom: 60px; right: 12px; z-index: 50; background: rgba(0,0,0,0.35); border-radius: 4px; pointer-events: none`, rect `x=282 y=762 w=96 h=22` — a floating chip painted OVER page content.
- **Fix (CSS only, `spinbike-ui/style.css`):** dropped `position: fixed` / `background` / `z-index` / `border-radius` / `pointer-events` from `.app-version`; it is now a plain centered muted-text block in normal document flow, last child of `.app-shell` after `.page`, so it lands below page content and above the reserved mobile bottom-nav padding.
- **Contract preserved deliberately:** `data-testid="version"` and the `v<semver>` text are load-bearing for post-deploy verification (`version-on-dashboard.md`) — both unchanged; only the presentation moved.
- **Regression guard:** `e2e/tests/version-display.spec.ts` now asserts computed `position !== 'fixed'` and a transparent background, so re-introducing the overlay fails CI rather than shipping.
- **Commits (dev):** `a445e3e` (CSS + E2E guard) → `05df460` (version bump 0.15.0-dev.116 → dev.117 via `scripts/sync-version.sh`), merged `origin/main` at `fd0f65f`. PR [#270](https://github.com/zbynekdrlik/spinbike/pull/270) (Closes #267), merged `ac06a1f`.
- **Main CI run:** [31088519890](https://github.com/zbynekdrlik/spinbike/actions/runs/31088519890) — all jobs green incl. Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` at a 390x844 phone viewport (stale service worker + `spinbike-v3` cache cleared first, per the `frontend-pwa` gotcha):** on the logged-out `/login` AND on a logged-in customer `/my/balance` (synthetic account via the `prod-verification` recipe, deleted right after), `[data-testid="version"]` reads `v0.15.0-dev.117` with computed `position: static`, `background: rgba(0,0,0,0)`, `z-index: auto`; rect `x=0 y=758 w=390 h=30` full-width in flow; `elementFromPoint` at its center returns the version div itself; zero overlap against any `.card` / nav element; no horizontal scroll (`scrollWidth == clientWidth == 390`); 0 console errors/warnings. The same page also re-confirms #263 still live — the pass card reads "Nemas aktivnu mesacnu permanentku", no "preplatok".
- **Verification gotcha worth repeating:** the harness treats a bare `sleep`-free foreground `gh run view` repeat as a CI-poll violation after loop 1 — the long main-CI wait must go through ONE `run_in_background: true` waiter, and a background waiter the harness did not register (empty `background_tasks`) will never wake the session, so re-derive from `gh run view` on the next turn instead of trusting silence.

## 2026-08-06 — #268: stale-session (deleted/blocked user) 404 -> 401 on /api/my/balance (solo, dev.120)

- **Bug (found during #258's own live-prod verification):** a still-valid, unexpired JWT for a hard-deleted/soft-deleted/blocked user got 404 from `GET /api/my/balance` — left the customer on a broken `/my/balance` page instead of being logged out. Reproducible in prod any time the CEO deletes a customer whose phone still holds a live session (customer JWTs are permanent by design, #108).
- **Root cause:** `my_balance`'s query filtered `deleted_at IS NULL` (soft-deleted rows silently fell into the `None`/404 branch, indistinguishable from a genuinely missing row) and never checked `blocked` at all (a blocked user's request quietly succeeded, 200). 404 means "resource missing"; the truth was "session invalid" — the client's existing generic `api::get_coded`/`handle_unauthorized` (`spinbike-ui/src/api.rs`) only clears-and-redirects on 401.
- **Fix:** query now also selects `blocked`/`deleted_at` (drops the `deleted_at IS NULL` filter so a soft-deleted row is fetched, not silently merged into "missing"); all three states -> `ApiError::Unauthorized(ErrorCode::SessionInvalid)` (401), mirroring the exact `deleted_at.is_some() || blocked` gate `token_login` already applies (`routes/auth.rs`). New `ErrorCode::SessionInvalid` in `spinbike-core::errors`.
- **No new client logic needed** — `api::get_coded`'s `handle_unauthorized` already clears `spinbike_token`/`spinbike_user` and redirects to `/login` on any 401 while a token is held (covers both `/my/balance` and `/door`, which reads the same endpoint). The login-loop trap the ticket warned about (a failed login attempt's own 401 must not clear-and-redirect from the login page) was already structurally avoided: `login.rs` submits via `api::post_public_coded`, which never calls `handle_unauthorized`.
- **Clippy gotcha:** the 7-tuple query type (`i64, String, f64, Option<String>, i64, i64, Option<String>`) tripped `clippy::type_complexity` under `-D warnings` — fixed with a named `type UserBalanceRow = (...)` alias, no behavior change, own commit.
- **RED->GREEN commit order:** `a81839c` [red] (3 new `users_routes.rs` integration tests — missing/soft-deleted/blocked, each isolating ONE condition so an `||`->`&&` mutant is independently killable — + new `e2e/tests/session-invalidation.spec.ts`) -> `ee1d2a3` [green] (the fix) -> `8b21051` (clippy type-alias polish). Verified deterministically RED against the pre-fix code by reading `git show a81839c^:...my_balance.rs` (old `deleted_at IS NULL` filter + no `blocked` check + 404-on-`None`) during the deep review pass — local `cargo test` is banned in this repo (Tier-0, CI-only); `cargo test --no-run`/`cargo check`/`cargo clippy --workspace -- -D warnings` all ran locally pre-push and were clean.
- **Two independent review passes** (both `Explore` subagents, security-boundary ticket = max scrutiny): pass 1 found 0 🔴 0 🟡 1 🔵 — `door.rs`'s own independent user-lookup has the SAME 403-vs-401 bug for a missing/soft-deleted user, but that file isn't touched by this diff, so filed as a follow-up rather than folded in: [#274](https://github.com/zbynekdrlik/spinbike/issues/274) (fix(auth): /api/door/open returns 403 instead of 401 for missing/soft-deleted user). Pass 2 (deep, `requesting-code-review`-equivalent) hand-traced 7 specific hypotheses (NULL-decode panic on `blocked` — schema is `NOT NULL DEFAULT 0`, safe; TOCTOU; `||`-vs-`&&` mutant survival — independently re-derived as killed by the two single-condition tests; i18n.rs exhaustive-match completeness; the `ErrorCode::ALL` wire-format test table; warn-log leak into the HTTP body — confirmed none; RED/GREEN ordering) and found 0 🔴 0 🟡 0 🔵.
- **Commits (dev):** `a985b2c` (bump 0.15.0-dev.119 -> dev.120) -> `a81839c` [red] -> `ee1d2a3` [green] -> `8b21051` (clippy polish). PR [#273](https://github.com/zbynekdrlik/spinbike/pull/273) (Closes #268), merged `e2c21c9`. Main CI green incl. E2E, all 8 mutation-testing shards, Deploy (prod) + Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.120, DOM matches; stale SW/cache cleared first) at 390x844:** synthetic customer (id 609) minted a JWT, confirmed 200 via local-origin curl, then soft-deleted (`deleted_at` set) -> local-origin curl now 401 `session_invalid`. Injected the SAME stale token into a fresh browser's `localStorage`, reloaded -> landed on `/login` ("Prihlasenie" heading) with `spinbike_token`/`spinbike_user` both `null`, **0 console errors/warnings for that navigation** (scoped console check, not the whole session). Separately confirmed the anti-loop requirement: submitted a deliberately wrong password on `/login` -> inline Slovak error "Nespravny email alebo heslo", stayed on `/login`, no clear-and-redirect. Synthetic user hard-deleted after (0 rows remaining); scratch JWT-secret/token files deleted.

## 2026-08-06 — #276: password login mints a token for a blocked/soft-deleted user, bounce loop (solo, dev.122)

- **Bug (found by #268's own review pass, filed as a follow-up):** `login()` (password path, `routes/auth.rs`) had NO `blocked`/`deleted_at` gate at all, unlike `token_login`/`code_login`. Since #268 made `/api/my/balance` 401 for such a session, a blocked customer entering the CORRECT password got 200 + a valid token, redirected to `/my/balance`, got bounced back to `/login` by the 401 -> forever, showing a false "session expired" message instead of telling them they're blocked.
- **Root cause / fix:** `get_user_by_email` filters `deleted_at IS NULL` at the SQL level, so a soft-deleted account was already unreachable via `login()` (fell into a generic "not found" -> `InvalidCredentials`) — only `blocked` was the live gap. Fix switches `login()`'s lookup to the already-existing `get_user_by_email_including_deleted` (used by #143's email-collision flow; the `email` UNIQUE constraint guarantees at most one row ever matches) so BOTH conditions are rejected explicitly, at the login call itself, via a new shared `account_is_active(&UserRow)` helper — rather than leaning on the query filter as an implicit side effect (which would also have left the check unreachable/unkillable by mutation testing). New `ErrorCode::AccountBlocked` (401, `account_blocked`), returned ONLY after `verify_password` succeeds, so it adds no enumeration surface beyond the endpoint's pre-existing `OauthAccount` leak. `token_login`/`code_login` deliberately left untouched — they keep their own generic "invalid" rejection by design (proof-of-possession-first). New `err_account_blocked` i18n key, unaccented Slovak "Tvoj ucet je zablokovany, ozvi sa nam."
- **RED->GREEN commit order:** `d523d35` [red] (3 new `auth_routes.rs` tests — blocked/deleted/active, each isolating one condition) -> `9bfebff` [green] (the fix). Two CI-driven E2E follow-up fixes (`b2babf0`, `f5ecf2a`) — the first attempt asserted the Slovak text but CI caught it rendering English, because `loginViaAPI`'s `setEnglishLanguage` registers a `page.addInitScript` that persists for the whole page lifetime and re-fires English on every later navigation (clearing `spinbike_lang` via `page.evaluate` doesn't survive the next `page.goto`); fixed by using a raw admin-login `fetch` instead (same pattern as `session-invalidation.spec.ts`), never touching the page at all.
- **Deep review pass** (`Explore` subagent, security-boundary ticket = max scrutiny): 8 hypotheses hand-traced (enumeration/leakage across every response the endpoint can return; `get_user_by_email_including_deleted` email-uniqueness safety via the `#143` free-email flow; `account_is_active`'s `&&`/negation mutation coverage; confirming `token_login`/`code_login` were genuinely untouched; `ErrorCode` exhaustive-match completeness in both `errors.rs` and `i18n.rs`; zero side-effects on the rejected path; RED/GREEN ordering) — found 0 🔴 0 🟡 1 🔵 (a pre-existing, out-of-diff observation: the `ErrorCode::ALL` test table is hand-maintained, not derived from the enum).
- **Commits (dev):** `3002374` (bump 0.15.0-dev.121 -> dev.122) -> `d523d35` [red] -> `9bfebff` [green] -> `b2babf0` + `f5ecf2a` (E2E locale-persistence fixes). PR [#279](https://github.com/zbynekdrlik/spinbike/pull/279) (Closes #276), merged `4085ccc`. Main CI green incl. E2E, Deploy (prod), Smoke (prod) — one transient GitHub Actions infra failure on `Smoke (prod)`'s action-download step ("Service Unavailable"), resolved by a single job rerun with no code change.
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.122, DOM matches; stale SW/cache cleared first) at 390x844:** synthetic blocked customer (id 611, argon2id password hash minted via the `argon2` CLI) — password login shows "Tvoj ucet je zablokovany, ozvi sa nam." inline on `/login`, stays on `/login` (no bounce; the only console entry is the browser's own benign "server responded with 401" network log, the exact pattern `e2e/tests/helpers.ts`'s `setupConsoleCheck` already filters). Separately, a fresh ACTIVE synthetic customer (id 612) logged in and reached `/my/balance`, 0 console errors/warnings — confirms the new gate doesn't affect normal accounts. Both synthetic users deleted after (0 rows remaining); scratch hash/secret files deleted.

## 2026-08-07 — #277: booking/my-bookings/self-edit endpoints accepted a dead session (solo, dev.124)

- **Bug (found by #274's own playbook-review sweep, filed as #281 then merged into #277 with cancel_booking added to scope):** `create_booking`/`cancel_booking`/`my_bookings` (`classes.rs`) had NO caller-session check at all — a blocked/soft-deleted customer with a still-valid permanent JWT could keep booking/cancelling classes and listing their own bookings indefinitely; a bogus/hard-deleted `sub` hit a bare FK-violation 500 on `create_booking` instead of any typed error. `update_user` self-edit (`users.rs`) checked `deleted_at` but returned 404 `user_not_found` (the exact wrong shape #268 already ruled out) and never checked `blocked` at all.
- **STEP 0 confirmed live on prod before any code** (`gh issue comment` on #277): matrix of live/blocked/soft-deleted/ghost tokens x all 4 endpoints on `http://127.0.0.1:8080` — exactly as described, plus the undocumented ghost-sub 500 on create_booking.
- **Fix:** new shared `auth::require_live_session(pool, user_id) -> Result<(), ApiError>` (`crates/spinbike-server/src/auth/mod.rs`), called first-thing in all four handlers -> `401 SessionInvalid` for missing/soft-deleted/blocked, same contract as #268/#274. `door.rs`/`my_balance.rs` deliberately left on their own pre-existing inline checks (unmigrated, zero behavior change needed there).
- **Design decision (recorded on the issue before the first commit):** shared helper, NOT the `AuthUser` extractor — the extractor would apply the check for free to ~17 handler call sites (payments/admin/user-management included), a much bigger blast radius than this ticket's named 4; cost of the extra SELECT is negligible either way (single SQLite file), scope discipline was the deciding factor. `.claude/rules/session-invalidation.md` updated with the resolved decision.
- **RED->GREEN commit order:** `719a1ff` [red] (15 new integration tests across `classes_routes.rs`/`users_routes.rs`, each isolating one dead-session shape per endpoint + a live-customer positive control, + the new `#277` E2E in `session-invalidation.spec.ts`) -> `467430e` [green] (the fix + rules-doc update).
- **Two independent review passes both flagged real findings, fixed in a 3rd commit `2b18932` before merge:** the E2E's week-date computation round-tripped through `toISOString()` (UTC) after deriving weekday from LOCAL time — the exact anti-pattern `helpers.ts` already warns against (#251) — fixed to format from local components throughout; a vacuous "no booking created" assertion (`?? 0` could compare `0` to `0`); `update_user`'s field-level audit `tracing::info!` was reordered to run BEFORE the new session check so a rejected self-edit attempt still leaves a full audit trail; a stale `#281` cross-reference fixed to `#277`; and the design comment's claim about `cancel_booking`'s check ordering was corrected via a follow-up issue comment (it said ownership-check-first, the code and tests correctly do session-check-first — the shipped behavior is the better one, only the comment was wrong).
- **One unrelated CI flake investigated, not band-aided:** `install-prompt.spec.ts`'s #261 round-5 mint-guard test failed twice in a row with `mintCount: 2` — confirmed via code (`install_prompt.rs`'s own doc comment describes a check-then-act race between `Effect` firings) and via history (other unrelated E2E flakes exist in this repo's CI log) to be pre-existing and unrelated to this diff (which touches no PWA/install-token code); filed as [#282](https://github.com/zbynekdrlik/spinbike/issues/282) with the root-cause hypothesis, third CI run green.
- **Follow-up filed:** [#284](https://github.com/zbynekdrlik/spinbike/issues/284) — both review passes independently found `user_transactions`'s self-view branch and `install_token` minting still accept a dead session (same gap class, two more `AuthUser` sites beyond #277's named four); also gives the rules doc's "future cleanup pass" (consolidating `door.rs`/`my_balance.rs` onto the new helper) a real tracking number.
- **Commits (dev):** `719a1ff` [red] -> `467430e` [green] -> `2b18932` (review-fix commit). PR [#283](https://github.com/zbynekdrlik/spinbike/pull/283) (Closes #277), merged `ec00882`. Main CI run [31161848185](https://github.com/zbynekdrlik/spinbike/actions/runs/31161848185) green incl. E2E, Deploy (prod), Smoke (prod).
- **Verified LIVE on `https://spinbike.sk` (v0.15.0-dev.124, DOM + `/api/version` match; stale SW/cache cleared first) at 390x844:** re-ran the STEP-0 matrix against the deployed build via a fresh synthetic live/blocked/deleted/ghost quartet — all 4 endpoints now 401 `session_invalid` for blocked/deleted/ghost, live customer unaffected on all 4 (booked+cancelled a real occurrence, self-edited their phone number). Playwright functional check: injected a blocked synthetic customer's session into `localStorage`, clicked "Rezervovat" on `/schedule` -> redirected to `/login`, `spinbike_token`/`spinbike_user` both cleared, 0 console errors. All synthetic rows/bookings deleted after (0 remaining); scratch JWT-secret/token files deleted.

## 2026-08-08 — #299: spawn_daily_job helper — resumed mid-flight, mutation-gap fix + review round (dev.135/dev.136)

- **Resumed a previous worker's in-flight PR #300** (already open, MERGEABLE/UNSTABLE) rather than restarting — the extraction itself (`f96a361`), a first mutation-gap fix (`20ba4bb`/`7e5719f`/`8865732`, whole-function-body mutant), version bump (`d75eff6`) and the design/validated comments on #299 were already done and left untouched.
- **CI run 31232550014 (head `8865732`) had 5 failing mutation shards, all pointing at the SAME line** — `crates/spinbike-server/src/jobs/mod.rs:64`, `Ok(n) if n > 0 => tracing::info!(...)` mutated to `true`/`n < 0`/`n == 0`/`false`/`n >= 0`. Root cause: the existing regression test always returned `Ok(0)` from `tick`, so the guard's actual outcome (log vs no-log) was never distinguished by any assertion.
- **Fix (`999c1fd`):** added `capture_tracing_output`, an in-memory `tracing::Subscriber` via `tracing-subscriber`'s already-present `fmt`/`MakeWriter` over `Arc<Mutex<Vec<u8>>>` (no new dependency), plus two tests — `tick` returning `Ok(5)` (asserts the info log fires) and `Ok(0)` (asserts it does not). Design rationale posted to #299 BEFORE the commit.
- **Two independent review passes** (`/code-review high` + a deep `requesting-code-review` subagent, both independently re-derived the mutant-by-mutant kill table and confirmed it correct) converged on the same two real findings, fixed in `4bf7683`: a stale doc comment (in `jobs/mod.rs` AND `.claude/rules/daily-job-scheduling.md`) still described the earlier-tried-and-reverted `#[tokio::test(start_paused = true)]` mechanism instead of the actual manual `tokio::time::pause()`-after-pool-creation fix; and the shared success-log line lost its unit noun (`"{job_name}: {n}"` instead of the old jobs' bespoke `"{n} rows removed"`/`"sent {n} notifications"` — a `comprehensive-logging.md` violation), fixed by adding a `unit: &'static str` parameter used only in the success line. Also extracted `spawn_and_drive_one_tick`, a shared test helper for the pool/pause/spawn/yield/advance/yield dance all three tests had independently duplicated (a third review finding). Two other suggestions (switch to the `tracing-test` crate; skip `run_migrations` in tests that don't touch the DB) were evaluated and declined with reasoning, posted to #299.
- **One CI-caught compile error mid-cycle (`7a450f7`):** `spawn_and_drive_one_tick` lives inside `mod tests`, which never imports `sqlx::SqlitePool` — a private `use` at the outer module's top isn't visible by bare name to a child module without its own `use`/fully-qualified reference. Caught by the `Lint` CI job (`E0425`), not by local `cargo fmt --all --check` (Tier-0 policy forbids local `cargo build`/`test`). Fixed by fully-qualifying `sqlx::SqlitePool`.
- **Commits (dev):** `999c1fd` (mutation-gap fix) -> `4bf7683` (review-fix) -> `7a450f7` (compile fix). PR [#300](https://github.com/zbynekdrlik/spinbike/pull/300) (Closes #299), merged `456b4f5`. Main CI green incl. E2E, Deploy (prod), Smoke (prod). Deployed + verified live on `https://spinbike.sk` (v0.15.0-dev.135, DOM+backend match, stale SW/cache cleared first, 0 console errors); confirmed both daily jobs' startup ticks ran without crashing (`notifications::tick`'s own internal `"push: daily tick complete sent=0"` log fired at boot) — the DEBUG-level "sleeping until next aligned run" line is filtered out at prod's `spinbike=info` log level, unchanged behavior from before the refactor.
- **Playbook-only follow-up (`3480ae1` bump to dev.136 + `e62132c` docs), PR [#301](https://github.com/zbynekdrlik/spinbike/pull/301) (no issue, pure playbook maintenance), merged `8b5bf8f`:** captured the `mod tests`-scoping gotcha and "a log-gating branch needs tracing-output capture, not just a call-counter test, to be mutation-testable" as reusable rules in `.claude/rules/daily-job-scheduling.md`. Self-reviewed (diff is doc + mechanical version bump only, zero logic changes; content already validated by both review passes above). Deployed + verified live (v0.15.0-dev.136, `/api/version` + `systemctl is-active` both confirm).
