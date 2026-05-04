# Visit-button Feedback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Fitness/Spinning "Log visit" buttons on a card with an active monthly pass produce clear per-press feedback: disable while in flight, show a localized success banner ("Visit added: Fitness" / "Vstup pridany: Fitness"), auto-clear the banner after 2.5 s, and route errors to the red error banner instead of the green success banner (a current bug).

**Architecture:** Mirror the existing charge-button pattern. The success message uses the existing `msg` signal in `dashboard/mod.rs`. To make the auto-clear race-safe, the `msg` ReadSignal is threaded down through `CardActionPanel` into `ActionForm` so the timer can compare the captured message text against the current `msg` value before clearing. No new components, no new CSS, no server changes.

**Tech Stack:** Leptos 0.7 CSR, WASM, `gloo-timers` 0.3 (already in `spinbike-ui/Cargo.toml`). Tests: `wasm-bindgen-test` (run via `wasm-pack test --node` in CI) + Playwright E2E.

---

## File Structure

| File | Responsibility | Change kind |
|---|---|---|
| `VERSION` | Single source of truth for the deployed version | Bump `0.13.19` → `0.13.20` |
| `spinbike-ui/src/i18n.rs` | Translation table + format helper | Add ONE key `visit_added_format` + two `#[wasm_bindgen_test]` cases |
| `spinbike-ui/src/pages/dashboard/mod.rs` | Composes the dashboard, owns `msg`/`err` signals | Pass `msg` ReadSignal to `<CardActionPanel msg=msg …>` |
| `spinbike-ui/src/pages/dashboard/card_panel.rs` | `CardActionPanel` component | Add `msg: ReadSignal<String>` prop; forward to `<ActionForm msg=msg …>` |
| `spinbike-ui/src/pages/dashboard/action_form.rs` | `ActionForm` component (charge / visit buttons) | Add `msg` prop; rewrite `visit_click_for`; add `disabled` to visit buttons |
| `e2e/tests/visit-button-feedback.spec.ts` | Playwright E2E for issue #53 | Create |

The threading change (mod.rs → card_panel.rs → action_form.rs) is mechanical: ONE new prop, three callsites updated.

---

## Task 1: Version bump (CONTROLLER-RUN, NOT a subagent task)

**Files:**
- Modify: `VERSION`
- Modify (auto, via script): `Cargo.toml`, `crates/*/Cargo.toml`, `spinbike-ui/Cargo.toml`

**Why first:** After PR #59 merged at `fb44281`, `dev` and `main` both read `0.13.19`. CI's version-bump-check job will fail any PR where `dev` version ≤ `main` version. Bumping is the cheapest first commit.

- [ ] **Step 1: Edit VERSION**

```bash
echo 0.13.20 > VERSION
```

- [ ] **Step 2: Run the sync script**

```bash
bash scripts/sync-version.sh
```

Expected: prints `Syncing version: 0.13.20`, modifies the workspace `Cargo.toml` files in place.

- [ ] **Step 3: Local fmt check (only allowed local check)**

```bash
cargo fmt --all --check
```

Expected: no output, exit 0.

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml Cargo.lock crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "$(cat <<'EOF'
chore(release): v0.13.20

Bump version ahead of #53 visit-button feedback work.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

If `Cargo.lock` did not change, drop it from the `git add` line. The `sync-version.sh` script may or may not touch `Cargo.lock` depending on whether `cargo build` ran.

---

## Task 2: i18n key `visit_added_format` + wasm-bindgen tests (SUBAGENT, sonnet)

**Files:**
- Modify: `spinbike-ui/src/i18n.rs:369-372` (insert new key right after `charge_ok_format`) and append a new `#[cfg(test)] mod format_key_tests` at the end of the file.

**Why this slot for the key:** the existing translation grouping in lines 365-376 covers payment-success format strings (`topup_ok_format`, `charge_ok_format`, etc.). The new `visit_added_format` belongs in the same cluster.

- [ ] **Step 1: Add the i18n key**

In `spinbike-ui/src/i18n.rs`, after the `charge_ok_format` entry at lines 369-372, add:

```rust
    m.insert(
        "visit_added_format",
        ("Vstup pridany: {}", "Visit added: {}"),
    );
```

