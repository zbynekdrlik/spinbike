# Card Search Keyboard Navigation Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the staff card search so the first suggestion is auto-selected on **every** query (not just the first after page load), and ArrowDown/ArrowUp/Enter work reliably across repeated searches.

**Architecture:** Three targeted changes in `spinbike-ui/src/pages/dashboard.rs`: (1) replace the HTML `autofocus` attribute with an explicit `NodeRef<Input>` so we can re-focus the search input after `pick_card` and after the ActionPanel closes; (2) move `set_highlighted_idx.set(0)` to fire eagerly on every query change inside the debounced Effect, not just on fetch success; (3) remove the `on:mouseenter` handler on suggestion rows so hover stops overwriting the keyboard-driven highlight. Verified end-to-end with a new Playwright spec that executes **two consecutive searches** in the same session.

**Tech Stack:** Rust + Leptos 0.7 (CSR / WASM), Playwright TypeScript for E2E, Trunk for frontend builds.

**Spec:** `docs/superpowers/specs/2026-04-15-card-search-keyboard-nav-fix-design.md`

---

## File Structure

**Modify:**
- `spinbike-ui/src/pages/dashboard.rs` — search input + dropdown in `DashboardPage` component
- `VERSION` — bump per `version-bumping` rule (dev must be > main)
- `Cargo.lock` — will update automatically via `scripts/sync-version.sh`

**Create:**
- `e2e/tests/card-search-keyboard.spec.ts` — single-test Playwright spec covering the regression

No other files are touched. No new dependencies.

---

## Task 1: Bump version

**Files:**
- Modify: `VERSION`
- Modify (auto): `spinbike-ui/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `crates/spinbike-core/Cargo.toml`

- [ ] **Step 1: Check current main version**

Run: `git fetch origin && git show origin/main:VERSION`

Record the value. Dev's VERSION must end strictly greater.

- [ ] **Step 2: Bump patch in `VERSION`**

Edit `VERSION`. If main is `0.3.0` and dev is currently `0.3.0`, set dev to `0.3.1`. If dev is already higher than main, skip this task entirely (nothing to do).

- [ ] **Step 3: Sync to Cargo.toml files**

Run: `bash scripts/sync-version.sh`
Expected: script updates the three `version = "..."` lines in the workspace Cargo.toml files.

- [ ] **Step 4: Commit**

```bash
git add VERSION spinbike-ui/Cargo.toml crates/spinbike-server/Cargo.toml crates/spinbike-core/Cargo.toml
git commit -m "chore: bump version for card search keyboard-nav fix

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Write the failing Playwright E2E test

**Files:**
- Create: `e2e/tests/card-search-keyboard.spec.ts`

TDD: this test MUST exist and MUST fail before we touch dashboard.rs. It pins down the exact regression behavior.

- [ ] **Step 1: Create the test file**

Create `e2e/tests/card-search-keyboard.spec.ts` with this EXACT content:

```typescript
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Card search — keyboard navigation', () => {
    test('auto-select + arrow keys work on first AND second search', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        const searchInput = page.locator('input[type="search"]');
        await searchInput.waitFor();

        // --- First search: "TestCorp" returns 2 results (Jana, Petr) ---
        await searchInput.focus();
        await page.keyboard.type('TestCorp', { delay: 30 });

        // Wait for the debounced fetch to populate the dropdown.
        await expect(page.locator('[data-testid="search-result"]')).toHaveCount(2, { timeout: 3000 });

        // First result must be auto-highlighted (no click, no arrow needed).
        const firstRow = page.locator('[data-testid="search-result"]').nth(0);
        await expect(firstRow).toHaveClass(/search-result-active/);

        // Enter picks the highlighted (first) card. Jana Testova sorts first
        // alphabetically by last_name; the backend orders by last_name asc.
        await page.keyboard.press('Enter');
        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();
        await expect(panel).toContainText('Testova');

        // Close the action panel (× button, title="close").
        await panel.locator('button[title="close"]').click();
        await expect(panel).toHaveCount(0);

        // --- Second search: "TestCorp" again, exercising the regression ---
        // After the first pick_card, the input must still (or again) have focus
        // so the user can just start typing.
        await expect(searchInput).toBeFocused();
        await page.keyboard.type('TestCorp', { delay: 30 });

        await expect(page.locator('[data-testid="search-result"]')).toHaveCount(2, { timeout: 3000 });

        // The regression check: first row auto-highlighted on the SECOND search too.
        await expect(page.locator('[data-testid="search-result"]').nth(0)).toHaveClass(/search-result-active/);

        // ArrowDown moves highlight to the second row.
        await page.keyboard.press('ArrowDown');
        await expect(page.locator('[data-testid="search-result"]').nth(1)).toHaveClass(/search-result-active/);
        await expect(page.locator('[data-testid="search-result"]').nth(0)).not.toHaveClass(/search-result-active/);

        // Enter picks the second card (Petr Vzorny).
        await page.keyboard.press('Enter');
        await expect(panel).toBeVisible();
        await expect(panel).toContainText('Vzorny');

        assertCleanConsole(consoleMessages);
    });
});
```

