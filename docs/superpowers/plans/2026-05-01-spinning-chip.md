# Spinning Quick-Charge Chip Implementation Plan (#34)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-05-01-spinning-chip-design.md` (committed at `bdb7540` on dev)

**Goal:** Re-add the `Spinning {price}€` quick-charge chip to the staff card desk so the most common transaction takes one click. Avoids the txn-list regression from PR #35 by snapshotting the Spinning service at component mount instead of subscribing the chip's view to the `services` signal. Bundles cleanup #38.

**Architecture:** In `action_form.rs`, build the chip view ONCE at component mount via `services.get_untracked()`, store the result as `Option<AnyView>`, and emit it as a static node in the `view!` macro. No `{move || services.get() ...}` wrapper. Click handler is identical-shape to the existing `do_charge` (`POST /api/payments/charge` with `service_id` + `amount` from the snapshot). `ActionForm` re-mounts whenever the parent's `selected` signal changes — so each card open re-runs the snapshot, picking up admin price edits made between card opens for free.

**Tech Stack:** Rust 1.x, Leptos 0.7 CSR/WASM, Axum 0.8 server, sqlx + SQLite WAL, Trunk, Playwright + Chromium for E2E. CI: GitHub Actions (lint + test + build-wasm + e2e + mutation + deploy-dev/prod + smoke).

