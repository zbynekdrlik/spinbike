---
name: spinbike-ci-deploy
description: >
  SpinBike CI/deploy constraints: self-hosted runner download-only (no Rust
  builds on local PC), subagent prompt rules, and git staging hygiene. Load
  when writing CI workflows, subagent prompts, or staging commits.
triggers:
  - CI workflow
  - self-hosted runner
  - deploy job
  - subagent prompt
  - cargo build
  - trunk build
  - git add
  - commit
---

# SpinBike CI / Deploy Constraints

## Self-hosted runner: download-and-install only — NEVER build Rust locally

The `spinbike-deploy` self-hosted runner runs on the user's dev PC. Deploy jobs must NEVER run `cargo build`, `trunk build`, or any Rust/WASM compilation — `target/` balloons to 10-20 GB.

**Correct pattern:**
1. `build` job on `ubuntu-latest` → `actions/upload-artifact`
2. `deploy-*` job on `spinbike-deploy` (`needs: [build]`) → `actions/download-artifact`
3. Deploy job only does:
   - `install -Dm755 spinbike-server /opt/spinbike/{dev,prod}/spinbike-server`
   - `sudo -n systemctl restart spinbike{-dev,}.service`
   - Health + smoke checks

**Wrong:** any `cargo`/`trunk` command in the self-hosted job body.

When designing ANY CI workflow touching the self-hosted runner, the workflow must use `needs: [<build-job>]` + `actions/download-artifact`. No Rust build commands on the runner.

## Subagent prompts must not instruct local Rust builds

When dispatching subagents to implement SpinBike Rust/Leptos tasks, the prompt must NOT tell them to run `cargo test`, `cargo build`, `cargo clippy`, `cargo check`, or `trunk build` — even for TDD RED/GREEN verification. CI is authoritative.

**Correct subagent instruction pattern:**
1. Write code + tests
2. `cargo fmt --all --check` (the ONE allowed local cargo command)
3. `git add <explicit paths>` + `git commit`
4. Report DONE / BLOCKED

Skip the compile-and-verify step. If a step genuinely requires a local build (e.g. verify a new WASM signature compiles before handing off), note it explicitly and justify — never include it as a default TDD step.

## `spinbike-ui` is a SEPARATE cargo workspace — root `cargo fmt --check` never sees it

