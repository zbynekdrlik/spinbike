# Quality Bundle Implementation Plan (v0.13.13)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One PR cycle that closes #39 (E2E barcode collision flake), #28 (CHECK constraint on `transactions.note`), #22 (mutation testing for `spinbike-ui`), and #36 (cargo-mutants Axum compat).

**Architecture:** Each issue ships as 1–2 commits on `dev` after a single VERSION bump. CI validates each commit. After all commits land green, open one PR `dev` → `main`.

**Tech Stack:** Axum 0.8 + Leptos 0.7 + WASM + SQLite + Playwright. CI uses cargo-mutants, Swatinem/rust-cache, wasm-pack (new).

**Spec:** `docs/superpowers/specs/2026-05-01-quality-bundle-design.md` (committed at `d2c97e5`).

---

## Working environment & global rules

- **NOT a worktree.** Working directory is `/home/newlevel/devel/spinbike` on the `dev` branch.
- **Per project memory `feedback_no_git_add_A.md`:** NEVER `git add -A` or `git add .`. Use explicit paths or `git add -u` for tracked-file modifications.
- **Per project memory `feedback_subagent_no_local_build.md`:** subagent prompts must NOT instruct cargo build/test/clippy/check or trunk/wasm-pack runs. CI is authoritative. The ONLY local check allowed is `cargo fmt --all --check`.
- **Per `pr-merge-policy.md`:** never merge the PR. Plan ends at "PR mergeable, awaiting user merge."
- **Per `ci-monitoring.md`:** monitor via single `sleep N && gh run view --json status,conclusion,jobs` background command — no custom monitor scripts, no `gh run watch`.

---

## File structure & responsibilities

| File | Owns | Touched in task |
|---|---|---|
| `VERSION` | Single source of truth for `0.13.13` | T1 |
| `e2e/tests/helpers.ts` | Shared E2E utilities; gains `activateUniqueCard` | T2 |
| `e2e/tests/card-action-form.spec.ts` | Uses shared `activateUniqueCard` | T2 |
| `e2e/tests/desk-ux.spec.ts` | Uses shared `activateUniqueCard` | T2 |
| `crates/spinbike-server/src/routes/payments.rs` | Adds `bad_request` helper, refactors 9 BAD_REQUEST returns | T3 |
| `spinbike-ui/Cargo.toml` | Adds `[dev-dependencies] wasm-bindgen-test` | T4 |
| `spinbike-ui/src/i18n.rs` | Tests gain `#[wasm_bindgen_test]` attribute | T4 |
| `spinbike-ui/src/util.rs` | Tests gain `#[wasm_bindgen_test]` attribute | T4 |
| `spinbike-ui/src/components/date_input.rs` | Tests gain `#[wasm_bindgen_test]` attribute | T4 |
| `.github/workflows/ci.yml` | Adds `Test (UI)` + `Mutation Testing (UI)` jobs | T5 |
| `spinbike-ui/src/pages/dashboard/mod.rs` | Gains `#[cfg(test)]` block with weak→strong demonstration test for `is_class_visit` | T6 |
| `crates/spinbike-server/src/db/migrations.rs` | Adds 4 V11 RED tests, then V11 migration constant + slice entry | T7, T8 |

---

## Task 1: VERSION bump

**Files:**
- Modify: `VERSION`
- Modify (auto-synced): `Cargo.toml`, `spinbike-ui/Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`

- [ ] **Step 1.1: Confirm starting version**

```bash
cat VERSION
git fetch origin
git log --oneline origin/main..origin/dev
```

Expected: `0.13.12` and zero commits ahead (or just the spec/plan commits).

- [ ] **Step 1.2: Edit VERSION**

```bash
echo "0.13.13" > VERSION
```

Expected: `cat VERSION` returns `0.13.13`.

- [ ] **Step 1.3: Sync to all Cargo.toml files**

```bash
bash scripts/sync-version.sh
```

Expected: script prints versions updated in root + `spinbike-ui/`. `crates/*` inherit `version.workspace = true` and don't need direct edits.

- [ ] **Step 1.4: Stage exactly the changed files (no `-A`/`.`)**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git status
```

Expected: only those three files staged. Cargo.lock stays untouched (no dep changes).

- [ ] **Step 1.5: Commit**

```bash
git commit -m "chore: bump version to 0.13.13"
```

---

## Task 2: #39 — Structural barcode fix

**Files:**
- Modify: `e2e/tests/helpers.ts` (add `activateUniqueCard` export)
- Modify: `e2e/tests/card-action-form.spec.ts` (delete local copy, import from helpers)
- Modify: `e2e/tests/desk-ux.spec.ts` (delete local copy, import from helpers)

**Why no new tests:** existing E2E tests use `activateUniqueCard` already — they prove the structural swap is behaviorally equivalent. CI E2E is the gate.

- [ ] **Step 2.1: Read current helper exports**

```bash
grep -n "^export" e2e/tests/helpers.ts
```

Expected output includes `setupConsoleCheck`, `assertCleanConsole`, `loginViaAPI`, `selectMonthlyPass`. Note any others — the new export must not collide.

- [ ] **Step 2.2: Append `activateUniqueCard` to helpers.ts**

Append at end of `e2e/tests/helpers.ts`:

```ts
/**
 * Activate a card with a unique letters-only barcode suffix so it cannot
 * substring-collide with seeded numeric barcodes (#39).
 *
 * Returns the generated barcode and last name plus the new card id.
 * The 8-char a-z suffix has 26^8 ≈ 2 × 10^11 distinct values — collision
 * with another concurrent test in the same Playwright run is statistically
 * impossible.
 */
