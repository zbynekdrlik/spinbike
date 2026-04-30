# Fitness Preselect on Charge Form Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When staff opens a card on the desk, the service `<select>` defaults to "Fitness" instead of the empty placeholder, saving one click per fitness charge.

**Architecture:** Add a single `Effect` inside `ActionForm` that watches the `services` signal. On first non-empty value AND when `selected_service_id` is `None`, find the active Fitness service, set the signal, and imperatively call `service_ref.set_value(...)` on the DOM `<select>`. Avoids the `prop:value` reactive binding that broke the txn list in `c533d7c`. Empty `<option value="">` placeholder stays as the missing-Fitness fallback.

**Tech Stack:** Rust 1.x, Leptos 0.7 CSR/WASM, Trunk, web-sys, Playwright, gh CLI.

**Spec:** `docs/superpowers/specs/2026-04-30-fitness-preselect-design.md` (committed at `fbca434`).

**Issue:** [#33 — Re-add Fitness preselect to charge form](https://github.com/zbynekdrlik/spinbike/issues/33)

**Out of scope:** #34 (Spinning quick-charge chip), #28 (transactions.note CHECK), #36 (cargo-mutants/Axum).

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `VERSION` | Single-source-of-truth version | bump 0.13.10 → 0.13.11 |
| `Cargo.toml` (root) | Workspace version | sync via `scripts/sync-version.sh` |
| `spinbike-ui/Cargo.toml` | UI crate version | sync via `scripts/sync-version.sh` |
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Action form component (`<select>`, signals, charge logic) | Add `use spinbike_core::services::FITNESS_NAME_EN`. Add `Effect::new(...)` block before `view!`. Empty `<option>` stays. |
| `e2e/tests/desk-ux.spec.ts` | Playwright tests for desk UX cluster | Add 2 tests for Fitness preselect. |

---

## Task 1: Version bump

**Files:**
- Modify: `VERSION`
- Modify (via script): `Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Verify current version**

```bash
cat VERSION
```

Expected: `0.13.10`

- [ ] **Step 2: Bump VERSION to 0.13.11**

Edit `/home/newlevel/devel/spinbike/VERSION` so the file content becomes exactly:

```
0.13.11
```

- [ ] **Step 3: Sync version into Cargo.toml files**

```bash
bash scripts/sync-version.sh
```

Expected: script prints "synced 0.13.11" or similar; `Cargo.toml` and `spinbike-ui/Cargo.toml` both show `version = "0.13.11"`.

Verify:
```bash
grep '^version' Cargo.toml spinbike-ui/Cargo.toml
```

Expected: both lines show `version = "0.13.11"`.

- [ ] **Step 4: Local format check**

```bash
cargo fmt --all --check
```

Expected: no output, exit code 0. (No code changed; this is a sanity gate before commit.)

- [ ] **Step 5: Commit**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump version to 0.13.11"
```

Expected: one commit with 3 files changed.

---

## Task 2: E2E tests for Fitness preselect (RED)

**Files:**
- Modify: `e2e/tests/desk-ux.spec.ts`

This task adds 2 Playwright tests that assert Fitness is preselected. They will FAIL on the current `dev` codebase because no preselect logic exists yet. Task 3 makes them GREEN.

- [ ] **Step 1: Read the current end of `desk-ux.spec.ts` to know where to append**

```bash
tail -30 e2e/tests/desk-ux.spec.ts
```

Note the closing `});` of the `test.describe('Staff desk UX cluster — issues #29 #30 #31 #32', ...)` block.

- [ ] **Step 2: Add 2 tests inside the existing describe block**

Insert the following two `test(...)` blocks **inside** the existing `test.describe('Staff desk UX cluster — issues #29 #30 #31 #32', () => { ... })` block, just before its closing `});`. Use the existing helper functions (`activateUniqueCard`, `sellPassToCard`, `lookupCardId`, `openCardByLastName`) that are already defined at the top of the file.

```typescript
    test('Fitness preselected when staff opens a card (#33)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        // ActionForm renders inside the action-panel.
        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

        // The <select> must show "Fitness" as the active option's visible text.
        // Force English so we assert against the literal "Fitness" name (which
        // matches name_en in the DB regardless of language; the option label
        // for "Fitness" service is "Fitness" in both Slovak and English).
        const selectedText = await page
            .locator('[data-testid="charge-service"] option:checked')
            .textContent();
        expect((selectedText ?? '').trim()).toBe('Fitness');

        assertCleanConsole(msgs);
    });

    test('Empty option is not the active selection when Fitness preselect succeeds (#33)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

        // The <select> value (which is the option's `value=` attribute, i.e.
        // the service id) must be a non-empty string that parses to a positive
        // integer. The empty <option value=""> placeholder still exists in the
        // DOM as the missing-Fitness fallback, but it must NOT be the active
        // selection in this normal-case test.
        const value = await page.locator('[data-testid="charge-service"]').inputValue();
        expect(value).not.toBe('');
        const parsed = Number.parseInt(value, 10);
        expect(Number.isFinite(parsed)).toBe(true);
        expect(parsed).toBeGreaterThan(0);

        assertCleanConsole(msgs);
    });
```

- [ ] **Step 3: Commit (the tests are RED on dev right now — that's expected)**

```bash
git add e2e/tests/desk-ux.spec.ts
git commit -m "test(e2e): Fitness preselect on charge form (#33, red)"
```

Expected: one commit with 1 file changed.

> **Note for the implementer:** Do NOT push at this point — tests are intentionally RED. The next task makes them GREEN locally; CI is exercised in Task 4 once both commits are on the branch.

---

## Task 3: Fitness preselect Effect (GREEN)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs`

This task adds the Effect that turns Task 2's tests GREEN.

- [ ] **Step 1: Read the file to confirm the insertion points**

```bash
grep -n "use spinbike_core\|use leptos::prelude\|let on_service_change\|let do_topup\|view!" spinbike-ui/src/pages/dashboard/action_form.rs
```

Expected output (line numbers may shift slightly):
- `use leptos::prelude::*;` near top (around line 1)
- `use super::{CardInfo, ...};` (around line 11) — no `spinbike_core::services` import yet
- `let on_service_change = move |_| { ... };` (around line 73)
- `view! {` (around line 275)

- [ ] **Step 2: Add the `FITNESS_NAME_EN` import**

Find this line in `spinbike-ui/src/pages/dashboard/action_form.rs`:

```rust
use super::helpers::pass_is_active;
```

Insert directly **after** that line (so it becomes a new line just before `use super::{CardInfo, ...};`):

```rust
use spinbike_core::services::FITNESS_NAME_EN;
```

The block of imports near the top should now read in this order:

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;
use crate::components::DateInput;
use crate::i18n::{self, Lang};
use crate::util::parse_money;

use super::helpers::pass_is_active;
use spinbike_core::services::FITNESS_NAME_EN;
use super::{CardInfo, CardPass, PaymentResp, ServiceInfo};
```

- [ ] **Step 3: Add the preselect Effect before `view!`**

Find the line that opens the `view!` macro:

```rust
    view! {
```

Insert the following block **immediately before** that line. It must sit AFTER all signals are declared (`selected_service_id`, `service_ref`, etc.) and BEFORE the `view!` macro:

```rust
    // Fitness preselect (#33).
    //
    // Watches the async-loaded services signal and, when it first arrives
    // non-empty AND no service is selected yet, finds the active Fitness
    // service and selects it both in the signal AND on the DOM <select>.
    //
    // The `set_value()` call is imperative DOM mutation, NOT a `prop:value`
    // reactive binding — that's deliberate. The previous attempt (commit
    // c533d7c, reverted in 471a0c0) used `prop:value` and triggered a
    // re-render lifecycle that broke `set_selected.update` in the parent,
    // causing the txn list to show empty after a successful charge. The
    // imperative path doesn't subscribe the <select> to any signal, so the
    // parent's update flow is untouched.
    //
    // The empty <option value=""> placeholder is intentionally kept in the
    // options list (see spec) — it serves as the missing-Fitness fallback
    // when the admin has disabled or renamed the Fitness service.
    Effect::new(move |_| {
        let svcs = services.get();
        if svcs.is_empty() {
            return;
        }
        if selected_service_id.get_untracked().is_some() {
            return;
        }
        let Some(fitness) = svcs
            .iter()
            .find(|s| s.name_en == FITNESS_NAME_EN && s.active != 0)
            .cloned()
        else {
            return;
        };
        set_selected_service_id.set(Some(fitness.id));
        if let Some(el) = service_ref.get() {
            let el: &HtmlSelectElement = &el;
            el.set_value(&fitness.id.to_string());
        }
    });
```

- [ ] **Step 4: Local format check**

```bash
cargo fmt --all --check
```

Expected: no output, exit code 0.

If it fails, run `cargo fmt --all` and re-check.

- [ ] **Step 5: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): preselect Fitness service in charge form (#33)

Adds an Effect inside ActionForm that watches the async-loaded services
signal and, on first non-empty value with no current selection, finds
the active Fitness service and selects it in both the signal and the
DOM <select>. Saves staff one click on the most common transaction.

Uses imperative service_ref.set_value() rather than prop:value to avoid
the re-render cascade that broke txn-list refresh in c533d7c.

Empty <option value=''> placeholder kept as the missing-Fitness fallback;
server-side null-service guard from #31 still rejects null service_id."
```

Expected: one commit with 1 file changed.

---

## Task 4: Push, monitor CI, open PR

**Files:** none (git + gh operations)

> **Pre-push gate:** `cargo fmt --all --check` already ran in Task 3. No other local checks. CI is authoritative per project memory `feedback_subagent_no_local_build.md`.

- [ ] **Step 1: Confirm branch state**

```bash
git status
git log --oneline -4
```

Expected: clean working tree on `dev`. Latest 3 commits are this PR's work (chore: bump version, test(e2e): Fitness preselect red, feat(ui): preselect Fitness). Below that, the merge commit from PR #35.

- [ ] **Step 2: Push to origin/dev**

```bash
git push origin dev
```

Expected: push succeeds. CI starts a workflow run.

- [ ] **Step 3: Capture the run id and monitor to terminal state**

Identify the run id of the run triggered by THIS push (the latest one on dev):

```bash
gh run list --branch dev --limit 3 --json databaseId,event,status,headSha,createdAt
```

Note the `databaseId` of the most recent `push` event whose `headSha` matches `git rev-parse HEAD`. Call this `${RUN_ID}`.

Then start a single background monitor command (per `ci-monitoring.md`):

```bash
sleep 600 && gh run view ${RUN_ID} --json status,conclusion,jobs
```

Run this with `run_in_background: true`. The result will be returned to the conversation when ready (~10 minutes later); the loop continues from there.

- [ ] **Step 4: When the monitor returns, inspect the result**

If `status == "completed"` and `conclusion == "success"`: all jobs green. Proceed to Step 6.

If `status == "completed"` but `conclusion != "success"`: read `--log-failed` and decide.

```bash
gh run view ${RUN_ID} --log-failed | head -200
```

Decision tree:
- **E2E test failed with the new `'Fitness preselected ...'` test**: the Effect didn't fire or DOM didn't update. Look at the test's screenshot/trace artifact via `gh run download ${RUN_ID} -n e2e-test-results` and adjust the Effect (most likely cause: timing — the page interaction in the test happens before the Effect fires; consider adding a `waitForFunction` for non-empty value).
- **E2E test failed with one of the existing 5 tests** (txn-note, charge flow, reports-attendance, etc.): regression — the Effect's signal write is interfering with parent re-render. Investigate immediately. (Spec architecture predicts this won't happen, but verify.)
- **Single-test E2E flake matching pattern from issue #24** (random POST-then-assert, "No transactions" symptom): per `ci-monitoring.md`, ONE rerun is acceptable. `gh run rerun --failed ${RUN_ID}` and re-monitor with the same `sleep 600 && gh run view ...` command.
- **Server test or unit test failed**: very unlikely (no server changes), but read the log and fix.

Do NOT just bump test timeouts. Per `airuleset/quality/no-timeout-band-aids.md`: investigate root cause.

If `status == "in_progress"` after the 10-minute sleep: re-monitor with another `sleep 300 && gh run view ${RUN_ID} --json status,conclusion,jobs` background command. Build WASM + E2E can take ~12-15 min total on first run.

- [ ] **Step 5: If a fix was needed, push it and re-monitor**

After fixing locally:

```bash
git add <specific files>
git commit -m "fix(<area>): <what>"
git push origin dev
```

Capture the new run id and monitor again with the same `sleep 600 && gh run view ...` pattern. Repeat until ALL jobs green.

- [ ] **Step 6: Open PR `dev` → `main`**

When CI is fully green on the latest dev push:

```bash
gh pr create --base main --head dev --title "feat(ui): preselect Fitness service on charge form (#33, v0.13.11)" --body "$(cat <<'EOF'
## Summary

Implements #33 (Re-add Fitness preselect to charge form), the follow-up to the reverted #29 work in PR #35.

When staff opens a card on the desk, the service `<select>` now defaults to "Fitness" — the most common selection for spin/fitness gym day-to-day transactions. Staff types "Fitness" much less frequently as a result.

## Approach

- Single `Effect` inside `ActionForm` watches the `services` signal. When it first arrives non-empty AND no service is selected yet, finds the active Fitness service and selects it via:
  - `set_selected_service_id.set(Some(id))` — keeps the existing form logic in sync
  - `service_ref.get().unwrap().set_value(...)` — imperative DOM update, no `prop:value` binding
- Empty `<option value="">` placeholder is **kept** as the missing-Fitness fallback (admin disables Fitness → dropdown shows empty, staff picks manually, server-side null-service guard from #31 catches null charges).

## Why imperative DOM update

Previous attempt (c533d7c, reverted in 471a0c0) used `prop:value` on the `<select>`. The reactive binding triggered a re-render cascade that broke `set_selected.update` in the parent → CardActionPanel re-mounted with fresh signals → txn-list rendered as "No transactions" after a successful charge. The imperative `set_value()` doesn't subscribe the `<select>` to any signal, so the parent's update flow is untouched.

## Test plan

- [x] Local format check (`cargo fmt --all --check`).
- [x] CI on push: Test Integrity, Lint, Build WASM, Test, E2E (incl. 2 new desk-ux tests), Mutation Testing, Deploy (dev), Smoke (dev).
- [ ] After merge: dev frontend at https://spinbike-dev.newlevel.media — open a card, confirm Fitness is preselected.
- [ ] After merge: prod at https://spinbike.newlevel.media — same check.

## E2E coverage added

| Feature/Fix | E2E Test File | What It Verifies |
|---|---|---|
| Fitness preselect | e2e/tests/desk-ux.spec.ts | `'Fitness preselected when staff opens a card (#33)'` — opens a card, asserts charge-service `<select>`'s active option text is "Fitness" |
| Empty placeholder is dormant | e2e/tests/desk-ux.spec.ts | `'Empty option is not the active selection when Fitness preselect succeeds (#33)'` — opens a card, asserts `<select>` value is non-empty positive integer |

## Spec & plan

- Spec: `docs/superpowers/specs/2026-04-30-fitness-preselect-design.md`
- Plan: `docs/superpowers/plans/2026-04-30-fitness-preselect.md`

Closes #33.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 7: Verify PR is mergeable + clean**

```bash
gh api repos/zbynekdrlik/spinbike/pulls/$(gh pr view --json number -q .number) --jq '{mergeable: .mergeable, mergeable_state: .mergeable_state}'
```

Expected: `{"mergeable": true, "mergeable_state": "clean"}`. Both must be true.

If `mergeable_state == "behind"`: sync dev with main first via `git fetch origin && git merge origin/main` then push.
If `mergeable_state == "blocked"`: investigate which check is blocking.
If `mergeable_state == "dirty"`: resolve conflicts.

> **STOP HERE.** Per `pr-merge-policy.md`: never merge the PR yourself. Report PR URL to the user with the standard completion-report template. Wait for explicit "merge it" instruction.

---

## Task 5: Post-deploy verification (after user merges)

**Triggers:** Only run this task AFTER the user explicitly says "merge it" AND the merge has happened AND main CI's deploy + smoke jobs are green.

**Files:** none (verification via Playwright MCP + curl)

- [ ] **Step 1: Confirm main has the merge**

```bash
git fetch origin
git log --oneline origin/main -3
```

Expected: top commit is the merge commit from this PR.

- [ ] **Step 2: Verify dev backend version**

```bash
curl -s https://spinbike-dev.newlevel.media/api/version
```

Expected: `0.13.11` (or `0.13.11-dev.X` depending on tagging).

- [ ] **Step 3: Verify dev frontend Fitness preselect via Playwright MCP**

Use `mcp__plugin_playwright_playwright__browser_navigate` to open `https://spinbike-dev.newlevel.media/staff`.

Login via the UI (or use a pre-authenticated test cookie if available — the test users staff@test.com / staff123 likely don't exist on prod-synced dev DB; use real staff credentials per project memory `feedback_prod_dev_same_machine.md` and `feedback_dev_ci_sync_prod_db.md`).

If the staff account credentials aren't in your context: ASK the user (per `ask-before-assuming.md`). Do NOT guess credentials against prod-shape data.

After login:
1. Navigate to `/staff`.
2. Search for any card with an active monthly pass (typing 1-2 chars of any common surname likely yields a hit on prod-synced data).
3. Click the first result.
4. Take a `mcp__plugin_playwright_playwright__browser_snapshot` of the action-form area.
5. Confirm the `<select>` shows "Fitness" as selected.
6. Confirm the version label in the dashboard footer/navbar reads `v0.13.11` (or matching).
7. Confirm browser console is clean via `mcp__plugin_playwright_playwright__browser_console_messages`.

- [ ] **Step 4: Verify prod backend version**

```bash
curl -s https://spinbike.newlevel.media/api/version
```

Expected: `0.13.11`.

- [ ] **Step 5: Verify prod frontend Fitness preselect via Playwright MCP**

Same procedure as Step 3, but at `https://spinbike.newlevel.media`.

- [ ] **Step 6: Send the completion report**

Use the EXACT template from `completion-report.md`. Required fields:
- `## ✅ Work Complete` header
- `**Audits & deploy:**` block with `✅ CI`, `✅ /plan-check: 5/5 fulfilled`, `✅ /review: clean — 0 🔴 0 🟡 0 🔵`, `✅ Deploy: ...`
- `---` separator
- `**Goal:** When staff opens a card, the service select defaults to Fitness (saves a click on the most common transaction).`
- `**What changed:** ...`
- `🌐 Dev:  https://spinbike-dev.newlevel.media`
- `🌐 Prod: https://spinbike.newlevel.media`
- `**[spinbike] PR #<N>: feat(ui): preselect Fitness service on charge form (#33, v0.13.11)**`
- `<full PR URL> — merged at <merge-commit-sha>`

No `❓ Question` line — work is complete.

---

## Self-Review Notes

**Spec coverage check (manual):**
- Goal (one click less per fitness charge) → Tasks 2 + 3
- Empty placeholder kept as missing-Fitness fallback → Task 3 (does NOT remove the empty option line)
- Server-side null-service guard from #31 still works → unchanged from prior PR (no edit to server)
- Imperative DOM update via `service_ref.set_value()` (no `prop:value`) → Task 3
- `is_none()` guard so manual selection wins → Task 3 (`if selected_service_id.get_untracked().is_some() { return; }`)
- Card-switch reset → CardActionPanel re-mount handles it (no special code needed; documented in spec)
- E2E test 1 (Fitness selected) → Task 2 first test
- E2E test 2 (empty option not active) → Task 2 second test
- Version bumped per `version-bumping.md` → Task 1
- VERSION script syncs Cargo.toml files → Task 1 Step 3

**Placeholder scan:** none ("TBD"/"TODO"/"implement later" not present in the plan).

**Type consistency:** `services`, `set_selected_service_id`, `selected_service_id`, `service_ref`, `HtmlSelectElement`, `FITNESS_NAME_EN` — all correct names matching the existing codebase (verified via grep before writing).

**Pre-implementation pause:** Per project memory `feedback_pre_implementation_pause.md`, the controller MUST pause once after committing this plan and ask the user whether to dispatch subagents now or hold for plan review first.