**Branch state at plan time:** dev is up-to-date with main (PR #37 merged at `c3e38b6`). VERSION on both is `0.13.11`. Two unpushed commits on dev: spec `b4510ec` + spec testid fix `bdb7540`.

---

## File Structure

| File | Responsibility | Touched by |
|---|---|---|
| `VERSION` | Single-source-of-truth semver string | Task 1 |
| `Cargo.toml` (workspace + crates + spinbike-ui) | Cargo `version` fields, kept in sync via script | Task 1 (auto-synced) |
| `spinbike-ui/src/pages/dashboard/transactions_list.rs` | Card transactions list view; expose stable testids for E2E | Task 2 |
| `e2e/tests/desk-ux.spec.ts` | Staff desk UX cluster Playwright spec; receives 3 new tests for #34 | Task 3 |
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Card action form; receives `spinning_chip` snapshot + view insertion + #38 cleanup | Task 4 |

No new files are created by this plan.

---

## Project Conventions (read once, apply throughout)

- **Branch:** all work happens on `dev`. Open the PR to `main` only after CI is green.
- **Local checks:** `cargo fmt --all --check` is the ONLY allowed local check. Do NOT run `cargo build`, `cargo test`, `cargo clippy`, or `trunk build` locally — CI is authoritative. (Repo memory: `feedback_subagent_no_local_build.md`.)
- **Git staging:** NEVER use `git add -A` or `git add .`. Always use explicit paths or `git add -u`. (Repo memory: `feedback_no_git_add_A.md`.)
- **Commits:** imperative mood, conventional-commit prefix (`chore:`, `feat:`, `test:`, `fix:`, `docs:`). Never amend, never rebase, never force-push.
- **CI monitoring:** after each push, monitor with a SINGLE backgrounded `sleep N && gh run view <run-id> --json status,conclusion,jobs`. Do NOT use `gh run watch` (rate-limit risk) or custom polling loops. (Repo memory: `ci-monitoring.md`.)
- **PR merge:** never auto-merge. Plan ends at "PR mergeable, awaiting user merge." (Repo memory: `pr-merge-policy.md`.)

---

## Task 1: Bump VERSION to 0.13.12

**Why first:** Both `main` and `dev` are at `0.13.11` after PR #37 merged. The CI version-bump check fails any PR where dev's VERSION is not strictly higher than main's. Bumping first is a 5-second safety net vs. discovering it after a 15-minute CI run.

**Files:**
- Modify: `VERSION`
- Auto-modified by `scripts/sync-version.sh`: `Cargo.toml`, `spinbike-ui/Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`

- [ ] **Step 1: Confirm starting version**

```bash
cat VERSION
```

Expected output: `0.13.11`

- [ ] **Step 2: Edit VERSION**

Replace contents of `VERSION` with `0.13.12` (single line, no trailing newline beyond the file's existing convention — the existing file is fine if you simply replace the version string).

- [ ] **Step 3: Sync version into all Cargo.toml files**

```bash
bash scripts/sync-version.sh
```

Expected: script edits all `version = "..."` lines in `Cargo.toml`, `spinbike-ui/Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml` to `0.13.12`. No errors.

- [ ] **Step 4: Confirm sync result**

```bash
grep -E '^version = "' Cargo.toml spinbike-ui/Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml
```

Expected: every line shows `version = "0.13.12"`.

- [ ] **Step 5: Commit**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml
git commit -m "chore: bump version to 0.13.12

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

Verify: `git log --oneline -1` shows the new commit.

---

## Task 2: Add stable E2E testids to transactions_list.rs

**Why this comes before the E2E tests:** the tests in Task 3 reference `[data-testid="transactions-list-empty"]` and `[data-testid="transaction-row"]`. Adding the testids first means the tests fail in Task 3 only because of the chip's absence (the *intended* RED), not because of missing selectors.

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/transactions_list.rs` (lines 49 and ~120)

The outer wrapper `<div class="group" data-testid="transactions-list">` near the bottom already exists — do NOT duplicate it.

- [ ] **Step 1: Open transactions_list.rs and locate the empty-state line**

Read `spinbike-ui/src/pages/dashboard/transactions_list.rs`. Confirm line 49 reads exactly:

```rust
                    <div class="empty-state">{move || i18n::t(lang.get(), "no_transactions_card")}</div>
```

If the line moved (line drift due to other commits), find the unique text `"empty-state"` + `no_transactions_card` — there's only one such div.

- [ ] **Step 2: Add empty-state testid**

Edit that line to add the testid:

```rust
                    <div class="empty-state" data-testid="transactions-list-empty">{move || i18n::t(lang.get(), "no_transactions_card")}</div>
```

- [ ] **Step 3: Locate the per-row div**

In the same file, find the `<div class=row_class>` opening tag inside the rows iterator (around line 120). It's the line immediately following:

```rust
                view! {
                    <div class=row_class>
```

There is only one `class=row_class` in the file. Do not confuse it with `class=amount_class` or other similar names.

- [ ] **Step 4: Add row testid**

Edit the row opening tag to add `data-testid="transaction-row"`:

```rust
                view! {
                    <div class=row_class data-testid="transaction-row">
```

- [ ] **Step 5: Format check**

```bash
cargo fmt --all --check
```

Expected: no diff. (If fmt complains, run `cargo fmt --all` and inspect — `Edit` tool changes shouldn't introduce any formatting drift on these edits, but verify.)

- [ ] **Step 6: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/transactions_list.rs
git commit -m "test(ui): add transactions-list-empty + transaction-row testids (#34 prep)

Stable selectors for the Task 3 E2E tests in PR #34. The outer
transactions-list wrapper testid already exists at the bottom; this
fills in the empty-state and per-row siblings.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Add three failing E2E tests (RED phase)

**Why RED before GREEN:** the spec's regression-fence test (Test 3) only proves anything if it FAILS without the fix. Pushing the tests first and confirming they fail in CI is the canonical bug-fix protocol per `tdd-workflow.md`.

**Files:**
- Modify: `e2e/tests/desk-ux.spec.ts`

**Existing structure of desk-ux.spec.ts:**
- Imports: `{ test, expect, Page }` from `'@playwright/test'`; `{ setupConsoleCheck, assertCleanConsole, loginViaAPI }` from `'./helpers'`.
- Helpers in-file: `activateUniqueCard`, `sellPassToCard`, `lookupCardId`, `openCardByLastName`.
- Describe block: `Staff desk UX cluster — issues #29 #30 #31 #32`.

**Existing infrastructure available:**
- `loginViaAPI(page, baseURL, email, password)` — returns the JWT string; `staff@test.com / staff123` and `admin@test.com / admin123` are both seeded by `e2e/global-setup.ts`.
- `activateUniqueCard(token, initialCredit)` returns `{ barcode, lastName }`.
- `openCardByLastName(page, lastName)` — types into the search box, clicks first result, waits for `[data-testid="action-panel"]`.

- [ ] **Step 1: Update describe block title**

In `e2e/tests/desk-ux.spec.ts`, change:

```typescript
test.describe('Staff desk UX cluster — issues #29 #30 #31 #32', () => {
```

to:

```typescript
test.describe('Staff desk UX cluster — issues #29 #30 #31 #32 #34', () => {
```

(Single-line edit to the describe title.)

- [ ] **Step 2: Add helper for fetching Spinning service info**

Above the `test.describe(...)` line (i.e. alongside `activateUniqueCard`, `sellPassToCard`, etc., near the top of the file), add this helper:

```typescript
async function getSpinningService(token: string): Promise<{ id: number; default_price: number; active: number }> {
    const resp = await fetch(`${BASE_URL}/api/services`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`GET /api/services failed: ${resp.status} ${await resp.text()}`);
    const all = await resp.json();
    const spinning = all.find((s: { name_en: string }) => s.name_en === 'Spinning');
    if (!spinning) throw new Error('Spinning service not found in /api/services response');
    return spinning as { id: number; default_price: number; active: number };
}
```

Note: `/api/services` returns the full list including `default_price` and `active` per the existing `ServiceInfo` shape used by the dashboard.

- [ ] **Step 3: Add helper for setting service active flag (used by Test 2)**

Below `getSpinningService`, add:

```typescript
async function setServiceActive(adminToken: string, svc: { id: number; default_price: number; active: number; name_sk?: string; name_en?: string; kind?: string }, active: 0 | 1): Promise<void> {
    // PUT /api/admin/services/{id} requires the full record.
    // Re-fetch via /api/services to get name_sk / name_en / kind that
    // getSpinningService didn't ask for, then PATCH the active flag.
    const resp = await fetch(`${BASE_URL}/api/services`, {
        headers: { Authorization: `Bearer ${adminToken}` },
    });
    if (!resp.ok) throw new Error(`GET /api/services failed: ${resp.status}`);
    const full = (await resp.json()).find((s: { id: number }) => s.id === svc.id);
    if (!full) throw new Error(`Service id ${svc.id} not found`);
    const put = await fetch(`${BASE_URL}/api/admin/services/${svc.id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({
            name_sk: full.name_sk,
            name_en: full.name_en,
            default_price: full.default_price,
            active,
        }),
    });
    if (!put.ok) throw new Error(`PUT /api/admin/services/${svc.id} failed: ${put.status} ${await put.text()}`);
}
```

- [ ] **Step 4: Add Test 1 — chip charges card in one click**

Inside the describe block, AFTER all existing tests, add:

```typescript
    test('Spinning chip charges card in one click (#34)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const spinning = await getSpinningService(token);
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

        const chip = page.locator('[data-testid="quick-charge-spinning"]');
        await expect(chip).toBeVisible();
        await expect(chip).toContainText(`Spinning ${spinning.default_price.toFixed(2)} €`);

        // Capture credit BEFORE the click — the credit reading lives in
        // [data-testid="card-credit"] inside the action panel header.
        const creditBefore = parseFloat(
            (await page.locator('[data-testid="card-credit"]').textContent()) ?? '0',
        );

        await chip.click();

        // After charge: txn list populated, empty-state absent, credit decreased.
        await expect(page.locator('[data-testid="transactions-list"]')).toBeVisible();
        await expect(page.locator('[data-testid="transactions-list-empty"]')).toHaveCount(0);
        const rowCount = await page.locator('[data-testid="transaction-row"]').count();
        expect(rowCount).toBeGreaterThanOrEqual(1);

        await expect
            .poll(async () => parseFloat((await page.locator('[data-testid="card-credit"]').textContent()) ?? '0'))
            .toBeCloseTo(creditBefore - spinning.default_price, 2);

        assertCleanConsole(msgs);
    });
```

- [ ] **Step 5: Add Test 2 — chip absent when Spinning inactive**

Immediately after Test 1, add:

```typescript
    test('Spinning chip is absent when service is inactive (#34)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const spinning = await getSpinningService(adminToken);

        // Deactivate Spinning. Use try/finally so the service is reactivated
        // even if assertions throw — leaking active=0 would break unrelated
        // tests (e.g. Test 1 in this very file) on shared CI state.
        await setServiceActive(adminToken, spinning, 0);
        try {
            // Re-login as staff for the staff UI flow.
            const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
            const { lastName } = await activateUniqueCard(staffToken, 50.0);
            await page.goto('/staff');
            await openCardByLastName(page, lastName);

            await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

            // Chip absent.
            await expect(page.locator('[data-testid="quick-charge-spinning"]')).toHaveCount(0);
            // Regular Charge button still present.
            await expect(page.locator('[data-testid="charge-submit"]')).toBeVisible();
        } finally {
            await setServiceActive(adminToken, spinning, 1);
        }

        assertCleanConsole(msgs);
    });
