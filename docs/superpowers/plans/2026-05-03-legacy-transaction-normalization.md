# Legacy Transaction Normalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Normalize ~88,000 legacy `transactions` rows from positive-magnitude + signed-by-action format (`debit`/`credit`/`activation`/`storno`) to the new signed-amount + neutral-action convention (`charge`/`topup`/`visit`) so per-card history, Reports activity feed, and KPIs work uniformly across all rows.

**Architecture:** One-time idempotent SQL migration (V12) inside `db::migrations`, run on server start. Plus defensive update to `migrate_legacy.rs::map_action` so future re-imports write the new convention directly. No changes to `classify()` (already correct for the new convention) and no changes to live SQL queries (they already target the new vocabulary). Verification happens against the prod-synced dev DB on every deploy-dev cycle.

**Tech Stack:** SQLite (sqlx 0.8), Axum 0.8 (server routes), Leptos 0.7 (UI not touched here), Playwright (E2E), cargo-mutants (mutation testing on PR diffs).

**Spec:** `docs/superpowers/specs/2026-05-03-legacy-transaction-normalization-design.md` (committed at 6b35981 on `dev`).

**Pre-flight notes (do not skip):**

- VERSION is **already bumped to 0.13.17** on dev (commit 0a37e1c). DO NOT add a version-bump task.
- Per project memory `feedback_no_git_add_A.md`: NEVER use `git add -A` or `git add .` — explicit paths only.
- Per project memory `feedback_subagent_no_local_build.md`: NEVER run `cargo test` / `cargo build` / `cargo clippy` / `trunk build` locally. CI is authoritative. The ONLY allowed local check is `cargo fmt --all --check`.
- Per ci-monitoring.md: monitor CI via single `sleep N && gh run view --json status,conclusion,jobs` background command; never with `gh run watch` and never via /loop or CronCreate.
- Per pr-merge-policy.md: never merge the PR. Plan ends at "PR mergeable, awaiting user merge".
- Issue #50 (Mesačný preplatok → Mesačná permanentka rename) is OUT OF SCOPE for this plan.

---

## File Structure (what each task touches)

| File | Owner task | Responsibility |
|---|---|---|
| `crates/spinbike-server/src/db/migrations.rs` | Task 1 | Append V12 migration entry + V12_NORMALIZE_LEGACY_ACTIONS const + unit tests |
| `crates/spinbike-server/src/bin/migrate_legacy.rs` | Task 2 | Refactor `map_action` → `map_legacy` returning `MappedTxn { action, amount }`; update call site at line 292; update tests |
| `crates/spinbike-server/src/routes/test_fixtures.rs` | Task 3 | Add optional `valid_until` field to `SeedEntry` and the INSERT |
| `e2e/tests/post-backfill-history.spec.ts` (NEW) | Task 3 | Playwright test asserting per-card list renders all four EventKind labels |

No other files need changes. The classifier (`crates/spinbike-core/src/reports.rs`) and SQL queries (`crates/spinbike-server/src/db/reports.rs`) are untouched — they already target the new vocabulary.

---

## Task 1: Add idempotent SQL backfill migration V12

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs:1-46` (MIGRATIONS array — append entry)
- Modify: `crates/spinbike-server/src/db/migrations.rs` (after V11 const declaration around line 275-317 — append `V12_NORMALIZE_LEGACY_ACTIONS` const)
- Modify: `crates/spinbike-server/src/db/migrations.rs` `mod tests` (append two new tests)

### Context

The migration runner at `crates/spinbike-server/src/db/mod.rs:78-153` reads the `MIGRATIONS` array, runs each pending migration inside a single transaction (it splits the SQL string by `;` and executes each statement individually), and records the version in the `schema_version` table. Since the runner already wraps each migration in a transaction, the V12 SQL must NOT include `BEGIN;` / `COMMIT;`.

The current max version in `MIGRATIONS` is **11** (last entry `V11_TRANSACTIONS_NOTE_CHECK`). The new entry is version **12**.

The pre-flight pattern for migration tests in this file (see `v4_is_idempotent` at line 358 and `v6_is_idempotent_and_does_not_duplicate` at line 460): create a fresh memory pool, run `run_migrations` once, run it again, assert the resulting state. For V12 we additionally need to seed legacy-shape rows BEFORE the migration mutates them — done by deleting the V12 row from `schema_version` (so the runner re-runs V12 on the next call) after seeding.

### Steps

- [ ] **Step 1: Add V12 entry to the MIGRATIONS array**

In `crates/spinbike-server/src/db/migrations.rs:41-46`, add an entry after V11:

```rust
    (
        11,
        "transactions: note length CHECK",
        V11_TRANSACTIONS_NOTE_CHECK,
    ),
    (
        12,
        "transactions: normalize legacy actions to new convention",
        V12_NORMALIZE_LEGACY_ACTIONS,
    ),
];
```

- [ ] **Step 2: Add the V12 SQL constant**

After the `V11_TRANSACTIONS_NOTE_CHECK` const declaration (around line 317, just before `#[cfg(test)]` at line 319), append:

