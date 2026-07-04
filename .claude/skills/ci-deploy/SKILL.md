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