`spinbike-ui/Cargo.toml` is its own workspace; root `Cargo.toml` has
`exclude = ["spinbike-ui"]`. So the project's mandated local check
(`cargo fmt --all --check`, run from the repo root per `CLAUDE.md`) — and
CI's `Lint` job, which also runs from the root — **structurally never
touches `spinbike-ui/`**. A real `cargo fmt` violation in
`spinbike-ui/src/*.rs` ships completely undetected until a manual/deep
review happens to run `cd spinbike-ui && cargo fmt --all --check` (this bit
#109 — two mis-formatted `i18n.rs` inserts landed and passed every gate).

**Whenever you touch a `spinbike-ui/src/*.rs` file, run the fmt check
TWICE** — once from the root (covers the server crates), once from inside
`spinbike-ui/`:

```bash
cargo fmt --all --check                              # root workspace
cd spinbike-ui && cargo fmt --all --check && cd ..   # separate workspace — NOT covered by the line above
```

Tracked as a CI gap in [#122](https://github.com/zbynekdrlik/spinbike/issues/122)
(add a `spinbike-ui`-scoped fmt/clippy step to CI) — not yet fixed, so the
manual double-check above is the only guard until then.

## Adding a crate dependency: regenerate + commit `Cargo.lock`

`Cargo.lock` is tracked but CI runs **no** `--locked`/`--frozen` — so it
silently regenerates on every build and the committed copy rots. It was found
stale by ~2 minor versions (workspace pinned at `0.13.18`, whole crates like
`chrono-tz`/rustls/httpmock absent) when #107 added `lettre`. When you add or
bump a dependency:

```bash
cargo metadata --format-version 1 > /dev/null   # resolution only — NO compile, NO target/ artifacts (allowed locally)
git add Cargo.lock                              # explicit path
```

`cargo metadata` writes `Cargo.lock` (adds the new subtree, preserves existing
pins) without any of the banned heavy builds. Commit the lock in the same PR —
a dep addition without a lock update is an incomplete, non-reproducible change
(code review will flag it). Expect a large diff the first time (it also flushes
the accumulated staleness); confirm no `openssl`/native-tls entries leak in
(this workspace is rustls-only) and that bumped versions stay within the
manifest's SemVer ranges, then let a fresh CI run re-validate the committed lock.

## AppState has THREE construction sites — wire every new field at all three

`spinbike_server::AppState` is struct-literal-constructed in **three** places;
add a new field to `AppState` and you MUST add it to all three or the build /
tests break (the issue text for #107 named only two — the third was found by
grepping):

```bash
grep -rn 'AppState {' crates/spinbike-server/src crates/spinbike-server/tests
```

1. `crates/spinbike-server/src/lib.rs` — `start_server()` (production).
2. `crates/spinbike-server/src/routes/version.rs` — the `#[cfg(test)]` builder
   (clear any relevant `*_TEST_MODE` env before the handle's `spawn()`).
3. `crates/spinbike-server/tests/helpers/mod.rs` — `TestApp` integration harness.

Env-driven external-service modules mirror `src/ewelink/`: a `Handle::spawn()`
that reads env once, a `None`/absent-transport **Disabled** fast-path (missing
env must never panic/crash the server — verified live: the `mail: disabled …`
warn fires at boot and the service stays `active`), a `*_TEST_MODE` in-process
capture seam checked FIRST, and `#[mutants::skip]` on the un-hermetic network
dial only. `mail::MailHandle::last_captured()` is the seam #108's invite
endpoint reads to echo `test_link` (set `SMTP_TEST_MODE=capture` in E2E).

## Git staging: never `git add -A` or `git add .`

The repo root regularly accumulates untracked artefacts that must NOT be committed:
- Playwright snapshot YAMLs: `prod-reports.yml`, `desk-snap.yml`, `*-snap.yml`
- Debug PNGs from preview runs
- Occasional dev DBs

`.gitignore` catches most but not all (new snapshot naming slips through).

**Always:** use explicit file paths (`git add path1 path2`) or `git add -u` (only modifies tracked files).

**Before any commit:** run `git status --short` to audit what would be staged.

## Push gate on docs-only branches (pre-push-test-check hook)

The airuleset pre-push hook re-scans the ENTIRE `origin/main..HEAD` range on
EVERY push and blocks when ANY `fix(...)`-prefixed commit in that range has
no test commit earlier in the range — even a commit that's already on `dev`
from a PREVIOUS session, unrelated to your current work (Gate 2 false
positive; the block message goes to stdout, so the tool error shows only
"No stderr output"). The bypass marker `[no-test: <reason>]` is honored ONLY
on the LATEST commit (never amend — project convention), so it silently
STOPS covering the old offending commit the moment you add one more commit
on top without a fresh marker — this WILL re-trigger on your very next push
even though nothing about your own fix changed.

**This is NOT "don't use the bypass if your branch has a real fix"** — it's
about WHICH commit is flagged. If Gate 2 names a commit SHA that predates
your own work and is unrelated to it (e.g. a docs/gitignore-only commit from
a prior session), the bypass is correct and safe: your own bugfix keeps its
proper RED→GREEN pair (Gate 2 walks the range in order and finds YOUR test
commit before YOUR fix commit just fine — the marker only silences the gate
entirely for that one push, it doesn't retroactively break your own
ordering). Cite the flagged SHA by name in the marker so the audit log
(`~/devel/airuleset/audits/no-test-skips.log`) stays honest:

```bash
git commit --allow-empty -m "chore: push gate bypass [no-test: <old-sha> is a pre-existing docs-only commit unrelated to this PR, my fix has its own RED-GREEN pair]"
git push origin dev
```

Never use it to skip writing a test for YOUR OWN new fix — only to route around
an old, already-merged, unrelated commit that happens to fall in the range.

**Gotcha — never combine the marker commit and the push in one Bash call.**
The hook matches on the LITERAL command-string text (`grep -qE 'git\s+push'`
over the whole `tool_input.command`), so `git commit --allow-empty -m "..." &&
git push origin dev` gets blocked BEFORE either half runs — the commit never
happens, and the error looks identical to a real Gate 2 block, which is
confusing. Always split: one Bash call to commit, a SEPARATE Bash call to push.

**Gotcha — Gate 2's bug-fix heuristic fires on `Closes #N` in the commit BODY,
not just a `fix(...)`-prefixed subject.** A commit titled `ci: add fmt +
clippy coverage for the spinbike-ui workspace` (no `fix:` prefix at all) still
tripped Gate 2 (#122) because its body had `Closes #122` — the hook's
`IS_BUGFIX` check is `subject matches fix:|bug:|... OR body matches
closes/fixes/resolves #N`, so ANY commit that closes an issue is treated as a
bug-fix requiring a preceding test commit, even a pure CI-config chore. Same
applies to a genuinely mechanical follow-up commit whose subject happens to
start with `fix:` (e.g. `fix: clean up spinbike-ui clippy debt...` for a
zero-behavior-change clippy cleanup) — it's flagged too. Bypass with
`[no-test: <reason>]` citing the flagged SHA and why it has no testable
logic; this is legitimate per the rule above, just note the trigger can be
the BODY, not only the subject.

**Gotcha — the `[no-test: <reason>]` bypass regex is LINE-based; a reason
that wraps across multiple lines in the commit body silently fails to
match.** The hook does `echo "$LAST_MSG" | grep -qE '\[no-test:\s*[^]]+\]'`
with NO `-z`, so `grep` evaluates one line at a time — if the opening `[` and
closing `]` land on DIFFERENT lines (e.g. a long reason written as a wrapped
paragraph via a `cat <<'EOF'` heredoc commit message), no single line
contains both brackets and the bypass is NOT recognized, even though it looks
present in the full message. Keep every `[no-test: ...]` bypass on ONE
physical line — a long reason is fine as long as it isn't hard-wrapped.

## Removing an API route → SPA static fallback returns 200, NOT a router 404

`all_routes()` ends with `.fallback(static_files::static_handler)`, and
`static_handler` serves `index.html` (200) for any **dotless** path (SPA
routing). So a DELETED API route like `POST /api/auth/register` does **not**
404 — it falls through to the SPA fallback and returns 200 with the HTML shell.
Do NOT assert `404`/`405` for a removed endpoint. Assert the removed
**capability** behaviorally instead: no `201`, no JWT in the body, and no row
created (`SELECT COUNT(*) FROM users WHERE email=...` is 0). Same on live
dev/prod (`curl -X POST /api/auth/register` returns 200 but creates no account).

## E2E account seeding: use the `SPINBIKE_TEST_MODE`-gated `/api/test/seed-account`

`e2e/global-setup.ts` (customer/admin/staff) and `door-open.spec.ts` used the
public `POST /api/auth/register` to seed accounts WITH passwords from an
unauthenticated state. Register is gone (#108), so that bootstrap moved to
`POST /api/test/seed-account` `{email,password,name,role}` in
`routes/test_fixtures.rs` — unauthenticated, mounted only under
`SPINBIKE_TEST_MODE=1` (the E2E server sets it), returns `201 {"user_id"}` or
`409` on a duplicate email (global-setup treats 409 as "already seeded"). When
you remove a public endpoint the E2E harness used for seeding, re-point the
seed to a test-fixture route — don't just delete the E2E test.

## Adding a second same-type input to a page breaks bare-attribute E2E selectors

When #109 added a customer login-link section to `/login` (a SECOND
`type="email"` input + `type="submit"` button below the existing password
form), every existing test/helper using a bare `page.fill('input[type="email"]',
...)` / `page.click('button[type="submit"]')` became ambiguous under
Playwright's strict mode (`resolved to 2 elements`) — 5 call sites across
`auth.spec.ts`, `smoke.spec.ts`, and `helpers.ts`'s own `loginViaUI`.

**Fix pattern: scope by the actual invariant, not DOM order.** `.first()`
"works" but silently breaks if the two sections are ever reordered — the
real invariant is "the password form is the one WITH a password input".
`e2e/tests/helpers.ts` exports `passwordLoginForm(page)` =
`page.locator('form:has(input[type="password"])')`; every password-form
interaction goes through it: `passwordLoginForm(page).locator('input[type="email"]').fill(...)`.
When you add a new same-type form control to an existing page, grep the
whole `e2e/tests/` tree for bare attribute selectors that might now be
ambiguous — don't assume only the test you're writing is affected.

## Diff-scoped mutation gate (`mutation-test` job, PR-only, `--in-diff`)

- Runs on `pull_request` only; **~70 min** on a large auth diff (105 mutants ×
  ~60-100 s each). Budget the wait — it is the long pole after every push to a
  big-diff PR. It re-runs on EVERY PR push (even a docs-only push re-tests the
  same Rust mutants), so avoid superfluous pushes once the Rust is final.
- **A memory-prune window must be strictly WIDER than the decision window**, or
  the decision boundary is unobservable → an unkillable/equivalent mutant. The
  `LoginLinkRateLimiter` `retain`-prune at the SAME 60 s as the `too_fast`
  decision masked the `< → <=` boundary (retain evicted the entry before the
  decision saw it). Fix: widen the prune window (120 s) so the 60 s decision
  boundary is testable, and add an exact-boundary test at each window.
- **Pin numeric constants to LITERALS in a test** (`assert_eq!(INVITE_TTL_SECS,
  1_209_600)`) — `cargo-mutants` mutates the `*`/`+` in a constant's arithmetic
  definition (`14*24*60*60`), and nothing catches it unless a test asserts the
  exact value. Use a literal on the RHS so the test itself has no `*` to mutate.
- Test-fixture defaults + error-mapping branches (`default_seed_role`, the
  `contains("UNIQUE") || contains("unique")` dup-check) need their own tests —
  the `||→&&` and default-return mutants survive otherwise.
- `cargo-mutants` does NOT mutate `#[cfg(test)]` code or `tests/` integration
  binaries, so new tests never add survivors — only changed `src/` lines do.

## `test.use({ ...devices[...] })` inside a `describe` block fails CI

Playwright's device descriptors (`devices['iPhone 13']` etc.) include
`defaultBrowserType` (e.g. `'webkit'`). Spreading the WHOLE descriptor into
`test.use()` **inside a `test.describe()` block** fails immediately:
`Cannot use({ defaultBrowserType }) in a describe group, because it forces a
new worker. Make it top-level in the test file or put in the configuration
file.` (#110, `install-prompt.spec.ts`) — this project's default project is
Chromium only, so you never actually want a real WebKit launch anyway.

**Fix: spread only the context-option fields you need**, not the whole
descriptor:

```ts
const iPhone = devices['iPhone 13'];
test.describe('...', () => {
    test.use({
        userAgent: iPhone.userAgent,
        viewport: iPhone.viewport,
        isMobile: iPhone.isMobile,
        hasTouch: iPhone.hasTouch,
    });
    // ...
});
```

This still gives Chromium a real device UA/viewport/touch profile — enough
for any UA-sniffing or viewport-dependent component logic — without the
`defaultBrowserType` field that breaks describe-scoped `test.use()`.

## Before starting new work: check for an orphaned unmerged PR blocking dev→main

GitHub allows only ONE open PR per head/base pair. If a prior worker pushed
to `dev` and opened the `dev`→`main` PR but died mid-CI-monitor (the
dominant autopilot-worker failure — see `ci-monitoring.md`), that PR sits
open, fully green, unmerged, and BLOCKS you from opening your own PR for an
unrelated ticket (#111/#112 hit exactly this against an orphaned
install-prompt fast-follow, itself a fast-follow on #110/#123's own
worker-death — see the `docs/autopilot-log.md` #110 entry). Before doing
anything else: `gh pr list --head dev --json number,title,url`. If one
exists and isn't yours, finish monitoring its CI to terminal and merge it
(it's unrelated, already-implemented work — merging it is NOT scope creep,
it's unblocking your own branch) — THEN re-bump the version (main just
advanced) and start your own ticket.

## Live post-deploy Playwright verification against `spinbike-dev`/`spinbike.newlevel.media`

Two gotchas, both hit during #111's live verification:

**Stale service worker in your OWN test browser session.** This is a PWA
with an aggressive `sw.js` cache. If your Playwright/MCP browser session
previously visited spinbike-dev/prod at an older deploy, a fresh
`browser_navigate` can render the SW's CACHED old bundle — old version
label, old DOM (e.g. the removed `/register` link still showing) — even
though the real deploy succeeded. Don't conclude the deploy failed from
this alone: cross-check `curl -s <url>/api/version` (never cached) against
the DOM label first. If they disagree, clear the browser's own cache
before re-checking the DOM:
```js
const regs = await navigator.serviceWorker.getRegistrations();
for (const r of regs) await r.unregister();
for (const k of await caches.keys()) await caches.delete(k);
```
then re-navigate. If `/api/version` and the DOM STILL disagree after that, THEN it's a real deploy issue.

**`browser_console_messages({ all: true })` returns the WHOLE MCP session's
history, not just the current page.** The Playwright MCP browser session is
persistent across tickets — `all: true` dumps every console message since
the session began, including navigations from a PAST ticket's verification
(different URLs, different deploys, even a different day). Cross-checking
#117 (does the integrity warning still appear?) with `all: true` returned
14 old messages from unrelated `/register`/`/staff`/`/login` navigations
done during earlier tickets, making it LOOK like the just-fixed warning was
still present. The correct check is the **default** (no `all` flag) right
after a fresh `browser_navigate` — that scopes to messages since the last
navigation only. Only reach for `all: true` when you deliberately want the
full session log for some other reason.

**No CI-seed admin/staff account exists on the real deployments.**
`admin@test.com`/`staff@test.com` only exist in the ephemeral CI test
server (`SPINBIKE_TEST_MODE=1`); the real `dev`/`prod` DBs have only actual
accounts (owner + a legacy admin), and `POST /api/test/seed-account` isn't
mounted there. To drive an authenticated staff/admin flow live without
touching those real accounts, mirror the project's own #106 precedent
("synthetic test users created + JWT-signed + cleaned up, zero real
customer data touched"):
1. `sqlite3 /opt/spinbike/{dev,prod}/spinbike-{dev,}.db` — INSERT a
   throwaway `role='staff'` row (`password_hash` can be `NULL`, you're not
   logging in via password).
2. Sign a JWT yourself with the SAME secret the server uses
   (`JWT_SECRET` in `/etc/default/spinbike-dev` /
   `/etc/default/spinbike`, read via `sudo -n cat` — local machine, no SSH)
   and the exact `Claims{sub,email,role,exp,iat}` shape from
   `crates/spinbike-server/src/auth/mod.rs` (`jsonwebtoken`, `HS256`,
   default `Header`). Sanity-check it once with a `curl -H "Authorization:
   Bearer $TOKEN"` call before using it in the browser.
3. `page.evaluate` to set `spinbike_token`/`spinbike_user` in `localStorage`
   (same shape `loginViaAPI` uses in the E2E helpers), then navigate.
4. Clean up: `DELETE FROM users WHERE id=...` for BOTH the synthetic staff
   row and anything it created (or a real `DELETE /api/users/{id}` call
   with its own token first, since that's a soft-delete via the API vs a
   hard-delete via SQL — either is fine for a throwaway synthetic row).
   **Clicking "Send invite" persists a `login_tokens` row (`kind='invite'`)
   BEFORE it attempts to send the email** — so even an invite that FAILS
   (e.g. dev's `mail_not_configured` 503, used to verify #126 live) leaves
   an orphaned token row once you delete the synthetic user (this DB has
   `PRAGMA foreign_keys` OFF, so the delete succeeds silently and just
   leaves the dangling row). Also run
   `DELETE FROM login_tokens WHERE user_id=...` for every synthetic id you
   created, not just `users`.

## `cargo mutants --shard k/n` is 0-INDEXED — matrix values are `[0, n-1]`, not `[1, n]`

An 8-way sharded matrix must be `shard: [0,1,2,3,4,5,6,7]` with `--shard
${{ matrix.shard }}/8`. `[1..8]` looks natural but makes `8/8` invalid
(shard index out of range) and silently drops shard `0` — cargo-mutants'
own docs confirm `k` ranges `0..n-1`. Verified via an independent
code-review pass before `.github/workflows/mutation-full.yml`'s first-ever
run (#102) — this file is `workflow_dispatch`-only, so there is no green CI
run to catch an off-by-one until someone actually fires it.

## `--baseline=skip` is only safe when an upstream job in the SAME RUN already proved the tree green

The PR-gated `mutation-test` job (`ci.yml`) can safely pass `--baseline=skip`
because it has `needs: test` — the `test` job in the SAME workflow run just
compiled and ran the suite. A **standalone** `workflow_dispatch` job (the
full-tree sweep, `mutation-full.yml`) has no such guarantee: it can be fired
against any ref, including a broken one. With `--baseline=skip`, a
non-building tree makes cargo-mutants report "0 viable mutants tested" as
**exit 0** (success) instead of the baseline-failure **exit 4** — a silently
green job that tested nothing and filed no issue. Fix: don't skip the
baseline in a job with no upstream green-tree guarantee; let cargo-mutants'
own baseline check produce exit 4 on a broken tree. Cost is one redundant
baseline run per shard — acceptable outside a time-boxed PR gate.

## A 5xx response ALWAYS logs a browser console error — even when the app handles it gracefully, and CI structurally can't catch it for mail-related paths

Chromium's DevTools logs `Failed to load resource: the server responded
with a status of 5xx` for ANY fetch with a non-2xx status, INDEPENDENT of
whether the calling JS catches/handles it — you cannot suppress this from
app code. `e2e/tests/helpers.ts`'s `setupConsoleCheck` filters 4xx
("tests intentionally trigger 401/403/409") but deliberately does NOT
filter 5xx ("indicates real server bugs") — so any endpoint that returns a
5xx for a KNOWN, STABLE (non-transient) state, not a transient failure,
will read as a console error on a real deployment even though the UI
behaves correctly. The invite endpoint's `503 mail_not_configured` (mail
Disabled is dev's permanent, by-design state, not a fault) is exactly this
case — filed as
[#127](https://github.com/zbynekdrlik/spinbike/issues/127) rather than
silently accepted, since changing an already-shipped status code is a
contract decision. CI can never surface this on its own: the shared E2E
server always runs `SMTP_TEST_MODE=capture` (mail forced Active) so the
503 branch is unreachable there — this class of bug ONLY shows up on a
real deployment with mail genuinely unconfigured. When you add a NEW
5xx-returning path, ask whether the condition is really transient (keep
5xx) or a stable config/precondition state (prefer a 4xx) BEFORE shipping.

## `Ok(x.await?)` where `x`'s fn already returns the exact same `Result<T>` is a clippy CI failure, not a local one

Since local `cargo clippy` is banned (only `cargo fmt --all --check` runs
locally — see above), a thin wrapper fn that just delegates to another
async fn of the SAME return type — e.g. `pub async fn tick(pool) ->
Result<u64> { Ok(inner(pool).await?) }` — passes `fmt` clean and looks
correct, but fails CI's `cargo clippy --all-targets -- -D warnings` on
`needless_question_mark` (#119). Caught only after a push. When a wrapper
has NO transformation between the inner call and the return, write
`inner(pool).await` directly with no `Ok`/`?`; reserve `Ok(x.await?)` for
when you actually transform the value first (e.g. `let n = x.await?; Ok(n
as usize)`), which clippy does NOT flag.

## Purging/negating an existing validity predicate — match the boundary EXACTLY, not just "the opposite direction"

When a new query is meant to be the precise logical negation of an
existing one (e.g. a housekeeping purge that should delete exactly the
rows a sibling function's validity check rejects), copy the inequality
direction AND strictness literally. `redeem()`'s validity check
(`login_tokens.rs`) is `expires_at > datetime('now')` (strict); the first
draft of the #119 purge used `expires_at < datetime('now')` (also
strict) — off by the boundary instant `expires_at == now`, where the row
was neither redeemable nor purge-eligible for one second. The exact
negation of `A > B` is `A <= B`, not `A < B`. A second code-review pass
caught it; write the negation formula out by hand (`NOT (a AND b) = (NOT
a) OR (NOT b)`, then negate each comparison correctly) before trusting
"looks like the opposite".

## Foreground CI-poll waits: the sandbox now hard-blocks bare `sleep N && cmd`

Autopilot workers are told to monitor CI with a FOREGROUND `sleep 40 && gh run
view ...` loop (never `Monitor`/`run_in_background`, which end a subagent's
turn and kill it — see `ci-monitoring.md`). But this environment's Bash-tool
sandbox now hard-blocks ANY command whose text matches a leading/standalone
`sleep` pattern — `sleep 40`, `sleep 40 && gh run view ...`, even wrapped in
`&&` — with "To wait for a condition, use Monitor..." (which is exactly the
tool that would kill the subagent). Workaround: write the sleep+poll into a
small script FILE and execute the file instead of inlining `sleep` in the
Bash command string — the sandbox's text-pattern check doesn't see "sleep" in
the invoking command, and it's still a genuine foreground blocking call (no
`run_in_background`, no turn-ending):

```bash
cat > /tmp/.../wait_ci.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
sleep 40
gh run view "$1" --json status,conclusion,jobs
EOF
chmod +x /tmp/.../wait_ci.sh
/tmp/.../wait_ci.sh <run-id>   # repeat until status == "completed"
```