```rust
// Normalize legacy positive-magnitude + signed-by-action transaction rows to
// the new signed-amount + neutral-action convention used by spinbike_core::
// reports::classify. Pre-rewrite, the MS Access importer wrote action='debit'
// (positive amount) for spends and action='credit'/'activation' (positive
// amount) for top-ups. The classifier only knows 'charge' (negative) /
// 'topup' (positive) / 'visit' (zero), so legacy rows mis-rendered as TopUp
// regardless of whether they were debits or credits. This migration mutates
// every legacy row to the new vocabulary; subsequent runs are no-ops because
// the action-name guards no longer match anything.
//
// Each statement is independently idempotent — re-running this migration
// finds zero matching rows after the first successful pass.
//
// The runner at db::mod runs every migration inside a single tx, so BEGIN/
// COMMIT are intentionally omitted here.
const V12_NORMALIZE_LEGACY_ACTIONS: &str = r#"
UPDATE transactions SET action='charge', amount = -amount
  WHERE action='debit' AND amount > 0;

UPDATE transactions SET action='visit'
  WHERE action='debit' AND amount = 0 AND valid_until IS NULL;

UPDATE transactions SET action='charge'
  WHERE action='debit' AND amount = 0 AND valid_until IS NOT NULL;

UPDATE transactions SET action='charge'
  WHERE action='credit' AND amount < 0;

UPDATE transactions SET action='topup'
  WHERE action='credit';

UPDATE transactions SET action='topup'
  WHERE action='activation';

UPDATE transactions SET action='topup'
  WHERE action='storno' AND amount > 0;
"#;
```

- [ ] **Step 3: Add the seven-pattern unit test**

Append to the `#[cfg(test)] mod tests { ... }` block in `crates/spinbike-server/src/db/migrations.rs` (the closing brace of the module is at the end of the file — insert before it):

```rust
    #[tokio::test]
    async fn v12_normalizes_every_legacy_pattern() {
        use crate::db::{create_memory_pool, run_migrations};
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed one row of every pattern from the spec mutation table.
        // Insert raw legacy-shape rows post-migration, then force V12 to
        // re-run by clearing its schema_version entry.
        sqlx::query(
            "INSERT INTO transactions (id, action, amount, valid_until) VALUES
               (1001, 'debit',      3.0,  NULL),
               (1002, 'debit',      0.0,  NULL),
               (1003, 'debit',      0.0,  '2026-12-31'),
               (1004, 'credit',     2.0,  NULL),
               (1005, 'credit',     0.0,  NULL),
               (1006, 'credit',    -30.0, NULL),
               (1007, 'activation', 30.0, NULL),
               (1008, 'storno',     2.5,  NULL),
               (1009, 'storno',     0.0,  NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Force V12 to re-run.
        sqlx::query("DELETE FROM schema_version WHERE version = 12")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();

        let rows: Vec<(i64, String, f64)> = sqlx::query_as(
            "SELECT id, action, amount FROM transactions
             WHERE id BETWEEN 1001 AND 1009
             ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let expected: Vec<(i64, &str, f64)> = vec![
            (1001, "charge", -3.0),  // debit > 0 → charge, negated
            (1002, "visit",   0.0),  // debit = 0, no valid_until → visit
            (1003, "charge",  0.0),  // debit = 0, valid_until set → charge
            (1004, "topup",   2.0),  // credit > 0 → topup
            (1005, "topup",   0.0),  // credit = 0 → topup
            (1006, "charge", -30.0), // credit < 0 → charge (already negative)
            (1007, "topup",  30.0),  // activation → topup
            (1008, "topup",   2.5),  // storno > 0 → topup
            (1009, "storno",  0.0),  // storno = 0 → unchanged
        ];

        assert_eq!(rows.len(), expected.len(), "all 9 rows must survive");
        for ((id, action, amount), (eid, eaction, eamount)) in
            rows.iter().zip(expected.iter())
        {
            assert_eq!(id, eid, "row id mismatch");
            assert_eq!(action, eaction, "row {id}: action mismatch");
            assert!(
                (amount - eamount).abs() < 1e-9,
                "row {id}: amount {amount} != {eamount}"
            );
        }
    }
```