The Slovak text is intentionally unaccented per project convention (see all other Slovak strings in the file — `Hladam`, `Skryt`, `Karta zablokovana`).

- [ ] **Step 2: Add a new test module at the end of `spinbike-ui/src/i18n.rs`**

After the existing `mod datetime_tests { … }` block (line 818), append:

```rust
#[cfg(test)]
mod format_key_tests {
    use super::{tf, Lang};
    use wasm_bindgen_test::*;

    // CRITICAL: do NOT add `wasm_bindgen_test_configure!(run_in_browser);`
    // here. CI runs these via `wasm-pack test --node`, where the
    // run_in_browser configure causes the entire test set to silently skip
    // (zero failures, zero passes — invisible). The existing `datetime_tests`
    // module above also has no configure line for the same reason.

    #[wasm_bindgen_test]
    fn visit_added_format_renders_slovak() {
        assert_eq!(
            tf(Lang::Sk, "visit_added_format", &["Fitness"]),
            "Vstup pridany: Fitness"
        );
    }

    #[wasm_bindgen_test]
    fn visit_added_format_renders_english() {
        assert_eq!(
            tf(Lang::En, "visit_added_format", &["Spinning"]),
            "Visit added: Spinning"
        );
    }
}
```

- [ ] **Step 3: Local fmt check**

```bash
cargo fmt --all --check
```

