# Autopilot Log

Terse per-issue log of autonomous work cycles: issue #, commit SHAs, RED→GREEN
test names, decisions, and the shared PR #. Newest entries at the top.

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