- [ ] **Step 4: Add the idempotency test**

Append the second test:

```rust
    #[tokio::test]
    async fn v12_is_idempotent() {
        use crate::db::{create_memory_pool, run_migrations};
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO transactions (id, action, amount) VALUES
               (2001, 'debit',  3.0),
               (2002, 'credit', 5.0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // First run: mutate.
        sqlx::query("DELETE FROM schema_version WHERE version = 12")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();

        let after_first: Vec<(i64, String, f64)> = sqlx::query_as(
            "SELECT id, action, amount FROM transactions
             WHERE id IN (2001, 2002) ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        // Second run: no-op (no rows match the legacy guards anymore).
        sqlx::query("DELETE FROM schema_version WHERE version = 12")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();

        let after_second: Vec<(i64, String, f64)> = sqlx::query_as(
            "SELECT id, action, amount FROM transactions
             WHERE id IN (2001, 2002) ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(
            after_first, after_second,
            "second V12 run must leave rows unchanged (idempotency)"
        );
        // Sanity: state is the post-backfill shape.
        assert_eq!(after_first[0].1, "charge");
        assert!((after_first[0].2 - (-3.0)).abs() < 1e-9);
        assert_eq!(after_first[1].1, "topup");
        assert!((after_first[1].2 - 5.0).abs() < 1e-9);
    }
```

- [ ] **Step 5: Local format check**

```bash
cargo fmt --all --check
```

Expected: no output (clean). If fmt complains, run `cargo fmt --all` to fix, then re-run the check.

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): V12 migration normalizes legacy transaction actions

Rewrites legacy positive-magnitude + signed-by-action rows
(debit/credit/activation/storno) into the new signed-amount +
neutral-action convention (charge/topup/visit). Idempotent — runs
inside the existing migration tx framework on server start; subsequent
runs find zero matching rows.

Per spec docs/superpowers/specs/2026-05-03-legacy-transaction-normalization-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Update `migrate_legacy.rs::map_action` to write new convention

**Files:**
- Modify: `crates/spinbike-server/src/bin/migrate_legacy.rs:107-120` (replace `map_action` with `map_legacy`)
- Modify: `crates/spinbike-server/src/bin/migrate_legacy.rs:286-322` (call site)
- Modify: `crates/spinbike-server/src/bin/migrate_legacy.rs:421-535` (`#[cfg(test)] mod tests` — replace with new test cases)

### Context

`migrate_legacy.rs::map_action` currently returns `Option<&'static str>` and writes legacy action labels (`debit`/`credit`/`activation`/`storno`/`unknown`). The amount is passed through unchanged at the call site. We refactor so the function returns a struct that also carries the amount transformation — mirroring the V12 mutation table — and so future re-imports skip the V12 backfill mismatch entirely.

Because the function now needs to know whether the row has a `valid_until` (to distinguish `Debet` zero-amount free passes from class-attendance visits), the call site at line 292 must pass `valid_until.is_some()` in addition to the amount.

The existing `Some("unknown")` fallback for unrecognized legacy actions is replaced with a positive-amount `topup` fallback (the safest choice — surfaces in the UI as a TopUp instead of Other, and the warn! log keeps the audit trail).

### Steps

- [ ] **Step 1: Replace `map_action` with `MappedTxn` struct + `map_legacy` function**

In `crates/spinbike-server/src/bin/migrate_legacy.rs`, replace lines 105-120 (the existing `/// Map a legacy action ...` doc comment plus `fn map_action`) with:

```rust
/// New-convention mapping output for a single legacy row. The migrator writes
/// `action` and `amount` directly; downstream consumers (classifier, SQL
/// filters) treat these rows identically to live writes from the new server.
#[derive(Debug, PartialEq)]
pub(crate) struct MappedTxn {
    pub action: &'static str,
    pub amount: f64,
}

/// Map a legacy action + amount + valid_until-presence into the new
/// signed-amount + neutral-action convention used everywhere in the rewrite
/// (charge / topup / visit / storno). Returns None for actions that should
/// not produce a transaction row (e.g., BLOKOVANA — handled by setting
/// card.blocked = true at the call site).
///
/// Mirrors the V12 schema migration table — re-imports via this function
/// produce rows that V12 would already consider new-convention, so V12 is a
/// no-op on freshly imported data.
fn map_legacy(action: &str, amount: f64, has_valid_until: bool) -> Option<MappedTxn> {
    let mapped = match action.trim().trim_matches('"') {
        "Debet" | "Vstup" => {
            if amount == 0.0 && !has_valid_until {
                MappedTxn { action: "visit", amount: 0.0 }
            } else {
                // A real debit / paid visit / pass purchase — flip sign so
                // amount < 0 (or amount = 0 for free pass purchases when
                // has_valid_until is true).
                MappedTxn { action: "charge", amount: -amount.abs() }
            }
        }
        "Kredit" | "Novy kredit" | "AKTIVACIA" => {
            MappedTxn { action: "topup", amount: amount.abs() }
        }
        "Storno" if amount > 0.0 => MappedTxn { action: "topup", amount },
        "Storno" => MappedTxn { action: "storno", amount },
        "BLOKOVANA" => return None,
        other => {
            warn!(
                "Unknown legacy action: '{other}', mapping to 'topup' \
                 with positive amount as fallback"
            );
            MappedTxn { action: "topup", amount: amount.abs() }
        }
    };
    Some(mapped)
}
```

