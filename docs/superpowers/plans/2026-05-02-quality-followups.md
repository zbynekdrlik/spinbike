# Quality Follow-ups (#43 + #42) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship one PR `dev → main` (v0.13.14) closing #43 (UI mutation gate plumbing) and #42 (maximal `bad_request` consolidation across all route files).

**Architecture:** No new modules. Extends `routes/mod.rs` with a shared `pub fn bad_request` (alongside existing `pub fn internal_error`); deletes two private duplicates and rewrites 13 inline `BAD_REQUEST` returns in 5 other route files; adds `.cargo/config.toml` runner config + a sanity-check step to the CI mutation-ui job.

**Tech Stack:** Rust (axum 0.8 / sqlx / tokio) for server changes. GitHub Actions YAML for CI. `wasm-bindgen-test-runner` (provided by the `wasm-bindgen-cli` crate) for the wasm32 test runner.

---

## File map

| File | Change |
|------|--------|
| `VERSION` | 0.13.13 → 0.13.14 (Task 1) |
| `Cargo.toml`, `crates/*/Cargo.toml`, `spinbike-ui/Cargo.toml` | Synced from VERSION via `scripts/sync-version.sh` (Task 1) |
| `crates/spinbike-server/src/routes/mod.rs` | Add `pub fn bad_request` (Task 2) |
| `crates/spinbike-server/src/routes/payments.rs` | Delete local helper, rebind 9 callsites to `super::bad_request` (Task 3) |
| `crates/spinbike-server/src/routes/reports.rs` | Delete local helper, rebind 2 callsites to `super::bad_request` (Task 3) |
| `crates/spinbike-server/src/routes/admin.rs` | Rewrite 3 inline sites (Task 4) |
| `crates/spinbike-server/src/routes/classes.rs` | Rewrite 3 inline sites (Task 4) |
| `crates/spinbike-server/src/routes/auth.rs` | Rewrite 3 inline sites (Task 4) |
| `crates/spinbike-server/src/routes/cards.rs` | Rewrite 2 inline sites (Task 5) |
| `crates/spinbike-server/src/routes/transactions.rs` | Rewrite 2 inline sites (Task 5) |
| `.cargo/config.toml` (NEW) | wasm32 runner config (Task 6) |
| `.github/workflows/ci.yml` | Install wasm-bindgen-cli (Task 6); add sanity-check step + replace apologetic comment (Task 7) |

No new test files. The mutation gate tells us which existing route tests need strengthening; that's executed inline in Task 8 if it surfaces.

---

## Task 1: Bump version to 0.13.14

**Why first:** CI runs a version-bump check on PRs that fails if dev version ≤ main version. PR #41 was just merged so dev and main are both at 0.13.13. The first commit on dev MUST bump.

**Files:**
- Modify: `VERSION`
- Modify (auto-synced by script): `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Update VERSION**

```bash
# From repo root
echo "0.13.14" > VERSION
```

- [ ] **Step 2: Sync to all Cargo.toml files**

```bash
bash scripts/sync-version.sh
```

Expected output: confirmation that root Cargo.toml, spinbike-core, spinbike-server, and spinbike-ui all match VERSION.

- [ ] **Step 3: Verify**

```bash
cat VERSION
grep -h "^version" Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
```

Expected: `0.13.14` in VERSION; `version = "0.13.14"` in each Cargo.toml.

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump version to 0.13.14"
```

(Do NOT use `git add -A` or `git add .` per project memory `feedback_no_git_add_A.md`.)

---

## Task 2: Add shared `bad_request` helper to `routes/mod.rs`

**Why now:** All later tasks call `super::bad_request(...)`. Adding the helper first makes Task 3's deletion of local copies a clean swap.

**Files:**
- Modify: `crates/spinbike-server/src/routes/mod.rs:14-26` (insert new helper after existing `internal_error`)

- [ ] **Step 1: Add helper to mod.rs**

Open `crates/spinbike-server/src/routes/mod.rs`. After the `internal_error` function definition (currently ends at line 26), add:

```rust
/// Build a BAD_REQUEST response with an error message body.
///
/// Wraps the `(StatusCode, Json<Value>)` tuple so cargo-mutants can mutate
/// the message string reliably (#36 — `axum::Json` newtype has no `::new()`
/// constructor for cargo-mutants to synthesize). Behaviorally identical to
/// inline `(StatusCode::BAD_REQUEST, Json(json!({"error": msg})))`.
pub fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": msg})),
    )
}
```

