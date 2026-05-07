# Negative-balance row single-line layout — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse desk negative-balance row from two lines to one. Drop `last_payment` rendering, render `{name} (Posledna navsteva: {value})` on a single inline element with credit right-aligned. Frontend-only.

**Architecture:** One Rust module touched (`negative_balance_list.rs`), one CSS section, one E2E spec. Extract `meta_inline()` pure helper for unit-test mutation pressure. Server response shape unchanged — `last_payment_at` field stays in struct (deserialized but not rendered).

**Tech Stack:** Leptos 0.7, wasm-bindgen-test, Playwright. Project conventions: NO local cargo build/test/clippy/trunk build, only `cargo fmt --all --check` allowed locally; CI authoritative. Slovak strings UNACCENTED. NEVER `git add -A` / `git add .`. NO `wasm_bindgen_test_configure!(run_in_browser);`.

**Spec:** `docs/superpowers/specs/2026-05-07-neg-balance-row-oneline-design.md` (committed `fba70e2`).

**Issue:** [#78](https://github.com/zbynekdrlik/spinbike/issues/78).

---

### Task 1: Bump VERSION 0.13.25 → 0.13.26 (CONTROLLER-RUN, NOT a subagent)

**Files:**
- Modify: `VERSION`
- Modify: `Cargo.toml`, `spinbike-ui/Cargo.toml` (sync via script)

- [ ] **Step 1: Update VERSION + sync**

```bash
echo "0.13.26" > VERSION
bash scripts/sync-version.sh
```

- [ ] **Step 2: Commit**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump version 0.13.25 → 0.13.26"
```

Per project memory `feedback_no_git_add_A.md`: explicit paths only.

---

### Task 2: Add `meta_inline` helper + 2 wasm-bindgen tests + apply to render (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/negative_balance_list.rs`

The helper is small and only useful when called. Land helper + tests + render-call in one commit so dead-code lint is not a concern.

- [ ] **Step 1: Add private helper at module scope (BELOW `format_optional_date`, ABOVE `today_local`):**

```rust
/// Inline meta suffix appended after the user's name in a negative-balance row.
/// Format: " ({label}: {value})" — leading space, parens, colon. Caller passes
/// the localized label (e.g. "Posledna navsteva") and pre-formatted value
/// (e.g. "vcera", "2 dni", "nikdy").
pub(super) fn meta_inline(label: &str, value: &str) -> String {
    format!(" ({label}: {value})")
}
```

- [ ] **Step 2: Add 2 wasm-bindgen test cases** inside the existing `#[cfg(test)] mod tests { ... }` block (already imports `use wasm_bindgen_test::*;` per project glob convention; do NOT add `wasm_bindgen_test_configure!(run_in_browser);`):

```rust
#[wasm_bindgen_test]
fn meta_inline_typical() {
    assert_eq!(meta_inline("Posledna navsteva", "vcera"), " (Posledna navsteva: vcera)");
}

#[wasm_bindgen_test]
fn meta_inline_never_label() {
    assert_eq!(meta_inline("Last visit", "never"), " (Last visit: never)");
}
```

- [ ] **Step 3: Replace render block** in the `view!` body. The current block (lines 70-100) builds a row with `__main`/`__name`/`__meta` divs. Replace with a single `__label` element + inline meta span. ALSO drop the `last_payment` variable and the `last_payment_label` `i18n::t` call (no longer used). Keep `last_visit`, `last_visit_label`, `never_label`, `today` — still needed.

NEW render block (replaces existing):

```rust
let items = rows.into_iter().map(|r| {
    let name = super::helpers::user_display_name(
        &r.name,
        r.company.as_deref(),
        r.card_code.as_deref(),
    );
    let credit = format!("{:.2} €", r.credit);
    let last_visit = format_optional_date(&r.last_visit_at, today, lang_now, &never_label);
    let meta = meta_inline(&last_visit_label, &last_visit);
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
            <div class="negative-balance-row__label">
                {name}
                <span class="negative-balance-row__meta-inline">{meta}</span>
            </div>
            <div class="negative-balance-row__credit credit-negative">{credit}</div>
        </div>
    }
}).collect_view();
```

ALSO remove the now-unused `last_payment_label` line from the closure header (it was: `let last_payment_label = i18n::t(lang_now, "last_payment_label").to_string();`).

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/negative_balance_list.rs
git commit -m "feat(neg-balance): single-line row layout (#78)"
```

Subagent CANNOT run `cargo test` / `cargo build` / `trunk build` — CI validates. Subagent MAY run `cargo fmt --all --check` only.

---

### Task 3: CSS + E2E row assertions (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/style.css`
- Modify: `e2e/tests/negative-balance.spec.ts`

- [ ] **Step 1: Replace CSS block** in `style.css` (the existing `.negative-balance-row__main`, `__name`, `__meta` selectors near line 1223):

```css
.negative-balance-row__label {
    min-width: 0;
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
}
.negative-balance-row__meta-inline {
    margin-left: 0;
    font-weight: 400;
    font-size: var(--fs-sm);
    color: var(--text-muted);
}
```

DELETE these existing rules:
```css
.negative-balance-row__main { min-width: 0; }
.negative-balance-row__name { font-weight: 600; }
.negative-balance-row__meta {
    font-size: var(--fs-sm);
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
```

Keep `.negative-balance-row` (flex container) and `.negative-balance-row__credit` rules untouched.

- [ ] **Step 2: Extend E2E test** in `e2e/tests/negative-balance.spec.ts`. Find the existing block that asserts `Alpha${RUN_TAG}` is visible (around line 89). AFTER the heading-regex block and BEFORE the order-check block, add row-text assertions for the seeded Alpha row:

```typescript
// Issue #78: single-line row layout — name + " (Posledna navsteva: ...)" + credit.
// Drops last-payment from the row entirely.
const alphaRowFull = rows.filter({ hasText: `Alpha${RUN_TAG}` }).first();
const alphaText = (await alphaRowFull.textContent()) ?? '';
expect(alphaText).toContain('(Posledna navsteva: ');
expect(alphaText).not.toContain('Posledna platba');
```

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/style.css e2e/tests/negative-balance.spec.ts
git commit -m "feat(neg-balance): single-line CSS + E2E row assertions (#78)"
```

---

### Task 4: Push + monitor CI to terminal state + open PR (CONTROLLER-RUN)

Per `ci-monitoring.md`: single `sleep N && gh run view --json status,conclusion,jobs` background command. NO `/loop`, NO custom monitor scripts. Per `pr-merge-policy.md`: NEVER merge.

- [ ] **Step 1: Push dev**

```bash
git push origin dev
```

- [ ] **Step 2: Capture run id and monitor**

```bash
RUN_ID=$(gh run list --branch dev --limit 1 --json databaseId -q '.[0].databaseId')
sleep 600 && gh run view "$RUN_ID" --json status,conclusion,jobs > /tmp/ci-status.json
```

(run in background; on completion, inspect)

- [ ] **Step 3: Open PR dev → main with `Closes #78` magic keyword**

```bash
gh pr create --base main --head dev --title "v0.13.26: negative-balance row single-line layout (#78)" --body "$(cat <<'EOF'
## Summary
- Collapse desk negative-balance row from two lines to one
- Drop per-row last_payment rendering (kept in API/struct as unused field)
- Extract `meta_inline()` helper with 2 wasm-bindgen tests for mutation pressure
- Update CSS: single `__label` + `__meta-inline` classes
- E2E asserts row contains "(Posledna navsteva: " AND not "Posledna platba"

Closes #78

## Test plan
- [ ] Test (UI) green — meta_inline_typical, meta_inline_never_label
- [ ] E2E green — row text assertions
- [ ] Mutation Testing green — kills paren/colon/space drop
- [ ] Deploy (prod) + Smoke (prod) green
EOF
)"
```

---

### Task 5: Post-deploy verification on prod (CONTROLLER-RUN, ONLY after user merges)

DO NOT execute Task 5 before merge. Per `post-deploy-verification.md`: open Playwright on https://spinbike.newlevel.media/staff, read DOM `[data-testid="version"]`, assert v0.13.26, read first `[data-testid="negative-balance-row"]` text.

- [ ] **Step 1:** Wait for explicit "merge it" from user. Do not merge autonomously.
- [ ] **Step 2:** After main CI green (Deploy prod + Smoke prod ✅), navigate Playwright to https://spinbike.newlevel.media/staff with cache-bust query.
- [ ] **Step 3:** Read DOM:
  - `[data-testid="version"]` text must equal `v0.13.26`.
  - First `[data-testid="negative-balance-row"]` textContent must match `/^.+ \(Posledna navsteva: .+\)\s*-?\d+\.\d{2}\s*€$/`.
  - 0 console errors.
- [ ] **Step 4:** Send completion report per `airuleset/modules/core/completion-report.md` template.
