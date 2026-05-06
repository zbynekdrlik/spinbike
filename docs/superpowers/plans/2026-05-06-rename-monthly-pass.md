# Rename `Mesačný preplatok` → `Mesačná permanentka` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the Slovak label of the monthly-pass service from the wrong word `preplatok` (overpayment) to the correct word `permanentka` (gym pass), with feminine adjective agreement. One DB row, one new migration step, six E2E string literals.

**Architecture:** Append a new idempotent V14 migration step `UPDATE services SET name_sk='Mesačná permanentka' WHERE name_sk='Mesačný preplatok'`. Frozen migrations (V1..V13) are not edited. E2E specs receive a flat string-literal swap. No schema change.

**Tech Stack:** Rust + sqlx + SQLite (server), Playwright + TypeScript (E2E).

---

### Task 1: VERSION bump 0.13.22 → 0.13.23

**Status: ALREADY DONE (committed at c332e9e on dev).** Controller-run, not a subagent task.

---

### Task 2: V14 migration + unit test + V8 assertion update (subagent, sonnet)

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs`

**Critical context the subagent must read before writing code:**

- The MIGRATIONS array lives at `migrations.rs:2-52`. Append the new entry as the 14th element. Existing entries are FROZEN — do not edit them.
- The runner is `db::run_migrations` (in `db/mod.rs`). It iterates MIGRATIONS in order and executes each `sql` block inside a transaction with `PRAGMA foreign_keys = OFF` toggled. V14 is a pure UPDATE on an existing column — no PRAGMA concerns.
- V8 (`V8_SERVICES_DUAL_LANG_KIND`, line 230) seeds `'Mesačný preplatok'` at line 252. **Do not edit V8.** V14 runs immediately after V8 in the chain, so fresh DBs end up at the new name.
- The existing test that asserts the V8-seeded value lives near line 778 inside `migrations.rs`. Find the assertion `assert_eq!(pass.2, "Mesačný preplatok");` and update its expected value to `"Mesačná permanentka"`. Reasoning: the test runs `run_migrations` (full chain), which now includes V14, so the post-migration value is the new name. The neighbour assertion `assert_eq!(pass.3, "Monthly pass");` (the English `name_en`) is unchanged.
- The unrelated test fixture `'Druhý preplatok'` at line 799 is intentionally distinct (it tests the unique-index reject path) — leave it alone.

**No local cargo build / test / clippy. CI is authoritative. Run only `cargo fmt --all --check` if you want to check formatting locally before commit.**

- [ ] **Step 1: Add the V14 SQL constant**

In `crates/spinbike-server/src/db/migrations.rs`, after the V13 const block ends, add:

```rust
const V14_RENAME_MONTHLY_PASS_LABEL: &str = r#"
-- Issue #50: 'preplatok' means overpayment, not pass. Correct Slovak word
-- for a gym pass is 'permanentka' (feminine), so the adjective also flips:
-- 'Mesačná' not 'Mesačný'. Idempotent: re-runs match zero rows.
UPDATE services
SET name_sk = 'Mesačná permanentka'
WHERE name_sk = 'Mesačný preplatok';
"#;
```

- [ ] **Step 2: Register V14 in the MIGRATIONS array**

Edit the array (currently ending at line 51 with the V13 entry). Append:

```rust
    (
        14,
        "rename monthly_pass label: Mesačný preplatok → Mesačná permanentka",
        V14_RENAME_MONTHLY_PASS_LABEL,
    ),
```

The closing `];` stays. Order in source: V13 entry, then V14 entry, then `];`.

- [ ] **Step 3: Update the V8 test assertion**

Find the line `assert_eq!(pass.2, "Mesačný preplatok");` (around line 778). Replace with:

```rust
        assert_eq!(pass.2, "Mesačná permanentka");
```

Leave `assert_eq!(pass.3, "Monthly pass");` (the next line) unchanged — `name_en` does not change.

- [ ] **Step 4: Add the V14 unit test**

Add a new `#[tokio::test]` function at the end of the existing `mod tests` block (just before its closing `}`). Place it after the last existing test in the module:

```rust
    #[tokio::test]
    async fn v14_renames_monthly_pass_label() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // The monthly_pass row now reads the corrected Slovak label.
        let pass_name: String = sqlx::query_scalar(
            "SELECT name_sk FROM services WHERE kind = 'monthly_pass'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pass_name, "Mesačná permanentka");

        // Other service rows are NOT touched by V14 — kills mutants that
        // broaden the WHERE clause (e.g. `LIKE '%preplatok%'`).
        for n_sk in ["Spinning", "Fitness", "Občerstvenie", "Doplnky výživy", "Aktivácia karty"] {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM services WHERE name_sk = ?",
            )
            .bind(n_sk)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(count, 1, "service '{n_sk}' must be unchanged after V14");
        }

        // No row still carries the old Slovak label.
        let stale: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM services WHERE name_sk = 'Mesačný preplatok'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stale, 0, "no services row should still carry the old label");

        // Idempotency: running the chain a second time must not error and
        // must not re-mutate the row.
        run_migrations(&pool).await.unwrap();
        let pass_name_again: String = sqlx::query_scalar(
            "SELECT name_sk FROM services WHERE kind = 'monthly_pass'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pass_name_again, "Mesačná permanentka");
    }
```

- [ ] **Step 5: Local format check**

Run: `cargo fmt --all --check`. Expected: clean. If not, run `cargo fmt --all` and re-check. Do NOT run `cargo build`, `cargo test`, or `cargo clippy` locally — CI handles those.

- [ ] **Step 6: Commit**