Expected: no output, exit 0.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "$(cat <<'EOF'
i18n(visit): add visit_added_format key + tests (#53)

Slovak "Vstup pridany: {}" / English "Visit added: {}". Used by the
Log-visit button success banner. Two wasm-bindgen tests assert the
exact format-string output for both languages — they kill the
format-string-mutation and key-swap mutants before integration.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Wire visit-button feedback (SUBAGENT, sonnet)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs:449-460` — pass `msg` to `<CardActionPanel msg=msg …>`
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs:27-34, 150-156` — add `msg: ReadSignal<String>` prop; forward to `<ActionForm msg=msg …>`
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs:14-22, 241-274, 408-418` — add `msg` prop; rewrite `visit_click_for`; add `disabled` to the visit buttons

**Architectural decisions locked here:**

- **Service-name lookup:** option (a) from the planner brief — `visit_click_for` takes the `svc_name: String` as a second argument. The button render block at lines 408-415 already has `svc_name` in scope (line 397), so passing it in is a one-character change at the call site.
- **Auto-clear:** captured-string comparison via `msg.get_untracked() == captured` (the spec's Risk section explains why this is race-safe).
- **Re-entry guard:** `if loading.get_untracked() { return; }` at the top of the click handler — same pattern the spec calls out.
- **Error path bug fix:** route `Err(e)` to `set_err.set(e)` directly. Drop the `error_format` wrapper.

- [ ] **Step 1: Add `msg` prop to `ActionForm`**

In `spinbike-ui/src/pages/dashboard/action_form.rs`, at the `ActionForm` signature (lines 16-22), add the new prop between `set_msg` and `set_txn_refresh`:

```rust
#[component]
pub fn ActionForm(
    card: CardInfo,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    msg: ReadSignal<String>,
    set_msg: WriteSignal<String>,
    set_txn_refresh: WriteSignal<u32>,
) -> impl IntoView {
```

- [ ] **Step 2: Add `msg` prop to `CardActionPanel` and forward**

In `spinbike-ui/src/pages/dashboard/card_panel.rs:27-34`, add the prop:

```rust
#[component]
pub fn CardActionPanel(
    card: CardInfo,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    msg: ReadSignal<String>,
    set_msg: WriteSignal<String>,
    #[prop(into)] on_close: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
```

In the same file at the `<ActionForm …>` call (lines 150-156), forward `msg`:

```rust
            <ActionForm
                card=card_for_form.clone()
                services=services
                set_selected=set_selected
                msg=msg
                set_msg=set_msg
                set_txn_refresh=txn_refresh.write_only()
            />
```

- [ ] **Step 3: Pass `msg` from `DashboardPage`**

In `spinbike-ui/src/pages/dashboard/mod.rs`, at the `<CardActionPanel …>` call (lines 449-460), add the `msg=msg` prop:

```rust
        {move || match selected.get() {
            None => view! { <span></span> }.into_any(),
            Some(c) => view! {
                <CardActionPanel
                    card=c
                    services=services
                    set_selected=set_selected
                    msg=msg
                    set_msg=set_msg
                    on_close=Callback::new(clear_selection)
                />
            }.into_any()
        }}
```

- [ ] **Step 4: Rewrite `visit_click_for` in `action_form.rs:241-274`**

Replace the entire `visit_click_for` closure with:

```rust
    let visit_click_for = move |service_id: i64, svc_name: String| {
        move |_: web_sys::MouseEvent| {
            // Re-entry guard: if a previous press is still in flight, the
            // disabled binding may not have repainted yet. This protects
            // against a fast double-tap before the next-frame disable
            // takes effect.
            if loading.get_untracked() {
                return;
            }
            set_err.set(String::new());
            set_loading.set(true);
            let note = read_note();
            let svc_name = svc_name.clone();
            spawn_local(async move {
                #[derive(serde::Serialize)]
                struct Req {
                    card_id: i64,
                    service_id: i64,
                    note: Option<String>,
                }
                #[derive(serde::Deserialize)]
                struct Resp {
                    #[allow(dead_code)]
                    transaction_id: i64,
                }
                match api::post::<Req, Resp>(
                    "/api/payments/log-visit",
                    &Req {
                        card_id,
                        service_id,
                        note,
                    },
                )
                .await
                {
                    Ok(_) => {
                        let m = i18n::tf(
                            lang.get_untracked(),
                            "visit_added_format",
                            &[&svc_name],
                        );
                        set_msg.set(m.clone());
                        set_txn_refresh.update(|n| *n += 1);
                        clear_note();
                        // Auto-clear: 2.5s after this set, clear msg only if
                        // it still equals m. A subsequent visit / charge in
                        // the window will replace msg with new text, the
                        // comparison fails, and this timer becomes a no-op.
                        spawn_local(async move {
                            gloo_timers::future::TimeoutFuture::new(2500).await;
                            if msg.get_untracked() == m {
                                set_msg.set(String::new());
                            }
                        });
                    }
                    Err(e) => set_err.set(e),
                }
                set_loading.set(false);
            });
        }
    };
```

Note: `gloo_timers::future::TimeoutFuture` is the existing pattern used in `staff_dashboard.rs:312`. No new import needed in `action_form.rs` — call it fully qualified as written above.

- [ ] **Step 5: Add `disabled` binding to both visit buttons + pass `svc_name` to the closure**

At `action_form.rs:408-415`, replace the inner `<button>…</button>` block with:

```rust
                                view! {
                                    <button
                                        class=format!("btn {color_cls}")
                                        data-testid="log-visit-btn"
                                        disabled=move || loading.get()
                                        on:click=visit_click_for(service_id, svc_name.clone())
                                    >
                                        {move || i18n::t(lang.get(), "log_visit")}" "{svc_name.clone()}
                                    </button>
                                }
```

Both arguments to `visit_click_for` are owned values; the `svc_name.clone()` is needed because the surrounding `.map(|svc| { … })` closure captures it twice (once for the button handler, once for the button label).

- [ ] **Step 6: Local fmt check**

```bash
cargo fmt --all --check
```

Expected: no output, exit 0.

- [ ] **Step 7: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs spinbike-ui/src/pages/dashboard/card_panel.rs spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "$(cat <<'EOF'
feat(ui): visit-button feedback (#53)

The Fitness/Spinning Log-visit buttons (visible on a card with an
active monthly pass) now mirror the charge-button pattern:

- disabled while the POST is in flight (with a re-entry guard for
  fast double-tap before the binding repaints)
- success banner "Vstup pridany: <svc>" / "Visit added: <svc>"
- banner auto-clears after 2.5s, with a captured-message guard so a
  second press in the window does not clear the second message early
- errors now route to set_err (red banner) instead of being shoved
  into set_msg (green banner) via the misplaced error_format key

Threads the existing msg ReadSignal from DashboardPage through
CardActionPanel into ActionForm so the auto-clear timer can compare
the captured text against the live signal.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Playwright E2E (SUBAGENT, sonnet)

**Files:**
- Create: `e2e/tests/visit-button-feedback.spec.ts`

**Test approach:** seed a fresh card with an active monthly pass (charge-action transaction whose `service_name_sk = 'Mesačný preplatok'` and `valid_until` is 30 days in the future). Open the card via Quick Search, click the Fitness `log-visit-btn`, assert the disabled→banner→re-enabled→auto-clear sequence with zero console errors. Use a letter-heavy unique RUN_TAG (PR #59 lessons learned — avoids collisions with the prod-synced dev DB).

- [ ] **Step 1: Write the spec file**

Create `e2e/tests/visit-button-feedback.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #53: visit buttons (Fitness/Spinning shown when a card has an
// active monthly pass) had no per-press feedback — staff could not tell
// if the press registered or if the visit was logged. After the fix, the
// button greys out during the POST, the success banner shows
// "Visit added: Fitness", the button re-enables on response, and the
// banner auto-clears after 2.5s.

test('visit button shows loading + success banner + auto-clears', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Letter-heavy unique tag to avoid collisions with prod-synced dev DB.
    const RUN_TAG = `VBFB${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
    const barcode = `Visit${RUN_TAG}`;

    // Pass valid 30 days from now → days_remaining >= 0 → pass_is_active true.
    const today = new Date();
    const validUntil = new Date(today.getTime() + 30 * 24 * 60 * 60 * 1000);
    const validUntilIso = validUntil.toISOString().slice(0, 10);

    // Seed: monthly pass purchase (active), no other history.
    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [{
                amount: -35.00,
                action: 'charge',
                service_name_sk: 'Mesačný preplatok',
                valid_until: validUntilIso,
            }],
        }),
    });
    if (!seed.ok) {
        throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);
    }

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(1);
    await results.first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    // Both visit buttons (Fitness, Spinning) should be visible because
    // pass_is_active is true. We click the first one (Fitness, by the
    // alphabetical sort applied in action_form.rs:394).
    const visitButtons = page.locator('[data-testid="log-visit-btn"]');
    await expect(visitButtons).toHaveCount(2);
    const fitnessBtn = visitButtons.first();
    await expect(fitnessBtn).toContainText('Fitness');

    // Click. Within 1s the disabled binding must have repainted.
    await fitnessBtn.click();
    await expect(fitnessBtn).toBeDisabled({ timeout: 1000 });

    // Within 2s the success banner appears with the visit-added text.
    const banner = page.locator('.alert-success');
    await expect(banner).toBeVisible({ timeout: 2000 });
    await expect(banner).toHaveText('Visit added: Fitness');

    // Within 3s after the click the POST resolves and the button re-enables.
    await expect(fitnessBtn).toBeEnabled({ timeout: 3000 });

    // After 3.5s the auto-clear has fired (2.5s + ~1s buffer).
    await expect(banner).not.toBeVisible({ timeout: 3500 });

    assertCleanConsole(msgs);
});

test('visit button re-entry guard: rapid double-click fires only one POST', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `VBFG${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
    const barcode = `Guard${RUN_TAG}`;

    const today = new Date();
    const validUntil = new Date(today.getTime() + 30 * 24 * 60 * 60 * 1000);
    const validUntilIso = validUntil.toISOString().slice(0, 10);

    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [{
                amount: -35.00,
                action: 'charge',
                service_name_sk: 'Mesačný preplatok',
                valid_until: validUntilIso,
            }],
        }),
    });
    if (!seed.ok) {
        throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);
    }

    // Track every POST to /api/payments/log-visit.
    const logVisitRequests: string[] = [];
    page.on('request', (req) => {
        if (req.url().endsWith('/api/payments/log-visit') && req.method() === 'POST') {
            logVisitRequests.push(req.url());
        }
    });

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);
    await expect(page.locator('[data-testid="search-result"]')).toHaveCount(1);
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    const fitnessBtn = page.locator('[data-testid="log-visit-btn"]').first();

    // Two clicks dispatched back-to-back. The first sets loading=true;
    // the second hits either the re-entry guard (loading still true at
    // get_untracked time) or the disabled DOM attribute. Either way,
    // exactly one POST should fire.
    await fitnessBtn.click();
    await fitnessBtn.click({ force: true });

    // Wait for the first POST to complete.
    await expect(page.locator('.alert-success')).toBeVisible({ timeout: 2000 });

    expect(logVisitRequests.length).toBe(1);

    assertCleanConsole(msgs);
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/visit-button-feedback.spec.ts
git commit -m "$(cat <<'EOF'
test(e2e): visit-button feedback + double-click guard (#53)

Two specs:
1) Click Fitness Log-visit button → button disables, success banner
   "Visit added: Fitness" appears, button re-enables, banner
   auto-clears within 3.5s, console clean.