- [ ] **Step 2: Update the call site to use `map_legacy`**

In the same file, at the call site (around line 292), replace:

```rust
        match map_action(action) {
            None => {
                // BLOKOVANA — mark card as blocked.
                if let Some(card_id) = new_card_id {
                    blocked_cards.push(card_id);
                }
                skipped_count += 1;
            }
            Some(mapped_action) => {
                // Format the legacy date for created_at.
                // Legacy format: "MM/DD/YY HH:MM:SS" — store as-is since SQLite is flexible.
                sqlx::query(
                    "INSERT INTO transactions (card_id, amount, action, created_at, service_id, valid_until)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(new_card_id)
                .bind(amount_eur)
                .bind(mapped_action)
                .bind(date)
                .bind(service_id)
                .bind(valid_until)
                .execute(&pool)
                .await
                .with_context(|| {
                    format!(
                        "Failed to insert transaction: card={legacy_card_id}, action={action}"
                    )
                })?;

                txn_count += 1;
            }
        }
```

with:

```rust
        match map_legacy(action, amount_eur, valid_until.is_some()) {
            None => {
                // BLOKOVANA — mark card as blocked.
                if let Some(card_id) = new_card_id {
                    blocked_cards.push(card_id);
                }
                skipped_count += 1;
            }
            Some(mapped) => {
                // Format the legacy date for created_at.
                // Legacy format: "MM/DD/YY HH:MM:SS" — store as-is since SQLite is flexible.
                sqlx::query(
                    "INSERT INTO transactions (card_id, amount, action, created_at, service_id, valid_until)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(new_card_id)
                .bind(mapped.amount)
                .bind(mapped.action)
                .bind(date)
                .bind(service_id)
                .bind(valid_until)
                .execute(&pool)
                .await
                .with_context(|| {
                    format!(
                        "Failed to insert transaction: card={legacy_card_id}, action={action}"
                    )
                })?;

                txn_count += 1;
            }
        }
```

- [ ] **Step 3: Add unit tests for `map_legacy` at the top of `mod tests`**

In the `#[cfg(test)] mod tests { ... }` block (currently around line 421-535), add a new test BEFORE the existing `parse_end_date_valid` test. This sits next to the existing tests, doesn't touch them. Insert just after `use super::*;`:

