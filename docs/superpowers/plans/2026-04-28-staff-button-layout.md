# Staff Button Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reorder + recolor the four buttons in the staff card-detail action panel so Charge sits left of Topup, Fitness sits left of Spinning, and each button has a distinct color reflecting its action.

**Architecture:** Pure UI change in `spinbike-ui/src/pages/dashboard/action_form.rs` plus one new CSS modifier (`.btn--info`) in `spinbike-ui/style.css`. New Playwright test asserts DOM order + class names + console hygiene. No backend, no DB, no new dependencies.

**Tech Stack:** Leptos 0.7 (Rust → WASM via Trunk), CSS custom properties, Playwright + TypeScript.

**Spec:** [docs/superpowers/specs/2026-04-28-staff-button-layout-design.md](../specs/2026-04-28-staff-button-layout-design.md)

**Bundling:** This work is delivered as part of the existing OPEN PR #25 (CI cache + E2E diagnostics). The PR's scope expands; its title/body are updated in Task 7. New commits land directly on `dev`.

---

### Task 1: Commit spec + plan, then bump VERSION

**Files:**
- Add: `docs/superpowers/specs/2026-04-28-staff-button-layout-design.md` (already on disk, untracked)
- Add: `docs/superpowers/plans/2026-04-28-staff-button-layout.md` (already on disk, untracked)
- Modify: `VERSION`
- Modify: `Cargo.toml` (workspace root) — synced from VERSION
- Modify: `spinbike-ui/Cargo.toml` — synced from VERSION

- [ ] **Step 1: Sync from origin**

```bash
git fetch origin
git status
```

Expected: on dev, up-to-date with origin/dev. The two doc files appear as untracked.

- [ ] **Step 2: Commit the design docs**

```bash
git add docs/superpowers/specs/2026-04-28-staff-button-layout-design.md docs/superpowers/plans/2026-04-28-staff-button-layout.md
git commit -m "docs: spec + plan for staff button layout (#13)"
```

- [ ] **Step 3: Bump VERSION and sync to all Cargo.toml**

```bash
echo "0.13.5" > VERSION
bash scripts/sync-version.sh
git diff --stat
```

Expected: `VERSION`, `Cargo.toml`, `spinbike-ui/Cargo.toml` modified. The script handles workspace-version propagation.

- [ ] **Step 4: Commit the version bump**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump VERSION to 0.13.5 (bundles #13 into PR #25)"
```

Do not push yet — bundle with subsequent commits.

---

### Task 2: Write the failing Playwright test

**Files:**
- Create: `e2e/tests/dashboard-button-layout.spec.ts`

This is the RED step of TDD. The test will fail against the current layout (Topup-first, no Fitness-first sort, all `.btn--primary`). After Tasks 3–5, it must pass.

- [ ] **Step 1: Create the test file**

```typescript
import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `BL-${suffix}`;
    const lastName = `Btnlayout${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'BL', last_name: lastName }),
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

async function sellMonthlyPass(page: Page) {
    const mpOption = page
        .locator('[data-testid="charge-service"] option')
        .filter({ hasText: /Monthly pass|Mesačný preplatok/ })
        .first();
    await expect(mpOption).toBeAttached();
    const mpValue = await mpOption.getAttribute('value');
    if (!mpValue) throw new Error('Monthly pass option had no value');
    await page.locator('[data-testid="charge-service"]').selectOption(mpValue);
    await expect(page.locator('[data-testid="charge-amount"]')).not.toHaveValue('');
    const sellPassResp = page.waitForResponse(
        (r) => r.url().includes('/api/payments/sell-pass') && r.request().method() === 'POST',
    );
    await page.locator('[data-testid="charge-submit"]').click();
    const resp = await sellPassResp;
    expect(resp.ok()).toBe(true);
    await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
}

test.describe('Staff dashboard — button layout & colors (#13)', () => {
    test('action-row: Charge left of Topup, Topup is ghost-styled', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const charge = page.locator('[data-testid="charge-submit"]');
        const topup = page.locator('[data-testid="topup-submit"]');
        await expect(charge).toBeVisible();
        await expect(topup).toBeVisible();

        // Charge precedes Topup in DOM order.
        const order = await charge.evaluate((c, t) => {
            // Node.DOCUMENT_POSITION_FOLLOWING (4) means the argument follows `this`.
            return (c.compareDocumentPosition(t) & 4) === 4;
        }, await topup.elementHandle());
        expect(order).toBe(true);

        await expect(charge).toHaveClass(/\bbtn--primary\b/);
        await expect(topup).toHaveClass(/\bbtn--ghost\b/);
        await expect(topup).not.toHaveClass(/\bbtn--primary\b/);

        assertCleanConsole(msgs);
    });

    test('visit-row: Fitness left of Spinning with distinct colors', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await sellMonthlyPass(page);

        const visits = page.locator('[data-testid="log-visit-btn"]');
        await expect(visits).toHaveCount(2);

        // Migrations seed name_en values "Spinning" and "Fitness". The UI must
        // sort by name_en alphabetically: Fitness first, Spinning second.
        const labels = await visits.allTextContents();
        expect(labels[0]).toMatch(/Fitness/);
        expect(labels[1]).toMatch(/Spinning/);

        await expect(visits.nth(0)).toHaveClass(/\bbtn--info\b/);
        await expect(visits.nth(1)).toHaveClass(/\bbtn--pass\b/);

        assertCleanConsole(msgs);
    });
});
```

- [ ] **Step 2: Confirm the test fails locally OR commit and push to let CI fail it**

This project's policy is CI-only Rust builds. Either:

(a) If a local Trunk + server toolchain is available, run:

```bash
cd e2e && npx playwright test dashboard-button-layout.spec.ts
```

Expected: 2 failures — Charge does NOT precede Topup; Fitness label is NOT first.

(b) Otherwise, push the test file alone (no UI changes) and confirm the PR's E2E job fails with these two specific assertions. CI is the authoritative verifier per the user's preference.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/dashboard-button-layout.spec.ts
git commit -m "test(e2e): assert Charge-left + Fitness-left button layout (#13)"
```

---

### Task 3: Add `.btn--info` CSS modifier

**Files:**
- Modify: `spinbike-ui/style.css` (add new block after `.btn--ghost`, around line 371)

- [ ] **Step 1: Add the modifier**

Insert immediately after the `.btn--ghost:hover:not(:disabled)` block (after the closing `}` at line 371):

```css