2) Rapid double-click → exactly one POST /api/payments/log-visit
   fires (re-entry guard + disabled binding both protect against
   double-submit).

Both seed a fresh card with an active monthly pass (Mesačný
preplatok charge with valid_until +30d) via /api/test/seed-transactions.
RUN_TAG uses a letter-heavy unique prefix to avoid collisions with the
prod-synced dev DB.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Push, monitor CI to terminal state, open PR (CONTROLLER-RUN)

**Files:** none.

- [ ] **Step 1: Push**

```bash
git push origin dev
```

- [ ] **Step 2: Monitor CI to terminal state**

Per `ci-monitoring.md`: ONE background command, no polling loops, no `/loop`, no scheduled wakeups.

```bash
# Wait for CI to nearly finish, then read terminal state
sleep 600 && gh run view --branch dev --json status,conclusion,jobs --limit 1
```

If the run isn't terminal yet, sleep another 300s and re-check.

ALL of the following jobs must be `success` (or `skipped` for the deploy/smoke-prod jobs that only fire on main):

- Test Integrity
- Lint
- Test
- Test (UI)
- Build WASM (UI)
- E2E Tests
- Mutation Testing (server-side; no server diff this PR → fast pass)
- Deploy (dev)
- Smoke (dev)
- Version Bump Check

