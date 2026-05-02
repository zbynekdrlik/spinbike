# Quality Bundle Design (v0.13.13)

**Date:** 2026-05-01
**Closes:** #39, #28, #22, #36
**Type:** CI / quality / DB schema (no user-visible feature changes)

## Goal

Resolve four CI and quality issues in a single PR cycle:

- **#39** — E2E flake: timestamp barcode in `card-action-form.spec.ts` substring-collides with seeded barcode `70701001` in `dashboard.spec.ts`
- **#28** — DB defense-in-depth: `CHECK(length(note) <= 200)` constraint on `transactions.note`
- **#22** — Mutation testing doesn't cover `spinbike-ui` (workspace-excluded WASM crate)
- **#36** — `cargo-mutants` reports 4/4 unviable mutants on `payments.rs` charge handler (Axum `Json` newtype has no `::new()`)

User explicitly requested "do in one run" — single PR closing all four.

## Architecture (per issue)

### #39 — Structural barcode fix

**Decision:** Issue option 1 (structural fix). Replace numeric `Date.now()`-based barcode suffix with non-numeric letters; barcode collision with seeded numeric barcodes (`70701001`, `70701142`, `70706819`) becomes impossible by construction, not just statistically rare.

**Changes:**

- New helper in `e2e/tests/helpers.ts`:
  ```ts
  export async function activateUniqueCard(
      adminToken: string,
      credit: number,
  ): Promise<{ lastName: string; barcode: string }> {
      // Letters-only suffix avoids substring collision with seeded numeric barcodes.
      const suffix = Array.from({ length: 8 }, () =>
          String.fromCharCode(97 + Math.floor(Math.random() * 26)),
      ).join('');
      const barcode = `AF-${suffix}`;
      // ... rest of activation logic identical to existing duplicates ...
  }
  ```
- `e2e/tests/card-action-form.spec.ts`: delete local `activateUniqueCard` definition, import from `./helpers`.
- `e2e/tests/desk-ux.spec.ts`: same — delete local definition, import from helpers.

**Why this is the right structural fix:** the previous suffix included `Date.now()` (13 numeric chars) which has nonzero probability of containing any 4-digit substring used elsewhere. A pure letters-a-z suffix never matches a digit substring search.

### #36 — `bad_request` helper wrapper

**Decision:** Issue option 2 (wrap error returns in helper). Cargo-mutants can mutate a function-call argument (the message string) reliably, while it fails on the `Json(...)` tuple-struct constructor.

**Changes:**

- Add helper near top of `crates/spinbike-server/src/routes/payments.rs`:
  ```rust
  fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
      (
          StatusCode::BAD_REQUEST,
          Json(serde_json::json!({ "error": msg })),
      )
  }
  ```
- Replace all 9 `BAD_REQUEST` Err returns in payments.rs with `return Err(bad_request("..."));` — 4 in `charge` (lines ~90, 107, 117, 125), 1 in `storno` (~206), 3 in `sell_pass` (~273, 280, 288), 1 in `log_visit` (~404).
- **Not in scope:** `FORBIDDEN`, `NOT_FOUND`, `CONFLICT` returns. Different status codes; over-scoping risks breaking unrelated tests.

**Behavior unchanged:** response JSON shape stays `{"error": "..."}` — identical to current. Existing `charge_rejects_zero_amount`, `charge_rejects_negative_amount`, `charge_rejects_null_service_id_with_400` tests continue to pass.

### #22 — wasm-pack node test + mutation testing for spinbike-ui

**Decision:** Issue option (b) — `wasm-pack test --node spinbike-ui` (user picked over host-target). True WASM target, no `cfg(target_arch)` gating needed on existing WASM-only deps (`wasm-bindgen`, `gloo-*`, `web-sys`, `js-sys`).

**Changes:**

- Add to `spinbike-ui/Cargo.toml`:
  ```toml
  [dev-dependencies]
  wasm-bindgen-test = "0.3"
  ```
- Convert all existing `#[test]` to `#[wasm_bindgen_test]` in 3 files (currently ~36 test markers across):
  - `spinbike-ui/src/i18n.rs`
  - `spinbike-ui/src/util.rs`
  - `spinbike-ui/src/components/date_input.rs`
- Add at top of each `#[cfg(test)]` module:
  ```rust
  use wasm_bindgen_test::*;
  wasm_bindgen_test_configure!(run_in_node);
  ```
- New CI jobs in `.github/workflows/ci.yml`:
  1. `Test (UI)` — installs pinned `wasm-pack`, runs `wasm-pack test --node spinbike-ui`, with `Swatinem/rust-cache@v2` keyed for the UI manifest.
  2. `Mutation Testing (UI)` — runs `cargo mutants --in-diff pr.diff --manifest-path spinbike-ui/Cargo.toml --timeout 60`. Skipped on `push` to dev (only runs on PR), matching the existing server mutants job pattern.
