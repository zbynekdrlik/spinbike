# Quality Follow-ups (#43 + #42) Design

**Bundle:** PR #41 follow-ups, both small.

- **#43** — Mutation Testing (UI) gate plumbing untested
- **#42** — Move `bad_request` helper to shared route module (maximal scope)

**Goal:** Ship one PR `dev → main` that closes both issues, in line with airuleset standards.

---

## #42 — Maximal `bad_request` consolidation

### Current state on disk

`bad_request` is privately duplicated in two route files:

- `crates/spinbike-server/src/routes/payments.rs:76` — added in PR #41
- `crates/spinbike-server/src/routes/reports.rs:125` — pre-existing

There are also 13 inline `(StatusCode::BAD_REQUEST, Json(json!({...})))` returns across 5 other route files:

| File | Lines |
|------|-------|
| `routes/admin.rs` | 486, 493, 672 |
| `routes/classes.rs` | 81, 87, 229 |
| `routes/auth.rs` | 57, 64, 71 |
| `routes/cards.rs` | 377, 385 |
| `routes/transactions.rs` | 144, 182 |

`crates/spinbike-server/src/routes/mod.rs:20` already exposes `pub fn internal_error(e: impl std::fmt::Display)` — the pattern to follow.

### Target state

Add to `routes/mod.rs`:

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

- `routes/payments.rs` and `routes/reports.rs` — delete local `fn bad_request`, replace internal call sites with `super::bad_request(...)` (no path change beyond removing the local definition).
- `routes/admin.rs`, `routes/classes.rs`, `routes/auth.rs`, `routes/cards.rs`, `routes/transactions.rs` — rewrite 13 inline sites from `Err((StatusCode::BAD_REQUEST, Json(json!({"error": "msg"}))))` to `Err(super::bad_request("msg"))`. Keep the exact same message strings and surrounding control flow.

### Mutation gate risk on this PR

Each inline rewrite creates a new diff line `bad_request("…")`. cargo-mutants will mutate the message string argument (e.g. swap to `""` or `"xyzzy"`). Existing route tests must assert the response body's `error` field (not just status `400`) for the mutation to be caught.

**Mitigation strategy (TDD-aligned):**

1. Push the change.
2. Read the Mutation Testing CI log; identify surviving mutants by file:line.
3. For each surviving mutant, strengthen the corresponding test to assert the message substring.
4. Push the test fixes; re-run.

Bounded blast radius: at most 13 test-strengthening edits.

### Behavior preservation (no new tests required)

The shared helper is byte-for-byte equivalent to each inline construction. Existing integration tests already cover behavior. No new test files are required for #42; only test strengthening if mutation gate flags surviving mutants.

---

## #43 — Mutation Testing (UI) plumbing

### Current state

`Mutation Testing (UI)` job in `.github/workflows/ci.yml:300-350`:

- Installs `cargo-mutants` and `wasm-pack`
- Runs `cargo mutants --in-diff pr.diff --manifest-path spinbike-ui/Cargo.toml -- --target wasm32-unknown-unknown`
- Has an apologetic comment acknowledging the runner config is missing

There is no `.cargo/config.toml` runner. `cargo test --target wasm32-unknown-unknown` cannot execute the test binary because no runner is wired up. cargo-mutants therefore cannot actually verify mutants on the UI crate.

### A — Structural fix (runner)

Add `.cargo/config.toml` at repo root (next to existing `.cargo/mutants.toml`):

```toml
[target.wasm32-unknown-unknown]
runner = "wasm-bindgen-test-runner"
```

Add to `mutation-ui` job in `ci.yml`, after the existing wasm-pack install:

```yaml
- name: Install wasm-bindgen-cli
  uses: taiki-e/install-action@v2
  with:
    tool: wasm-bindgen-cli
```

`wasm-bindgen-cli` provides the `wasm-bindgen-test-runner` binary the runner config calls. Repo-root scoped: only affects wasm32 invocations; x86_64 builds for the server crate are unaffected.

### B — Sanity check (silent no-op detector)