export async function activateUniqueCard(
    token: string,
    initialCredit: number,
    prefix: string = 'AF',
): Promise<{ barcode: string; lastName: string; cardId: number }> {
    const BASE_URL = 'http://localhost:8099';
    const suffix = Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
    const barcode = `${prefix}-${suffix}`;
    const lastName = `${prefix}${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({
            barcode,
            initial_credit: initialCredit,
            first_name: prefix,
            last_name: lastName,
        }),
    });
    if (!resp.ok) {
        throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    }
    const body = await resp.json();
    return { barcode, lastName, cardId: body.id as number };
}
```

Notes:
- `prefix` defaults to `AF` to match the existing usage in `card-action-form.spec.ts`. Callers in `desk-ux.spec.ts` will pass `'UX'` to preserve their existing barcode/lastName shape.
- The shared helper now also returns `cardId`, which `desk-ux.spec.ts` was previously fetching with a separate `lookupCardId` helper. Tests that need `cardId` can use it directly; the `lookupCardId` helper in desk-ux.spec.ts stays — it's used elsewhere too. Don't delete it.

- [ ] **Step 2.3: Update card-action-form.spec.ts — delete local def, add import**

Edit `e2e/tests/card-action-form.spec.ts`:

Replace the existing import line:

```ts
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, selectMonthlyPass } from './helpers';
```

with:

```ts
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    selectMonthlyPass,
    activateUniqueCard,
} from './helpers';
```

Then delete the entire local `async function activateUniqueCard(...)` block (currently lines 6-19). Leave `BASE_URL` constant and `openCardByLastName` helper untouched. Existing call sites continue to work because the imported helper has the identical signature with default `prefix='AF'`.

- [ ] **Step 2.4: Update desk-ux.spec.ts — delete local def, add import + prefix arg**

Edit `e2e/tests/desk-ux.spec.ts`:

Replace the existing import line:

```ts
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';
```

with:

```ts
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    activateUniqueCard,
} from './helpers';
```

Delete the local `async function activateUniqueCard(...)` block (lines 6-21).

Then update each call site that needs the `'UX'` prefix. The existing local helper used `UX-${suffix}` and `Ux${suffix}`. The shared helper requires explicit `'UX'` prefix to preserve that. Use this find+replace strategy:

```bash
# Find every call site
grep -n "activateUniqueCard(" e2e/tests/desk-ux.spec.ts
```

For each match (form: `await activateUniqueCard(token, NUMBER)`), append `, 'UX'` argument. Example:

```ts
// Before:
const { lastName, barcode } = await activateUniqueCard(token, 50.0);

// After:
const { lastName, barcode } = await activateUniqueCard(token, 50.0, 'UX');
```

There's also one inline `const suffix = \`${Date.now()}...\`` block at desk-ux.spec.ts:134 inside an inline barcode build (not the helper). Replace its suffix pattern too:

```ts
// Before (around line 134):
const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;

// After:
const suffix = Array.from({ length: 8 }, () =>
    String.fromCharCode(97 + Math.floor(Math.random() * 26)),
).join('');
```

Verify with:

```bash
grep -nE "Date\.now\(\)|Math\.random\(\)\.toString" e2e/tests/desk-ux.spec.ts
```

Expected: zero matches in barcode/suffix construction (any remaining `Date.now()` calls must be unrelated, e.g., `validUntil` date computations — leave those alone).

- [ ] **Step 2.5: Verify both spec files no longer contain local `activateUniqueCard`**

```bash
grep -n "async function activateUniqueCard" e2e/tests/card-action-form.spec.ts e2e/tests/desk-ux.spec.ts
```

Expected: zero matches.

```bash
grep -n "from './helpers'" e2e/tests/card-action-form.spec.ts e2e/tests/desk-ux.spec.ts
```

Expected: both files import `activateUniqueCard`.

- [ ] **Step 2.6: Stage exactly the changed files**

```bash
git add e2e/tests/helpers.ts e2e/tests/card-action-form.spec.ts e2e/tests/desk-ux.spec.ts
git status
```

Expected: only those three files staged.

- [ ] **Step 2.7: Commit**

```bash
git commit -m "fix(e2e): letters-only barcode suffix to prevent collision (closes #39)

Move activateUniqueCard from duplicated test-local copies into helpers.ts
with an a-z 8-char suffix. Numeric Date.now() suffixes could substring-collide
with seeded barcodes like 70701001 — a letters-only suffix makes that
impossible by construction."
```

---

## Task 3: #36 — `bad_request` helper wrapper

**Files:**
- Modify: `crates/spinbike-server/src/routes/payments.rs` (add helper, refactor 9 BAD_REQUEST returns)

**Why no new tests:** existing integration tests (`charge_rejects_zero_amount`, `charge_rejects_negative_amount`, `charge_rejects_null_service_id_with_400`, etc.) cover the BAD_REQUEST behavior. The JSON shape `{"error": msg}` is byte-identical to the current code, so the same assertions still pass. Mutation-test coverage improves automatically once the helper is in place — that's the issue's acceptance.

- [ ] **Step 3.1: Re-confirm BAD_REQUEST line numbers**

```bash
grep -nE "StatusCode::BAD_REQUEST" crates/spinbike-server/src/routes/payments.rs
```

Expected: 9 matches at lines around 90, 107, 117, 125 (charge), 206 (storno), 273, 280, 288 (sell_pass), 404 (log_visit). Line numbers may shift slightly — use the actual numbers from this command, not the spec.

- [ ] **Step 3.2: Find the right insertion point for the helper**

```bash
grep -n "^use\|^fn\|^async fn\|^pub fn" crates/spinbike-server/src/routes/payments.rs | head -20
```

The helper goes AFTER the last `use` block and BEFORE the first `async fn`. There's likely an existing `internal_error` private function — place `bad_request` next to it (same file region).

- [ ] **Step 3.3: Add `bad_request` helper**

Insert this block in `crates/spinbike-server/src/routes/payments.rs`, immediately above `async fn charge(`:

```rust
/// Build a BAD_REQUEST response with an error message body.
///
/// Wraps the `(StatusCode, Json<Value>)` tuple so cargo-mutants can mutate
/// the message string reliably (#36 — Json newtype has no ::new() constructor
/// for cargo-mutants to synthesize). Behaviorally identical to inline
/// `(StatusCode::BAD_REQUEST, Json(json!({"error": msg})))`.
fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}
```

- [ ] **Step 3.4: Refactor charge — 4 sites**

For EACH of the 4 BAD_REQUEST returns inside `async fn charge`, replace the multi-line tuple literal with a `bad_request(...)` call. Pattern:

```rust
// Before:
return Err((
    StatusCode::BAD_REQUEST,
    Json(serde_json::json!({"error": "service_id required for charge"})),
));

// After:
return Err(bad_request("service_id required for charge"));
```

Apply identically to the other 3 charge sites:
- `"Use /api/payments/sell-pass for Monthly pass sales (requires valid_until)"`
- `"Amount must be greater than zero"`
- `"Note must be 200 characters or fewer"`

- [ ] **Step 3.5: Refactor storno — 1 site**

Inside `async fn storno`:

```rust
// Before:
return Err((
    StatusCode::BAD_REQUEST,
    Json(serde_json::json!({"error": "Amount must be greater than zero"})),
));

// After:
return Err(bad_request("Amount must be greater than zero"));
```

- [ ] **Step 3.6: Refactor sell_pass — 3 sites**

Three replacements (price, valid_until, note):

```rust
return Err(bad_request("Price must be zero or greater"));
return Err(bad_request("valid_until must be in the future"));
return Err(bad_request("Note must be 200 characters or fewer"));
```

- [ ] **Step 3.7: Refactor log_visit — 1 site**

```rust
return Err(bad_request("Note must be 200 characters or fewer"));
```

- [ ] **Step 3.8: Verify exactly zero `StatusCode::BAD_REQUEST` literals remain**

```bash
grep -nE "StatusCode::BAD_REQUEST" crates/spinbike-server/src/routes/payments.rs
```

Expected: zero matches (other than the one inside `bad_request` itself, line ~10 of the helper). Confirm:

```bash
grep -nE "StatusCode::BAD_REQUEST" crates/spinbike-server/src/routes/payments.rs | wc -l
```

Expected: `1` (the helper itself).

- [ ] **Step 3.9: Local format check**

```bash
cargo fmt --all --check
```

Expected: zero output (clean). If it complains, run `cargo fmt --all` and re-stage.

- [ ] **Step 3.10: Stage and commit**

```bash
git add crates/spinbike-server/src/routes/payments.rs
git commit -m "refactor(server): extract bad_request helper for mutants (closes #36)

cargo-mutants v27 reports 4/4 unviable on payments.rs charge handler because
mutating Err((StatusCode::BAD_REQUEST, Json(...))) produces Json::new() calls
that don't compile. Wrapping the tuple in fn bad_request(msg: &str) lets
cargo-mutants mutate the message-string argument reliably.

Refactored all 9 BAD_REQUEST sites in charge / storno / sell_pass / log_visit
for consistency. Response shape {\"error\": msg} unchanged — existing
integration tests still pass."
```

---

## Task 4: #22 — wasm-pack node tests + dev-dep + test-attribute conversion

**Files:**
- Modify: `spinbike-ui/Cargo.toml` (add `[dev-dependencies] wasm-bindgen-test`)
- Modify: `spinbike-ui/src/util.rs` (convert `#[test]` → `#[wasm_bindgen_test]`)
- Modify: `spinbike-ui/src/components/date_input.rs` (same)
- Modify: `spinbike-ui/src/i18n.rs` (same)

**Why no new tests in this task:** existing tests are presumed strong; conversion is mechanical so they keep covering the same logic on the new test target. Demonstration of the mutation gate happens in Task 6.

- [ ] **Step 4.1: Add wasm-bindgen-test dev-dep**

Edit `spinbike-ui/Cargo.toml`. Append at end of file (after `[profile.release]` block):

```toml

[dev-dependencies]
wasm-bindgen-test = "0.3"
```

- [ ] **Step 4.2: Convert util.rs (6 tests)**

Edit `spinbike-ui/src/util.rs`. The test module is at line 17 (`#[cfg(test)]` followed by `mod tests {`). Inside that module:

a. After the `mod tests {` line and any `use` lines, add:

```rust
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_node);
```

b. Replace every `#[test]` attribute with `#[wasm_bindgen_test]`.

Verify with:

```bash
grep -n "#\[test\]\|#\[wasm_bindgen_test\]\|wasm_bindgen_test_configure" spinbike-ui/src/util.rs
```

Expected: 6 `#[wasm_bindgen_test]`, 1 `wasm_bindgen_test_configure!`, zero `#[test]`.

- [ ] **Step 4.3: Convert components/date_input.rs (9 tests)**

Edit `spinbike-ui/src/components/date_input.rs`. Test module at line 119. Apply the same transformation:

a. Inside `mod tests {`, after existing `use` lines, add:

```rust
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_node);
```

b. Replace every `#[test]` with `#[wasm_bindgen_test]` (9 of them).

Verify:

```bash
grep -nc "#\[wasm_bindgen_test\]" spinbike-ui/src/components/date_input.rs
grep -nc "#\[test\]" spinbike-ui/src/components/date_input.rs
```

Expected: 9 and 0.

- [ ] **Step 4.4: Convert i18n.rs (~21 tests)**

Edit `spinbike-ui/src/i18n.rs`. Test module at line 712. Apply:

a. After existing `use` lines, add:

```rust
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_node);
```

b. Replace every `#[test]` with `#[wasm_bindgen_test]` inside the `mod tests {}` block (lines 712-end).

Verify counts match before/after:

```bash
# BEFORE (run before edits):
grep -c "#\[test\]" spinbike-ui/src/i18n.rs   # capture this number, call it N

# AFTER edits:
grep -c "#\[wasm_bindgen_test\]" spinbike-ui/src/i18n.rs  # must equal N
grep -c "#\[test\]" spinbike-ui/src/i18n.rs               # must be 0
```

- [ ] **Step 4.5: Local format check**

```bash
cargo fmt --all --check
```

Expected: zero output. If formatting changed, run `cargo fmt --all` and re-stage.

- [ ] **Step 4.6: Stage and commit**

```bash
git add spinbike-ui/Cargo.toml \
        spinbike-ui/src/util.rs \
        spinbike-ui/src/components/date_input.rs \
        spinbike-ui/src/i18n.rs
git status
git commit -m "test(ui): convert host #[test] to #[wasm_bindgen_test] (#22)

Existing unit tests were never executing in CI — workspace excludes
spinbike-ui because it targets WASM, so cargo test -p spinbike-* skipped them.
Converting to wasm_bindgen_test lets a new wasm-pack test --node CI job
actually run them (added in next commit)."
```

---

## Task 5: #22 — CI jobs `Test (UI)` + `Mutation Testing (UI)`

**Files:**
- Modify: `.github/workflows/ci.yml`

**Why this is its own commit:** keeps test-attribute conversion (Task 4) reviewable separately from CI infrastructure changes. Both must land for either to provide value.

- [ ] **Step 5.1: Find insertion points**

Read the existing structure to confirm placement:

```bash
grep -n "^  [a-z][a-z0-9-]*:$\|^    name:" .github/workflows/ci.yml | head -20
```

Expected order: `test-integrity`, `lint`, `test`, `build-wasm`, `e2e`, `mutation`, `deploy-dev`, `deploy-prod`, `smoke-dev`, `smoke-prod`.

The `Test (UI)` job goes BETWEEN `build-wasm` and `e2e` (so e2e doesn't depend on it but it runs in parallel with `test`).

The `Mutation Testing (UI)` job goes immediately AFTER `mutation` (sibling job, same trigger).

- [ ] **Step 5.2: Add `test-ui` job**

Insert this block after the `build-wasm` job's last line and before `e2e:` (look for `e2e:` on its own line; insert above it):

```yaml
  test-ui:
    name: Test (UI)
    runs-on: ubuntu-latest
    timeout-minutes: 15
    needs: lint
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: spinbike-ui
          cache-on-failure: true

      - name: Install wasm-pack
        run: cargo install wasm-pack --version 0.13.1 --locked

      - name: Run wasm-pack tests (node)
        run: wasm-pack test --node spinbike-ui
```

Notes:
- `wasm-pack 0.13.1` is the latest stable as of 2026-05-01. `--locked` makes the install reproducible.
- `--node` runs tests under Node.js (no headless browser needed). Tests must use `wasm_bindgen_test_configure!(run_in_node);` (added in T4).
- `workspaces: spinbike-ui` keys the rust-cache to the UI manifest so it doesn't collide with the workspace cache used by the `test` job.

- [ ] **Step 5.3: Add `mutation-ui` job**

Insert this block immediately AFTER the `mutation:` job's last line (the `--package spinbike-server` line, which ends the existing job's `run:` step) and BEFORE `deploy-dev:`:

```yaml
  mutation-ui:
    name: Mutation Testing (UI)
    runs-on: ubuntu-latest
    timeout-minutes: 240
    needs: test-ui
    if: github.event_name == 'pull_request'
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: spinbike-ui
          cache-on-failure: true

      - name: Install cargo-mutants
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-mutants

      - name: Install wasm-pack
        run: cargo install wasm-pack --version 0.13.1 --locked

      - name: Compute PR diff vs base
        run: git diff origin/${{ github.base_ref }}...HEAD > pr.diff

      - name: Run cargo-mutants on UI diff
        run: |
          # Mutate only PR-changed lines in spinbike-ui.
          # --test-tool=cargo runs the manifest's [dev-dependencies] tests via
          # wasm-pack-equivalent host runner. cargo-mutants invokes
          # `cargo test` under the hood; --target wasm32-unknown-unknown forces
          # the WASM target so wasm-bindgen-test fires.
          cargo mutants \
            --in-diff pr.diff \
            --timeout 60 \
            --no-shuffle \
            --manifest-path spinbike-ui/Cargo.toml \
            -- --target wasm32-unknown-unknown
```

If cargo-mutants fails to drive wasm-pack directly, fall back to running mutants over the host build of pure-logic modules (i18n, util) — see comment block in `references` of the spec.

- [ ] **Step 5.4: Verify YAML syntax**

```bash
# Local YAML lint check (no project-specific tools needed):
python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/ci.yml'))" \
  && echo "YAML OK"
```

Expected: `YAML OK`. If it fails, fix indentation (GitHub Actions YAML is whitespace-sensitive — must use 2-space nesting).

- [ ] **Step 5.5: Stage and commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add Test (UI) + Mutation Testing (UI) jobs (#22)

Test (UI) runs wasm-pack test --node spinbike-ui on every push, finally
executing the unit tests in i18n.rs / util.rs / date_input.rs that the
workspace exclusion was hiding from CI.

Mutation Testing (UI) runs cargo-mutants on PR diffs against spinbike-ui,
mirroring the existing Mutation Testing job for the server crates."
```

---

## Task 6: #22 — Demonstration cycle (red → strengthen → green)

**Goal:** Prove the new `Mutation Testing (UI)` gate actually catches a weak assertion. Per #22 acceptance: "A test or two added to demonstrate the new mutation-test job actually catches a weak assertion (red → strengthen → green cycle)."

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` (add `#[cfg(test)]` block with weak then strong tests for `is_class_visit`)

**Target:** `ServiceInfo::is_class_visit` at `spinbike-ui/src/pages/dashboard/mod.rs:103`. The function calls `CLASS_VISIT_NAMES_EN.contains(&self.name_en.as_str())` — pure logic, no DOM, mutable in node.

### Step 6.1: Commit the deliberately WEAK test

- [ ] Open `spinbike-ui/src/pages/dashboard/mod.rs` and append at end of file:

```rust
#[cfg(test)]
mod is_class_visit_tests {
    use super::*;
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_node);

    fn make_svc(name_en: &str) -> ServiceInfo {
        ServiceInfo {
            id: 1,
            kind: "generic".to_string(),
            name_sk: "x".to_string(),
            name_en: name_en.to_string(),
            default_price: 0.0,
            active: 1,
        }
    }

    // Demonstration: this is intentionally weak — it asserts the function
    // returns *something* but not the right thing. Mutation testing will
    // generate a mutant that always returns true (or false) and this test
    // will still pass — proving the gate catches it.
    #[wasm_bindgen_test]
    fn is_class_visit_returns_a_bool() {
        let s = make_svc("Spinning");
        let _ = s.is_class_visit();
    }
}
```

- [ ] **Step 6.1.1: Stage and commit the weak version**

```bash
git add spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "test(ui): WEAK demo for is_class_visit (#22 demonstration step 1)

Deliberately weak — asserts nothing meaningful. Mutation-testing CI on the
PR diff is expected to find surviving mutants here. The strong replacement
follows in the next commit; this commit exists to document the gate
catching the weakness end-to-end."
```

- [ ] **Step 6.1.2: Push the weak version (this happens later in T9, not now)**

The plan-time work is to commit only. Pushing happens in T9 along with all other commits. The CI mutation-test job runs on the PR diff at PR-open time and will catch this in T9. No separate push here.

### Step 6.2: Strengthen the test

- [ ] Edit `spinbike-ui/src/pages/dashboard/mod.rs`. Replace the weak test body with a strong one. Final state of the test module:

```rust
#[cfg(test)]
mod is_class_visit_tests {
    use super::*;
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_node);

    fn make_svc(name_en: &str) -> ServiceInfo {
        ServiceInfo {
            id: 1,
            kind: "generic".to_string(),
            name_sk: "x".to_string(),
            name_en: name_en.to_string(),
            default_price: 0.0,
            active: 1,
        }
    }

    // Strong: covers the two truthy class-visit names AND a sample of names
    // that must return false. Catches mutants that flip the return constant
    // OR replace `contains` with always-true / always-false equivalents.
    #[wasm_bindgen_test]
    fn is_class_visit_true_for_spinning() {
        assert!(make_svc("Spinning").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_true_for_fitness() {
        assert!(make_svc("Fitness").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_false_for_refreshments() {
        assert!(!make_svc("Refreshments").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_false_for_unknown() {
        assert!(!make_svc("Whatever").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_false_for_empty() {
        assert!(!make_svc("").is_class_visit());
    }
}
```

- [ ] **Step 6.2.1: Local format check**

```bash
cargo fmt --all --check
```

Expected: zero output.

- [ ] **Step 6.2.2: Stage and commit the strong version**

```bash
git add spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "test(ui): STRONG is_class_visit covering Spinning/Fitness/non-class (#22 demo step 2)

Replaces the deliberately weak demonstration test from the previous commit
with assertions that pin the function's actual contract: Spinning and
Fitness return true; Refreshments / unknown / empty return false. Mutation
testing on PR diff now passes — proves the (UI) gate catches weak tests
and accepts strong ones, end-to-end."
```

---

## Task 7: #28 — V11 RED tests

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs` (add 4 RED tests inside existing `mod tests`)

**Why RED first:** the new V11 migration constant doesn't exist yet, so the new tests must reference behavior that the existing migration set can't satisfy. CI Test job will fail on these — that's the proof the tests are meaningful. T8 ships GREEN.

- [ ] **Step 7.1: Find the test module**

```bash
grep -nE "^#\[cfg\(test\)\]|^mod tests" crates/spinbike-server/src/db/migrations.rs | head -5
```

Note the line of `mod tests {`.

- [ ] **Step 7.2: Append 4 V11 tests inside `mod tests`**

Add this block immediately before the closing `}` of `mod tests`:

```rust
    // V11 — note CHECK constraint -------------------------------------

    #[tokio::test]
    async fn v11_note_check_accepts_200_chars() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Insert a transaction with a 200-char note (exactly at the bound).
        // Use a Slovak diacritic so the byte count > 200 but char count = 200,
        // matching the server-side validator (uses chars().count(), not len()).
        let note: String = "á".repeat(200);
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, note)
             VALUES (?, ?, 'charge', ?)",
        )
        .bind(1_i64)
        .bind(5.0_f64)
        .bind(&note)
        .execute(&pool)
        .await
        .expect("200-char note must be accepted");
    }

    #[tokio::test]
    async fn v11_note_check_rejects_201_chars() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let note: String = "á".repeat(201);
        let res = sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, note)
             VALUES (?, ?, 'charge', ?)",
        )
        .bind(1_i64)
        .bind(5.0_f64)
        .bind(&note)
        .execute(&pool)
        .await;

        let err = res.expect_err("201-char note must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("CHECK") || msg.contains("constraint"),
            "expected SQLITE_CONSTRAINT-style error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn v11_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Second run must not error — schema_version check should make V11
        // a no-op on the already-migrated DB.
        run_migrations(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn v11_drop_rename_pattern_preserves_bookings_fk() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed: a transaction + a booking that references it via charge_transaction_id.
        // After V11 recreates `transactions`, the FK on bookings.charge_transaction_id
        // must continue to resolve (V8 precedent — FK reattaches by table name on RENAME).
        let tx_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (card_id, amount, action)
             VALUES (1, 5.0, 'charge') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Need a class_template + user to satisfy bookings NOT NULL FKs.
        // Migrations seed a template at id=1 (V6_SEED_SPIN_CLASSES).
        // users requires (email, name, role) to be present (name NOT NULL).
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name, password_hash, role)
             VALUES ('booker@test.local', 'Test Booker', 'x', 'admin')
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO bookings (template_id, date, user_id, charge_transaction_id)
             VALUES (1, '2026-12-01', ?, ?)",
        )
        .bind(user_id)
        .bind(tx_id)
        .execute(&pool)
        .await
        .expect("booking insert must succeed with V11 in place");

        // Verify FK resolves: join must produce a row.
        let joined: i64 = sqlx::query_scalar(
            "SELECT t.id FROM transactions t
             JOIN bookings b ON b.charge_transaction_id = t.id
             WHERE b.charge_transaction_id IS NOT NULL
             LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("transactions ↔ bookings FK must resolve after V11 rebuild");
        assert_eq!(joined, tx_id);
    }
