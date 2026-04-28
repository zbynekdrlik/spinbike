# No Predefined Prices on Staff Dashboard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the staff dashboard amount input always start empty when staff picks a service, and drop the price annotation from the service dropdown labels — so staff types the price every time. Admin and the 4-hour Spinning class auto-charger keep using `default_price` unchanged.

**Architecture:** UI-only change in one Leptos file (`spinbike-ui/src/pages/dashboard/action_form.rs`). Two edits: delete the `el.set_value(...)` auto-fill block in `on_service_change`, and simplify the dropdown `format!` to drop the `(price €)` part. No backend, no schema, no migration, no API contract change.

**Tech Stack:** Leptos 0.7 CSR (WASM via Trunk), Playwright E2E (TypeScript), GitHub Actions CI.

**Spec:** `docs/superpowers/specs/2026-04-28-no-predefined-prices-design.md`

**Issue:** [#17](https://github.com/zbynekdrlik/spinbike/issues/17)

---

## Pre-flight notes for the implementer

- **This work bundles into the existing OPEN PR #25** (button layout, v0.13.6). Same file (`action_form.rs`) is touched, and the user explicitly asked to deliver both changes at once. PR #25 is currently `MERGEABLE` + `clean`. Pushing the new commits to `dev` will retrigger PR #25's CI — that is expected.
- The repo already has 3 unpushed local commits on `dev` for issue #17: `dc8d0c1` (spec), `4c5358f` (spec correction), `a82439c` (this plan). They will go up with the next push.
- Do NOT merge PR #25 first. Do NOT sync dev with main. The work continues on the same `dev` branch ahead of `origin/dev` and folds into PR #25's diff.
- Do NOT run `cargo build`, `cargo test`, `cargo clippy`, or `trunk build` locally. CI is the authoritative gate. The only allowed local check is `cargo fmt --all --check` per `CLAUDE.md`.
- After every push, monitor CI to terminal state per `ci-monitoring.md` — single `sleep N && gh run view --json status,conclusion,jobs` background command, no scheduled polling.
- Frequent commits per task. No `--amend`. No history rewrite.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `VERSION` | Single source of truth for project version | Bump dev from `0.13.6` (PR #25's earlier scope) to `0.13.7` (this work bundled into PR #25 per CEO direction) |
| `Cargo.toml`, `crates/*/Cargo.toml`, `spinbike-ui/Cargo.toml` | Per-crate version | Synced via `scripts/sync-version.sh` |
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Staff action panel (charge / sell pass / topup) | Delete auto-fill block in `on_service_change` (~lines 68-77); simplify dropdown label `format!` (~line 308) |
| `e2e/tests/no-predefined-prices.spec.ts` | NEW — Playwright E2E for the new behavior | Asserts: empty input on service change, dropdown labels without prices, empty submit shows `.alert-error`, typed amount still works, zero console errors |
| `e2e/tests/dashboard.spec.ts:141` | Existing charge-flow test | Update misleading comment only — test logic still works because it already calls `.fill('5')` after `selectOption` |
| `e2e/tests/card-action-form.spec.ts:83` | Existing sell-pass test | **Real change:** add `.fill('35.00')` before `charge-submit` click, since the test relied on the auto-fill to populate the amount |
| `e2e/tests/card-action-form.spec.ts:127` | Existing empty-amount-error test | Update misleading comment only — test logic still works (input is empty post-`selectOption`, the `Ctrl+A; Delete` clear is redundant but harmless) |

---

## Task 1: Bump VERSION to 0.13.7 (bundled scope of PR #25)

**Files:**
- Read: `VERSION`
- Modify: `VERSION`, `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml` (the last 4 are written by `scripts/sync-version.sh`)

- [ ] **Step 1: Confirm we're on the right branch and ahead of origin/dev**

```bash
git rev-parse --abbrev-ref HEAD
git log --oneline origin/dev..HEAD
```

Expected: branch is `dev`. The 3 unpushed local commits (`dc8d0c1`, `4c5358f`, `a82439c`) appear in `git log` — all `docs(spec):` / `docs(plan):` for #17. Nothing else.

If anything else appears, STOP and ask the controller — there's drift to investigate.

- [ ] **Step 2: Read current VERSION**

```bash
cat VERSION
```

Expected: `0.13.6` (the version PR #25 currently ships).

- [ ] **Step 3: Bump VERSION to 0.13.7**

```bash
echo "0.13.7" > VERSION
```

This bumps PR #25's scope from v0.13.6 (button layout only) to v0.13.7 (button layout + no predefined prices).

- [ ] **Step 4: Run sync-version.sh to propagate to all Cargo.toml files**

```bash
scripts/sync-version.sh
```

Expected: no errors. The script edits `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, and `spinbike-ui/Cargo.toml` to set `version = "0.13.7"`.

- [ ] **Step 5: Verify the sync**

```bash
grep '^version' Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
```

Expected: every line shows `version = "0.13.7"`.

- [ ] **Step 6: Commit the version bump**

```bash
git add VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump VERSION to 0.13.7 (bundles #17 into PR #25)"
```

---

## Task 2: Write the new Playwright test (TDD — RED)

**Files:**
- Create: `e2e/tests/no-predefined-prices.spec.ts`

- [ ] **Step 1: Create the new Playwright test file**

Write this exact content to `e2e/tests/no-predefined-prices.spec.ts`:

```typescript
import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `NP-${suffix}`;
    const lastName = `NoPrePrice${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'NP', last_name: lastName }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    return { barcode, lastName };
}

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Staff dashboard — no predefined prices (#17)', () => {
    test('service dropdown labels show only the service name (no euro, no number)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        const labels = await page
            .locator('[data-testid="charge-service"] option')
            .allTextContents();

        // Filter out the placeholder "(select service)" option.
        const realLabels = labels.filter((l) => l.trim().length > 0 && !/select service/i.test(l));
        expect(realLabels.length).toBeGreaterThan(0);

        for (const label of realLabels) {
            // No euro symbol.
            expect(label).not.toContain('€');
            // No N.NN numeric price.
            expect(label).not.toMatch(/\d+\.\d{2}/);
            // No parenthesised price annotation like "(5.00 €)".
            expect(label).not.toMatch(/\(.*\)/);
        }

        assertCleanConsole(msgs);
    });

    test('amount input stays empty when staff picks Spinning, Fitness, or Monthly pass', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        const amountInput = page.locator('[data-testid="charge-amount"]');
        const select = page.locator('[data-testid="charge-service"]');

        // The seed creates Spinning, Fitness, and Monthly pass. Pick each by
        // its option text rather than by index — index is unstable across
        // ordering changes.
        for (const name of ['Spinning', 'Fitness', 'Monthly pass']) {
            const option = select.locator('option').filter({ hasText: name }).first();
            const optValue = await option.getAttribute('value');
            expect(optValue, `option "${name}" missing value`).toBeTruthy();
            await select.selectOption(optValue!);
            await expect(amountInput).toHaveValue('');
        }

        assertCleanConsole(msgs);
    });

    test('submit empty amount surfaces inline error and posts no payment request', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        // Confirm starting credit before any submit.
        const creditLocator = page.locator('[data-testid="card-credit"]');
        await expect(creditLocator).toContainText('50.00');

        // Pick Spinning (input stays empty post-#17).
        const select = page.locator('[data-testid="charge-service"]');
        const spinningOption = select.locator('option').filter({ hasText: 'Spinning' }).first();
        const spinningValue = await spinningOption.getAttribute('value');
        await select.selectOption(spinningValue!);
        await expect(page.locator('[data-testid="charge-amount"]')).toHaveValue('');

        // Track any payment POST that fires during the next 1s. We expect zero.
        let paymentRequestFired = false;
        const offRequest = (req: import('@playwright/test').Request) => {
            if (
                /\/api\/payments\/(charge|sell-pass)/.test(req.url())
                && req.method() === 'POST'
            ) {
                paymentRequestFired = true;
            }
        };
        page.on('request', offRequest);

        await page.locator('[data-testid="charge-submit"]').click();

        // Inline error appears.
        await expect(
            page.locator('[data-testid="action-panel"] .alert-error'),
        ).toBeVisible();

        // Card credit unchanged.
        await expect(creditLocator).toContainText('50.00');

        // Give the page 500ms to fire any async POST. None should.
        await page.waitForTimeout(500);
        page.off('request', offRequest);
        expect(paymentRequestFired).toBe(false);

        assertCleanConsole(msgs);
    });

    test('typed amount still works end-to-end (charge debits the typed value)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        const select = page.locator('[data-testid="charge-service"]');
        const spinningOption = select.locator('option').filter({ hasText: 'Spinning' }).first();
        const spinningValue = await spinningOption.getAttribute('value');
        await select.selectOption(spinningValue!);

        // Staff types the price.
        await page.locator('[data-testid="charge-amount"]').fill('7.50');

        const chargeResp = page.waitForResponse(
            (r) => r.url().includes('/api/payments/charge') && r.request().method() === 'POST',
        );
        await page.locator('[data-testid="charge-submit"]').click();
        const resp = await chargeResp;
        expect(resp.ok()).toBe(true);

        // Card credit dropped by 7.50.
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('42.50');

        assertCleanConsole(msgs);
    });
});
```

- [ ] **Step 2: Run the new test against the CURRENT (unchanged) code to verify it fails**

```bash
cd e2e && npx playwright test tests/no-predefined-prices.spec.ts --project=chromium --reporter=line
```

Expected: 4 tests, **3-4 of them FAIL** with messages like:
- `Expected: not contain "€"` — dropdown label test
- `Expected: '' / Received: '5.00'` — empty-input test
- (possibly) the empty-submit test fails too because the auto-fill makes input non-empty by the time the submit fires

The "typed amount works" test may pass even before the change (it explicitly fills the value), which is fine.

If ALL FOUR pass before any code change, the test is broken — stop and reinvestigate. If 3-4 fail as expected, proceed.

- [ ] **Step 3: Commit the failing test**

```bash
git add e2e/tests/no-predefined-prices.spec.ts
git commit -m "test(e2e): no-predefined-prices spec (RED, #17)"
```

---

## Task 3: Make the code changes (TDD — GREEN)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs`

- [ ] **Step 1: Open `spinbike-ui/src/pages/dashboard/action_form.rs` and locate the `on_service_change` closure (~lines 58-77)**

The current code looks like this:

```rust
let on_service_change = move |_| {
    let raw = service_ref
        .get()
        .map(|el| {
            let el: &HtmlSelectElement = &el;
            el.value()
        })
        .unwrap_or_default();
    let id: Option<i64> = raw.parse().ok();
    set_selected_service_id.set(id);
    if let Some(id) = id {
        if let Some(svc) = services.get().iter().find(|s| s.id == id) {
            if let Some(el) = amount_ref.get() {
                let el: &HtmlInputElement = &el;
                el.set_value(&format!("{:.2}", svc.default_price));
            }
        }
    }
};
```

- [ ] **Step 2: Replace the closure body — delete the auto-fill block**

Replace the closure (Edit `old_string` is the whole closure above; `new_string` is below):

```rust
let on_service_change = move |_| {
    let raw = service_ref
        .get()
        .map(|el| {
            let el: &HtmlSelectElement = &el;
            el.value()
        })
        .unwrap_or_default();
    let id: Option<i64> = raw.parse().ok();
    set_selected_service_id.set(id);
    // Auto-fill from default_price was removed (#17). Staff types the price
    // every time. The is_monthly_pass() helper still reads
    // selected_service_id, so the date-row visibility and Sell-vs-Charge
    // submit-label flip continue to work.
};
```

This deletes the `if let Some(id) = id { ... el.set_value(...) }` block. The `services`, `amount_ref`, and `HtmlInputElement` captures that were only used by that block become unused; rustc/clippy will tell you if any of those need pruning from the outer `let` bindings — leave that to clippy on CI.

- [ ] **Step 3: Locate the dropdown `<option>` label `format!` (~line 308)**

The current code looks like this:

```rust
{move || {
    let lang_now = lang.get();
    services.get().into_iter().map(|s| {
        let val = s.id.to_string();
        let kind = s.kind.clone();
        let label = format!("{} ({:.2} €)", s.display_name(lang_now), s.default_price);
        view! { <option value=val data-kind=kind>{label}</option> }
    }).collect::<Vec<_>>()
}}
```

- [ ] **Step 4: Drop the `(price €)` part of the label**

Replace the `let label = ...` line:

```rust
{move || {
    let lang_now = lang.get();
    services.get().into_iter().map(|s| {
        let val = s.id.to_string();
        let kind = s.kind.clone();
        // No price annotation (#17) — staff sees just the service name.
        let label = s.display_name(lang_now).to_string();
        view! { <option value=val data-kind=kind>{label}</option> }
    }).collect::<Vec<_>>()
}}
```

- [ ] **Step 5: Run the local fmt check**

```bash
cargo fmt --all --check
```

Expected: exit code 0, no diff. If fmt complains, run `cargo fmt --all` and re-verify.

- [ ] **Step 6: Re-run the new Playwright test against the changed code**

```bash
cd e2e && npx playwright test tests/no-predefined-prices.spec.ts --project=chromium --reporter=line
```

Expected: 4/4 PASS.

If any test still fails, read the error and the page snapshot in `e2e/test-results/`. Common pitfalls:
- The dev server hasn't been rebuilt with `trunk build` — but per project rules we DO NOT run `trunk build` locally. Instead, push to CI and let CI rebuild. Or, if running E2E locally, use the project's standard E2E start sequence (the controller will tell you the correct invocation).

If the controller is running E2E in CI rather than locally, skip Step 6 and rely on Task 6's CI run.

- [ ] **Step 7: Commit the implementation**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): no predefined prices on staff dashboard (#17)

Delete the on_service_change auto-fill from default_price.
Drop the (price €) annotation from service dropdown labels.
Staff types the price every time. Admin and the 4-hour Spinning
auto-charger keep using default_price unchanged."
```

---

## Task 4: Update the 3 existing E2E tests (1 real fix + 2 comment cleanups)

**Files:**
- Modify: `e2e/tests/dashboard.spec.ts`
- Modify: `e2e/tests/card-action-form.spec.ts` (two locations)

- [ ] **Step 1: Update `e2e/tests/dashboard.spec.ts` around line 141**

Current code:

```typescript
        // Pick the first (non-placeholder) service — global-setup seeds "Spinning".
        const select = page.locator('[data-testid="charge-service"]');
        await select.selectOption({ index: 1 });

        // The amount input should auto-fill from default_price. Override to 5 for a
        // deterministic charge that never exceeds the card balance.
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.fill('5');
```

Replace the comment block with one that reflects post-#17 reality. Use Edit:

`old_string`:
```
        // The amount input should auto-fill from default_price. Override to 5 for a
        // deterministic charge that never exceeds the card balance.
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.fill('5');
```

`new_string`:
```
        // Staff types the amount every time (#17 — no predefined prices).
        // 5 is below the card's starting balance so the charge always succeeds.
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.fill('5');
```

The `.fill('5')` call is unchanged. Only the comment is updated.

- [ ] **Step 2: Update `e2e/tests/card-action-form.spec.ts` around line 83 (the sell-pass test) — REAL FIX**

Current code:

```typescript
        await selectMonthlyPass(page);
        // Amount auto-filled from default_price (35.00). Accept it.
        await page.locator('[data-testid="charge-submit"]').click();
```

Replace with an explicit `.fill('35.00')` step (post-#17 the input stays empty after `selectMonthlyPass`):

`old_string`:
```
        await selectMonthlyPass(page);
        // Amount auto-filled from default_price (35.00). Accept it.
        await page.locator('[data-testid="charge-submit"]').click();
```

`new_string`:
```
        await selectMonthlyPass(page);
        // Staff types the price every time (#17 — no auto-fill). Use 35.00
        // to match the assertion `expect(body.price).toBe(35.0)` below.
        await page.locator('[data-testid="charge-amount"]').fill('35.00');
        await page.locator('[data-testid="charge-submit"]').click();
```

The downstream `expect(body.price).toBe(35.0)` assertion is unchanged.

- [ ] **Step 3: Update `e2e/tests/card-action-form.spec.ts` around line 127 (the empty-amount-error test) — comment only**

Current code:

```typescript
        // Pick a non-pass service so default_price auto-fills, then clear it.
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.focus();
        await amountInput.press('ControlOrMeta+a');
        await amountInput.press('Delete');
        await expect(amountInput).toHaveValue('');
```

Replace ONLY the comment line:

`old_string`:
```
        // Pick a non-pass service so default_price auto-fills, then clear it.
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
```

`new_string`:
```
        // Pick a non-pass service. Post-#17 the input is already empty after
        // selectOption — the Ctrl+A; Delete clear below is redundant but
        // harmless and kept defensively in case a future regression
        // reintroduces auto-fill.
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
```

The rest of the test (focus, ctrl+A, delete, expect empty) is unchanged.

- [ ] **Step 4: Commit the test updates**

```bash
git add e2e/tests/dashboard.spec.ts e2e/tests/card-action-form.spec.ts
git commit -m "test(e2e): update charge/sell-pass tests for #17 (no auto-fill)

dashboard.spec.ts:141 — comment cleanup; .fill('5') already explicit.
card-action-form.spec.ts:83 — real fix: .fill('35.00') before submit
  (test relied on auto-fill to populate amount).
card-action-form.spec.ts:127 — comment cleanup; selectOption now
  leaves input empty, the Ctrl+A; Delete clear is redundant but kept."
```

---

## Task 5: Push and monitor CI to terminal state

**Files:** none (push action only)

- [ ] **Step 1: Push to dev**

```bash
git push origin dev
```

This pushes commits from Tasks 1-4 (version bump + new test + impl + test updates) plus any pre-existing local commits (the spec doc commit `dc8d0c1` and any spec-correction follow-ups).

- [ ] **Step 2: Identify the CI run**

```bash
sleep 10 && gh run list --branch dev --event push --limit 3 --json databaseId,status,conclusion,headSha,name
```

The newest run with status `in_progress` or `queued` and `headSha` matching `git rev-parse HEAD` is the one to monitor.

- [ ] **Step 3: Monitor the CI run to terminal state (single background command)**

```bash
sleep 600 && gh run view <run-id> --json status,conclusion,jobs
```

(Run via Bash with `run_in_background: true`, then read with `BashOutput` when it completes — per `ci-monitoring.md`. Do not poll, do not use `gh run watch`.)

Expected end state: `status: completed`, `conclusion: success`, all jobs (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing) green.

- [ ] **Step 4: If any job fails, investigate and fix in ONE commit**

```bash
gh run view <run-id> --log-failed
```

Common fail modes for this PR:
- `cargo fmt --check`: run `cargo fmt --all` locally and recommit.
- `clippy -D warnings`: an unused variable from the deleted auto-fill block. Remove the unused capture from the closure or its outer `let`.
- Mutation testing finds a survivor in the changed `format!`: the new label has no operators to mutate; if it surfaces, the survivor is likely in a previously-existing line and unrelated. Read the report and fix.
- E2E flake (#24, SQLITE_BUSY): per `ci-monitoring.md`, "One rerun is acceptable to rule out transient issues". If a single E2E test fails on the documented `(code: 5) database is locked` line and a rerun passes, accept the rerun. If it fails twice, escalate.

After fix, push and re-monitor.

- [ ] **Step 5: All green — proceed to Task 6**

No commit in this task.

---

## Task 6: Update PR #25 title and body to reflect bundled scope, verify mergeable

**Files:** none (PR metadata only — PR #25 already exists)

- [ ] **Step 1: Update PR #25 title and body via the API**

`gh pr edit` hits a deprecated GraphQL projects-classic codepath in this repo's setup, so use the REST API directly:

```bash
gh api -X PATCH repos/zbynekdrlik/spinbike/pulls/25 \
  -f title="feat: button layout + no predefined prices on staff dashboard (#13 #17, v0.13.7)" \
  -f body="$(cat <<'EOF'
## Summary

This PR bundles two staff-dashboard changes plus the CI cache + E2E diagnostics work that landed earlier on the branch.

### #13 — Charge/Topup button order, Fitness/Spinning visit order, soft-sibling colors (v0.13.6)

- **Charge** moves left, keeps `.btn--primary` (solid blue, most-used action).
- **Topup** moves right, uses `.btn--primary-soft` (same blue hue, low saturation).
- **Visit Fitness** is left of **Visit Spinning** in the visit row.
- **Visit Fitness** keeps `.btn--info`, **Visit Spinning** uses `.btn--info-soft`.
- Three new CSS modifiers added (`.btn--info`, `.btn--primary-soft`, `.btn--info-soft`).
- New Playwright test `e2e/tests/dashboard-button-layout.spec.ts` asserts DOM order + class names + zero console errors.

### #17 — No predefined prices on staff dashboard (v0.13.7)

- Staff types the price every time when charging a service or selling a monthly pass. No more auto-fill from `default_price`.
- Service dropdown labels show only the name (\"Spinning\", \"Fitness\", \"Monthly pass\"). The `(5.00 €)` annotation is gone.
- Admin services CRUD page is unchanged. The 4-hour Spinning class auto-charger (\`crates/spinbike-server/src/jobs/charger.rs\`) still uses \`default_price\` to bill booked classes — no functional impact on automatic billing.
- New Playwright test \`e2e/tests/no-predefined-prices.spec.ts\` asserts dropdown labels without prices, empty input on service change, empty-submit inline error, and the typed-amount path end-to-end with zero console errors.
- Three existing E2E tests updated: \`card-action-form.spec.ts:83\` now \`.fill('35.00')\` before submit (was auto-filled); two comment cleanups in \`dashboard.spec.ts:141\` and \`card-action-form.spec.ts:127\`.

### CI / E2E diagnostics (carried over from earlier on the branch)

- All 3 E2E jobs cache npm + Playwright browsers across runs.
- Server stdout/stderr captured to \`/tmp/spinbike-server.log\` and uploaded as artifact on E2E failure.
- \`RUST_LOG=spinbike_server=info\` on the E2E server invocation so \`internal_error\` lines surface.
- These diagnostics already paid off — flake #24 (\`SQLITE_BUSY\` writer race) was root-caused with concrete server-log evidence on this branch.

Closes #13. Closes #17.

## Test plan

- [x] CI green on push and PR runs — all jobs (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing).
- [x] New Playwright test for button layout (\`dashboard-button-layout.spec.ts\`).
- [x] New Playwright test for no-predefined-prices (\`no-predefined-prices.spec.ts\`).
- [x] Existing E2E tests still pass (with the 3 small updates above).
- [ ] Post-deploy on dev frontend: pick each service, confirm input stays empty, type 7.50 + Charge to confirm typed-amount path works end-to-end. Read DOM version label, confirm v0.13.7.
- [ ] Auto-charger regression: existing \`charger_*\` unit tests remain green without modification.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 2: Confirm the PR title and body updated**

```bash
gh pr view 25 --json title,body | jq -r '.title, "---", (.body | .[:300])'
```

Expected: title contains both `#13` and `#17` and `v0.13.7`; body opens with the new Summary section.

- [ ] **Step 3: Wait for the latest CI run on PR #25 to complete**

The push from Task 5 already retriggered PR #25's CI. Monitor the same run:

```bash
sleep 10 && gh pr view 25 --json statusCheckRollup -q '.statusCheckRollup[] | {name, status, conclusion}'
```

Expected end state: every check `conclusion: SUCCESS`.

- [ ] **Step 4: Verify PR #25 is mergeable + clean**

```bash
gh api repos/zbynekdrlik/spinbike/pulls/25 --jq '{mergeable: .mergeable, mergeable_state: .mergeable_state}'
```

Expected:
```json
{"mergeable":true,"mergeable_state":"clean"}
```

If `mergeable_state` is anything other than `"clean"` (e.g. `unstable`, `behind`, `blocked`, `dirty`), STOP and investigate. Per `autonomous-quality-discipline.md`, UNSTABLE is not mergeable. Fix the cause before reporting done.

- [ ] **Step 5: Report PR URL to the controller**

Output: `https://github.com/zbynekdrlik/spinbike/pull/25` — mergeable, clean. Awaiting user "merge it".

**Per `pr-merge-policy.md`, do NOT merge. The user merges explicitly. Task 6 ends here.**

---

## Task 7: Post-deploy verification (after user merges)

**Files:** none (verification only)

This task runs ONLY after the user explicitly tells the agent to merge (or merges manually). Until then, the implementation is not "done".

- [ ] **Step 1: Wait for main branch CI + deploy job to complete**

```bash
# Identify the main run triggered by the merge.
sleep 60 && gh run list --branch main --limit 3 --json databaseId,status,conclusion,headSha,name
# Then monitor:
sleep 600 && gh run view <main-run-id> --json status,conclusion,jobs
```

Expected: deploy-dev and smoke-dev jobs succeed.

- [ ] **Step 2: Verify the dev frontend reads v0.13.7 from the DOM**

Per `version-on-dashboard.md` and `post-deploy-verification.md`: open the dev frontend in Playwright and read the version label.

```typescript
// Inline Playwright session via mcp__plugin_playwright_playwright__* tools:
// 1. browser_navigate to http://10.77.8.134:3000 (or the dev URL — read project CLAUDE.md for the canonical IP)
// 2. browser_evaluate: document.querySelector('[data-testid="version"]').textContent
// 3. assert it matches /^v0\.13\.7(-dev\.\d+)?(\s\([0-9a-f]{7}.*\))?$/
```

If the version label does NOT match v0.13.7, the deploy failed silently. Investigate (CDN cache, build skipped, wrong host) before reporting done.

- [ ] **Step 3: Functional verification on dev — Playwright through the staff dashboard**

```typescript
// 1. Navigate to /staff?lang=en (or run loginViaAPI then navigate)
// 2. Search for an existing test card by last name OR activate a fresh one via the API
// 3. Open the card detail
// 4. browser_snapshot the action panel — confirm dropdown options are "Spinning" / "Fitness" / "Monthly pass" with NO "€" or numeric price visible
// 5. selectOption "Spinning" — read [data-testid="charge-amount"] value, confirm '' (empty)
// 6. selectOption "Fitness" — confirm input still ''
// 7. selectOption "Monthly pass" — confirm input still ''
// 8. Type 7.00 into [data-testid="charge-amount"], click [data-testid="charge-submit"]
// 9. Wait for /api/payments/charge POST, confirm 2xx
// 10. Confirm card credit dropped by 7.00 in [data-testid="card-credit"]
// 11. Read browser_console_messages — confirm zero errors / zero warnings
```

- [ ] **Step 4: Send the completion report**

Per `completion-report.md`, send the EXACT template:

```
## ✅ Work Complete

**Audits & deploy:**
✅ CI: green (all jobs on push and PR runs)
✅ /plan-check: 7/7 fulfilled
✅ /review: clean — 0 🔴 0 🟡 0 🔵
✅ Deploy: dev frontend shows v0.13.7 (matches backend /api/version); selecting Spinning/Fitness/Monthly pass leaves the amount input empty; charging €7 via the typed-amount path debited the card to the new balance; console clean.

**Plan steps:**
- Bumped VERSION to 0.13.7 (bundled into open PR #25)
- Added Playwright test asserting dropdown shows only service names, input stays empty, empty submit shows error, typed amount works
- Removed auto-fill of amount from default_price in action_form.rs
- Removed price annotation from service dropdown labels
- Updated 3 existing E2E tests (1 real fix, 2 comment cleanups)
- Verified deploy on dev: typed-amount workflow works end-to-end

---

**Goal:** Make staff type the price every time on the dashboard, and stop showing predefined prices next to service names.
**What changed:** When staff picks a service, the amount stays empty — they type it. Service names show without prices. Admin and automatic class billing are unchanged.

🌐 Dev:  http://10.77.8.134:3000
🌐 Prod: <prod URL from project CLAUDE.md>

**[spinbike] PR #25: feat: button layout + no predefined prices on staff dashboard (#13 #17, v0.13.7)**
https://github.com/zbynekdrlik/spinbike/pull/25 — merged
```

If the deploy verification surfaces any console error, broken behavior, or version mismatch — DO NOT send the report. Investigate and fix first.

---

## Self-review (the planner's checklist, ran inline)

**Spec coverage:** Every spec section maps to a task —
- "Service dropdown label" → Task 3 Step 4.
- "Amount input on service change" → Task 3 Step 2.
- "Submit guard (inline error preserved)" → Task 2 Step 1's third test case + Task 4 Step 3 (kept-defensively comment).
- "Topup unchanged" → no task needed; not modified.
- "What does NOT change" → out of scope; explicitly enforced by the closed file list in File Structure.
- "Files affected" — every file in the spec table has at least one task step.
- "Testing (TDD)" → Task 2 (RED) → Task 3 (GREEN) → Task 4 (existing tests) → Task 5 (full E2E pass).
- "Versioning" → Task 1.
- "Acceptance criteria" → covered by the new test cases plus Task 7 functional verification.

**Placeholder scan:** None. Every step has the actual command or actual code. No "implement later" / "TBD".

**Type consistency:** Method/property names referenced match: `[data-testid="charge-service"]`, `[data-testid="charge-amount"]`, `[data-testid="charge-submit"]`, `[data-testid="card-credit"]`, `[data-testid="action-panel"]`, `[data-testid="search-result"]`, `.alert-error`, `loginViaAPI`, `setupConsoleCheck`, `assertCleanConsole`. All exist in the current `e2e/tests/helpers.ts` and the `action_form.rs` view tree (verified by grep prior to writing this plan).

**No gaps found.**