```

- [ ] **Step 6: Add Test 3 — regression fence**

Immediately after Test 2, add:

```typescript
    test('Regression fence: txn list still populates after Spinning chip charge (#34)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();
        await page.locator('[data-testid="quick-charge-spinning"]').click();

        // The exact regression class from PR #35: empty-state must NOT appear,
        // and at least one row must be present.
        await expect(page.locator('[data-testid="transactions-list-empty"]')).toHaveCount(0);
        const rows = page.locator('[data-testid="transaction-row"]');
        await expect(rows.first()).toBeVisible();
        expect(await rows.count()).toBeGreaterThanOrEqual(1);

        assertCleanConsole(msgs);
    });
```

- [ ] **Step 7: Format check (no Rust touched, but verify the Rust state from Task 2 is still clean)**

```bash
cargo fmt --all --check
```

Expected: no diff.

- [ ] **Step 8: Commit RED tests**

```bash
git add e2e/tests/desk-ux.spec.ts
git commit -m "test(e2e): Spinning quick-charge chip on card desk (#34, red)

Three Playwright tests added to the desk UX cluster:
- chip charges card in one click
- chip absent when Spinning service is inactive (uses admin PUT
  /api/admin/services/{id} with try/finally to reactivate)
- regression fence — txn list populated after chip charge (the exact
  scenario that broke in PR #35)

Tests fail at this commit because the chip + spec implementation
land in the next commit (Task 4). RED → GREEN per tdd-workflow.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 9: Push and confirm RED in CI**

```bash
git push
sleep 60 && gh run list --branch dev --limit 3 --json databaseId,status,conclusion
```

Capture the latest run ID for `dev`. Then monitor in the background:

```bash
RUN_ID=<latest-run-id>
# Single backgrounded sleep+view, NOT gh run watch.
nohup bash -c "sleep 480 && gh run view $RUN_ID --json status,conclusion,jobs > /tmp/run-${RUN_ID}.json" >/dev/null 2>&1 &
```

After the sleep elapses, read `/tmp/run-${RUN_ID}.json`. Expected: E2E job `conclusion: "failure"` (the 3 new tests fail because the chip doesn't exist). Other jobs (lint, test, build-wasm, mutation, test-integrity) should be `success`. Deploy jobs should be `skipped` (this isn't main).

If E2E fails with anything OTHER than the 3 new tests' assertions, STOP and investigate — that means the testids in Task 2 are wrong, the helpers don't work, or there's a regression elsewhere. Do NOT proceed to Task 4 until you've confirmed RED is the *intended* RED.

If E2E SUCCEEDS at this commit, the tests aren't actually testing the chip — STOP and read what the tests assert.

---

## Task 4: Implement the Spinning chip (GREEN phase) + bundle #38

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs`

The single source of truth for the chip's view is the spec at `docs/superpowers/specs/2026-05-01-spinning-chip-design.md` (Architecture section). This task instantiates that spec with exact code.

- [ ] **Step 1: Update imports**

Open `spinbike-ui/src/pages/dashboard/action_form.rs`. Line 2 currently reads:

```rust
use spinbike_core::services::FITNESS_NAME_EN;
```

Replace with:

```rust
use spinbike_core::services::{FITNESS_NAME_EN, SPINNING_NAME_EN};
```

(Single combined import is the idiomatic Rust style and minimizes diff churn. Both consts live in the same `spinbike_core::services` module.)

- [ ] **Step 2: Apply #38 cleanup at the inline reference**

In the same file, find this existing line (around line 336):

```rust
                                let color_cls = if svc.name_en == spinbike_core::services::FITNESS_NAME_EN {
```

Change to:

```rust
                                let color_cls = if svc.name_en == FITNESS_NAME_EN {
```

The `FITNESS_NAME_EN` import is already available (now alongside `SPINNING_NAME_EN` from Step 1). Bundled cleanup #38 is now done.

- [ ] **Step 3: Build the spinning_chip snapshot at component body**

In `action_form.rs`, find the existing Fitness preselect Effect block. It looks like:

```rust
    // Fitness preselect (#33). On first non-empty services load with no
    // current selection, selects Fitness in both the signal and the DOM
    // <select>. ...
    Effect::new(move |_| {
        let svcs = services.get();
        // ...
        if let Some(el) = service_ref.get() {
            let el: &HtmlSelectElement = &el;
            el.set_value(&fitness.id.to_string());
        }
    });
```

The Effect ends just before the `view! {` macro at the start of the JSX-like block (the `<div class="stack-12" data-testid="action-form">` line).

Immediately AFTER the Effect's closing `});` and BEFORE the `view! {` line, insert:

```rust

    // Spinning quick-charge chip (#34). Snapshot the active Spinning
    // service ONCE at component mount via get_untracked() — no reactive
    // subscription. Renders as static DOM. ActionForm re-mounts on each
    // card open (parent's match selected.get() in dashboard/mod.rs), so
    // each fresh mount re-runs this snapshot, picking up admin price
    // edits between card opens. The previous attempt (PR #35, fa7b34d)
    // wrapped the chip in {move || services.get() ...}, which subscribed
    // the chip's DOM to the services signal — that subscription
    // interleaved with set_selected.update's parent re-mount and dropped
    // set_txn_refresh, leaving the txn list empty after a charge.
    let spinning_chip = services
        .get_untracked()
        .into_iter()
        .find(|s| s.name_en == SPINNING_NAME_EN && s.active != 0)
        .map(|svc| {
            let svc_id = svc.id;
            let price = svc.default_price;
            let card_id_for_click = card_id;
            let on_click = move |_ev: web_sys::MouseEvent| {
                set_err.set(String::new());
                set_loading.set(true);
                spawn_local(async move {
                    #[derive(serde::Serialize)]
                    struct Req {
                        card_id: i64,
                        amount: f64,
                        service_id: Option<i64>,
                        note: Option<String>,
                    }
                    match api::post::<Req, PaymentResp>(
                        "/api/payments/charge",
                        &Req {
                            card_id: card_id_for_click,
                            amount: price,
                            service_id: Some(svc_id),
                            note: None,
                        },
                    )
                    .await
                    {
                        Ok(r) => {
                            set_msg.set(i18n::tf(
                                lang.get_untracked(),
                                "charge_ok_format",
                                &[&format!("{:.2}", r.new_credit)],
                            ));
                            set_selected.update(|s| {
                                if let Some(c) = s {
                                    c.credit = r.new_credit;
                                }
                            });
                            set_txn_refresh.update(|n| *n += 1);
                        }
                        Err(e) => set_err.set(e),
                    }
                    set_loading.set(false);
                });
            };
            view! {
                <div class="chip-row quick-charge-row">
                    <button
                        type="button"
                        class="btn btn--info"
                        data-testid="quick-charge-spinning"
                        on:click=on_click
                        disabled=move || loading.get()
                    >
                        {format!("Spinning {price:.2} €")}
                    </button>
                </div>
            }
            .into_any()
        });
```

This is the snapshot. `spinning_chip` is `Option<AnyView>`; `None` renders nothing.

- [ ] **Step 4: Insert {spinning_chip} into the view!**

Locate the existing class-visit chip-row block in the `view!` macro. It looks like:

```rust
            {if pass_active {
                view! {
                    <div class="chip-row chip-row--spaced chip-row--readable">
                        ...class-visit buttons...
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }}

            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "select_service")}</label>
                <select
                    ...
```

Between the closing `}}` of the `if pass_active` block and the opening `<div class="form-group">` of the service select, insert `{spinning_chip}` on its own line:

```rust
            {if pass_active {
                view! {
                    <div class="chip-row chip-row--spaced chip-row--readable">
                        ...class-visit buttons...
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }}

            {spinning_chip}

            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "select_service")}</label>
                <select
                    ...