The `use axum::{Json, Router, http::StatusCode};` at line 14 already brings `StatusCode` and `Json` into scope, so no new imports.

- [ ] **Step 2: Local format check**

```bash
cargo fmt --all --check
```

Expected: no diff. If it complains about style, run `cargo fmt --all` and re-check.

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/routes/mod.rs
git commit -m "refactor(routes): add shared bad_request helper

Mirrors the existing internal_error helper. Subsequent commits
delete the private copies in payments.rs/reports.rs and rewrite
13 inline BAD_REQUEST sites in 5 other route files to use this
helper. Closes #42."
```

(No tests in this commit — the helper is dead until callers are switched. Existing route tests will exercise it from Task 3 onward.)

---

## Task 3: Retarget `payments.rs` and `reports.rs` to the shared helper

**Files:**
- Modify: `crates/spinbike-server/src/routes/payments.rs:70-81` (delete local helper) and 9 callsites (lines 102, 116, 123, 128, 206, 270, 274, 279, 392 — verify with grep first)
- Modify: `crates/spinbike-server/src/routes/reports.rs:125-130` (delete local helper) and 2 callsites (lines 79, 83 — verify with grep first)

- [ ] **Step 1: Verify line numbers**

```bash
grep -n "fn bad_request\|bad_request(" crates/spinbike-server/src/routes/payments.rs crates/spinbike-server/src/routes/reports.rs
```

Expected: shows the helper definitions and 9 + 2 callsites. If line numbers shifted, use the actual numbers from grep output below.

- [ ] **Step 2: Delete local `fn bad_request` from payments.rs**

In `crates/spinbike-server/src/routes/payments.rs`, find the helper block (currently at lines 70-81, including doc comment) and delete it entirely:

```rust
/// Build a BAD_REQUEST response with an error message body.
///
/// Wraps the `(StatusCode, Json<Value>)` tuple so cargo-mutants can mutate
/// the message string reliably (#36 — `axum::Json` newtype has no `::new()`
/// constructor for cargo-mutants to synthesize). Behaviorally identical to
/// inline `(StatusCode::BAD_REQUEST, Json(json!({"error": msg})))`.
fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}
```

This removes lines ~70-81 (the doc comment + body).

- [ ] **Step 3: Replace `bad_request(` with `super::bad_request(` in payments.rs callsites**

Each callsite currently looks like:

```rust
return Err(bad_request("service_id required for charge"));
return Err(bad_request("Amount must be greater than zero"));
// etc.
```

Replace each `bad_request(` (call expression, NOT the deleted definition) with `super::bad_request(` so they resolve to the parent module's helper.

There are 9 callsites in payments.rs (lines ~102, ~116, ~123, ~128, ~206, ~270, ~274, ~279, ~392). Use Edit with `replace_all` for the simplest mechanical swap, OR a sed pass:

```bash
sed -i 's/Err(bad_request(/Err(super::bad_request(/g' crates/spinbike-server/src/routes/payments.rs
```

Verify no spurious changes:

```bash
grep -n "bad_request" crates/spinbike-server/src/routes/payments.rs
```

Expected: 9 lines, all of the form `return Err(super::bad_request("..."));` or the multi-line variant `return Err(super::bad_request(`. No standalone `bad_request(` (that was the deleted definition).

- [ ] **Step 4: Delete local `fn bad_request` from reports.rs**

In `crates/spinbike-server/src/routes/reports.rs`, find and delete the helper block at lines ~125-130:

```rust
fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": msg})),
    )
}
```

(No doc comment in this copy — just the function.)

- [ ] **Step 5: Replace `bad_request(` with `super::bad_request(` in reports.rs callsites**

```bash
sed -i 's/Err(bad_request(/Err(super::bad_request(/g' crates/spinbike-server/src/routes/reports.rs
```

Verify:

```bash
grep -n "bad_request" crates/spinbike-server/src/routes/reports.rs
```

Expected: 2 lines, both `return Err(super::bad_request("..."));`. No standalone `fn bad_request` or `bad_request(` calls.

- [ ] **Step 6: Local format check**

```bash
cargo fmt --all --check
```

Expected: no diff.

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-server/src/routes/payments.rs crates/spinbike-server/src/routes/reports.rs
git commit -m "refactor(routes): use shared bad_request in payments + reports

Deletes the two private duplicates and rebinds 9 + 2 callsites
to super::bad_request. Behaviorally identical."
```

---

## Task 4: Rewrite inline `BAD_REQUEST` sites in `admin.rs`, `classes.rs`, `auth.rs` (9 sites total)

**Files:**
- Modify: `crates/spinbike-server/src/routes/admin.rs` lines 484-489, 491-496, 670-675
- Modify: `crates/spinbike-server/src/routes/classes.rs` lines 79-84, 85-90, 227-232
- Modify: `crates/spinbike-server/src/routes/auth.rs` lines 55-59, 62-66, 69-73

- [ ] **Step 1: Re-verify exact line numbers**

```bash
grep -n "BAD_REQUEST" crates/spinbike-server/src/routes/admin.rs crates/spinbike-server/src/routes/classes.rs crates/spinbike-server/src/routes/auth.rs
```

Expected to roughly match plan lines; use actual output if offsets shifted.

- [ ] **Step 2: Rewrite admin.rs site 1 (lines ~484-489)**

Find:

```rust
    if body.name_sk.trim().is_empty() || body.name_en.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name_sk and name_en are required"})),
        ));
    }
```

Replace with:

```rust
    if body.name_sk.trim().is_empty() || body.name_en.trim().is_empty() {
        return Err(super::bad_request("name_sk and name_en are required"));
    }
```

- [ ] **Step 3: Rewrite admin.rs site 2 (lines ~491-496)**

Find:

```rust
    let kind = body.kind.as_deref().unwrap_or("generic");
    if !matches!(kind, "generic" | "monthly_pass") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "kind must be 'generic' or 'monthly_pass'"})),
        ));
    }
```

Replace with:

```rust
    let kind = body.kind.as_deref().unwrap_or("generic");
    if !matches!(kind, "generic" | "monthly_pass") {
        return Err(super::bad_request(
            "kind must be 'generic' or 'monthly_pass'",
        ));
    }
```

- [ ] **Step 4: Rewrite admin.rs site 3 (lines ~670-675)**

Find:

```rust
    // I6: Validate role string before writing to DB.
    if !["admin", "staff", "customer"].contains(&body.role.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid role. Must be admin, staff, or customer"})),
        ));
    }
```

Replace with:

```rust
    // I6: Validate role string before writing to DB.
    if !["admin", "staff", "customer"].contains(&body.role.as_str()) {
        return Err(super::bad_request(
            "Invalid role. Must be admin, staff, or customer",
        ));
    }
```

- [ ] **Step 5: Rewrite classes.rs site 1 (lines ~79-84)**

Find:

```rust
    let from = NaiveDate::parse_from_str(&query.from, "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid 'from' date format, expected YYYY-MM-DD"})),
        )
    })?;
```

Replace with:

```rust
    let from = NaiveDate::parse_from_str(&query.from, "%Y-%m-%d")
        .map_err(|_| super::bad_request("Invalid 'from' date format, expected YYYY-MM-DD"))?;
```

- [ ] **Step 6: Rewrite classes.rs site 2 (lines ~85-90)**

Find:

```rust
    let to = NaiveDate::parse_from_str(&query.to, "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid 'to' date format, expected YYYY-MM-DD"})),
        )
    })?;
```

Replace with:

```rust
    let to = NaiveDate::parse_from_str(&query.to, "%Y-%m-%d")
        .map_err(|_| super::bad_request("Invalid 'to' date format, expected YYYY-MM-DD"))?;
```

- [ ] **Step 7: Rewrite classes.rs site 3 (lines ~227-232)**

Find:

```rust
        let Some(uid) = uid else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Card has no linked user"})),
            ));
        };
```

Replace with:

```rust
        let Some(uid) = uid else {
            return Err(super::bad_request("Card has no linked user"));
        };
```

- [ ] **Step 8: Rewrite auth.rs site 1 (lines ~55-59)**

Find:

```rust
    let name = body.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Name must not be empty"})),
        ));
    }
```

Replace with:

```rust
    let name = body.name.trim();
    if name.is_empty() {
        return Err(super::bad_request("Name must not be empty"));
    }
```

- [ ] **Step 9: Rewrite auth.rs site 2 (lines ~62-66)**

Find:

```rust
    if !body.email.contains('@') || !body.email.contains('.') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid email address"})),
        ));
    }
```

Replace with:

```rust
    if !body.email.contains('@') || !body.email.contains('.') {
        return Err(super::bad_request("Invalid email address"));
    }
```

- [ ] **Step 10: Rewrite auth.rs site 3 (lines ~69-73)**

Find:

```rust
    if body.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Password must be at least 8 characters"})),
        ));
    }
```

Replace with:

```rust
    if body.password.len() < 8 {
        return Err(super::bad_request("Password must be at least 8 characters"));
    }
```

- [ ] **Step 11: Verify no inline BAD_REQUEST sites remain in these 3 files**

```bash
grep -n "BAD_REQUEST" crates/spinbike-server/src/routes/admin.rs crates/spinbike-server/src/routes/classes.rs crates/spinbike-server/src/routes/auth.rs
```

Expected: zero matches. (All 9 sites are now `super::bad_request(...)`.)

- [ ] **Step 12: Local format check**

```bash
cargo fmt --all --check
```

Expected: no diff. If `cargo fmt --all --check` flags any of the rewrites (e.g. line-length forcing the call onto its own line), run `cargo fmt --all` and re-check.

- [ ] **Step 13: Commit**

```bash
git add crates/spinbike-server/src/routes/admin.rs crates/spinbike-server/src/routes/classes.rs crates/spinbike-server/src/routes/auth.rs
git commit -m "refactor(routes): rewrite 9 inline BAD_REQUEST sites in admin/classes/auth

Switches to super::bad_request(). Exact same message strings.
Behaviorally identical."
```

---

## Task 5: Rewrite inline `BAD_REQUEST` sites in `cards.rs`, `transactions.rs` (4 sites total)

**Files:**
- Modify: `crates/spinbike-server/src/routes/cards.rs` lines 375-380, 381-388
- Modify: `crates/spinbike-server/src/routes/transactions.rs` lines 142-147, 178-186

- [ ] **Step 1: Re-verify exact line numbers**

```bash
grep -n "BAD_REQUEST" crates/spinbike-server/src/routes/cards.rs crates/spinbike-server/src/routes/transactions.rs
```

Expected: 4 lines.

- [ ] **Step 2: Rewrite cards.rs site 1 (lines ~375-380)**

Find:

```rust
    // I7: Validate topup amount is positive.
    if body.amount <= 0.0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Amount must be greater than zero"})),
        ));
    }
```

Replace with:

```rust
    // I7: Validate topup amount is positive.
    if body.amount <= 0.0 {
        return Err(super::bad_request("Amount must be greater than zero"));
    }
```

- [ ] **Step 3: Rewrite cards.rs site 2 (lines ~381-388)**

Find:

```rust
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Note must be 200 characters or fewer"})),
        ));
    }
```

Replace with:

```rust
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(super::bad_request("Note must be 200 characters or fewer"));
    }
```

- [ ] **Step 4: Rewrite transactions.rs site 1 (lines ~142-147)**

Find:

```rust
    if row.valid_until.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Only pass transactions have valid_until"})),
        ));
    }
```

Replace with:

```rust
    if row.valid_until.is_none() {
        return Err(super::bad_request("Only pass transactions have valid_until"));
    }
```

- [ ] **Step 5: Rewrite transactions.rs site 2 (lines ~178-186)**

Find:

```rust
            if s.chars().count() > NOTE_MAX_CHARS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Note must be 200 characters or fewer"})),
                ));
            }
```

Replace with:

```rust
            if s.chars().count() > NOTE_MAX_CHARS {
                return Err(super::bad_request("Note must be 200 characters or fewer"));
            }
```

- [ ] **Step 6: Verify no inline BAD_REQUEST sites remain across all 5 files**

```bash
grep -n "BAD_REQUEST" crates/spinbike-server/src/routes/admin.rs crates/spinbike-server/src/routes/auth.rs crates/spinbike-server/src/routes/cards.rs crates/spinbike-server/src/routes/classes.rs crates/spinbike-server/src/routes/transactions.rs
```

Expected: zero matches.

- [ ] **Step 7: Verify only the shared helper definition remains in mod.rs**

```bash
grep -rn "fn bad_request" crates/spinbike-server/src/routes/
```

Expected: exactly one match — `crates/spinbike-server/src/routes/mod.rs:N: pub fn bad_request(...)`.

- [ ] **Step 8: Local format check**

```bash
cargo fmt --all --check
```

Expected: no diff.

- [ ] **Step 9: Commit**

```bash
git add crates/spinbike-server/src/routes/cards.rs crates/spinbike-server/src/routes/transactions.rs
git commit -m "refactor(routes): rewrite 4 inline BAD_REQUEST sites in cards/transactions

Closes the maximal #42 scope: 13 inline sites + 2 dedupes all
now use the shared super::bad_request helper. Server-side mutation
gate now mutates the message strings reliably."
```

---

## Task 6: #43 — `.cargo/config.toml` runner + wasm-bindgen-cli install

**Files:**
- Create: `.cargo/config.toml`
- Modify: `.github/workflows/ci.yml` (mutation-ui job, after the existing wasm-pack install at ~line 328)

- [ ] **Step 1: Create `.cargo/config.toml`**

Create the file at repo root:

```toml
# Repo-wide cargo runner config.
#
# This makes `cargo test --target wasm32-unknown-unknown` work by routing
# wasm test binaries through wasm-bindgen-test-runner (provided by the
# wasm-bindgen-cli crate). Without this, cargo can compile the wasm test
# binary but cannot execute it, so cargo-mutants on spinbike-ui silently
# no-ops on every mutation. (See issue #43.)
#
# Only affects wasm32 invocations. x86_64 builds for the server crate
# are unaffected.
[target.wasm32-unknown-unknown]
runner = "wasm-bindgen-test-runner"
```

- [ ] **Step 2: Verify file was created next to existing mutants.toml**

```bash
ls .cargo/
```

Expected: `config.toml  mutants.toml`.

- [ ] **Step 3: Add wasm-bindgen-cli install step to `mutation-ui` job in ci.yml**

Open `.github/workflows/ci.yml`. Find the `mutation-ui` job (starts at line ~300). Locate the existing `Install wasm-pack` step (around lines 325-328):

```yaml
      - name: Install wasm-pack
        uses: taiki-e/install-action@v2
        with:
          tool: wasm-pack
```

Immediately AFTER it, add:

```yaml
      - name: Install wasm-bindgen-cli
        uses: taiki-e/install-action@v2
        with:
          tool: wasm-bindgen-cli
```

- [ ] **Step 4: Verify the mutation-ui job structure**

```bash
sed -n '300,360p' .github/workflows/ci.yml
```

Expected: the `mutation-ui` job now has both `Install wasm-pack` AND `Install wasm-bindgen-cli` steps before `Compute PR diff vs base`.

- [ ] **Step 5: Commit**

```bash
git add .cargo/config.toml .github/workflows/ci.yml
git commit -m "ci: add wasm32 test runner for mutation-ui (#43 A)

.cargo/config.toml routes wasm test binaries through
wasm-bindgen-test-runner; mutation-ui CI job installs
wasm-bindgen-cli (which provides that binary). With this,
cargo test --target wasm32-unknown-unknown can actually
execute the test binary, so cargo-mutants will see real
mutant outcomes on the next PR that touches non-test
spinbike-ui src/."
```

---

## Task 7: #43 — Sanity-check step + comment cleanup

**Files:**
- Modify: `.github/workflows/ci.yml` (insert sanity-check step in mutation-ui job, between "Compute PR diff" and "Run cargo-mutants on UI diff"; replace apologetic comment in the run step)

- [ ] **Step 1: Insert sanity-check step**

In `.github/workflows/ci.yml`, in the `mutation-ui` job, find the `Compute PR diff vs base` step (line ~331):

```yaml
      - name: Compute PR diff vs base
        run: git diff origin/${{ github.base_ref }}...HEAD > pr.diff
```

Immediately AFTER it, insert:

```yaml
      - name: Sanity check — fail on silent no-op
        run: |
          # If non-test spinbike-ui/src/ code changed AND mutants list is empty,
          # something is wrong (likely the wasm runner is broken again).
          # Conservative heuristic: counts any added line outside #[cfg(test)],
          # blank lines, or single-line // comments. May produce false positives
          # on test-only PRs that put non-test attributes near test blocks; we
          # accept that for first cut.
          NONTEST_CHANGED=$(git diff origin/${{ github.base_ref }}...HEAD -- 'spinbike-ui/src/**/*.rs' \
            | grep -E '^\+' | grep -vE '^(\+\+\+|\+\s*//|\+\s*$)' \
            | grep -v '#\[cfg(test)\]' | wc -l)
          MUTANTS_COUNT=$(cargo mutants --list --in-diff pr.diff \
            --manifest-path spinbike-ui/Cargo.toml \
            -- --target wasm32-unknown-unknown 2>/dev/null | wc -l)
          if [ "$NONTEST_CHANGED" -gt 0 ] && [ "$MUTANTS_COUNT" -eq 0 ]; then
            echo "::error::Non-test spinbike-ui/src/ code changed but cargo mutants found 0 candidates."
            echo "This likely means the wasm32 test runner is broken. Investigate before merging."
            exit 1
          fi
          echo "Sanity check OK: $NONTEST_CHANGED non-test src/ lines changed, $MUTANTS_COUNT mutants."
```

- [ ] **Step 2: Replace apologetic comment in "Run cargo-mutants on UI diff" step**

Find the existing `Run cargo-mutants on UI diff` step (currently around line 333-350) which has the multi-line apologetic comment:

```yaml
      - name: Run cargo-mutants on UI diff
        run: |
          # Mutate only PR-changed lines in spinbike-ui.
          #
          # NOTE: this gate's wasm-target plumbing is UNTESTED on its first
          # green PR (#41) — every spinbike-ui change in #41 was inside
          # `#[cfg(test)]` blocks, so cargo-mutants generated zero candidates
          # ("No mutants to filter"). The next PR that modifies non-test
          # spinbike-ui code will be the gate's first real run; if it fails
          # because cargo test on wasm32 has no runner, follow up by adding
          # `[target.wasm32-unknown-unknown.runner]` to .cargo/config.toml
          # (see issue tracking the structural fix).
          cargo mutants \
            --in-diff pr.diff \
            --timeout 60 \
            --no-shuffle \
            --manifest-path spinbike-ui/Cargo.toml \
            -- --target wasm32-unknown-unknown
```

Replace it with the cleaner version:

```yaml
      - name: Run cargo-mutants on UI diff
        run: |
          # Mutate only PR-changed lines in spinbike-ui. The preceding
          # sanity-check step guards against the silent-no-op pattern
          # (mutants list empty when production code changed).
          cargo mutants \
            --in-diff pr.diff \
            --timeout 60 \
            --no-shuffle \
            --manifest-path spinbike-ui/Cargo.toml \
            -- --target wasm32-unknown-unknown
```

- [ ] **Step 3: Verify YAML well-formed**

```bash
sed -n '300,365p' .github/workflows/ci.yml
```

Expected: the `mutation-ui` job now has, in order:

1. `actions/checkout`
2. `dtolnay/rust-toolchain` (with wasm32 target)
3. `Swatinem/rust-cache@v2`
4. `Install cargo-mutants`
5. `Install wasm-pack`
6. `Install wasm-bindgen-cli` (added in Task 6)
7. `Compute PR diff vs base`
8. `Sanity check — fail on silent no-op` (added in this task)
9. `Run cargo-mutants on UI diff` (with cleaned-up comment)

Optional second sanity: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` should not raise.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add silent-no-op sanity check to mutation-ui (#43 B)

Fails fast if non-test spinbike-ui/src/ code changed but cargo
mutants found 0 candidates — the silent-no-op pattern PR #41
fell into. Drops the now-stale apologetic comment in the
cargo-mutants run step."
```

---

## Task 8: Push, monitor CI, mitigate surviving server mutants if any, open PR

**This is a controller-level task — NOT subagent-dispatched.** The controller runs Bash + AskUserQuestion + monitors CI directly.

- [ ] **Step 1: Push to origin/dev**

```bash
git push origin dev
```

- [ ] **Step 2: Identify the latest CI run for the push**

```bash
gh run list --branch dev --limit 5 --json databaseId,headSha,event,status,createdAt
```

Note the `databaseId` of the latest `event=push` run for the current SHA (`git rev-parse HEAD`).

- [ ] **Step 3: Monitor CI to terminal state with one background command**

```bash
# In background — single sleep + gh run view per ci-monitoring.md.
sleep 600 && gh run view <RUN_ID> --json status,conclusion,jobs
```

(Run with `run_in_background: true`. When it completes, read the output.)

If the run is still in progress when the command returns, sleep again with another 600s. Do NOT use `gh run watch` (rate-limit risk) and do NOT use `/loop` for this.

- [ ] **Step 4: All-jobs-green decision**

Check the result. ALL of the following must be `conclusion: success`:

- Test Integrity
- Version Bump Check
- Lint
- Test
- Test (UI)
- Build WASM (UI)
- E2E Tests
- Mutation Testing
- Mutation Testing (UI)
- Deploy (dev) (push run only — skipped on PR run)
- Smoke (dev) (push run only — skipped on PR run)

If green → go to Step 6.

- [ ] **Step 5: Mitigate surviving mutants on `Mutation Testing` (server) if it failed**

If `Mutation Testing` fails, fetch the failure log:

```bash
gh run view <RUN_ID> --log-failed | grep -A2 "MISSED\|UNCAUGHT\|missed:"
```

For each surviving mutant:

1. Identify the route file and message string. Example: `crates/spinbike-server/src/routes/admin.rs:486:replace string "name_sk and name_en are required" with ""` survived.
2. Find the corresponding integration test (typically in `crates/spinbike-server/tests/<route>.rs` or in a `#[cfg(test)] mod tests` block). Use grep to locate the test that hits the endpoint with the failing input:

   ```bash
   grep -rn "name_sk\|name_en" crates/spinbike-server/tests/ crates/spinbike-server/src/
   ```

3. Strengthen the assertion. Currently the test likely does only `assert_eq!(resp.status(), 400)`. Strengthen to also check the body:

   ```rust
   let body: serde_json::Value = resp.json().await.unwrap();
   assert!(
       body["error"].as_str().unwrap().contains("name_sk"),
       "expected error message to mention 'name_sk', got: {}",
       body["error"]
   );
   ```

   Use a substring of the message that's distinctive enough — short enough to survive future copy-edits but long enough to fail when mutated to `""` or `"xyzzy"`.

4. Commit each batch of test-strengthening edits with explicit paths:

   ```bash
   git add <specific_test_file_paths>
   git commit -m "test(routes): assert error message body for <site>"
   ```

5. Push and re-monitor:

   ```bash
   git push origin dev
   gh run list --branch dev --limit 1 --json databaseId
   # then monitor as in Step 3
   ```

Repeat until ALL jobs green. **Do NOT skip mutants** (no `#[mutants::skip]`) — that defeats the purpose of #36/#42.

- [ ] **Step 6: Confirm Deploy (dev) and Smoke (dev) are green**

The push-run on dev runs Deploy (dev) and Smoke (dev) automatically. Confirm both `conclusion: success` from the same `gh run view` output.

Visit https://spinbike-dev.newlevel.media in Playwright (real browser):

```bash
# (Use the playwright MCP tool to navigate; read [data-testid="version"]; expect v0.13.14.)
```

- [ ] **Step 7: Open the PR `dev` → `main`**

```bash
gh pr create --base main --head dev --title "v0.13.14: quality follow-ups (#43 #42)" --body "$(cat <<'EOF'
## Summary

Two small PR #41 follow-ups, bundled in one cycle.

- **#42 maximal** — `bad_request` helper consolidated in `routes/mod.rs` (alongside existing `internal_error`). Two private duplicates (`payments.rs`, `reports.rs`) removed; 13 inline `(StatusCode::BAD_REQUEST, Json(json!({...})))` sites in `admin.rs`, `classes.rs`, `auth.rs`, `cards.rs`, `transactions.rs` rewritten to `super::bad_request("...")`. Behaviorally identical; cargo-mutants now mutates the message strings everywhere.
- **#43 A + B** — `.cargo/config.toml` adds `wasm-bindgen-test-runner` for `wasm32-unknown-unknown`; `mutation-ui` CI job installs `wasm-bindgen-cli`. New sanity-check step fails fast if non-test `spinbike-ui/src/` changed AND `cargo mutants --list` is empty (the silent-no-op pattern PR #41 fell into).

## Test plan

- [x] CI green: Test Integrity, Lint, Test, Test (UI), Build WASM (UI), E2E Tests, Mutation Testing, Mutation Testing (UI), Deploy (dev), Smoke (dev)
- [x] Server-side mutation gate caught any surviving mutants from the new helper rewrites; route tests strengthened to assert message body where needed
- [ ] Post-deploy: dev frontend `[data-testid="version"]` reads `v0.13.14`, matches `/api/version`
- [ ] After merge: prod at https://spinbike.newlevel.media verified the same way + spot-check one BAD_REQUEST endpoint returns 400 with `{"error": ...}`

## Honest scope note

The `Mutation Testing (UI)` runner is plumbed but not yet exercised end-to-end on this PR — every change here is server-side, so spinbike-ui's PR diff is empty. Same catch-22 as PR #41. The new sanity-check step (B) is the safety net for any future silent no-op; the runner's first real exercise is the next PR that touches non-test `spinbike-ui/src/`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 8: Wait for PR run to be mergeable + clean**

After PR creation, the PR also runs CI (push event was already green; PR event runs the additional `Mutation Testing` + `Mutation Testing (UI)` + `Version Bump Check` gates).

Monitor the PR run:

```bash
gh run list --branch dev --event pull_request --limit 1 --json databaseId
sleep 600 && gh run view <PR_RUN_ID> --json status,conclusion,jobs
```

Once green, verify mergeable+clean:

```bash
gh pr view --json number,mergeable,mergeStateStatus
```

Expected: `mergeable: MERGEABLE`, `mergeStateStatus: CLEAN`.

If not clean (UNSTABLE / BLOCKED / BEHIND / DIRTY): investigate the cause and fix before reporting done. Per `autonomous-quality-discipline.md`: never report "functionally ready" or "merge despite". Either CLEAN or not done.

- [ ] **Step 9: Stop here**

**Do NOT merge.** Per `pr-merge-policy.md`, only the user explicitly says "merge it". Send the completion report (per `completion-report.md` template) with the PR URL and wait.

---

## Task 9: Post-deploy verification (RUNS ONLY AFTER USER MERGES)

**This task is gated on the user's explicit "merge it" instruction. Do NOT execute until then.**

- [ ] **Step 1: Wait for user merge instruction**

Once the user merges, monitor the main-branch CI run:

```bash
gh run list --branch main --limit 1 --json databaseId
sleep 300 && gh run view <MAIN_RUN_ID> --json status,conclusion,jobs
```

Wait for `Deploy (prod)` and `Smoke (prod)` to reach `conclusion: success`.

- [ ] **Step 2: Verify dev frontend version**

Use the Playwright MCP tool to navigate to https://spinbike-dev.newlevel.media. Read `[data-testid="version"]`:

```javascript
// Through Playwright MCP:
// browser_navigate to https://spinbike-dev.newlevel.media
// browser_snapshot or browser_evaluate to read the data-testid="version" text
```

Expected: `v0.13.14`. Console: zero errors.

- [ ] **Step 3: Verify dev backend `/api/version`**

```bash
curl -s https://spinbike-dev.newlevel.media/api/version
```

Expected: `{"version":"0.13.14"}` (or similar; exact shape per existing `version.rs`).

Frontend label and backend response MUST match.

- [ ] **Step 4: Spot-check one BAD_REQUEST endpoint on dev**

```bash
# POST a clearly malformed body to /api/payments/charge — needs auth, so use the
# unauthenticated 401 path OR a curl with a stale token to confirm 400 still
# returns the expected JSON shape.
# A simpler probe: hit /api/auth/register with empty name to exercise auth.rs:55
curl -s -X POST https://spinbike-dev.newlevel.media/api/auth/register \
  -H 'Content-Type: application/json' \
  -d '{"name": "", "email": "x@y.z", "password": "longenoughpw"}' \
  -w "\nHTTP %{http_code}\n"
```

Expected: HTTP 400 + body `{"error": "Name must not be empty"}`. Confirms the helper consolidation didn't change observable behavior.

- [ ] **Step 5: Repeat steps 2-4 for prod**

URLs: https://spinbike.newlevel.media. Expected: same `v0.13.14`, same `/api/version` match, same 400 + expected error body.

- [ ] **Step 6: Send completion report**

Per `completion-report.md` template — Audits & deploy at top, Goal/What changed/URLs/PR at bottom. Include the version label confirmed visible on both dev and prod.

---

## Done

After Task 9, both #43 and #42 are closed by the merge (PR description's `Closes #42 #43` lines). No further follow-up issues expected from this PR.

---

Plan committed locally as <pending>. Dispatching subagents now.