- [ ] **Step 2: Run the test against the current (broken) build to confirm it fails**

The CI runs Playwright against a fresh server. Locally, you can skip this if a dev server isn't running — CI will fail the test on the next push. To run locally:

```bash
cd e2e && npx playwright test tests/card-search-keyboard.spec.ts
```

Expected: FAIL on the **second** `await expect(searchInput).toBeFocused()` or on the auto-highlight assertion for the second search. If it passes unexpectedly, investigate — the regression may already be fixed and the spec is obsolete.

- [ ] **Step 3: Commit the failing test**

```bash
git add e2e/tests/card-search-keyboard.spec.ts
git commit -m "test: failing e2e for card search keyboard nav on second search

Reproduces the regression where auto-select and arrow keys stop working
after the first pick_card in a session.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Add NodeRef + explicit focus control to search input

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs` (lines around 70–200)

This is fix #1 from the spec: replace `autofocus` with an explicit `NodeRef`, focus on mount, focus after `pick_card`, focus when ActionPanel closes.

- [ ] **Step 1: Add the NodeRef declaration**

In `DashboardPage`, just below the `highlighted_idx` signal declaration (currently around line 83), add:

```rust
// Explicit ref so we can restore focus after pick_card and after the
// action panel closes. HTML `autofocus` only runs once on mount.
let search_input_ref = NodeRef::<leptos::html::Input>::new();
```

- [ ] **Step 2: Wire the ref and remove `autofocus`**

Find the `<input type="search" ...>` element (currently around line 185). Change:

```rust
<input
    type="search"
    class="form-control"
    autofocus
    inputmode="search"
    prop:value=move || query.get()
    placeholder=move || i18n::t(lang.get(), "search_cards_placeholder")
    on:input=on_search_input
    on:keydown=on_search_keydown
    style="font-size:1.1rem;padding:12px"
/>
```

to:

```rust
<input
    type="search"
    class="form-control"
    node_ref=search_input_ref
    inputmode="search"
    prop:value=move || query.get()
    placeholder=move || i18n::t(lang.get(), "search_cards_placeholder")
    on:input=on_search_input
    on:keydown=on_search_keydown
    style="font-size:1.1rem;padding:12px"
/>
```

- [ ] **Step 3: Focus on mount**

Below the `let search_input_ref = ...` line, add a one-shot effect that focuses the input once it renders:

```rust
Effect::new(move |_| {
    if let Some(el) = search_input_ref.get() {
        let _ = el.focus();
    }
});
```

The `NodeRef::get()` returns `None` on the first pass (before the element is mounted) and `Some(el)` once it is. Leptos re-runs the effect when the ref becomes available.

- [ ] **Step 4: Re-focus after pick_card**

Find `pick_card` (currently around line 147):

```rust
let pick_card = move |card: CardInfo| {
    set_selected.set(Some(card));
    set_query.set(String::new());
    set_results.set(Vec::new());
    set_err.set(String::new());
};
```

Change to:

```rust
let pick_card = move |card: CardInfo| {
    set_selected.set(Some(card));
    set_query.set(String::new());
    set_results.set(Vec::new());
    set_err.set(String::new());
    // Keep the keyboard-first workflow alive: the user should be able to
    // start typing the next card's name immediately without reaching for
    // the mouse.
    if let Some(el) = search_input_ref.get() {
        let _ = el.focus();
    }
};
```

- [ ] **Step 5: Re-focus when action panel closes**

Find `clear_selection` (currently around line 139):

```rust
let clear_selection = move |_| {
    set_selected.set(None);
    set_msg.set(String::new());
};
```

Change to:

```rust
let clear_selection = move |_| {
    set_selected.set(None);
    set_msg.set(String::new());
    if let Some(el) = search_input_ref.get() {
        let _ = el.focus();
    }
};
```

- [ ] **Step 6: Sanity-check the build**

Run: `cargo fmt --all --check`
Expected: exits 0. If formatting drifted, run `cargo fmt --all` and re-check.