Add to `mutation-ui` job, AFTER the diff computation and BEFORE the cargo-mutants run:

```yaml
- name: Sanity check — fail on silent no-op
  run: |
    # Count non-test, non-comment, non-blank added lines in spinbike-ui/src/.
    NONTEST_CHANGED=$(git diff origin/${{ github.base_ref }}...HEAD -- 'spinbike-ui/src/**/*.rs' \
      | grep -E '^\+' | grep -vE '^(\+\+\+|\+\s*//|\+\s*$)' \
      | grep -v '#\[cfg(test)\]' | wc -l)
    # Count mutants cargo would generate.
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

**Conservative heuristic acknowledgement:** the grep counts any added line outside `#[cfg(test)]`, blank lines, or single-line `//` comments. It will count lines inside a `#[cfg(test)] mod tests { ... }` block as "non-test" because the cfg attribute itself is on a different line. That can cause false positives (job fails when it shouldn't on a test-only PR). Acceptable for first cut; refine later if it actually bites.

### Comment cleanup

The apologetic comment in the cargo-mutants run step (`ci.yml:336-344`) gets replaced with a short note pointing at the now-active sanity check.

### Why no end-to-end demo on this PR

The maximal #42 refactor only touches server-side route files. This PR's spinbike-ui src/ diff is empty, so cargo-mutants generates zero UI candidates regardless of runner state — same catch-22 as PR #41. We accept this and document it: the gate's first real exercise is the next PR that touches non-test `spinbike-ui/src/`. The new sanity check (B) is the safety net for any future silent no-op.

---

## Verification on this PR

| Subsystem | Expected CI signal |
|-----------|-------------------|
| `Test Integrity`, `Lint`, `Build WASM (UI)` | Green |
| `Test`, `Test (UI)` | Green (no test changes; helper move is behaviorally equivalent) |
| `E2E Tests` | Green (no UX change) |
| `Mutation Testing` (server) | Will mutate new `bad_request` helper + 13 callsite message strings. May surface surviving mutants; strengthen those route tests in the same PR. |
| `Mutation Testing (UI)` | Sanity check reports `0 non-test src/ lines changed, 0 mutants`. cargo mutants runs and exits clean (no candidates). |
| `Deploy (dev)`, `Smoke (dev)` | Green |

### Post-deploy verification

After CI deploys to dev, then prod after merge:

- Open `https://spinbike-dev.newlevel.media` in Playwright; read `[data-testid="version"]`; confirm matches `/api/version`.
- Spot-check one route that returns BAD_REQUEST (e.g. POST a malformed `/api/payments/charge` body); confirm response is still `{"error": "..."}` with status 400 — proves no behavioral regression from the helper consolidation.

---

## Out of scope

- Server-side test strengthening beyond what surviving mutants on this PR require. We do not pre-audit all 13 sites.
- The sanity check's grep heuristic is conservative (may have false positives on test-only PRs). Refining the heuristic is deferred until it actually bites.
- Demonstrating the wasm runner end-to-end with a real mutant. That happens implicitly on the next PR that touches non-test UI code.
- Deduplication beyond `bad_request` — `internal_error` already lives in `routes/mod.rs` and other helpers are not in scope.

---

## Summary of files changed

| File | Change |
|------|--------|
| `routes/mod.rs` | Add `pub fn bad_request` |
| `routes/payments.rs` | Delete local helper, retarget calls |
| `routes/reports.rs` | Delete local helper, retarget calls |
| `routes/admin.rs` | 3 inline → helper |
| `routes/classes.rs` | 3 inline → helper |
| `routes/auth.rs` | 3 inline → helper |
| `routes/cards.rs` | 2 inline → helper |
| `routes/transactions.rs` | 2 inline → helper |
| `.cargo/config.toml` (new) | wasm-bindgen-test-runner |
| `.github/workflows/ci.yml` | Install wasm-bindgen-cli + sanity-check step + comment cleanup |
| `VERSION` | Bump to next dev (handled per Task 1) |
| `Cargo.toml` (root + spinbike-ui) | Synced from VERSION |

11 files. No new tests. No schema changes.