- **Demonstration cycle (red→strengthen→green) for issue acceptance:**
  - Pick one pure-logic helper (chosen: `ServiceInfo::is_class_visit` in `dashboard/mod.rs`).
  - Commit a deliberately weak assertion that mutants would survive (e.g. `let _ = s.is_class_visit();` with no assertion).
  - Strengthen the assertion to pin the contract end-to-end.
  - The two commits in the PR (weak → strengthened) live in git history as a code-review-level demonstration. Per acceptance bullet "tests added to demonstrate the new mutation-test job actually catches a weak assertion (red → strengthen → green cycle)".

  **Honest scope of CI verification:** the cumulative PR diff only exposes the strong-test state to cargo-mutants (the weak commit's lines are net-removed). On this PR, every spinbike-ui change is inside `#[cfg(test)]` blocks, so cargo-mutants generated zero candidates ("No mutants to filter") — the gate ran without exercising any mutant. The demonstration is documented in git history but not empirically verified by CI on this PR. The first PR that modifies a non-test spinbike-ui function body will be the gate's first real run; if its wasm-target plumbing turns out to need work, follow up structurally (see issues filed alongside this PR).

**Pin:** `wasm-pack` is installed via `taiki-e/install-action@v2` with prebuilt binaries (not `cargo install --locked`) to keep CI overhead low.

### #28 — V11 transactions.note CHECK constraint migration

**Decision:** Follow V8 precedent (`v8_drop_rename_pattern_works_with_fk_child_rows` test confirms FK reattaches by table name after `RENAME TO`). The migration runner already toggles `PRAGMA foreign_keys = OFF/ON` around each migration (`db/mod.rs:100,129`).

**Changes:**

- Add to `crates/spinbike-server/src/db/migrations.rs`:
  ```rust
  // In MIGRATIONS slice, append:
  (11, "transactions: note length CHECK", V11_TRANSACTIONS_NOTE_CHECK),

  const V11_TRANSACTIONS_NOTE_CHECK: &str = r#"
  -- Defense-in-depth: server already validates note ≤ 200 chars at every
  -- entry point. This adds the same constraint at the DB level so a direct
  -- sqlite3 write (or a future endpoint that forgets to validate) cannot
  -- store an unbounded string.
  --
  -- SQLite cannot ALTER TABLE to add CHECK constraints on existing columns.
  -- Use the CREATE_NEW + INSERT + DROP + RENAME pattern (V8 precedent).
  -- Migration runner toggles PRAGMA foreign_keys around the transaction;
  -- bookings.charge_transaction_id FK reattaches by name after RENAME.

  CREATE TABLE transactions_new (
      id                  INTEGER PRIMARY KEY AUTOINCREMENT,
      user_id             INTEGER REFERENCES users(id),
      card_id             INTEGER REFERENCES cards(id),
      staff_id            INTEGER REFERENCES users(id),
      service_id          INTEGER REFERENCES services(id),
      amount              REAL    NOT NULL,
      action              TEXT    NOT NULL,
      created_at          TEXT    NOT NULL DEFAULT (datetime('now')),
      valid_until         TEXT,
      deleted_at          TEXT,
      legacy_backfilled   INTEGER NOT NULL DEFAULT 0,
      note                TEXT    CHECK (note IS NULL OR length(note) <= 200)
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
- **Index handling:** confirmed by grep across migrations.rs — no explicit `CREATE INDEX` on `transactions` exists in any migration. The auto-PK index is preserved by the new `INTEGER PRIMARY KEY AUTOINCREMENT` column. No index recreation needed.
- **Tests** (added to `migrations.rs` `mod tests`):
  - `v11_note_check_accepts_200_chars`: insert a 200-char Slovak diacritic note, expect Ok.
  - `v11_note_check_rejects_201_chars`: insert a 201-char note, expect SQLITE_CONSTRAINT.
  - `v11_is_idempotent`: run migrations twice, no error.
  - `v11_drop_rename_pattern_preserves_bookings_fk`: insert a transaction + a booking referencing it, run V11, verify the booking's `charge_transaction_id` FK still resolves.
- **Prod-DB validation** (per `feedback_validate_against_real_data.md`):
  - After deploy-dev (which auto-syncs prod → dev), open the dev DB via `sqlite3` and confirm the migration applied cleanly.
  - Note the migration duration in the deploy-dev logs as a heads-up for prod deploy timing on 88K rows.

## Components affected

| File | Issue | Change |
|---|---|---|
| `e2e/tests/helpers.ts` | #39 | Add `activateUniqueCard` |
| `e2e/tests/card-action-form.spec.ts` | #39 | Import helper, delete local copy |
| `e2e/tests/desk-ux.spec.ts` | #39 | Import helper, delete local copy |
| `crates/spinbike-server/src/routes/payments.rs` | #36 | Add `bad_request` helper, refactor 6 BAD_REQUEST returns |
| `crates/spinbike-server/src/db/migrations.rs` | #28 | New V11 migration + 4 tests |
| `spinbike-ui/Cargo.toml` | #22 | Add `wasm-bindgen-test` dev-dep |
| `spinbike-ui/src/i18n.rs` | #22 | Convert tests to `#[wasm_bindgen_test]` |
| `spinbike-ui/src/util.rs` | #22 | Convert tests to `#[wasm_bindgen_test]` |
| `spinbike-ui/src/components/date_input.rs` | #22 | Convert tests to `#[wasm_bindgen_test]` |
| `.github/workflows/ci.yml` | #22 | 2 new jobs: Test (UI), Mutation Testing (UI) |
| `VERSION` + Cargo.toml synced | bump | 0.13.12 → 0.13.13 |

## Order of work

1. **Version bump** — VERSION 0.13.12 → 0.13.13 + `bash scripts/sync-version.sh`. First commit, before any other change.
2. **#39 structural** — helpers.ts gains `activateUniqueCard`, both spec files import it.
3. **#36 helper** — `bad_request` in payments.rs, refactor BAD_REQUEST guards.
4. **#22 wasm-pack** — dev-deps, convert tests to `#[wasm_bindgen_test]`, add 2 CI jobs.
5. **#22 demonstration cycle** — commit weak assertion, push, observe mutants survive, strengthen assertion, push, observe green.
6. **#28 V11 migration** — RED test (insert 201-char note expects SQLITE_CONSTRAINT) commits first; GREEN migration commit makes it pass.
7. **Final push, monitor CI to terminal** — single `sleep N && gh run view --json` background command.
8. **Open PR `dev` → `main`** once green; await user merge.
9. **Post-deploy verification** — after merge, dev frontend reads `[data-testid="version"]` matches `v0.13.13`, then check migration timing on prod-synced dev DB.

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| V11 migration slow on 88K-row prod table | Medium | Time it on prod-synced dev DB pre-merge; doc the duration |
| `wasm-pack` install fails in CI | Low | Pin version with `--locked`; fail-loudly on install error |
| Test conversion misses `#[cfg(test)]` modules in spinbike-ui | Medium | Grep all `#[test]` in spinbike-ui/ pre-commit; ensure each module has `wasm_bindgen_test_configure!` |
| `bad_request` helper changes JSON serialization shape | Very low | `Json(serde_json::json!({"error": msg}))` is byte-identical to current |
| Bundle size — 4 issues in one PR — one regression blocks all | Medium | Each issue gets its own commits; if one fails, revert that commit only and ship the rest |
| Demonstration weak-assertion commit ships in PR history | Low | Acceptable — it's the proof the gate works; could squash before merge if user prefers |

## Acceptance

- **#39:** `activateUniqueCard` exported from `helpers.ts`, both spec files import it, suffix is letters-only. CI E2E runs green for full suite.
- **#36:** `bad_request` helper added; 6 call sites use it; `cargo mutants` on a fresh PR diff shows ≥1 mutant killed on string args (vs current 0/4 viable).
- **#22:** `Test (UI)` job runs the converted tests on every push; `Mutation Testing (UI)` job runs on PR diffs and fails on surviving mutants; demonstration commit shows the gate caught a weak assertion.
- **#28:** V11 migration applies cleanly on a fresh DB AND on prod-synced dev DB; 4 tests pass; insert 201-char note returns SQLITE_CONSTRAINT.
- **PR scope:** version 0.13.13, single PR, mergeable + clean.

## Out of scope

- Constraints on other text columns (`reason`, etc.) — issue #28 explicitly out-of-scope.
- Refactoring `FORBIDDEN`/`NOT_FOUND`/`CONFLICT` returns in `payments.rs` — different status codes; only `BAD_REQUEST` is in scope for #36.
- Mutation testing for other workspace-excluded crates — none exist; only `spinbike-ui` was excluded.
- E2E suite empirical collision test (run 50× to verify zero collisions) — letters-only suffix makes collision logically impossible; empirical test would be slow and unnecessary.

## References

- Issue #39: <https://github.com/zbynekdrlik/spinbike/issues/39>
- Issue #28: <https://github.com/zbynekdrlik/spinbike/issues/28>
- Issue #22: <https://github.com/zbynekdrlik/spinbike/issues/22>
- Issue #36: <https://github.com/zbynekdrlik/spinbike/issues/36>
- V8 migration precedent: `crates/spinbike-server/src/db/migrations.rs:219` and `v8_drop_rename_pattern_works_with_fk_child_rows` test
- Migration runner FK toggle: `crates/spinbike-server/src/db/mod.rs:100,129`
- airuleset: `mutation-testing.md`, `validate-against-real-data.md`, `version-bumping.md`