```rust
    #[test]
    fn map_legacy_debet_positive_amount_becomes_negative_charge() {
        assert_eq!(
            map_legacy("Debet", 3.0, false),
            Some(MappedTxn { action: "charge", amount: -3.0 })
        );
    }

    #[test]
    fn map_legacy_debet_zero_no_valid_until_becomes_visit() {
        assert_eq!(
            map_legacy("Debet", 0.0, false),
            Some(MappedTxn { action: "visit", amount: 0.0 })
        );
    }

    #[test]
    fn map_legacy_debet_zero_with_valid_until_becomes_zero_charge() {
        assert_eq!(
            map_legacy("Debet", 0.0, true),
            Some(MappedTxn { action: "charge", amount: 0.0 })
        );
    }

    #[test]
    fn map_legacy_debet_with_valid_until_becomes_negative_charge_pass_sale() {
        assert_eq!(
            map_legacy("Debet", 28.0, true),
            Some(MappedTxn { action: "charge", amount: -28.0 })
        );
    }

    #[test]
    fn map_legacy_vstup_positive_amount_becomes_negative_charge() {
        assert_eq!(
            map_legacy("Vstup", 2.5, false),
            Some(MappedTxn { action: "charge", amount: -2.5 })
        );
    }

    #[test]
    fn map_legacy_vstup_zero_no_valid_until_becomes_visit() {
        assert_eq!(
            map_legacy("Vstup", 0.0, false),
            Some(MappedTxn { action: "visit", amount: 0.0 })
        );
    }

    #[test]
    fn map_legacy_kredit_becomes_topup() {
        assert_eq!(
            map_legacy("Kredit", 30.0, false),
            Some(MappedTxn { action: "topup", amount: 30.0 })
        );
    }

    #[test]
    fn map_legacy_novy_kredit_becomes_topup() {
        assert_eq!(
            map_legacy("Novy kredit", 30.0, false),
            Some(MappedTxn { action: "topup", amount: 30.0 })
        );
    }

    #[test]
    fn map_legacy_aktivacia_becomes_topup() {
        assert_eq!(
            map_legacy("AKTIVACIA", 30.0, false),
            Some(MappedTxn { action: "topup", amount: 30.0 })
        );
    }

    #[test]
    fn map_legacy_storno_positive_becomes_topup() {
        assert_eq!(
            map_legacy("Storno", 2.5, false),
            Some(MappedTxn { action: "topup", amount: 2.5 })
        );
    }

    #[test]
    fn map_legacy_storno_zero_stays_storno() {
        assert_eq!(
            map_legacy("Storno", 0.0, false),
            Some(MappedTxn { action: "storno", amount: 0.0 })
        );
    }

    #[test]
    fn map_legacy_blokovana_returns_none() {
        assert_eq!(map_legacy("BLOKOVANA", 0.0, false), None);
    }

    #[test]
    fn map_legacy_unknown_falls_back_to_positive_topup() {
        assert_eq!(
            map_legacy("MysteryAction", 5.0, false),
            Some(MappedTxn { action: "topup", amount: 5.0 })
        );
    }

    #[test]
    fn map_legacy_strips_quotes_and_whitespace() {
        assert_eq!(
            map_legacy("  \"Debet\"  ", 3.0, false),
            Some(MappedTxn { action: "charge", amount: -3.0 })
        );
    }
```

- [ ] **Step 4: Local format check**

```bash
cargo fmt --all --check
```

Expected: no output. If fmt complains, run `cargo fmt --all` to fix.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/bin/migrate_legacy.rs
git commit -m "feat(import): migrate-legacy writes new-convention rows directly

Refactors map_action -> map_legacy returning MappedTxn { action, amount }.
Debet/Vstup get amount sign-flipped to charge; Kredit/Novy kredit/AKTIVACIA
become topup; Storno splits on amount sign; zero-amount Debet/Vstup without
valid_until become visit. Future imports produce rows that V12 considers
already-normalized (V12 becomes a no-op on fresh imports).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Extend `seed-transactions` test endpoint and add post-backfill E2E

**Files:**
- Modify: `crates/spinbike-server/src/routes/test_fixtures.rs:20-25` (add `valid_until` to `SeedEntry`)
- Modify: `crates/spinbike-server/src/routes/test_fixtures.rs:99-109` (extend INSERT to bind `valid_until`)
- Create: `e2e/tests/post-backfill-history.spec.ts`

### Context

The existing test endpoint at `crates/spinbike-server/src/routes/test_fixtures.rs:71-113` (`POST /api/test/seed-transactions`) accepts `(amount, action, service_name_sk)` per entry but always inserts `valid_until = NULL`. To E2E-test PassSale rendering we need to seed a row with `valid_until` set. Adding an optional field is backward-compatible — existing E2E tests that don't send `valid_until` keep working unchanged.

The English event labels (from `spinbike-ui/src/i18n.rs:499-502` and rendered by `spinbike-ui/src/pages/dashboard/transactions_list.rs:57-63`) are:

- `EventKind::TopUp`    → `"Top-up"`
- `EventKind::Charge`   → `"Spent from credit"`
- `EventKind::Visit`    → `"Entry with pass"`
- `EventKind::PassSale` → `"Sale of pass"`

`loginViaAPI` in `e2e/tests/helpers.ts` forces English via `setEnglishLanguage`, so the test asserts on the English strings.

The transactions list container is `[data-testid="transactions-list"]` (`spinbike-ui/src/pages/dashboard/transactions_list.rs:224`).

### Steps

- [ ] **Step 1: Extend `SeedEntry` to accept optional `valid_until`**

In `crates/spinbike-server/src/routes/test_fixtures.rs`, replace the existing `SeedEntry` struct (lines 20-25):

```rust
#[derive(Deserialize)]
pub struct SeedEntry {
    pub amount: f64,
    pub action: String,
    pub service_name_sk: String,
}
```

with:

