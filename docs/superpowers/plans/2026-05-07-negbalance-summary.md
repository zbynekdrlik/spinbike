# Negative-Balance Summary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show count of users in negative balance + sum of their debt in the heading of the desk negative-balance list, and rename "Karty s dlhom" / "Cards with negative balance" to "Klienti v minuse" / "Customers with negative balance" (cards table dropped in PR #67).

**Architecture:** Frontend-only. No API change. Sum + count computed client-side from the already-fetched `Vec<NegativeBalanceUser>`. Three files modified: `negative_balance_list.rs` (helper + render), `i18n.rs` (key text + tests), `e2e/tests/negative-balance.spec.ts` (heading assertion).

**Tech Stack:** Leptos 0.7 (CSR/WASM), `wasm_bindgen_test` (UI unit tests), Playwright (E2E).

**Spec:** `docs/superpowers/specs/2026-05-07-negbalance-summary-design.md` (committed at `cb26558` on `dev`).

---

## File map

| File | Responsibility | Tasks |
|------|----------------|-------|
| `VERSION` | Single source of truth for project version. | 1 |
| `Cargo.toml`, `spinbike-ui/Cargo.toml`, etc. | Synced from `VERSION` by `scripts/sync-version.sh`. | 1 |
| `spinbike-ui/src/i18n.rs` | i18n key table + per-key unit tests. | 2 |
| `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` | Component rendering the list; private `summary_suffix` helper. | 3, 4 |
| `e2e/tests/negative-balance.spec.ts` | Playwright E2E. Existing test extended; no new spec file. | 4 |

---

## Project conventions (every task MUST honor)

- Working directory: `/home/newlevel/devel/spinbike`. Branch: `dev`. Never push to main; never merge.
- **NO local cargo build / test / clippy / trunk build.** Only `cargo fmt --all --check` is allowed locally. CI is authoritative.
- **NEVER use `git add -A` or `git add .`.** Use explicit paths or `git add -u`.
- Slovak strings are UNACCENTED (no diacritics: `minuse`, not `mínuse`).
- Subagents must NOT add `wasm_bindgen_test_configure!(run_in_browser);` — it silently skips tests under `wasm-pack test --node`.
- Each task ends with a single commit using a Conventional Commit message ending with `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>`.

---

### Task 1: VERSION bump 0.13.24 → 0.13.25

**Files:**
- Modify: `VERSION`
- Sync (auto): `Cargo.toml`, `spinbike-ui/Cargo.toml` (via `scripts/sync-version.sh`)

**Owner:** CONTROLLER (not a subagent).

- [ ] **Step 1: Edit VERSION**

Replace contents of `VERSION` with `0.13.25` (single line, no trailing whitespace).

- [ ] **Step 2: Sync version into Cargo.tomls**

Run:

```bash
bash scripts/sync-version.sh
```

Expected: script exits 0, both `Cargo.toml` files now show `version = "0.13.25"`.

- [ ] **Step 3: Verify only the 3 expected files changed**

Run:

```bash
git status --short
```

Expected output:

```
 M Cargo.toml
 M VERSION
 M spinbike-ui/Cargo.toml
```

If anything else appears, STOP and investigate.

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "$(cat <<'EOF'
chore: bump version 0.13.24 → 0.13.25

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: i18n heading text rename + unit tests

**Files:**
- Modify: `spinbike-ui/src/i18n.rs:642`
- Modify: `spinbike-ui/src/i18n.rs:834-836`
- Modify: `spinbike-ui/src/i18n.rs:839-841`

**Owner:** SUBAGENT (sonnet).

**Subagent prompt MUST include:** "Read the spec section 'i18n change' in `docs/superpowers/specs/2026-05-07-negbalance-summary-design.md` verbatim. Slovak is UNACCENTED — `Klienti v minuse` (no `í` on `minuse`). Ask any clarifying questions before editing."

- [ ] **Step 1: Update the failing tests first (RED)**

In `spinbike-ui/src/i18n.rs`, replace the body of `negative_balance_heading_slovak` (currently at line 834):

```rust
    fn negative_balance_heading_slovak() {
        assert_eq!(t(Lang::Sk, "negative_balance_heading"), "Klienti v minuse");
    }
```