If any job fails, run `gh run view <id> --log-failed`, fix the root cause, push ONE follow-up commit, and re-monitor.

- [ ] **Step 3: Open PR `dev` → `main`**

```bash
gh pr create --base main --head dev --title "v0.13.20: visit-button feedback (#53)" --body "$(cat <<'EOF'
## Summary

- Visit buttons (Fitness/Spinning, shown when card has an active monthly pass) now mirror the charge-button pattern: disabled during in-flight POST, success banner with localized "Vstup pridany: <svc>" / "Visit added: <svc>", banner auto-clears after 2.5s.
- Bug fix: error path on the visit POST was setting `set_msg` (green success banner) with a translated error string. Now routes to `set_err` (red error banner) like every other handler.
- Re-entry guard prevents fast double-tap from firing duplicate `/api/payments/log-visit` POSTs.

Closes #53.

## Test plan

- [ ] Two `wasm-bindgen-test` cases assert the new `visit_added_format` key renders correctly in Slovak and English.
- [ ] Playwright E2E `e2e/tests/visit-button-feedback.spec.ts` (two tests):
  - Click visit button → disabled within 1s → banner appears with "Visit added: Fitness" → re-enabled within 3s → banner gone within 3.5s → console clean.
  - Rapid double-click → exactly ONE `/api/payments/log-visit` POST fires.
- [ ] Manual verification on dev after deploy: open a real card with an active monthly pass, click visit button, confirm the disabled→banner→re-enabled→auto-clear sequence.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Verify PR is mergeable + clean**

```bash
gh pr view --json number,mergeable,mergeStateStatus,statusCheckRollup
```

Expected: `mergeable: "MERGEABLE"`, `mergeStateStatus: "CLEAN"`, all checks `SUCCESS`.

If `mergeStateStatus` is `UNSTABLE` or `BLOCKED`, investigate and fix per `autonomous-quality-discipline.md` — never merge with admin bypass.

- [ ] **Step 5: Send completion report and STOP. Per `pr-merge-policy.md`, never merge.**

End plan at "PR mergeable, awaiting user merge".

---

## Task 6: Post-deploy verification (CONTROLLER-RUN, ONLY after user merges)

**Files:** none.

**Trigger:** user explicitly says "merge it" or equivalent. Then GitHub auto-merges the PR, main CI deploys to prod.

- [ ] **Step 1: Wait for main-branch CI run terminal state after merge**

```bash
sleep 600 && gh run view --branch main --json status,conclusion,jobs --limit 1
```

All jobs must be green, including `Deploy (prod)` and `Smoke (prod)`.

- [ ] **Step 2: Verify dev frontend version label**

Use Playwright MCP (or `mcp__plugin_playwright_playwright__browser_navigate`):

1. Navigate to `https://spinbike-dev.newlevel.media`
2. Read `[data-testid="version"]` from the DOM
3. Expected: `v0.13.20` (or the equivalent dirty/short-SHA suffix matching the dev build)
4. Cross-check against `https://spinbike-dev.newlevel.media/api/version` JSON