```rust
#[derive(Deserialize)]
pub struct SeedEntry {
    pub amount: f64,
    pub action: String,
    pub service_name_sk: String,
    /// Optional pass-sale expiry. None for normal transactions; Some(date)
    /// when the seeded row should classify as PassSale. The serde default
    /// keeps existing E2E callers source-compatible.
    #[serde(default)]
    pub valid_until: Option<chrono::NaiveDate>,
}
```

- [ ] **Step 2: Update the INSERT to bind `valid_until`**

In the same file (around lines 99-109), replace:

```rust
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, legacy_backfilled, created_at)
             VALUES (?, ?, ?, ?, 1, datetime('now'))",
        )
        .bind(card_id)
        .bind(svc_id)
        .bind(e.amount)
        .bind(&e.action)
        .execute(&state.pool)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
```

with:

```rust
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, valid_until, legacy_backfilled, created_at)
             VALUES (?, ?, ?, ?, ?, 1, datetime('now'))",
        )
        .bind(card_id)
        .bind(svc_id)
        .bind(e.amount)
        .bind(&e.action)
        .bind(e.valid_until)
        .execute(&state.pool)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
```

- [ ] **Step 3: Create the E2E test file**

Create `e2e/tests/post-backfill-history.spec.ts` with:

```typescript
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// After the V12 backfill migration, a legacy card's history must classify
// rows by EventKind correctly. We seed a card with one row of each post-
// backfill action shape and assert the per-card transactions list renders
// each EventKind's English label.
test('per-card history renders charge / topup / visit / pass-sale labels', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    const barcode = `PBH-${Date.now()}`;

    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [
                // TopUp: positive amount, action=topup
                { amount: 10.0, action: 'topup', service_name_sk: 'Občerstvenie' },
                // Charge: negative amount, action=charge, no valid_until
                { amount: -3.0, action: 'charge', service_name_sk: 'Spinning' },
                // Visit: zero amount, action=visit
                { amount: 0.0, action: 'visit', service_name_sk: 'Spinning' },
                // PassSale: any amount with valid_until set wins precedence
                { amount: -35.0, action: 'charge', service_name_sk: 'Mesačný preplatok', valid_until: '2099-12-31' },
            ],
        }),
    });
    if (!seed.ok) throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);

    // Open the card via search.
    await page.goto('/staff');
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(barcode, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    // English labels (loginViaAPI forces English):
    //   EventKind::TopUp    → "Top-up"
    //   EventKind::Charge   → "Spent from credit"
    //   EventKind::Visit    → "Entry with pass"
    //   EventKind::PassSale → "Sale of pass"
    const list = page.locator('[data-testid="transactions-list"]');
    await expect(list).toBeVisible();
    await expect(list).toContainText('Top-up');
    await expect(list).toContainText('Spent from credit');
    await expect(list).toContainText('Entry with pass');
    await expect(list).toContainText('Sale of pass');

    assertCleanConsole(msgs);
});
```

- [ ] **Step 4: Local format check**

```bash
cargo fmt --all --check
```

Expected: no output. If fmt complains, run `cargo fmt --all` to fix.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/routes/test_fixtures.rs e2e/tests/post-backfill-history.spec.ts
git commit -m "test(e2e): per-card history renders all four EventKind labels

Extends /api/test/seed-transactions with optional valid_until to allow
seeding pass-sale rows. Adds post-backfill-history.spec.ts asserting
all four English event labels (Top-up / Spent from credit / Entry with
pass / Sale of pass) render for a card with one row of each kind.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Push, monitor CI to terminal state, mitigate surviving mutants, open PR

**Controller-level checkpoint — NOT a subagent dispatch.** The orchestrating agent (the controller running this plan) executes these steps directly.

### Steps

- [ ] **Step 1: Confirm clean local state**

```bash
git status
git log --oneline origin/main..HEAD
```

Expected: working tree clean. The log should show (oldest → newest):
1. `chore: bump version to 0.13.17` (already on dev)
2. `docs(spec): legacy transaction normalization` (already on dev)
3. `feat(db): V12 migration normalizes legacy transaction actions` (Task 1)
4. `feat(import): migrate-legacy writes new-convention rows directly` (Task 2)
5. `test(e2e): per-card history renders all four EventKind labels` (Task 3)

- [ ] **Step 2: Push**

```bash
git push origin dev
```

- [ ] **Step 3: Identify the run**

```bash
sleep 5 && gh run list --branch dev --limit 3 --json databaseId,status,conclusion,headSha
```

Pick the latest run with `headSha` matching `git rev-parse HEAD`. Save the `databaseId` as `<RUN_ID>`.