And replace the body of `negative_balance_heading_english` (currently at line 839):

```rust
    fn negative_balance_heading_english() {
        assert_eq!(t(Lang::En, "negative_balance_heading"), "Customers with negative balance");
    }
```

Note: the function declarations and their `#[wasm_bindgen_test]` (or `#[test]`) attributes must remain unchanged — only the assertion strings change.

- [ ] **Step 2: Update the i18n key (GREEN)**

In `spinbike-ui/src/i18n.rs`, find the line:

```rust
    m.insert("negative_balance_heading", ("Karty s dlhom", "Cards with negative balance"));
```

Replace with:

```rust
    m.insert("negative_balance_heading", ("Klienti v minuse", "Customers with negative balance"));
```

- [ ] **Step 3: Lint check (the only allowed local check)**

```bash
cargo fmt --all --check
```

Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "$(cat <<'EOF'
i18n(neg-balance): rename heading 'Karty s dlhom' → 'Klienti v minuse'

The cards table was dropped in PR #67. The negative-balance list now
operates on users; the heading wording follows. Slovak unaccented per
project convention. EN: 'Customers with negative balance'.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Add `summary_suffix` helper + 2 wasm-bindgen tests

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` (add private fn near line 137; add `#[cfg(test)]` module at the bottom)

**Owner:** SUBAGENT (sonnet).

**Subagent prompt MUST include:** "Read the spec section 'Heading format' verbatim. The format string uses U+00B7 middle-dot `·` and TWO spaces on each side. Sum format is `format!(\"{:.2} €\", sum)` with ASCII hyphen for negatives. Do NOT add `wasm_bindgen_test_configure!(run_in_browser);` — that silently skips tests under `wasm-pack test --node`."

- [ ] **Step 1: Add the failing tests first (RED)**

Append to `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` (at the end of the file, after the existing `neg_to_card_info` function):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    fn neg_user(credit: f64) -> NegativeBalanceUser {
        NegativeBalanceUser {
            id: 0,
            name: String::new(),
            card_code: None,
            credit,
            blocked: false,
            company: None,
            last_visit_at: None,
            last_payment_at: None,
            pass: None,
        }
    }

    #[wasm_bindgen_test]
    fn summary_suffix_three_users() {
        let rows = vec![neg_user(-1.50), neg_user(-3.10), neg_user(-7.80)];
        assert_eq!(summary_suffix(&rows), "  ·  3  ·  -12.40 €");
    }

    #[wasm_bindgen_test]
    fn summary_suffix_single_user() {
        let rows = vec![neg_user(-0.50)];
        assert_eq!(summary_suffix(&rows), "  ·  1  ·  -0.50 €");
    }
}
```

- [ ] **Step 2: Add the implementation (GREEN)**

Insert this private function in `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` just before the existing `fn today_local()` at line 137 (so it sits with the other private helpers and is visible to the `#[cfg(test)]` module via `use super::*;`):

```rust
/// Heading suffix for the negative-balance list: `"  ·  {count}  ·  {sum} €"`.
/// Separator is U+00B7 with two spaces on each side. Sum uses ASCII hyphen
/// (matches the per-row credit formatting). Caller short-circuits the empty
/// case before this is ever invoked.
fn summary_suffix(rows: &[NegativeBalanceUser]) -> String {
    let count = rows.len();
    let sum: f64 = rows.iter().map(|r| r.credit).sum();
    format!("  ·  {count}  ·  {sum:.2} €")
}
```

- [ ] **Step 3: Lint check**