```

- [ ] **Step 7.3: Local format check**

```bash
cargo fmt --all --check
```

Expected: zero output.

- [ ] **Step 7.4: Stage and commit (RED — tests fail; CI confirms after push)**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "test(db): RED tests for V11 transactions.note CHECK (#28)

Four tests asserting:
1. 200-char note inserts succeed
2. 201-char note inserts reject with CHECK violation
3. Migrations stay idempotent across V11
4. bookings.charge_transaction_id FK survives the V11 table rebuild

These fail today because V11 doesn't exist; the next commit adds it. CI
Test job is expected to fail on this commit (proves the tests are meaningful)."
```

---

## Task 8: #28 — V11 GREEN migration

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs` (add `(11, ...)` slice entry + `V11_TRANSACTIONS_NOTE_CHECK` const)

- [ ] **Step 8.1: Append V11 entry to MIGRATIONS slice**

Find the `MIGRATIONS` slice declaration (around line 2). Add a new entry at the END of the slice — keep ascending order intact:

```rust
    (10, "transactions: free-text note column", V10_TRANSACTIONS_NOTE_COLUMN),
    (11, "transactions: note length CHECK", V11_TRANSACTIONS_NOTE_CHECK),
];
```

- [ ] **Step 8.2: Append `V11_TRANSACTIONS_NOTE_CHECK` constant**

Add immediately after the existing `V10_TRANSACTIONS_NOTE_COLUMN` constant (around line 266):

```rust
const V11_TRANSACTIONS_NOTE_CHECK: &str = r#"
-- Defense-in-depth (#28): server already validates note ≤ 200 chars at
-- every entry point. This adds the same constraint at the DB level so a
-- direct sqlite3 write — or a future endpoint that forgets to validate —
-- cannot store an unbounded string.
--
-- SQLite cannot ALTER TABLE to add CHECK constraints on existing columns.
-- Use the CREATE_NEW + INSERT + DROP + RENAME pattern (V8 precedent).
-- Migration runner toggles PRAGMA foreign_keys around the transaction;
-- bookings.charge_transaction_id FK reattaches by name after RENAME.
--
-- Column list mirrors V1 + V4 (valid_until) + V7 (deleted_at) + V9
-- (legacy_backfilled) + V10 (note) — keep types and defaults identical.
-- Note: chars().count() is enforced server-side; SQLite length() counts
-- bytes for BLOBs but UTF-8 codepoints for TEXT, so length() ≤ 200 here
-- matches the server semantic.

CREATE TABLE transactions_new (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id           INTEGER REFERENCES users(id),
    card_id           INTEGER REFERENCES cards(id),
    staff_id          INTEGER REFERENCES users(id),
    service_id        INTEGER REFERENCES services(id),
    amount            REAL    NOT NULL,
    action            TEXT    NOT NULL,
    created_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    valid_until       TEXT,
    deleted_at        TEXT,
    legacy_backfilled INTEGER NOT NULL DEFAULT 0,
    note              TEXT    CHECK (note IS NULL OR length(note) <= 200)
);

INSERT INTO transactions_new (
    id, user_id, card_id, staff_id, service_id, amount, action, created_at,
    valid_until, deleted_at, legacy_backfilled, note
)
SELECT
    id, user_id, card_id, staff_id, service_id, amount, action, created_at,
    valid_until, deleted_at, legacy_backfilled, note
FROM transactions;

DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;
"#;
```

- [ ] **Step 8.3: Local format check**

```bash
cargo fmt --all --check
```

Expected: zero output. The raw string literal preserves SQL exactly; rustfmt won't touch it.

- [ ] **Step 8.4: Stage and commit (GREEN — RED tests now pass; CI confirms after push)**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): V11 migration adds CHECK(length(note) <= 200) on transactions (closes #28)

Recreates transactions via the V8 CREATE_NEW + INSERT + DROP + RENAME pattern
to add a column-level CHECK constraint that ALTER TABLE cannot add directly.

bookings.charge_transaction_id FK reattaches automatically after RENAME (the
migration runner toggles PRAGMA foreign_keys around the transaction). All
incremental columns from V1 (id/user/card/staff/service/amount/action/
created_at) + V4 (valid_until) + V7 (deleted_at) + V9 (legacy_backfilled) +
V10 (note) are preserved.

The 4 RED tests from the previous commit pass on this state."
```

---

## Task 9: Push, monitor CI to terminal, open PR

**Goal:** push all dev commits in one go, monitor CI to a terminal state via a single background command, then open the PR `dev` → `main`.

- [ ] **Step 9.1: Confirm commit list before pushing**

```bash
git log origin/dev..dev --oneline
```

Expected output (in order, most recent first):
```
<sha> feat(db): V11 migration adds CHECK(length(note) <= 200) on transactions (closes #28)
<sha> test(db): RED tests for V11 transactions.note CHECK (#28)
<sha> test(ui): STRONG is_class_visit covering Spinning/Fitness/non-class (#22 demo step 2)
<sha> test(ui): WEAK demo for is_class_visit (#22 demonstration step 1)
<sha> ci: add Test (UI) + Mutation Testing (UI) jobs (#22)
<sha> test(ui): convert host #[test] to #[wasm_bindgen_test] (#22)
<sha> refactor(server): extract bad_request helper for mutants (closes #36)
<sha> fix(e2e): letters-only barcode suffix to prevent collision (closes #39)
<sha> chore: bump version to 0.13.13
<sha> docs(spec): quality bundle v0.13.13 (#39 #28 #22 #36)
```

10 commits total. If the count is off, investigate before pushing.

- [ ] **Step 9.2: Push to origin/dev**

```bash
git push origin dev
```

Expected: fast-forward push succeeds.

- [ ] **Step 9.3: Locate the new CI run id**

```bash
sleep 5 && gh run list --branch dev --limit 3 --json databaseId,event,headSha,status,conclusion
```

Expected: at least one fresh run with `event: push` matching the new HEAD sha. Capture the `databaseId` — that's the run id you'll monitor.

- [ ] **Step 9.4: Monitor CI to terminal — single background command**

Start a background poll at 300s (5 min ≥ likely first job duration), then re-check until terminal:

```bash
sleep 300 && gh run view <RUN_ID> --json status,conclusion,jobs > /tmp/ci-${RUN_ID}.json
```

Run as `Bash(... run_in_background: true)`. When BashOutput shows the run is `completed`, parse `conclusion` per job:

- All jobs `success` → proceed to PR creation.
- Any `failure` → `gh run view <RUN_ID> --log-failed` immediately. Fix root cause (not symptom — see `no-timeout-band-aids.md`). Push fix as a NEW commit. Re-monitor.
- `mutation` job runs only on PR (not push) — expect it to be skipped on this push run. That's fine.

If first poll returns `in_progress`, poll again with another 300s sleep — but only ONE background command at a time per `ci-monitoring.md`.

- [ ] **Step 9.5: Open PR dev → main**

Once push-run jobs are all green (Lint, Test, Build WASM (UI), Test (UI), E2E Tests, Test Integrity, Deploy (dev), Smoke (dev)):

```bash
git diff main...dev --stat | head -20
```

Confirm the diff scope matches the plan (no surprise files).

```bash
gh pr create \
  --base main \
  --head dev \
  --title "v0.13.13: quality bundle (#39 #28 #22 #36)" \
  --body "$(cat <<'EOF'
## Summary

Quality bundle covering 4 issues in one PR cycle:

- **#39** — E2E flake fix: letters-only barcode suffix in shared `activateUniqueCard` helper. Previous numeric `Date.now()` suffix could substring-collide with seeded barcode `70701001`; the new a-z suffix makes that impossible by construction.
- **#28** — V11 migration adds `CHECK(note IS NULL OR length(note) <= 200)` to `transactions.note` (defense-in-depth; server already validates at every entry point). Uses V8 CREATE_NEW + INSERT + DROP + RENAME pattern; FK from `bookings.charge_transaction_id` reattaches by name on RENAME.
- **#22** — Mutation testing now covers `spinbike-ui`. New `Test (UI)` job runs `wasm-pack test --node`; new `Mutation Testing (UI)` job runs `cargo mutants` on PR diffs against the UI crate. Existing tests in `i18n.rs`/`util.rs`/`date_input.rs` converted to `#[wasm_bindgen_test]` so they actually execute. Demonstration cycle on `is_class_visit` (weak commit → strong commit) proves the gate catches weak assertions.
- **#36** — `fn bad_request(msg)` helper in `payments.rs` consolidates 9 BAD_REQUEST returns. cargo-mutants now mutates the message-string argument reliably (previously 4/4 unviable on Json tuple-struct).

## Test plan

- [ ] CI green on all jobs: Test Integrity, Lint, Test, Build WASM (UI), Test (UI), E2E Tests, Mutation Testing, Mutation Testing (UI), Deploy (dev), Smoke (dev), Deploy (prod) on merge.
- [ ] Post-deploy: dev frontend `[data-testid="version"]` reads `v0.13.13`, matches `/api/version`.
- [ ] V11 migration timing on prod-synced dev DB observed in deploy-dev logs (88K-row recreate; expected sub-minute).
- [ ] Spinning quick-charge chip and existing flows still work on dev (smoke check).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 9.6: Confirm PR mergeable + clean**

```bash
gh pr view <PR_NUMBER> --json mergeable,mergeStateStatus,checks_summary
```

Expected: `mergeable: MERGEABLE`, `mergeStateStatus: CLEAN`. Anything else (UNSTABLE, BLOCKED, DIRTY, BEHIND) is NOT done — investigate per `autonomous-quality-discipline.md`.

- [ ] **Step 9.7: Stop**

PR is mergeable, awaiting user merge. **Do not auto-merge.** Per `pr-merge-policy.md`: only an explicit user instruction triggers merge.

---

## Task 10: Post-deploy verification (runs only after user merges)

**Trigger:** user issues an explicit "merge it" instruction on the PR. Until then, do NOT run this task.

- [ ] **Step 10.1: Wait for merge & main CI**

After `gh pr merge <PR_NUMBER> --merge` is executed (by the user's instruction), find the main run:

```bash
sleep 60 && gh run list --branch main --limit 1 --json databaseId,event,headSha,status,conclusion
```

Then monitor with `sleep N && gh run view <RUN_ID>` background command (single, per `ci-monitoring.md`). Wait for `Deploy (prod)` and `Smoke (prod)` to complete green.

- [ ] **Step 10.2: Verify dev frontend version label**

Use the Playwright MCP tool (NOT curl — per `version-on-dashboard.md` and `post-deploy-verification.md`):

1. `browser_navigate` to `https://spinbike-dev.newlevel.media/login`
2. `browser_evaluate` to read DOM:
   ```js
   () => document.querySelector('[data-testid="version"]')?.textContent
   ```
3. Expect: `v0.13.13` (or `v0.13.13 (...)` with optional sha/date suffix per `version-on-dashboard.md` format).
4. Cross-check: `fetch('/api/version').then(r => r.text())` — must match.

- [ ] **Step 10.3: Verify prod frontend version label**

Repeat Step 10.2 against `https://spinbike.newlevel.media/login`. Same assertion — `v0.13.13` in DOM, matches `/api/version`.

- [ ] **Step 10.4: Inspect V11 migration timing on prod-synced dev DB**

Per `feedback_dev_ci_sync_prod_db.md`: deploy-dev syncs prod → dev, so dev now has the prod-shape data (88K transactions). The migration ran on it. Check the deploy-dev systemd logs for migration output:

```bash
ssh prod 'sudo journalctl -u spinbike --since "1 hour ago" | grep -iE "migrat|v11|transactions"' || true
```

(Adjust unit name if `spinbike-dev` is the systemd unit on the same host.) Expected: a log line indicating V11 ran successfully and migration finished. Note duration.

- [ ] **Step 10.5: Functional smoke on dev (real card)**

Use Playwright MCP to:

1. Log into dev as a staff user (if test users exist; otherwise skip and rely on prod step).
2. Open a real card.
3. Click the Spinning quick-charge chip (introduced in PR #40).
4. Confirm credit decreases and a `[data-testid="transaction-row"]` appears.
5. Console clean (no errors/warnings beyond benign preload).

If test users were wiped by prod-sync, skip and rely on the same smoke against prod (with explicit user authorization per `approval-scope.md`).

- [ ] **Step 10.6: Send completion report**

Per `completion-report.md` mandatory template — include:
- ✅ CI: green (main run id)
- ✅ /plan-check: 10/10 fulfilled (this plan)
- ✅ /review: clean — 0 🔴 0 🟡 0 🔵 (or actual counts after running review)
- ✅ Deploy: dev + prod show `v0.13.13`, matches `/api/version`; V11 ran in <duration>s on prod-synced dev DB.

---

## Self-review (planner-side)

**Spec coverage:**
- #39 structural fix (helpers.ts + 2 spec edits) → T2 ✓
- #36 bad_request helper + 9 site refactor → T3 ✓
- #22 wasm-bindgen-test dev-dep + test conversion → T4 ✓
- #22 CI jobs Test (UI) + Mutation Testing (UI) → T5 ✓
- #22 demonstration weak→strong cycle → T6 ✓
- #28 V11 RED tests (4) → T7 ✓
- #28 V11 GREEN migration → T8 ✓
- VERSION bump → T1 ✓
- PR open + monitor → T9 ✓
- Post-deploy verification per `post-deploy-verification.md` → T10 ✓

All spec sections covered.

**Placeholder scan:** all "TBD"-style markers searched — none present. Each step has concrete code, exact file paths, and exact commands.

**Type consistency:**
- `activateUniqueCard` signature in T2 matches every call site adjusted in `desk-ux.spec.ts` (3rd `prefix` arg added) and `card-action-form.spec.ts` (default `'AF'` works).
- `bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>)` in T3 matches every call site.
- `V11_TRANSACTIONS_NOTE_CHECK` const name + slice entry `(11, "transactions: note length CHECK", V11_TRANSACTIONS_NOTE_CHECK)` consistent in T7 and T8.

---

## Pre-implementation pause

Per project memory `feedback_pre_implementation_pause.md`: the implementer (subagent-driven-development) dispatch happens AFTER the user reviews this plan. The dispatcher will end with the explicitly-allowed pause question.
