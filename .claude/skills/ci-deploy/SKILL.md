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