Stage explicit paths (NEVER `git add -A` per project memory):

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "$(cat <<'EOF'
feat(db): V14 rename monthly_pass label to Mesačná permanentka (#50)

'preplatok' means overpayment — wrong word for a gym pass. Correct Slovak
is 'permanentka' (feminine), so the adjective flips to 'Mesačná'. The rest
of the UI already uses permanentka (Predaj permanentky · do <date>).

Idempotent UPDATE on existing prod DBs; fresh DBs run V8 then V14 so the
final state is the corrected label. V8 stays frozen.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: E2E string-literal updates across 5 specs (subagent, sonnet)

**Files:**
- Modify: `e2e/tests/post-backfill-history.spec.ts`
- Modify: `e2e/tests/visit-button-feedback.spec.ts`
- Modify: `e2e/tests/services-admin.spec.ts`
- Modify: `e2e/tests/dashboard-button-layout.spec.ts`
- Modify: `e2e/tests/log-visit-class-only.spec.ts`

**Context for the subagent:** these are flat string-literal swaps. No logic changes. Two of the files use a regex with `Monthly pass | Mesačný preplatok` — keep the English alternative, swap only the Slovak alternative.

- [ ] **Step 1: Update `post-backfill-history.spec.ts:28`**

Change:
```ts
{ amount: -35.0, action: 'charge', service_name_sk: 'Mesačný preplatok', valid_until: '2099-12-31' },
```
to:
```ts
{ amount: -35.0, action: 'charge', service_name_sk: 'Mesačná permanentka', valid_until: '2099-12-31' },
```

- [ ] **Step 2: Update `visit-button-feedback.spec.ts:35` and `:99`**

At both lines, replace `service_name_sk: 'Mesačný preplatok',` with `service_name_sk: 'Mesačná permanentka',`. Use `replace_all` only if the file truly contains exactly this string twice — verify with `grep -n "Mesačný preplatok" e2e/tests/visit-button-feedback.spec.ts` before editing.

- [ ] **Step 3: Update `services-admin.spec.ts:60` and `:64`**

Line 60 is a comment listing seeded services. Update the Slovak word inside the comment. Line 64 is `expect(byKind[0].name_sk).toBe('Mesačný preplatok');` — change to `expect(byKind[0].name_sk).toBe('Mesačná permanentka');`.

- [ ] **Step 4: Update `dashboard-button-layout.spec.ts:34`**

The regex form is `.filter({ hasText: /Monthly pass|Mesačný preplatok/ })`. Change to `.filter({ hasText: /Monthly pass|Mesačná permanentka/ })`. The English alternative is preserved.

- [ ] **Step 5: Update `log-visit-class-only.spec.ts:45`**

Same regex shape as Task 3 Step 4. Apply the same Slovak swap, keep `Monthly pass` alternative.

- [ ] **Step 6: Verify no leftover occurrences**

Run:
```bash
grep -rn "Mesačný preplatok" e2e/tests/ crates/ spinbike-ui/ 2>/dev/null
```
Expected: zero output. If any line shows up that is NOT the V8 const at `migrations.rs:252` (which we keep) or the V8-fixture comment, investigate. The V8 const itself MUST still print `Mesačný preplatok` — that's the seed value V14 then renames.

A grep for the canonical V8 seed should still show one match:
```bash
grep -n "Mesačný preplatok" crates/spinbike-server/src/db/migrations.rs
```
Expected: exactly one line (the V8 seed line ≈252) plus zero or one line in V14's WHERE clause depending on quoting.

- [ ] **Step 7: Commit**

```bash
git add e2e/tests/post-backfill-history.spec.ts \
        e2e/tests/visit-button-feedback.spec.ts \
        e2e/tests/services-admin.spec.ts \
        e2e/tests/dashboard-button-layout.spec.ts \
        e2e/tests/log-visit-class-only.spec.ts
git commit -m "$(cat <<'EOF'
test(e2e): update Slovak label literals to Mesačná permanentka (#50)

Match the V14 rename in DB migrations: 5 spec files, 6 string literals.
Two regex-based filters keep their English 'Monthly pass' alternative;
only the Slovak branch swaps to permanentka.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Push, monitor CI to terminal state, open PR (controller-run)

**Per `ci-monitoring.md`:** monitor with a single `sleep N && gh run view --json status,conclusion,jobs` background command. NO `/loop`, NO custom monitor scripts.

- [ ] Push: `git push origin dev`
- [ ] Monitor latest run: `gh run list --branch dev --limit 1` then `sleep 600 && gh run view <id> --json status,conclusion,jobs` in background. ALL jobs must reach terminal state (lint, test, build, E2E, mutation, deploy-dev, smoke-dev). Investigate any failure, fix root cause, push, re-monitor.
- [ ] Validate against prod-synced dev DB after deploy-dev:
  ```bash
  sqlite3 /var/lib/spinbike/dev.db "SELECT name_sk, name_en FROM services WHERE kind='monthly_pass'"
  ```
  Expected: `Mesačná permanentka|Monthly pass`
- [ ] Open PR `dev` → `main`. Per `pr-merge-policy.md`: never merge. Confirm `mergeable: true` AND `mergeable_state: "clean"` before sending the URL.

---

### Task 5: Post-deploy verification (controller-run, ONLY after user merges)

- [ ] Wait for explicit user "merge it". Then merge.
- [ ] Monitor main CI run + main deploy job to terminal state.
- [ ] On `https://spinbike.newlevel.media`: Playwright reads the version label, expects `v0.13.23`. Open Desk, find a customer with a pass-sale row in their card history (any user with a transaction whose `service_id` = monthly_pass), confirm the label cell reads `Mesačná permanentka` and the row title reads `Predaj permanentky · do <date>`.
- [ ] On `https://spinbike-dev.newlevel.media`: same checks after dev redeploys.