```

`Option<AnyView>` implements `IntoView`, so this renders the chip when `Some` and nothing when `None`. No `.into_any()` is needed at the use site because `spinning_chip` is already an `Option<AnyView>`.

- [ ] **Step 5: Format check**

```bash
cargo fmt --all --check
```

Expected: no diff. If diff exists, run `cargo fmt --all` and re-stage. Format-only changes never break behavior.

- [ ] **Step 6: Commit GREEN**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): Spinning quick-charge chip on card desk (#34)

Snapshot the active Spinning service via services.get_untracked() ONCE
at ActionForm component mount; render as Option<AnyView> with no
reactive wrapper around the view. Click handler is identical-shape to
do_charge (POST /api/payments/charge with service_id and amount).

ActionForm re-mounts on every card open (parent's match selected.get()),
so each fresh mount picks up admin price edits made between opens —
'next card open' price freshness, no Effect needed.

Hidden when Spinning is missing or inactive (Option<AnyView>::None).
Bundles #38: action_form.rs:336 now uses bare FITNESS_NAME_EN.

Fixes the txn-list regression class from PR #35 (commit fa7b34d, reverted
in 9918d34) by avoiding the {move || services.get() ...} reactive
wrapper that subscribed the chip's DOM to the services signal.

Closes #34, closes #38

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: Push and confirm GREEN in CI**

```bash
git push
sleep 60 && gh run list --branch dev --limit 3 --json databaseId,status,conclusion
```

Pick up the latest run ID. Monitor:

```bash
RUN_ID=<latest-run-id>
nohup bash -c "sleep 720 && gh run view $RUN_ID --json status,conclusion,jobs > /tmp/run-${RUN_ID}.json" >/dev/null 2>&1 &
```

After the sleep elapses (CI typically 9-13 minutes for full pipeline), read `/tmp/run-${RUN_ID}.json`.

**Expected on dev push:**

| Job | Expected conclusion |
|---|---|
| `Test Integrity` | `success` |
| `Lint` | `success` |
| `Build WASM (UI)` | `success` |
| `Test` | `success` |
| `E2E Tests` | `success` (the 3 new tests now pass; existing tests unchanged) |
| `Mutation Testing` | `success` (only runs on PRs/main) — may be `skipped` on dev push, that is fine |
| `Deploy (dev)` | `skipped` (deploys gated on push to main, not dev) |
| `Deploy (prod)` | `skipped` |
| `Smoke (dev)` | `skipped` |
| `Smoke (prod)` | `skipped` |
| `Version Bump Check` | `skipped` (this is a push, not a PR) |

If E2E fails: read `gh run view $RUN_ID --log-failed` for the specific test failures and investigate the root cause. Do NOT bump timeouts or rerun blindly per `no-timeout-band-aids.md`.

If a known transient flake (#39 — barcode substring collision in `dashboard.spec.ts`, NOT in our 3 new tests) appears, ONE rerun is acceptable per `ci-monitoring.md`. The flake's repro signature is `expect(locator).toContainText('Jana Testova') failed - unexpected value "AF ActionForm<digits-with-1001>"` in `dashboard.spec.ts`. If THAT specific failure shows up:

```bash
gh run rerun $RUN_ID --failed
sleep 60 && gh run list --branch dev --limit 3 --json databaseId,status,conclusion
```

Then re-monitor. If the rerun fails for the same flake, STOP and investigate — that means it's no longer transient.

---

## Task 5: Open PR and reach mergeable state

**Files:** none (PR-creation only).

- [ ] **Step 1: Confirm dev is green**

Verify the latest run on `dev` ended in `conclusion: "success"`. If anything failed, do NOT proceed. Loop back to Task 4 fix until green.

- [ ] **Step 2: Read final commit list to draft PR body**

```bash
git log --oneline main..dev
```

Expected commits on dev not in main (chronological, oldest first reading bottom-up):

```
docs(spec): Spinning quick-charge chip on card desk (#34)         # b4510ec
docs(spec): align testid names with existing transactions-list... # bdb7540
chore: bump version to 0.13.12                                    # Task 1
test(ui): add transactions-list-empty + transaction-row testids... # Task 2
test(e2e): Spinning quick-charge chip on card desk (#34, red)    # Task 3
feat(ui): Spinning quick-charge chip on card desk (#34)          # Task 4
```

- [ ] **Step 3: Create PR**

```bash
gh pr create --base main --head dev --title "feat(ui): Spinning quick-charge chip on card desk (#34, v0.13.12)" --body "$(cat <<'EOF'
## Summary

- Re-adds the `Spinning {price}€` quick-charge chip reverted in PR #35 (`9918d34`). One click charges the card from credit at Spinning's current `default_price`. Closes #34.
- Bundles #38: `action_form.rs:336` now uses bare `FITNESS_NAME_EN` (consistent with the rest of the file after the `{FITNESS_NAME_EN, SPINNING_NAME_EN}` import).
- Adds three Playwright E2E tests under the desk UX cluster, including the regression fence for the txn-list cascade that broke in PR #35.

## Architectural fix

The previous attempt (commit `fa7b34d`) wrapped the chip in `{move || services.get() ...}`, subscribing the chip's DOM to the `services` signal. That subscription interleaved with `set_selected.update`'s parent re-mount and dropped `set_txn_refresh`, leaving the txn list empty after charges.

This PR snapshots the active Spinning service via `services.get_untracked()` ONCE at component mount and renders as `Option<AnyView>` — no signal subscription, static DOM after mount. Same lifecycle pattern as #33 Fitness preselect (PR #37) where imperative `set_value()` replaced reactive `prop:value`.

`ActionForm` re-mounts on every card open (parent's `match selected.get()` in `dashboard/mod.rs`), so each fresh mount re-runs the snapshot — admin price edits between card opens are picked up for free.

## Test plan

- [ ] CI green on dev push (confirmed before opening PR)
- [ ] E2E `Spinning chip charges card in one click (#34)` passes
- [ ] E2E `Spinning chip is absent when service is inactive (#34)` passes
- [ ] E2E `Regression fence: txn list still populates after Spinning chip charge (#34)` passes
- [ ] Existing dashboard / payments / desk-UX E2E tests unchanged (no regression)
- [ ] Browser console clean per `setupConsoleCheck`
- [ ] Post-deploy: open a real card on dev, confirm chip shows + charge works
- [ ] Post-deploy: same on prod after merge

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Confirm PR mergeable**

```bash
gh pr view --json number,url,mergeable,mergeStateStatus
```

Wait until `mergeable: "MERGEABLE"` AND `mergeStateStatus: "CLEAN"`. Anything else (`UNSTABLE`, `BLOCKED`, `BEHIND`, `DIRTY`) means more work — investigate and fix. Per `autonomous-quality-discipline.md`, never present an UNSTABLE PR as ready.

The PR run will trigger a duplicate CI cycle (push event already triggered one; PR opening triggers another). Both should succeed. The `Version Bump Check` job runs only on PRs and should `success` since dev is `0.13.12` > main's `0.13.11`.

- [ ] **Step 5: Apply self-audits**

Run plan-fulfillment audit and code-review audit per `completion-report.md`'s pre-completion gate:

1. Invoke `Skill(skill: "plan-check")` — confirms every numbered step in this plan is `[x]` complete with evidence.
2. Apply `/review` standards: skim diff for Correctness / Security / Performance / Maintainability / Style. Output `0 🔴 0 🟡 0 🔵`. Any in-diff finding gets fixed in a fresh commit (NEVER amend) and re-pushed; out-of-diff 🔵 findings get filed as new GitHub issues.

If either audit comes back non-clean, fix and re-run. Do NOT send the completion report until both are clean.

- [ ] **Step 6: Send completion report**

Per `completion-report.md` template (mandatory exact structure). Include both `🌐` URLs (dev + prod), the PR title with number, the PR URL with `mergeable, clean`. End with no `❓` question (this PR has no decisions for the user beyond the merge instruction).

The completion report is the LAST message of this work cycle. After sending, stop and wait for the user's explicit `merge it` per `pr-merge-policy.md`.

---

## Task 6: Post-deploy verification (gated on user merge)

**Do NOT start this task until the user explicitly says `merge it` AND the merge has happened.** This task is what runs AFTER the user merges; it does not run autonomously.

**Files:** none (verification only).

- [ ] **Step 1: Wait for user merge instruction**

The completion report at end of Task 5 ends with the green PR URL. The user reads it and decides. They will say `merge it` (or similar) when they want the merge to happen.

- [ ] **Step 2: Merge per user instruction**

```bash
gh pr merge --merge $(gh pr view --json number -q .number)
```

(Merge commit, NOT squash, per `two-branch-workflow.md`.)

- [ ] **Step 3: Monitor main CI to terminal state**

```bash
sleep 60 && gh run list --branch main --limit 3 --json databaseId,status,conclusion
RUN_ID=<latest-main-run-id>
nohup bash -c "sleep 900 && gh run view $RUN_ID --json status,conclusion,jobs > /tmp/run-${RUN_ID}.json" >/dev/null 2>&1 &
```

Main CI deploys to dev AND prod (deploy-dev runs unconditionally on main pushes; deploy-prod runs gated on the new VERSION). Watch ALL jobs to terminal — including `Deploy (dev)`, `Deploy (prod)`, `Smoke (dev)`, `Smoke (prod)`. If any fail, investigate and fix.

If the known #39 barcode-collision flake hits on main E2E, ONE rerun per `ci-monitoring.md`.

- [ ] **Step 4: Verify dev deployment**

Use Playwright MCP (via the `mcp__plugin_playwright_playwright__*` tools, NOT a local Playwright CLI):

1. `browser_navigate` to `https://spinbike-dev.newlevel.media/login`
2. Login as staff (`staff@test.com` / `staff123`)
3. Navigate to `/staff`
4. Read `[data-testid="version"]` — assert it shows `v0.13.12`. If not, the deploy didn't reach the dev frontend; STOP and investigate (CDN, build, wrong host).
5. Search a known card from prod-synced data (per `feedback_dev_ci_sync_prod_db.md` dev DB has prod-shape data after deploy). Pick one with active credit.
6. Open the card. Wait for `[data-testid="action-form"]`.
7. Read `[data-testid="quick-charge-spinning"]` text — assert it contains `Spinning ` + a price + ` €`.
8. (Optional, if a non-precious card is available) Click the chip and verify credit decreased by exactly the displayed price, transaction row appeared.
9. `browser_console_messages` — assert no errors/warnings.
10. `browser_close`.

- [ ] **Step 5: Verify prod deployment**

Repeat Step 4 against `https://spinbike.newlevel.media/`. Expected version: `v0.13.12`. Per `approval-scope.md`, prod deploy is automatic on main push (gated on the version bump check); no separate approval needed for the verification — the user already approved the merge.

For prod, do NOT click the chip on a real card unless the user explicitly authorizes test-charging real customer credit. Visual + version check + chip-presence is sufficient verification of the deploy reaching prod.

- [ ] **Step 6: Final completion report**

Per `completion-report.md`, send the post-deploy completion report with `✅ Deploy: dev frontend shows v0.13.12 (matches backend), Spinning chip visible on card <name>; prod shows v0.13.12, chip visible on card <name>`. PR is now `merged at <sha>`. Goal + What changed restated in plain language. End the work cycle.

---

## Self-Review (planner audit)

**Spec coverage:**

| Spec section / requirement | Plan task |
|---|---|
| Goal: one-click Spinning charge | Task 4 (chip + handler) |
| Architecture: snapshot via get_untracked, no reactive wrapper | Task 4 Step 3 |
| Click handler matches do_charge shape | Task 4 Step 3 (full code) |
| "Next card open" freshness | Achieved automatically via component re-mount; documented in Task 4 Step 3 comment |
| Hide chip when Spinning missing/inactive | Task 4 Step 3 (Option<AnyView>::None when `find` returns None) |
| Test 1 — chip charges card | Task 3 Step 4 |
| Test 2 — chip absent when inactive | Task 3 Step 5 (uses PUT /api/admin/services/{id}, try/finally cleanup) |
| Test 3 — regression fence | Task 3 Step 6 |
| Layout — between Quick Action and service select | Task 4 Step 4 (insertion point) |
| #38 cleanup — bare FITNESS_NAME_EN at line 336 | Task 4 Step 2 |
| transactions-list testids | Task 2 (uses existing wrapper testid + adds two new ones) |
| VERSION bump 0.13.11 → 0.13.12 | Task 1 |
| No reactive `{move || ...}` wrapper | Task 4 Step 3 (compile-time guarantee — `Option<AnyView>` has no signal access in the use site) |
| Browser console clean | All three tests assert via `setupConsoleCheck`/`assertCleanConsole` |
| Post-deploy real-data check | Task 6 (dev + prod) |

**Placeholder scan:** No "TBD", "TODO", "implement later", or vague phrases. Every code step shows complete code. Every command shows expected output.

**Type consistency:** `spinning_chip: Option<AnyView>`. Used as `{spinning_chip}` in the view — `Option<AnyView>` implements `IntoView`. Same `card_id: i64`, `svc_id: i64`, `price: f64` types throughout. Click handler returns nothing (it's a `move |_ev: web_sys::MouseEvent| {...}`). `Req` struct fields match the existing `do_charge` non-pass branch exactly.

**Scope check:** Single feature + one trivial cleanup, all in one PR. No multi-subsystem expansion. Appropriate for one implementation cycle.

---

## Pre-dispatch pause (per `feedback_pre_implementation_pause.md`)

Per project memory, this plan SHOULD be reviewed by the user before dispatching all tasks via subagent-driven-development. The next step depends on user input.