.btn--info {
    background: var(--info);
    border-color: var(--info);
    color: var(--info-fg);
    font-weight: 600;
}
.btn--info:hover:not(:disabled) {
    background: var(--info-hover);
    border-color: var(--info-hover);
}
```

The `--info`, `--info-fg`, `--info-hover` tokens already exist in the palette block.

- [ ] **Step 2: Run formatter (CSS only — no Rust impact)**

```bash
cargo fmt --all --check
```

Expected: pass (CSS is not Rust-formatted, but routine pre-push check.)

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/style.css
git commit -m "style: add .btn--info button modifier (uses existing --info tokens)"
```

---

### Task 4: Reorder action-row + change Topup class

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs:331-354`

- [ ] **Step 1: Swap button order and update Topup class**

Replace the existing `<div class="action-row">` block (the one with the two `<button>`s for topup-submit and charge-submit) with:

```rust
            <div class="action-row">
                <button
                    type="button"
                    class="btn btn--primary"
                    data-testid="charge-submit"
                    on:click=do_charge
                    disabled=move || loading.get()
                >
                    {move || if is_monthly_pass() {
                        i18n::t(lang.get(), "sell_pass_action").to_string()
                    } else {
                        i18n::t(lang.get(), "charge").to_string()
                    }}
                </button>
                <button
                    type="button"
                    class="btn btn--ghost"
                    data-testid="topup-submit"
                    on:click=do_topup
                    disabled=move || loading.get()
                >
                    "+ "{move || i18n::t(lang.get(), "topup")}
                </button>
            </div>
```

Key changes:
- Charge `<button>` now comes FIRST.
- Topup `<button>` now uses `class="btn btn--ghost"` (was `btn btn--primary`).
- All `data-testid`, handlers, `disabled` predicates, and label expressions are byte-for-byte identical to before — only order and Topup's class change.

- [ ] **Step 2: Format**

```bash
cargo fmt --all
git diff --stat
```

Expected: only `action_form.rs` modified.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): swap Charge/Topup order, ghost-style Topup (#13)"
```

---

### Task 5: Reorder visit-row + per-name color class

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs:255-269`

- [ ] **Step 1: Replace the visit-row generator**

Find the existing `chip-row chip-row--spaced` block:

```rust
                view! {
                    <div class="chip-row chip-row--spaced">
                        {services.get().into_iter()
                            .filter(|svc| svc.is_class_visit())
                            .map(|svc| {
                                let service_id = svc.id;
                                let svc_name = svc.display_name(lang.get_untracked()).to_string();
                                view! {
                                    <button
                                        class="btn btn--compact btn--primary"
                                        data-testid="log-visit-btn"
                                        on:click=visit_click_for(service_id)
                                    >
                                        {move || i18n::t(lang.get(), "log_visit")}" "{svc_name.clone()}
                                    </button>
                                }
                            }).collect::<Vec<_>>()}
                    </div>
                }.into_any()
```

Replace with:

```rust
                view! {
                    <div class="chip-row chip-row--spaced">
                        {
                            // Sort so Fitness renders left of Spinning. is_class_visit()
                            // restricts name_en to "Fitness" | "Spinning", so a plain
                            // alphabetical sort (Fitness < Spinning) yields the right order.
                            let mut visits: Vec<_> = services.get().into_iter()
                                .filter(|svc| svc.is_class_visit())
                                .collect();
                            visits.sort_by(|a, b| a.name_en.cmp(&b.name_en));
                            visits.into_iter().map(|svc| {
                                let service_id = svc.id;
                                let svc_name = svc.display_name(lang.get_untracked()).to_string();
                                let color_cls = if svc.name_en == "Fitness" {
                                    "btn--info"
                                } else {
                                    "btn--pass"
                                };
                                view! {
                                    <button
                                        class=format!("btn btn--compact {color_cls}")
                                        data-testid="log-visit-btn"
                                        on:click=visit_click_for(service_id)
                                    >
                                        {move || i18n::t(lang.get(), "log_visit")}" "{svc_name.clone()}
                                    </button>
                                }
                            }).collect::<Vec<_>>()
                        }
                    </div>
                }.into_any()