- [ ] **Step 3: Verify prod frontend version label**

Same as Step 2 but against `https://spinbike.newlevel.media`. Expected: `v0.13.20`.

- [ ] **Step 4: Functional verification on PROD**

In Playwright on prod (`https://spinbike.newlevel.media/staff`):

1. Log in.
2. Search for a real card known to have an active monthly pass (look up a candidate via SQL on the prod DB if needed; ask the user or run `sqlite3 /var/lib/spinbike/spinbike.db "SELECT c.barcode, MAX(t.valid_until) FROM cards c JOIN transactions t ON t.card_id=c.id WHERE t.valid_until IS NOT NULL AND t.deleted_at IS NULL AND t.valid_until >= date('now') GROUP BY c.id LIMIT 3;"`).
3. Open the card.
4. Click the Fitness `log-visit-btn`.
5. Confirm: button disables briefly, `.alert-success` shows "Vstup pridany: Fitness", button re-enables, banner clears within ~3 s, no console errors.
6. (Optional) refresh and confirm the txn list shows the new visit row.

- [ ] **Step 5: Send final completion report (per `completion-report.md` template)**

Audits at top, Goal/URLs/PR at bottom, version label evidence in the `✅ Deploy:` line.

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| Loading state on click | Task 3 step 4 (`set_loading.set(true)`) |
| Disabled binding on visit buttons | Task 3 step 5 |
| Success banner with `visit_added_format` | Task 3 step 4 (`set_msg.set(m.clone())`) |
| Auto-clear with captured-message guard | Task 3 step 4 (the inner `spawn_local`) |
| Error path bug fix (`set_err` not `set_msg`) | Task 3 step 4 (`Err(e) => set_err.set(e)`) |
| New i18n key `visit_added_format` | Task 2 step 1 |
| wasm-bindgen unit tests for the i18n key | Task 2 step 2 |
| Playwright E2E with all 7 ordered checks | Task 4 step 1 (test 1) |
| Optional second test for double-click | Task 4 step 1 (test 2) |
| Mutation kill (button text + disabled state) | Task 4 step 1 (`toHaveText` + `toBeDisabled`) |
| Out-of-scope items honored (no toast, no flash, charge unchanged) | Tasks contain no charge changes |

**Type-consistency cross-check:**

- `visit_click_for(service_id, svc_name.clone())` — both args used in the closure body (Task 3 step 4) and at the call site (step 5). Match.
- `msg: ReadSignal<String>` prop — added in `ActionForm` (step 1), `CardActionPanel` (step 2), forwarded from `DashboardPage` (step 3). Match.
- `i18n::tf(lang.get_untracked(), "visit_added_format", &[&svc_name])` — key matches the one inserted in Task 2 step 1. Format-string `{}` placeholder consumes the single arg `svc_name`. Match.
- `gloo_timers::future::TimeoutFuture::new(2500).await` — same fully-qualified call as `staff_dashboard.rs:312`. No new imports required.

**Placeholder scan:** none — every code step contains the actual code, every command shows the exact invocation, every assertion shows the exact text.

**Out-of-tree gotchas anticipated:**

- The seed-transactions endpoint is gated by `SPINBIKE_TEST_MODE=1`. The dev/CI server already runs with that flag (PR #59's E2E used the same endpoint). No new server config needed.
- `Mesačný preplatok` contains diacritics. The seed body sends UTF-8 JSON; the SQLite `name_sk` column is UTF-8. The seed endpoint matches `name_sk = ?` exactly — no normalization. We pass the same string the migration uses (with diacritics).
- The `chrono::Local::now().date_naive()` call in `action_form.rs:45` runs on the WASM side. The `valid_until` for the seeded pass is set in the test as `today + 30 days` (UTC `toISOString` slice). At local-midnight ± 2h windows the date can shift, but +30 days gives an enormous safety margin — the pass will always be active.