Do NOT run `cargo build` or `trunk build` locally — CI handles those (see project CLAUDE.md). Push will be gated by the CI WASM build job.

- [ ] **Step 7: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs
git commit -m "fix: restore search input focus after pick and panel close

autofocus only runs on initial mount. After the user picks a card,
focus moved to the action panel and never came back, breaking keyboard
navigation on every subsequent search. Use an explicit NodeRef and
call .focus() on mount, after pick_card, and when the panel closes.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Reset highlighted_idx eagerly on every query change

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs` (the debounced search Effect, lines ~97–128)

This is fix #2 from the spec: make the "first suggestion highlighted" invariant hold immediately, not only after the 250ms fetch resolves.

- [ ] **Step 1: Add the eager reset**

Find the debounced search Effect (currently around line 97). It starts:

```rust
Effect::new(move |_| {
    let q = query.get();
    set_msg.set(String::new());
    if q.trim().is_empty() {
        set_results.set(Vec::new());
        set_searching.set(false);
        return;
    }
    set_searching.set(true);
    ...
```

Change the opening to reset highlight **before** any branch:

```rust
Effect::new(move |_| {
    let q = query.get();
    set_msg.set(String::new());
    // Every new query resets the keyboard highlight to row 0. Without
    // this, a prior mouseenter or a stale `highlighted_idx` from the
    // last search can survive into the new dropdown.
    set_highlighted_idx.set(0);
    if q.trim().is_empty() {
        set_results.set(Vec::new());
        set_searching.set(false);
        return;
    }
    set_searching.set(true);
    ...
```

The existing `set_highlighted_idx.set(0)` call inside the `Ok(list)` arm of the fetch match becomes redundant — remove it to keep a single source of truth.

Find this block further down (currently around line 115–120):

```rust
match api::get::<Vec<CardInfo>>(&format!("/api/cards/search?q={encoded}&limit=10")).await {
    Ok(list) => {
        if query.get_untracked() == q_at_start {
            set_results.set(list);
            set_highlighted_idx.set(0);
        }
    }
    Err(e) => set_err.set(e),
}
```

Change to:

```rust
match api::get::<Vec<CardInfo>>(&format!("/api/cards/search?q={encoded}&limit=10")).await {
    Ok(list) => {
        if query.get_untracked() == q_at_start {
            set_results.set(list);
        }
    }
    Err(e) => set_err.set(e),
}
```

- [ ] **Step 2: Sanity-check the build**

Run: `cargo fmt --all --check`
Expected: exits 0.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs
git commit -m "fix: reset highlighted_idx on every query, not just on fetch success

Moving the reset into the top of the debounced Effect guarantees the
'first suggestion is pre-highlighted' invariant holds on every query,
not only when the fetch completes. Also eliminates a race where a
stale idx could survive between a query change and the fetch reply.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Remove mouseenter hover-hijack

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard.rs` (suggestion row view, around line 231)

This is fix #3 from the spec. Hover should not overwrite keyboard-driven highlight.

- [ ] **Step 1: Delete the on:mouseenter handler**

Find the suggestion row view (currently around line 220–236):

```rust
view! {
    <div
        class=move || {
            if highlighted_idx.get() == idx {
                "search-result search-result-active"
            } else {
                "search-result"
            }
        }
        data-testid="search-result"
        style="display:flex;justify-content:space-between;align-items:center;padding:10px;border-bottom:1px solid var(--border);cursor:pointer;gap:8px"
        on:mouseenter=move |_| set_highlighted_idx.set(idx)
        on:click={
            let card = card_for_pick.clone();
            move |_| pick_card(card.clone())
        }
    >
```

Remove the `on:mouseenter` line entirely:

```rust
view! {
    <div
        class=move || {
            if highlighted_idx.get() == idx {
                "search-result search-result-active"
            } else {
                "search-result"
            }
        }
        data-testid="search-result"
        style="display:flex;justify-content:space-between;align-items:center;padding:10px;border-bottom:1px solid var(--border);cursor:pointer;gap:8px"
        on:click={
            let card = card_for_pick.clone();
            move |_| pick_card(card.clone())
        }
    >
```

Visual hover feedback still comes from the existing CSS `.search-result:hover` rule in `spinbike-ui/style.css` (unchanged). Clicking a row still picks the card.

- [ ] **Step 2: Sanity-check the build**

Run: `cargo fmt --all --check`
Expected: exits 0.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard.rs
git commit -m "fix: remove mouseenter hijack of keyboard highlight

The on:mouseenter handler overwrote highlighted_idx based on cursor
position — silently breaking the 'first item auto-selected' invariant
whenever the dropdown re-rendered under the mouse. CSS :hover still
provides visual hover feedback, and click-to-pick is unchanged.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Push, monitor CI, verify on live site

**Files:** none (pipeline-level)

- [ ] **Step 1: Push to dev**

```bash
git push origin dev
```

- [ ] **Step 2: Monitor CI through terminal state**

Run: `gh run list --branch dev --limit 3`
Identify the latest run id, then monitor it until ALL jobs (lint, test, build-wasm, e2e, deploy) reach a terminal state. Per `ci-monitoring` rule:

```bash
# Replace <id> with the actual run id
sleep 300 && gh run view <id> --json status,conclusion,jobs
```

Use `run_in_background: true` so you can continue other work; react when the result arrives.

Expected: all jobs `success`. The new `card-search-keyboard.spec.ts` test MUST be in the e2e job's output and MUST pass.

- [ ] **Step 3: If any job fails, diagnose and fix**

Run: `gh run view <id> --log-failed`

Common failure modes:
- `cargo fmt` drift → run `cargo fmt --all` locally, amend with a **new** commit (never `--amend`), push.
- Playwright timeout on `toBeFocused()` → the new focus call may need a microtask before the DOM reflects it. Not expected, but if seen, investigate WHY (check devtools via Playwright `--headed` locally) before patching.
- E2E assertion on `.toHaveClass(/search-result-active/)` for second search still fails → the Effect ordering is wrong. Re-read Task 4 and confirm `set_highlighted_idx.set(0)` runs **before** the empty-query early return.

Fix the root cause, commit once, push once, monitor again.

- [ ] **Step 4: Verify on the deployed site**

Once CI deploy is green, open `https://spinbike.newlevel.media/staff` in Playwright (or the local browser MCP) and manually reproduce the user scenario:

1. Log in as staff.
2. Type any name that matches ≥2 cards → confirm first row is highlighted.
3. Press Enter → action panel opens.
4. Close the panel → **the search input should be focused again** (blink cursor visible).
5. Without clicking, type another query → confirm first row is again highlighted.
6. ArrowDown → second row highlighted.
7. Enter → that card opens.

Record what you observed with real values. Also open devtools Console and confirm zero errors/warnings during the interaction.

- [ ] **Step 5: Create PR from dev to main**

Once CI is green AND live verification passes:

```bash
gh pr create --base main --head dev --title "fix: card search keyboard nav on repeat searches" --body "$(cat <<'EOF'
## Summary
- Restore search input focus after pick_card and action-panel close (was broken because `autofocus` only fires on initial mount)
- Reset `highlighted_idx` on every query change, not only after fetch success
- Remove `on:mouseenter` hijack that overwrote keyboard-driven highlight

## Test plan
- [x] New Playwright test `card-search-keyboard.spec.ts` reproduces the regression and passes after the fix
- [x] CI green (lint, test, build-wasm, e2e, deploy)
- [x] Manual verification on https://spinbike.newlevel.media: first + second search both auto-highlight, ArrowDown works, Enter picks correct card, zero console errors

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Verify the PR is mergeable:

```bash
gh pr view --json mergeable,mergeableState
```

Expected: `mergeable: true` and `mergeableState: "clean"`.

- [ ] **Step 6: WAIT for the user's explicit merge instruction**

Per `pr-merge-policy`: a green PR is NOT permission to merge. Report the PR URL with status and wait for the user to say "merge it".

---

## Self-review (done by the plan author)

**Spec coverage:**
- Root cause #1 (focus loss) → Task 3 ✓
- Root cause #2 (late `highlighted_idx` reset + mouseenter) → Tasks 4 & 5 ✓
- Playwright E2E covering both reported symptoms → Task 2 ✓
- Zero console errors assertion → included in Task 2 test ✓
- Version bump + rollout via self-hosted runner auto-deploy → Tasks 1 & 6 ✓
- Out-of-scope items (visual redesign, new shortcuts, scroll-into-view) → not mentioned in any task ✓

**Placeholder scan:** no TBD / TODO / "similar to" / "appropriate error handling" strings.

**Type consistency:** `search_input_ref` is the only new name and is used consistently (Tasks 3 steps 1, 2, 4, 5). `pick_card` and `clear_selection` signatures unchanged. Existing `data-testid` values (`search-result`, `action-panel`, `topup-20`, `charge-service`, `charge-submit`) match the current dashboard.rs and are reused in the test. The close button has no dedicated `data-testid` — the test selects it via `button[title="close"]` which matches line 389 of dashboard.rs.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-15-card-search-keyboard-nav-fix.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