```

- [ ] **Step 2: Format**

```bash
cargo fmt --all
```

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): Fitness-first visit row with per-activity colors (#13)"
```

---

### Task 6: Push, monitor CI, fix any failures

- [ ] **Step 1: Pre-push lint**

```bash
cargo fmt --all --check
```

Expected: clean. (Per `ci-push-discipline.md`, fix any failures locally before pushing.)

- [ ] **Step 2: Push**

```bash
git push origin dev
```

- [ ] **Step 3: Monitor the latest run to terminal state**

```bash
gh run list --branch dev --limit 1
gh run view <run-id> --json status,conclusion,jobs
```

Per `ci-monitoring.md`: poll until ALL jobs reach a terminal state (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing, Smoke (dev)). Use `sleep 300 && gh run view <run-id>` in background — do NOT use loops or watch.

Expected: all green. If E2E flake (issue #24) hits, check the uploaded `playwright-test-results` and `spinbike-server-log` artifacts before deciding rerun vs root-cause fix.

- [ ] **Step 4: Confirm post-deploy on dev**

After Smoke (dev) passes and `Deploy (dev)` is green:

```bash
# Use Playwright MCP or curl to load the dev frontend, then read the version label
curl -s https://spinbike.newlevel.media | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+(-dev\.[0-9]+)?'
```

Open `https://spinbike.newlevel.media` in the Playwright browser tool. Activate a card with monthly pass and visually confirm:
- Charge button is to the LEFT of Topup
- Topup is gray-outlined (not green-solid)
- Visit Fitness button is to the LEFT of Visit Spinning
- Each visit button has its color (Fitness = blue solid, Spinning = yellow-green solid)
- Browser console has zero errors / warnings

The `✅ Deploy:` line in the completion report must reference the version read from the live DOM.

---

### Task 7: Update existing PR #25 to reflect combined scope

- [ ] **Step 1: Update title + body**

```bash
gh pr edit 25 \
  --title "feat: CI cache + E2E diagnostics + Charge/Topup + Fitness/Spinning button layout (v0.13.5)" \
  --body "$(cat <<'EOF'
## Summary

CI infra:
- npm + Playwright browser cache across all 3 e2e jobs
- Upload Playwright `test-results/` artifact on E2E failure
- Capture & upload server log on E2E failure (with `RUST_LOG=spinbike_server=info`)

Staff dashboard button layout (#13):
- Charge sits LEFT of Topup in the action row (most-used action on the left)
- Topup uses `.btn--ghost` (outlined / muted)
- Log Visit row sorts Fitness LEFT of Spinning
- Visit Fitness uses `.btn--info` (blue), Visit Spinning uses `.btn--pass` (yellow-green) — each activity has its own stable color
- New `.btn--info` CSS modifier (reuses existing `--info` palette tokens)
- New Playwright test `dashboard-button-layout.spec.ts` asserts DOM order + class names + zero console errors

Closes #13.
Spec: `docs/superpowers/specs/2026-04-28-staff-button-layout-design.md`
Plan: `docs/superpowers/plans/2026-04-28-staff-button-layout.md`

## Test plan
- [x] All Cargo tests pass (CI)
- [x] All Playwright E2E tests pass (CI), including new `dashboard-button-layout.spec.ts`
- [x] Mutation testing passes (CI)
- [x] Smoke (dev) verifies dashboard renders with new layout
- [x] Manual Playwright check on dev frontend — confirmed Fitness-left, Charge-left, colors, zero console errors
EOF
)"
```

- [ ] **Step 2: Verify mergeable**

```bash
gh pr view <pr-number> --json mergeable,mergeStateStatus
```

Expected: `mergeable: "MERGEABLE"`, `mergeStateStatus: "CLEAN"`. If `UNSTABLE` / `BLOCKED` / `BEHIND` / `DIRTY`, fix the underlying cause per `autonomous-quality-discipline.md`.

- [ ] **Step 3: Run /plan-check and /review**

Per `completion-report.md` pre-completion gate: invoke `Skill(skill: "plan-check")` and apply the `/review` standards. Address every 🔴 / 🟡 / 🔵 finding inside this PR's diff. Re-run until both come back clean.

- [ ] **Step 4: Send completion report**

Use the EXACT template from `completion-report.md`. The audits block must include `✅ /plan-check: N/N fulfilled` and `✅ /review: clean — 0 🔴 0 🟡 0 🔵`. Goal + What changed lines must be in plain user-facing language. Include `🌐 Dev frontend`, `🌐 Dev backend`, `🌐 Prod frontend`, `🌐 Prod backend` URLs (read project CLAUDE.md for the URL list). Final line is the PR URL — STOP there per `pr-merge-policy.md`.