- [ ] **Step 4: Monitor to terminal state**

Use ONE background command per ci-monitoring.md (single sleep+view, no /loop, no CronCreate, no custom poll script):

```bash
sleep 360 && gh run view <RUN_ID> --json status,conclusion,jobs
```

Run with `run_in_background: true`. Read the result via BashOutput when it completes. Repeat with longer sleeps if `status != "completed"`.

ALL these jobs must end with `conclusion=success` (or `skipped` if not applicable to dev pushes, e.g. Deploy (prod) / Smoke (prod)):

- Test Integrity
- Lint
- Test
- Test (UI)
- Build WASM (UI)
- E2E Tests
- Mutation Testing
- Deploy (dev)
- Smoke (dev)

If a job fails: `gh run view <RUN_ID> --log-failed` → investigate → fix the root cause in a NEW commit (do NOT --amend) → push → identify the new run → monitor again.

- [ ] **Step 5: If Mutation Testing fails on surviving mutants — mitigate**

Read the failed log:

```bash
gh run view <RUN_ID> --log --job=<MUTATION_JOB_ID> | grep -E "MISSED|UNVIABLE|surviving"
```

For each surviving mutant in the diff:
- Identify which line of new code (V12 SQL UPDATE statement, `map_legacy` arm, `seed-transactions` field) the mutation touched.
- Add a stronger assertion to the corresponding test that would have failed under the mutation.
  - Example: if cargo-mutants flips `amount > 0` to `amount >= 0` in a UPDATE guard, add a test row with `amount = 0` whose post-migration state proves the guard's strictness.
  - Example: if cargo-mutants negates `amount.abs()` to `amount`, the existing tests on negative inputs catch it; if they don't, add a `Debet` test with a negative amount.
- Commit the test strengthening with a clear message: `test: kill mutation in <function/area>`
- Push, monitor again.

Repeat until Mutation Testing reports zero surviving mutants (job conclusion = success).

- [ ] **Step 6: Open PR**

Once ALL jobs are green:

```bash
gh pr create --base main --head dev \
  --title "v0.13.17: normalize legacy transaction actions" \
  --body "$(cat <<'EOF'
## Summary

- Adds idempotent V12 migration that mutates ~88,000 legacy `transactions` rows from positive-magnitude + signed-by-action format (`debit`/`credit`/`activation`/`storno`) to the new signed-amount + neutral-action convention (`charge`/`topup`/`visit`).
- Refactors `migrate-legacy` so future re-imports write the new convention directly (V12 becomes a no-op on fresh imports).
- Per-card transaction history now renders correctly classified pills across the entire dataset; Reports activity feed and KPIs uniformly include legacy rows that were silently filtered out before.
- Card balances are unchanged — `cards.credit` is a separately stored column, not derived from transactions.

## Test plan

- [x] `db::migrations` unit tests cover every pattern in the V12 mutation table (including idempotency)
- [x] `migrate_legacy::map_legacy` unit tests cover every legacy-action mapping (Debet/Vstup zero-vs-nonzero, with-and-without valid_until, Kredit/AKTIVACIA, Storno split, BLOKOVANA, unknown fallback, quote-stripping)
- [x] Playwright `post-backfill-history.spec.ts` asserts all four EventKind English labels render on a per-card list
- [x] CI green (Test Integrity, Lint, Test, Test (UI), Build WASM (UI), E2E Tests, Mutation Testing, Deploy (dev), Smoke (dev))
- [ ] Post-deploy verification on dev frontend (open `/staff?card=70701712` and `/staff?card=70701050`, confirm history shows mix of Charge + TopUp pills, balances unchanged at 6.40 / 0.60)
- [ ] Post-deploy verification on prod frontend (same checks after main deploy)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Save the PR URL.

- [ ] **Step 7: Verify PR mergeable + clean**

```bash
gh pr view <PR_NUMBER> --json mergeable,mergeStateStatus
```

Required: `mergeable: "MERGEABLE"` AND `mergeStateStatus: "CLEAN"`.

- [ ] **Step 8: Send completion report and STOP — gate at "merge it"**

Per pr-merge-policy.md: never merge without explicit user instruction. Report the green PR URL using the completion-report template. STOP and wait until the user explicitly types "merge it" (or equivalent: "approved", "go ahead and merge"). Do NOT proceed to Task 5 / Task 6 on any other input.

---

## Task 5: Pre-merge prod backup + merge (controller checkpoint, runs after "merge it")

**Controller-level checkpoint.** Runs only after the user explicitly says "merge it" (or equivalent). The migration mutates ~88k rows; backup is the only rollback path. The merge itself is the controller's action — `gh pr merge` after the user's explicit instruction is in policy.

### Steps

- [ ] **Step 1: Snapshot prod DB to a timestamped backup**

```bash
cp /opt/spinbike/prod/spinbike.db /opt/spinbike/prod/spinbike.db.bak-pre-normalize-v0.13.17
ls -la /opt/spinbike/prod/spinbike.db /opt/spinbike/prod/spinbike.db.bak-pre-normalize-v0.13.17
```

Expected: both files present, identical size.

- [ ] **Step 2: Sanity-check the backup**

```bash
sqlite3 /opt/spinbike/prod/spinbike.db.bak-pre-normalize-v0.13.17 \
  "SELECT action, COUNT(*) FROM transactions GROUP BY action ORDER BY COUNT(*) DESC;"