```bash
cargo fmt --all --check
```

Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/negative_balance_list.rs
git commit -m "$(cat <<'EOF'
feat(neg-balance): add summary_suffix helper + wasm-bindgen tests (#72)

Computes the heading suffix '  ·  {count}  ·  {sum:.2} €' from a slice
of NegativeBalanceUser rows. Pure function so it can be unit-tested in
the Node-based wasm-bindgen-test harness. Two tests pin the exact
format string (separator spacing, decimal precision, ASCII hyphen) to
kill the obvious mutants on len()/sum/format-string changes.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Wire helper into render + extend E2E

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/negative_balance_list.rs:106` (the `<h3>` element)
- Modify: `e2e/tests/negative-balance.spec.ts` (extend the existing single test; do NOT add a new test file)

**Owner:** SUBAGENT (sonnet).

**Subagent prompt MUST include:** "Read the spec section 'Heading format' AND 'Files touched' verbatim. The dev DB is prod-synced and may already contain many negative-balance users; the seeded count of 2 in this E2E will be ADDED to that existing total, so assert `count >= 2`, not `count == 2`. Ask any clarifying questions before writing code."

- [ ] **Step 1: Update component render (calls helper from Task 3)**

In `spinbike-ui/src/pages/dashboard/negative_balance_list.rs`, locate the heading line (currently line 106):

```rust
                        <h3 class="negative-balance-list__heading">{heading}</h3>
```

Replace with:

```rust
                        <h3 class="negative-balance-list__heading">
                            {format!("{heading}{}", summary_suffix(&rows))}
                        </h3>
```

Note: `rows` is the local `Vec<NegativeBalanceUser>` already in scope at that point in the closure (declared at line 58 via `let rows = rows.get();`). It is **moved** by `rows.into_iter()` in the items map at line 69 — so `summary_suffix(&rows)` MUST be called BEFORE `rows.into_iter()`. The render block must be restructured slightly so the heading suffix is computed before the items collection consumes `rows`.

Concretely, replace lines 67-110 (the entire body inside the `if rows.is_empty()` short-circuit) with:

```rust
            let lang_now = lang.get();
            let heading = i18n::t(lang_now, "negative_balance_heading").to_string();
            let last_visit_label = i18n::t(lang_now, "last_visit_label").to_string();
            let last_payment_label = i18n::t(lang_now, "last_payment_label").to_string();
            let never_label = i18n::t(lang_now, "never_label").to_string();
            let today = today_local();
            let suffix = summary_suffix(&rows);

            let items = rows.into_iter().map(|r| {
                let name = super::helpers::user_display_name(
                    &r.name,
                    r.company.as_deref(),
                    r.card_code.as_deref(),
                );
                let credit = format!("{:.2} €", r.credit);
                let last_visit = format_optional_date(&r.last_visit_at, today, lang_now, &never_label);
                let last_payment = format_optional_date(&r.last_payment_at, today, lang_now, &never_label);
                let lv = last_visit_label.clone();
                let lp = last_payment_label.clone();
                let card_for_pick = neg_to_card_info(&r);
                view! {
                    <div
                        class="negative-balance-row"
                        data-testid="negative-balance-row"
                        on:click={
                            let card = card_for_pick.clone();
                            move |_| on_pick.run(card.clone())
                        }
                    >
                        <div class="negative-balance-row__main">
                            <div class="negative-balance-row__name">{name}</div>
                            <div class="negative-balance-row__meta">
                                {format!("{lv}: {last_visit}")}
                                {" · "}
                                {format!("{lp}: {last_payment}")}
                            </div>
                        </div>
                        <div class="negative-balance-row__credit credit-negative">{credit}</div>
                    </div>
                }
            }).collect_view();

            view! {
                <div class="card mb-2 negative-balance-list" data-testid="negative-balance-list">
                    <div class="card__body">
                        <h3 class="negative-balance-list__heading">{format!("{heading}{suffix}")}</h3>
                        {items}
                    </div>
                </div>
            }.into_any()
```

The only behavioral change is the new `let suffix = summary_suffix(&rows);` line (added BEFORE `rows.into_iter()`) and the heading rendering `{format!("{heading}{suffix}")}` instead of `{heading}`.

- [ ] **Step 2: Extend the E2E to assert the heading**

In `e2e/tests/negative-balance.spec.ts`, find the existing assertion block on the idle desk list (after line 65, where `await expect(list).toBeVisible(...)` succeeds, and before the row-order assertions at lines 76-86). Insert this new block right after `await expect(list).toBeVisible({ timeout: 5000 });` (after line 65):

```typescript
    // ---- Surface 1a: heading carries count + sum (Issue #72) ---------------------
    // Heading format: "{HEADING}  ·  {count}  ·  {-DD.DD €}".
    // Dev DB is prod-synced — the count is whatever-is-already-there + our 2
    // negatives. We assert structural shape (regex), not exact values, with a
    // floor of >= 2 from our seeded rows.
    const heading = list.locator('.negative-balance-list__heading');
    await expect(heading).toBeVisible();
    const headingText = (await heading.textContent())?.trim() ?? '';
    const headingMatch = headingText.match(
        /^(Klienti v minuse|Customers with negative balance)\s+·\s+(\d+)\s+·\s+(-?\d+\.\d{2})\s+€$/,
    );
    expect(headingMatch, `heading text "${headingText}" did not match expected pattern`).not.toBeNull();
    const negCount = Number(headingMatch![2]);
    const negSum = Number(headingMatch![3]);
    expect(negCount).toBeGreaterThanOrEqual(2);
    expect(negSum).toBeLessThanOrEqual(-13.5); // Alpha (-3.50) + Bravo (-10.00) + any prior negatives
```

- [ ] **Step 3: Lint check**

```bash
cargo fmt --all --check
```

Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/negative_balance_list.rs e2e/tests/negative-balance.spec.ts
git commit -m "$(cat <<'EOF'
feat(neg-balance): show count + sum in heading; E2E asserts (#72)

Heading now reads e.g. "Klienti v minuse  ·  5  ·  -12.40 €" — gives
the desk operator at-a-glance totals on top of the per-row list.

E2E asserts heading regex + structural floor (count >= 2, sum <= -13.5
from seeded rows). Uses regex over either-language so the test is
agnostic to the operator's lang setting.

Closes #72

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Push + monitor CI to terminal state + open PR

**Owner:** CONTROLLER (not a subagent).

- [ ] **Step 1: Push to dev**

```bash
git push origin dev
```

- [ ] **Step 2: Identify the CI run for this push**

```bash
gh run list --branch dev --limit 1 --json databaseId,headSha,event,status
```

Capture the `databaseId` of the push run.

- [ ] **Step 3: Monitor CI to terminal state**

Single background command — NO `/loop`, NO custom monitor scripts.

```bash
sleep 600 && gh run view <RUN_ID> --json status,conclusion,jobs
```

Run with `run_in_background: true`. When the result lands, inspect jobs:

- ALL jobs must be `success` (or `skipped` for cross-event jobs like Mutation Testing on push).
- If any job is `failure`: `gh run view <RUN_ID> --log-failed`, fix root cause, push fix, re-monitor.
- If still `in_progress` after 10 min: re-run `sleep 300 && gh run view <RUN_ID> ...` once.

- [ ] **Step 4: Open the PR**

After CI is green:

```bash
gh pr create --base main --head dev --title "v0.13.25: negative-balance count+sum + heading rename (#72)" --body "$(cat <<'EOF'
## Summary

- Desk negative-balance list heading now shows total user count + sum of negative credit, e.g. `Klienti v minuse  ·  5  ·  -12.40 €`.
- Heading reworded from cards to users (the `cards` table was dropped in PR #67): `Karty s dlhom` → `Klienti v minuse` (SK), `Cards with negative balance` → `Customers with negative balance` (EN).
- VERSION bump 0.13.24 → 0.13.25.

Closes #72.

## Implementation

Frontend-only. No API change. Sum + count computed client-side via a new private `fn summary_suffix(rows: &[NegativeBalanceUser]) -> String` helper in `negative_balance_list.rs`. Two `wasm-bindgen-test` cases pin the exact format string. E2E extended to read the heading via regex and assert `count >= 2` + sum floor from seeded rows.

## Test plan

- [ ] CI green: Test Integrity, Lint, Test, Test (UI), Build WASM, E2E, Mutation Testing, Deploy (dev), Smoke (dev)
- [ ] Post-deploy verification on https://spinbike.newlevel.media (after merge): version DOM `[data-testid="version"]` reads `v0.13.25`; heading on `/staff` matches the regex; 0 console errors

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Verify PR is mergeable + clean**

```bash
gh pr view --json number,mergeable,mergeStateStatus
```

Expected: `mergeable: MERGEABLE` and `mergeStateStatus: CLEAN`.

If `mergeStateStatus` is anything other than `CLEAN` (UNSTABLE, BLOCKED, BEHIND, DIRTY) — investigate and fix per `autonomous-quality-discipline.md`. **DO NOT MERGE.** End the plan at "PR mergeable, awaiting user merge".

---

### Task 6: Post-deploy verification on prod (only after user merges)

**Owner:** CONTROLLER (not a subagent).

**Trigger:** User explicitly says "merge it" → controller merges PR → main CI runs → `Deploy (prod)` + `Smoke (prod)` jobs complete.

- [ ] **Step 1: Wait for main CI green**

After the merge:

```bash
gh run list --branch main --limit 1 --json databaseId,status
```

Run `sleep 600 && gh run view <RUN_ID> --json status,conclusion,jobs` in background. ALL jobs (including `Deploy (prod)` and `Smoke (prod)`) must be `success`.

- [ ] **Step 2: Verify backend version**

```bash
curl -s https://spinbike.newlevel.media/api/version
```

Expected: `{"version":"0.13.25"}`.

- [ ] **Step 3: Open Playwright on prod and read DOM**

Navigate (Playwright MCP):

- `mcp__plugin_playwright_playwright__browser_navigate` to `https://spinbike.newlevel.media/staff`
- Login as admin (use the same credentials path the existing E2E uses; or use the prod credentials from environment)
- `mcp__plugin_playwright_playwright__browser_evaluate`:
  ```javascript
  () => {
    const v = document.querySelector('[data-testid="version"]')?.textContent ?? '';
    const h = document.querySelector('.negative-balance-list__heading')?.textContent ?? '';
    return { version: v, heading: h };
  }
  ```
- Assert:
  - `version` is `v0.13.25`
  - `heading` matches `/^(Klienti v minuse|Customers with negative balance)\s+·\s+\d+\s+·\s+-?\d+\.\d{2}\s+€$/`
- `mcp__plugin_playwright_playwright__browser_console_messages`: assert 0 errors. Pre-existing warnings (e.g. Chrome `integrity` preload notice) are not regressions.
- `mcp__plugin_playwright_playwright__browser_close`

- [ ] **Step 4: Send completion report**

Per `airuleset/modules/core/completion-report.md` — full template with `✅ CI`, `✅ /plan-check`, `✅ /review`, `✅ Deploy: prod frontend shows v0.13.25, heading regex matched`, `🌐 Dev` + `🌐 Prod` URLs, PR title + URL, no `❓ Question` line (work is done).

---

## Self-review

**Spec coverage:**

| Spec section                  | Plan task |
|-------------------------------|-----------|
| Goal                          | All tasks |
| Scope (frontend-only)         | Tasks 2, 3, 4 |
| Heading format string         | Task 3 (helper), Task 4 (render wiring) |
| i18n change (table)           | Task 2 |
| Files touched                 | Tasks 2, 3, 4 (and VERSION via Task 1) |
| Test coverage / mutation pressure | Task 3 (unit) + Task 4 (E2E) |
| Out of scope                  | Honored — no server change, no pagination, no CSS change |
| Risks / non-risks             | Empty-state short-circuit verified at line 59 of `negative_balance_list.rs` |

**Placeholder scan:** none.

**Type consistency:** `NegativeBalanceUser` is the same struct everywhere (already imported at the top of `negative_balance_list.rs`). `summary_suffix` signature is `&[NegativeBalanceUser] -> String` in Task 3 and called the same way in Task 4. The format string in Task 3's tests matches Task 4's render exactly (separator + decimal precision).

**Risk callouts the implementer needs to know:**

1. **rows is moved into items.** Task 4 explicitly restructures so `summary_suffix(&rows)` runs BEFORE `rows.into_iter()`. The provided code block already orders this correctly — copy it verbatim.

2. **Dev DB has prod data.** The E2E in Task 4 cannot assert `count == 2` because the dev DB is prod-synced and contains ~9 negative-balance users at last sync. Use `>=` floor only.

3. **Slovak unaccented.** `Klienti v minuse` (no `í`). The plan repeats this in Tasks 2 and 4; the implementer should not "fix" it to add the diacritic.
