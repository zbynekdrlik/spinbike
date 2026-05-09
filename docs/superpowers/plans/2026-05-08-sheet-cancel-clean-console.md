# Sheet Cancel Clean-Console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate `closure invoked recursively or after being dropped` console errors when clicking Cancel on `DeleteUserSheet`, `EditTxDateSheet`, or `EditPassDateSheet`, and tighten the E2E console-error filter so this bug class is detectable in CI.

**Architecture:** Three independent code changes coordinated for testability: (1) narrow the `setupConsoleCheck` filter so it stops swallowing wasm-stack errors, (2) drop the redundant `set_err.set("")` from each sheet's `on_cancel` so the synchronous reactive write before unmount goes away, (3) add Playwright Cancel-branch regression tests in two existing specs.

**Tech Stack:** Rust 1.x · Leptos 0.7 (CSR/WASM) · Playwright (TypeScript) · Trunk · Axum 0.8 (CI deploy).

**Spec:** `docs/superpowers/specs/2026-05-08-sheet-cancel-clean-console-design.md` (committed at `ed32cef`).

**Issue:** [#84](https://github.com/zbynekdrlik/spinbike/issues/84).

---

## File map

| File | Responsibility | Touched by |
|---|---|---|
| `VERSION` + `Cargo.toml` × 2 | version source of truth | Task 1 |
| `e2e/tests/helpers.ts` | console-message filter | Task 2 |
| `spinbike-ui/src/pages/dashboard/sheets/delete_user.rs` | DeleteUserSheet `on_cancel` | Task 4 |
| `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs` | EditTxDateSheet `on_cancel` | Task 4 |
| `spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs` | EditPassDateSheet `on_cancel` | Task 4 |
| `e2e/tests/users-by-movement.spec.ts` | DeleteUserSheet Cancel regression test | Task 5 |
| `e2e/tests/edit-tx-date.spec.ts` | EditTxDateSheet Cancel regression test | Task 5 |
| `e2e/tests/redesign-sheets.spec.ts` | EditPassDateSheet Cancel test (already present, only verified green) | none |

---

### Task 1: Bump VERSION 0.13.28 → 0.13.29 (controller)

**Files:**
- Modify: `VERSION`
- Modify (via script): `Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Read current VERSION**

```bash
cat VERSION
```

Expected: `0.13.28`

- [ ] **Step 2: Bump VERSION to 0.13.29**

Replace contents of `VERSION` with:

```
0.13.29
```

- [ ] **Step 3: Sync version to all Cargo.toml files**

Run:

```bash
bash scripts/sync-version.sh
```

- [ ] **Step 4: Verify sync**

Run:

```bash
grep -E '^version =' Cargo.toml spinbike-ui/Cargo.toml
```

Expected: every line shows `version = "0.13.29"`.

- [ ] **Step 5: Commit**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "$(cat <<'EOF'
chore: bump version to 0.13.29 for #84 (sheet cancel clean console)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Tighten setupConsoleCheck filter (subagent, sonnet)

**Files:**
- Modify: `e2e/tests/helpers.ts:7-35`

The current filter swallows any console message whose text contains the substring `wasm`. The closure-after-drop runtime error's stack trace contains `wasm-function[...]`, so the filter masks it. Drop the broad clause; the two `using deprecated parameters` clauses on adjacent lines already pin the only legitimate-noise messages.

- [ ] **Step 1: Open `e2e/tests/helpers.ts` and locate the filter block**

The file currently has, inside `setupConsoleCheck`, an `if (...)` that lists allow-listed substrings separated by `||`.

- [ ] **Step 2: Delete the broad `wasm` substring clause**

Remove this exact line from the `if` condition:

```typescript
                text.includes('wasm') ||
```

The surrounding lines remain unchanged. After the change, the relevant block reads:

```typescript
            if (
                text.includes('SharedArrayBuffer') ||
                text.includes('integrity') ||
                text.includes('subresource integrity') ||
                text.includes('crbug.com') ||
                // Trunk bootstrap calls wasm-bindgen init with the legacy
                // positional arg form; wasm-bindgen 0.2.x emits a deprecation
                // warning until Trunk migrates to the single-object form.
                // Not our code's bug; filter until upstream upgrade.
                text.includes('using deprecated parameters for the initialization function') ||
                text.includes('using deprecated parameters for `initSync()`') ||
                /the server responded with a status of 4\d\d/.test(text)
            ) {
                return;
            }
```

- [ ] **Step 3: Local sanity check (TypeScript only — no Rust build)**

Run:

```bash
cd e2e && npx tsc --noEmit && cd ..
```

Expected: no TypeScript errors. (CI is authoritative for full verification — do NOT run any cargo or trunk commands.)

- [ ] **Step 4: Commit**

```bash
git add e2e/tests/helpers.ts
git commit -m "$(cat <<'EOF'
test(e2e): drop broad 'wasm' substring filter in setupConsoleCheck (#84)

The substring 'wasm' appears in every closure-after-drop error stack trace
(\`wasm-function[NNNN]\`). Removing the broad clause lets assertCleanConsole
catch real runtime errors that originate in Leptos/wasm-bindgen.

The two specific 'using deprecated parameters' clauses on adjacent lines
continue to pin the only legitimate-noise message (Trunk bootstrap).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Push Tasks 1+2 and triage CI surfaces (controller)

**Files:** none (CI / triage only).

This task confirms the filter narrowing does not silently break unrelated specs BEFORE we land the actual sheet fix. If something surfaces, we know it is either a true latent bug that must be fixed inline (per `browser-console-zero-errors.md`) or a noise message that warrants its own *specific* substring (never re-broadening to `wasm`).

- [ ] **Step 1: Push to dev**

```bash
git push origin dev
```

- [ ] **Step 2: Identify the latest workflow run**

```bash
gh run list --branch dev --limit 3
```

Note the most-recent run id (top row).

- [ ] **Step 3: Wait for the run to reach a terminal state**

```bash
sleep 600 && gh run view <run-id> --json status,conclusion,jobs
```

Run in background (`run_in_background: true`) per `ci-monitoring.md`.

- [ ] **Step 4: If the run is green → proceed to Task 4**

Nothing to triage; the filter narrowing did not surface latent errors.

- [ ] **Step 5: If the run is red → triage each failure**

For each failed job:

```bash
gh run view <run-id> --log-failed
```

For each newly-surfaced console error:
1. Identify the spec file and the offending line/component.
2. Determine root cause (real bug vs. external noise).
3. **Real bug:** fix it inline within this PR (no silencing). Add a regression assertion if useful. Push the fix as its own commit.
4. **External noise (e.g. third-party deprecation, browser version warning):** add a SPECIFIC `text.includes('<exact phrase>')` clause next to the existing two deprecation lines in `e2e/tests/helpers.ts`. NEVER re-introduce the broad `wasm` substring. Push as its own commit.

- [ ] **Step 6: Re-monitor after each fix**

Repeat Steps 2–5 until the workflow run is fully green.

- [ ] **Step 7: Sanity check the worktree state before Task 4**

```bash
git status
git log --oneline -5
```

Expected: working tree clean; the version-bump and filter-narrowing commits are at the tip plus any triage commits.

---

### Task 4: Drop set_err.set in 3 sheet on_cancel handlers (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/sheets/delete_user.rs` (around line 50–53 in current source)
- Modify: `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs` (around line 63–66)
- Modify: `spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs` (around line 68–71)

Each file has an identical `on_cancel` two-line body. Drop the redundant first line in all three. Per-mount fresh state means the err signal is recreated empty on the next sheet open — clearing it manually is dead code.

- [ ] **Step 1: Edit `delete_user.rs`**

Find:

```rust
            let on_cancel = move |_| {
                set_err.set(String::new());
                show.set(false);
            };
```

Replace with:

```rust
            let on_cancel = move |_| {
                show.set(false);
            };
```

- [ ] **Step 2: Edit `edit_tx_date.rs`**

Find:

```rust
            let on_cancel = move |_| {
                set_err.set(String::new());
                show.set(false);
            };
```

Replace with:

```rust
            let on_cancel = move |_| {
                show.set(false);
            };
```

- [ ] **Step 3: Edit `edit_pass_date.rs`**

Find:

```rust
            let on_cancel = move |_| {
                set_err.set(String::new());
                show.set(false);
            };
```

Replace with:

```rust
            let on_cancel = move |_| {
                show.set(false);
            };
```

- [ ] **Step 4: Format check (only allowed local Rust command)**

Run:

```bash
cargo fmt --all --check
```

Expected: silent (no diff) or auto-fix. If auto-fix is needed, run `cargo fmt --all` and re-stage.

- [ ] **Step 5: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/sheets/delete_user.rs \
        spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs \
        spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs
git commit -m "$(cat <<'EOF'
fix(ui): drop redundant set_err in sheet on_cancel handlers (#84)

The synchronous set_err.set("") immediately before show.set(false) drove a
reactive run on the err-display subscriber while the outer move|| was about
to drop that subscriber. Leptos emitted "closure invoked recursively or
after being dropped".

The write was dead code anyway — every sheet open creates a fresh
(err, set_err) pair via the per-mount state pattern, so err is empty when
the user reopens the sheet.

Affects DeleteUserSheet, EditTxDateSheet, EditPassDateSheet.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Add Cancel-branch E2E regression tests (subagent, sonnet)

**Files:**
- Modify: `e2e/tests/users-by-movement.spec.ts` — add a second test in the existing describe block.
- Modify: `e2e/tests/edit-tx-date.spec.ts` — add a second test in the existing describe block.
- Existing untouched: `e2e/tests/redesign-sheets.spec.ts:51-76` already exercises EditPassDateSheet Cancel + `assertCleanConsole`. After Task 2 the filter no longer hides the bug, so this existing test now functions as a regression gate at zero diff cost.

The new tests deliberately do NOT click the Confirm button — that path is out-of-scope per spec. Each test seeds a unique user/transaction (independent of the Confirm tests in the same files), opens the relevant sheet, clicks Cancel, asserts the sheet is hidden, and asserts the console is clean.

- [ ] **Step 1: Add DeleteUserSheet Cancel test in `users-by-movement.spec.ts`**

Append a second test inside the existing `test.describe('Users by last movement (#56)', ...)` block (right after the existing test at line 12–123). The new test:

```typescript
    test('DeleteUserSheet Cancel closes modal with clean console (#84)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // Seed an independent user so the Cancel test never collides with the
        // Confirm test above. Prefix uses CAN- to make the test row easy to
        // spot in CI logs.
        const u = await createUniqueUser(token, 0.0, 'CAN-D');

        // Open the user via /staff?card=<code> directly — same nav target the
        // Reports row click produces. Cheaper than re-paginating the list.
        await page.goto(`/staff?card=${u.card_code}`);
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // Open delete modal.
        await page.click('[data-testid="delete-user-button"]');
        const sheet = page.locator('[data-testid="sheet-delete-user"]');
        await expect(sheet).toBeVisible();

        // Click Cancel — the bug under test.
        await page.click('[data-testid="delete-user-cancel"]');

        // Sheet hides; the action panel stays open since no destructive action ran.
        await expect(sheet).toBeHidden({ timeout: 2000 });
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        assertCleanConsole(msgs);
    });
```

- [ ] **Step 2: Add EditTxDateSheet Cancel test in `edit-tx-date.spec.ts`**

Append a second test inside the existing `test.describe('Edit transaction date (#76)', ...)` block. The new test:

```typescript
    test('EditTxDateSheet Cancel closes modal with clean console (#84)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const { card_code, user_id } = await createUniqueUser(token, 0.0, 'TXC');

        // Seed a Spinning charge so the txn list has a row.
        const svcResp = await fetch(`${BASE_URL}/api/admin/services`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        if (!svcResp.ok) throw new Error(`/api/admin/services failed: ${svcResp.status}`);
        const services = (await svcResp.json()) as Array<{ id: number; name_en: string }>;
        const spinning = services.find((s) => s.name_en === 'Spinning');
        if (!spinning) throw new Error('Spinning service not found');
        const chargeResp = await fetch(`${BASE_URL}/api/payments/charge`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                Authorization: `Bearer ${token}`,
            },
            body: JSON.stringify({ user_id, amount: 1.0, service_id: spinning.id }),
        });
        if (!chargeResp.ok) throw new Error(`charge POST failed: ${chargeResp.status}`);

        // Open the card.
        await page.goto('/staff');
        const search = page.locator('input[type="search"]');
        await search.waitFor();
        await search.focus();
        await page.keyboard.type(card_code, { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // Click date-edit pencil to open the sheet.
        const list = page.locator('[data-testid="transactions-list"]');
        await expect(list).toBeVisible();
        const row = list.locator('[data-testid="transaction-row"]').first();
        await row.locator('[data-testid="txn-date-edit"]').click();
        const sheet = page.locator('[data-testid="sheet-edit-tx-date"]');
        await expect(sheet).toBeVisible();

        // Click Cancel — the bug under test. The sheet has no testid'd Cancel
        // button (only Save has tx-date-save), so filter by i18n text just
        // like redesign-sheets.spec.ts does for EditPassDateSheet.
        await sheet.locator('button').filter({ hasText: /zrusit|cancel/i }).click();
        await expect(sheet).not.toBeVisible({ timeout: 2000 });

        assertCleanConsole(msgs);
    });
```

- [ ] **Step 3: TypeScript sanity check**

```bash
cd e2e && npx tsc --noEmit && cd ..
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add e2e/tests/users-by-movement.spec.ts e2e/tests/edit-tx-date.spec.ts
git commit -m "$(cat <<'EOF'
test(e2e): Cancel-branch regression tests for DeleteUser + EditTxDate sheets (#84)

Each test opens the relevant sheet and clicks its Cancel button (NOT
Confirm/Save), asserts the sheet hides, and assertCleanConsole verifies
zero closure-after-drop errors leak into the browser console.

Pairs with the on_cancel fix in 4d1c2c… so a regression in either fix
trips CI immediately.

EditPassDateSheet Cancel is already covered by
redesign-sheets.spec.ts:51-76 — left untouched.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Push, monitor CI to terminal state, open PR (controller)

**Files:** none (push + PR only).

- [ ] **Step 1: Push to dev**

```bash
git push origin dev
```

- [ ] **Step 2: Identify the latest run**

```bash
gh run list --branch dev --limit 3
```

- [ ] **Step 3: Wait for terminal state**

```bash
sleep 900 && gh run view <run-id> --json status,conclusion,jobs
```

Run in background. Expected: ALL jobs (lint, test, e2e, mutation, deploy-dev, smoke-dev) pass.

- [ ] **Step 4: If any job fails, investigate and fix**

```bash
gh run view <run-id> --log-failed
```

Fix root cause, push, re-monitor. Repeat until green.

- [ ] **Step 5: Open PR `dev` → `main`**

```bash
gh pr create --base main --head dev \
  --title "v0.13.29: sheet Cancel clean-console fix (#84)" \
  --body "$(cat <<'EOF'
## Summary

- Drops a redundant \`set_err.set("")\` call from \`on_cancel\` in three sheet components (\`DeleteUserSheet\`, \`EditTxDateSheet\`, \`EditPassDateSheet\`). The synchronous reactive write before the outer scope unmount caused Leptos to emit "closure invoked recursively or after being dropped" on every Cancel click.
- Narrows \`setupConsoleCheck\` in \`e2e/tests/helpers.ts\` so the bug class is no longer silently filtered out (the previous broad \`text.includes('wasm')\` clause caught any error whose stack trace mentioned \`wasm-function[…]\`).
- Adds Cancel-branch regression tests for \`DeleteUserSheet\` and \`EditTxDateSheet\`. \`EditPassDateSheet\` already had a Cancel test in \`redesign-sheets.spec.ts\` — that test now functions as a real gate after the filter narrowing.

Closes #84.

## Test plan

- [x] CI green: lint, test, e2e, mutation, deploy-dev, smoke-dev
- [ ] Post-deploy verification on prod (after merge): each Cancel button on each sheet → sheet closes → browser console reports zero errors
EOF
)"
```

- [ ] **Step 6: Verify PR is mergeable + clean**

```bash
gh pr view --json number,mergeable,mergeStateStatus
```

Expected: `mergeable: MERGEABLE`, `mergeStateStatus: CLEAN`. If `UNSTABLE`, `BEHIND`, or `BLOCKED` — investigate and fix per `autonomous-quality-discipline.md`. Never propose admin-merge.

- [ ] **Step 7: Wait for explicit user "merge it"**

The PR is ready for review. Do NOT merge. Wait for explicit user instruction per `pr-merge-policy.md`.

---

### Task 7: Post-deploy verification on prod (controller, AFTER user merges)

**Files:** none (verification only).

- [ ] **Step 1: Confirm main CI is green**

After merge, identify and monitor the main-branch run that includes the prod-deploy job.

```bash
gh run list --branch main --limit 3
sleep 600 && gh run view <main-run-id> --json status,conclusion,jobs
```

Expected: ALL jobs incl. Deploy (prod) + Smoke (prod) green.

- [ ] **Step 2: Backend version sanity**

```bash
curl -s https://spinbike.newlevel.media/api/version
```

Expected JSON: `{"version":"0.13.29"}`.

- [ ] **Step 3: Open prod /staff in Playwright**

Navigate via `mcp__plugin_playwright_playwright__browser_navigate` to `https://spinbike.newlevel.media/staff`. Login via the prod admin account (browser-managed credentials).

- [ ] **Step 4: Read deployed version from DOM**

Locate `[data-testid="version"]` and confirm its text equals `v0.13.29`.

- [ ] **Step 5: Exercise DeleteUserSheet Cancel on prod**

Navigate to `/reports`, click `[data-testid="reports-tab-users"]`, click any row to open `/staff?card=<code>`, click `[data-testid="delete-user-button"]`, wait for `[data-testid="sheet-delete-user"]`, click `[data-testid="delete-user-cancel"]`.

Expected: sheet closes; `mcp__plugin_playwright_playwright__browser_console_messages` returns zero errors/warnings.

- [ ] **Step 6: Exercise EditTxDateSheet Cancel on prod**

Open any user with transactions, click any transaction's date pencil (`[data-testid="txn-date-edit"]`), wait for `[data-testid="sheet-edit-tx-date"]`, click the Cancel button (filter by `hasText: /zrusit|cancel/i` since no testid).

Expected: sheet closes; console clean.

- [ ] **Step 7: Exercise EditPassDateSheet Cancel on prod (only if a card with active pass is at hand)**

Same pattern. Skip if no active pass available — covered by E2E in CI.

- [ ] **Step 8: Do NOT click Confirm on any sheet on prod**

Confirm paths are destructive (delete user, alter transaction date). Per `no-destructive-remote-actions.md` they need separate explicit user approval, which is out-of-scope for verification of #84.

- [ ] **Step 9: Send completion report**

Per `airuleset/modules/core/completion-report.md` template. Include:

- ✅ CI: green (cite main run id)
- ✅ /plan-check: 7/7 fulfilled
- ✅ /review: clean — 0 🔴 0 🟡 0 🔵
- ✅ Deploy: prod backend `/api/version` = `0.13.29`; frontend DOM `[data-testid="version"]` = `v0.13.29`. Cancel buttons exercised on DeleteUserSheet + EditTxDateSheet — modal closes, browser console clean.
- 🌐 Dev / Prod URLs
- PR ref with full title.

---

## Self-Review

### Spec coverage

- ✅ "Drop redundant set_err.set in on_cancel" → Task 4 (3 files, exact diffs).
- ✅ "Tighten setupConsoleCheck filter" → Task 2.
- ✅ "Add Cancel-branch regression tests in 3 specs" → Task 5 (2 new + verifies the existing redesign-sheets.spec.ts test).
- ✅ "Risk: filter tightening surfaces latent errors" → Task 3 (dedicated triage step before the sheet fix).
- ✅ "First commit = VERSION bump 0.13.28 → 0.13.29" → Task 1.
- ✅ "Never merge — wait for user" → Task 6 step 7.
- ✅ "Post-deploy verification" → Task 7.
- ✅ "Out of scope: Confirm path, backdrop close, Show component swap" → no task touches them.

### Placeholder scan

- No TBD/TODO/"implement later".
- Each step contains exact code or exact commands.
- Commit messages fully written.
- PR body fully written.

### Type / identifier consistency

- `setupConsoleCheck`, `assertCleanConsole`, `loginViaAPI`, `createUniqueUser` are referenced in Task 5 and exist in `e2e/tests/helpers.ts` (verified).
- `[data-testid]` selectors used in Task 5 match those in production code (`sheet-delete-user`, `delete-user-button`, `delete-user-cancel`, `sheet-edit-tx-date`, `txn-date-edit`, `transactions-list`, `transaction-row`, `action-panel`, `reports-tab-users`, `users-by-movement`, `users-by-movement-show-more`, `version`) per recent commits.
- Sheet file paths and on_cancel signatures match current source (verified at commit `ed32cef`).