```

Expected: shows the legacy distribution (`debit ~78000`, `credit ~9376`, `activation 568`, `charge ~261`, `visit 149`, `storno 72`, `topup 77`).

- [ ] **Step 3: Merge the PR**

```bash
gh pr merge <PR_NUMBER> --merge
```

(Standard merge commit per project policy — no squash, no rebase.)

---

## Task 6: Post-deploy verification (controller checkpoint, runs after user merges)

**Controller-level checkpoint.** After the merge to main triggers the prod deploy workflow, verify the migration ran correctly and the user-visible outcome matches the spec.

### Steps

- [ ] **Step 1: Wait for main CI + deploy to complete**

```bash
sleep 5 && gh run list --branch main --limit 3 --json databaseId,status,conclusion
```

Pick the latest run on main, then poll its terminal state via the standard `sleep N && gh run view <RUN_ID> --json status,conclusion,jobs` background pattern.

Required terminal jobs: Deploy (prod) and Smoke (prod) both `success`.

- [ ] **Step 2: Verify deployed version on dev frontend**

Open `https://spinbike-dev.newlevel.media/staff` in Playwright (dev syncs prod DB on every deploy). Read `[data-testid="version"]`:

```javascript
const version = await page.locator('[data-testid="version"]').textContent();
// Expect: v0.13.17
```

- [ ] **Step 3: Verify dev card 70701712 history**

Open `https://spinbike-dev.newlevel.media/staff?card=70701712` in Playwright. Assertions:

- Action panel visible (card opened directly, no dropdown).
- Card balance reads `6.40` (unchanged from before).
- Transactions list visible.
- List contains "Spent from credit" (post-backfill Charge label) — at least one occurrence.
- List contains "Top-up" (post-backfill TopUp label) — at least one occurrence.
- Zero browser console errors.

- [ ] **Step 4: Verify dev card 70701050 history**

Open `https://spinbike-dev.newlevel.media/staff?card=70701050` in Playwright. Same assertion shape as Step 3, with balance `0.60`.

- [ ] **Step 5: Verify card balances unchanged on prod via SQL**

```bash
sqlite3 /opt/spinbike/prod/spinbike.db \
  "SELECT barcode, ROUND(credit,2) FROM cards WHERE barcode IN ('70701712','70701050') ORDER BY barcode;"
```

Required output:

```
70701050|0.6
70701712|6.4
```

- [ ] **Step 6: Verify deployed version on prod frontend**

Open `https://spinbike.newlevel.media/staff` in Playwright. Read `[data-testid="version"]` → expect `v0.13.17`. Repeat the card-history assertions from Steps 3 and 4 on prod URLs:

- `https://spinbike.newlevel.media/staff?card=70701712` (balance 6.40, mixed pill kinds)
- `https://spinbike.newlevel.media/staff?card=70701050` (balance 0.60, mixed pill kinds)

- [ ] **Step 7: Spot-check legacy day on Reports**

Open `https://spinbike.newlevel.media/reports` in Playwright. Use the date picker to navigate to `2018-06-13` (a known-active legacy day on card 70701050). Assert:

- The activity feed renders ≥ 1 row (proving legacy rows now appear — they were silently filtered out before V12 because the SQL clause `action='charge' AND amount<0` excluded `action='debit'` rows).
- KPI tiles for that day show non-zero `revenue_eur` (was 0 before V12).
- Zero browser console errors.

If the date picker is unreliable, navigate via direct URL parameter if the route accepts one (check `spinbike-ui/src/pages/reports/mod.rs` for the parameter name); otherwise use the picker.

- [ ] **Step 8: Final completion report**

Send the post-merge completion report using the completion-report template, with both 🌐 lines (dev + prod), the prod card-balance SQL evidence in the `✅ Deploy:` line, and the merged PR URL.
