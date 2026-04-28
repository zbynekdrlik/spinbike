# Reports — Fix NAVSTEVY/ATTENDANCE Visit Count Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the reports-page `NAVSTEVY` / `ATTENDANCE` KPI so it counts only `(Fitness | Spinning)` AND `(paid charge | logged €0 visit)`, instead of "any paid non-pass transaction" which today wrongly includes Refreshments / Supplements / Card-activation-fee and wrongly excludes free pass-visit rows.

**Architecture:** Backend-only fix in `crates/spinbike-server/src/db/reports.rs`. Two SQL CASE expressions get a service-id subquery filter and the `OR action='visit'` second leg. No schema, no migration, no API contract change. Playwright E2E and Rust unit-test coverage added.

**Tech Stack:** Rust + sqlx + SQLite (server), Leptos 0.7 CSR (consumer), Playwright E2E (TypeScript), GitHub Actions CI.

**Spec:** `docs/superpowers/specs/2026-04-28-reports-visit-count-design.md`

**Issue:** [#23](https://github.com/zbynekdrlik/spinbike/issues/23)

---

## Pre-flight notes for the implementer

- **This work bundles into the existing OPEN PR #25** (currently v0.13.7, contains #13 button layout + #17 no-predefined-prices + CI diagnostics). Same precedent as the #17 bundling. Pushing to `dev` retriggers PR #25's CI — that is expected. After Tasks 2-4 build the fix, Task 5 pushes once.
- The repo already has the spec for #23 committed locally as `6ef1a37` and not yet pushed. It will go up with the next push (Task 5).
- Do NOT merge PR #25 first. Do NOT sync `dev` with `main`. The work continues on the same `dev` branch already ahead of `origin/dev` and folds into PR #25's diff.
- Do NOT run `cargo build`, `cargo test`, `cargo clippy`, or `trunk build` locally. CI is authoritative. The only allowed local check is `cargo fmt --all --check`.
- After every push, monitor CI to terminal state per `ci-monitoring.md` — single `sleep N && gh run view --json status,conclusion,jobs` background command, no scheduled polling.
- E2E flake #24 (`SQLITE_BUSY`) is a known issue. Per `ci-monitoring.md`, ONE rerun is acceptable to rule out a transient. Two failures of the same E2E test on the same commit means the bug is real — escalate.
- Frequent commits per task. No `--amend`. No history rewrite.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `VERSION` | Single source of truth for project version | Bump from `0.13.7` (PR #25's current scope after #17 bundled) to `0.13.8` |
| `Cargo.toml`, `spinbike-ui/Cargo.toml` | Per-crate version | Synced via `scripts/sync-version.sh` (workspace member crates use `version.workspace = true`, only root + UI need bumping) |
| `crates/spinbike-server/src/db/reports.rs` (~line 121) | `day_report` KPI aggregation | Replace the `attendance` CASE expression with the service+action filter |
| `crates/spinbike-server/src/db/reports.rs` (~line 233) | `range_report` KPI aggregation | Same replacement, identical CASE expression |
| `crates/spinbike-server/src/db/reports.rs` (`#[cfg(test)] mod tests`, line 592+) | New unit test that seeds discriminating fixture and asserts attendance | Adds one async test using `create_memory_pool` + `run_migrations` |
| `e2e/tests/reports-attendance.spec.ts` | NEW Playwright E2E | Logs in as admin, seeds mixed transactions via API, navigates to `/reports`, asserts `[data-testid="kpi-attendance"]` value, zero console errors |

No frontend changes. No schema or migration changes. No new dependencies.

---

## Task 1: Bump VERSION to 0.13.8 (bundled scope of PR #25)

**Files:**
- Read: `VERSION`
- Modify: `VERSION`, `Cargo.toml`, `spinbike-ui/Cargo.toml` (the last two via `scripts/sync-version.sh`; workspace member crates use `version.workspace = true` and don't need editing)

- [ ] **Step 1: Confirm we're on dev and ahead of origin/dev**

```bash
git rev-parse --abbrev-ref HEAD
git log --oneline origin/dev..HEAD
```

Expected: branch is `dev`. The unpushed local commits are exactly the spec for #23 (`6ef1a37 docs(spec): fix NAVSTEVY/ATTENDANCE visit-count KPI (#23)`). Nothing else.

If anything else appears, STOP and report BLOCKED — there's drift.

- [ ] **Step 2: Read current VERSION**

```bash
cat VERSION
```

Expected: `0.13.7` (the version PR #25 currently ships after #17 bundled in). If anything else, STOP and report BLOCKED.

- [ ] **Step 3: Bump VERSION to 0.13.8**

```bash
echo "0.13.8" > VERSION
```

This bumps PR #25's scope from v0.13.7 (button layout + no predefined prices) to v0.13.8 (+ visit-count fix).

- [ ] **Step 4: Run sync-version.sh to propagate to all Cargo.toml files**

```bash
scripts/sync-version.sh
```

Expected: no errors. The script edits `Cargo.toml` (root) and `spinbike-ui/Cargo.toml` to set `version = "0.13.8"`. Workspace member crates (`crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`) inherit via `version.workspace = true` and don't get edited.

- [ ] **Step 5: Verify the sync**

```bash
grep '^version' Cargo.toml spinbike-ui/Cargo.toml
```

Expected: both lines show `version = "0.13.8"`. If different, STOP and report BLOCKED.

- [ ] **Step 6: Commit the version bump**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump VERSION to 0.13.8 (bundles #23 into PR #25)"
```

---

## Task 2: Write the failing Rust unit test (TDD — RED)

**Files:**
- Modify: `crates/spinbike-server/src/db/reports.rs` (extend the existing `#[cfg(test)] mod tests` block at line 592)

- [ ] **Step 1: Open `crates/spinbike-server/src/db/reports.rs` and locate the existing `#[cfg(test)] mod tests` block (~line 592)**

The current test block has only pure-function tests for `parse_hhmm_to_mins` and `pick_current_and_next`. Use the Read tool to confirm the closing `}` of `mod tests` is the last line of the file (or near it). The new test will be appended INSIDE that module before its closing brace.

- [ ] **Step 2: Add the new test**

Use the Edit tool. The closing `}` of `mod tests` is the unique end-of-mod marker — find it by reading the last few lines of the file (`tail -10 crates/spinbike-server/src/db/reports.rs`) and use that exact context as `old_string`.

`old_string` (replace with whatever the actual last 3 lines of the file are — likely `    }\n}\n` for the final test fn closing brace, then the mod's closing brace; copy the EXACT last function's closing pattern):

```rust
    fn picks_next_when_before_start() {
        let tmpls = vec![(1080, 60)];
        let (current, next) = pick_current_and_next(&tmpls, 1020);
        assert_eq!(current, None);
        assert_eq!(next, Some(0));
    }
}
```

`new_string` (keep the existing `picks_next_when_before_start` test, then append the new one before the `mod tests` closing brace):

```rust
    fn picks_next_when_before_start() {
        let tmpls = vec![(1080, 60)];
        let (current, next) = pick_current_and_next(&tmpls, 1020);
        assert_eq!(current, None);
        assert_eq!(next, Some(0));
    }

    // ----- Issue #23: NAVSTEVY/ATTENDANCE visit-count fix -----
    //
    // Today's attendance SQL counts ANY `amount < 0 AND valid_until IS NULL`
    // row, which wrongly includes Refreshments/Supplements/Card-activation-fee
    // charges AND wrongly excludes €0 `action='visit'` rows logged for
    // monthly-pass holders. Per CEO direction (#23), attendance should equal
    // (Fitness | Spinning) AND (paid charge | logged visit).
    //
    // The fixture is intentionally discriminating: it inserts 2 Refreshments
    // charges so the OLD SQL returns 5 (paid Fitness + paid Spinning + 2 ×
    // Refreshments + Card-fee) while the NEW SQL returns 4 (paid Fitness +
    // paid Spinning + free Fitness visit + free Spinning visit). A 1×
    // Refreshments fixture would coincidentally return 4 under both SQLs and
    // the test would not detect the bug. Do not change the count of
    // Refreshments rows without re-running the discriminator math.
    use crate::db::{create_memory_pool, run_migrations};
    use crate::db::transactions::{create_transaction, create_transaction_with_valid_until};
    use sqlx::SqlitePool;

    async fn setup_pool_with_card() -> (SqlitePool, i64) {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // The standard migrations seed Spinning, Fitness, Monthly pass,
        // Refreshments, Supplements, Card activation fee. We need a card to
        // satisfy NOT-NULL-ish FK semantics on `transactions.card_id`.
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, credit, allow_debit) VALUES ('T-23', 100.0, 1) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        (pool, card_id)
    }

    async fn service_id_by_name_en(pool: &SqlitePool, name_en: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?")
            .bind(name_en)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| panic!("service '{name_en}' missing from seed"))
    }

    #[tokio::test]
    async fn attendance_counts_only_fitness_and_spinning_visits() {
        let (pool, card_id) = setup_pool_with_card().await;

        let fitness_id = service_id_by_name_en(&pool, "Fitness").await;
        let spinning_id = service_id_by_name_en(&pool, "Spinning").await;
        let monthly_pass_id = service_id_by_name_en(&pool, "Monthly pass").await;
        let refreshments_id = service_id_by_name_en(&pool, "Refreshments").await;
        let card_fee_id = service_id_by_name_en(&pool, "Card activation fee").await;

        // 4 rows that SHOULD count.
        create_transaction(&pool, None, Some(card_id), None, Some(fitness_id), -5.0, "charge")
            .await
            .unwrap();
        create_transaction(&pool, None, Some(card_id), None, Some(spinning_id), -5.0, "charge")
            .await
            .unwrap();
        create_transaction(&pool, None, Some(card_id), None, Some(fitness_id), 0.0, "visit")
            .await
            .unwrap();
        create_transaction(&pool, None, Some(card_id), None, Some(spinning_id), 0.0, "visit")
            .await
            .unwrap();

        // 5 rows that should NOT count. TWO Refreshments rows so the buggy SQL
        // returns 5 and the fixed SQL returns 4 — the test would otherwise
        // pass against the bug. See header comment.
        create_transaction(&pool, None, Some(card_id), None, Some(refreshments_id), -2.50, "charge")
            .await
            .unwrap();
        create_transaction(&pool, None, Some(card_id), None, Some(refreshments_id), -2.50, "charge")
            .await
            .unwrap();
        create_transaction(&pool, None, Some(card_id), None, Some(card_fee_id), -3.0, "charge")
            .await
            .unwrap();
        let valid_until = chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap();
        create_transaction_with_valid_until(
            &pool,
            None,
            Some(card_id),
            None,
            Some(monthly_pass_id),
            -35.0,
            "charge",
            valid_until,
        )
        .await
        .unwrap();
        create_transaction(&pool, None, Some(card_id), None, None, 10.0, "topup")
            .await
            .unwrap();

        // Use today's date — all `create_transaction*` calls default
        // `created_at = datetime('now')`, so day_report(today) sees them all.
        let today = chrono::Local::now().naive_local().date();

        let (day_kpi, _, _) = super::day_report(&pool, today, 50, None).await.unwrap();
        assert_eq!(
            day_kpi.attendance, 4,
            "day_report attendance must count only Fitness/Spinning paid+visit rows"
        );

        let (range_kpi, _, _) = super::range_report(&pool, today, today, 50, None).await.unwrap();
        assert_eq!(
            range_kpi.attendance, 4,
            "range_report attendance must agree with day_report on the same date"
        );

        // Sanity: adjacent KPIs aren't disturbed by the change.
        // revenue_eur sums all negative amounts: 5+5+2.50+2.50+3+35 = 53.00.
        assert!((day_kpi.revenue_eur - 53.00).abs() < 0.001);
        // passes_sold counts valid_until-set rows: exactly 1.
        assert_eq!(day_kpi.passes_sold, 1);
        // cash_in_eur sums positive-amount rows: just the topup.
        assert!((day_kpi.cash_in_eur - 10.00).abs() < 0.001);
    }
}
```

If `tail -10 crates/spinbike-server/src/db/reports.rs` shows a different last test (or an additional one beyond `picks_next_when_before_start`), copy that exact pattern as `old_string` instead — the new test code goes right BEFORE the `mod tests` closing `}` regardless.

- [ ] **Step 3: Run the local fmt check**

```bash
cargo fmt --all --check
```

Expected: exit code 0, no diff. If fmt complains, run `cargo fmt --all` and re-verify.

- [ ] **Step 4: Verify the test code compiles in isolation by reading the diff**

```bash
git diff crates/spinbike-server/src/db/reports.rs | head -100
```

Expected: a clean unified diff that adds (a) `use` statements inside the module, (b) two helper `async fn`s, (c) one `#[tokio::test]` async fn. No edits outside the `mod tests` block.

DO NOT run `cargo test` locally. CI runs the test on push and proves it FAILS against the unchanged SQL — that's the RED step. (We can't run TDD's RED→GREEN locally for compiled Rust workspaces per project rules, so the discriminator math + the in-comment explanation in the test header is the proof of RED.)

- [ ] **Step 5: Commit the failing test**

```bash
git add crates/spinbike-server/src/db/reports.rs
git commit -m "test(reports): add attendance discriminator unit test (RED, #23)

Seeds 4 should-count rows (paid Fitness, paid Spinning, free Fitness
visit, free Spinning visit) plus 5 should-not-count rows including
TWO Refreshments charges. Old SQL returns 5; new SQL must return 4."
```

---

## Task 3: Update the SQL — day_report + range_report (TDD — GREEN)

**Files:**
- Modify: `crates/spinbike-server/src/db/reports.rs` (the two SELECT statements at ~line 121 and ~line 233)

- [ ] **Step 1: Edit `day_report` — replace the attendance CASE**

Use the Edit tool.

`old_string`:
```
            COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0)   AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE date(created_at) = ?1 AND deleted_at IS NULL",
```

`new_string`:
```
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))
                 AND (
                   (action = 'charge' AND amount < 0 AND valid_until IS NULL)
                   OR action = 'visit'
                 )
                THEN 1 ELSE 0
              END
            ), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE date(created_at) = ?1 AND deleted_at IS NULL",
```

The other lines in the SELECT (`revenue_eur` line above attendance, `passes_sold` and `cash_in_eur` lines below, the FROM/WHERE clause) are unchanged — only the `attendance` CASE expression is replaced.

- [ ] **Step 2: Edit `range_report` — replace the attendance CASE**

Use the Edit tool. The `range_report` SQL has `WHERE date(created_at) BETWEEN ?1 AND ?2` instead of `= ?1`, but the CASE expression is identical to `day_report`'s.

`old_string`:
```
            COALESCE(SUM(CASE WHEN amount < 0 AND valid_until IS NULL THEN 1 ELSE 0 END), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE date(created_at) BETWEEN ?1 AND ?2 AND deleted_at IS NULL",
```

`new_string`:
```
            COALESCE(SUM(
              CASE
                WHEN service_id IN (SELECT id FROM services WHERE name_en IN ('Fitness','Spinning'))
                 AND (
                   (action = 'charge' AND amount < 0 AND valid_until IS NULL)
                   OR action = 'visit'
                 )
                THEN 1 ELSE 0
              END
            ), 0) AS attendance,
            COALESCE(SUM(CASE WHEN valid_until IS NOT NULL THEN 1 ELSE 0 END), 0) AS passes_sold,
            COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0.0 END), 0.0) AS cash_in_eur
         FROM transactions
         WHERE date(created_at) BETWEEN ?1 AND ?2 AND deleted_at IS NULL",
```

- [ ] **Step 3: Spot-check both SQL strings are now identical in their attendance CASE**

```bash
grep -A 8 "AS attendance," crates/spinbike-server/src/db/reports.rs
```

Expected: two matches, each showing the same multi-line CASE expression. If they differ, the edit was incomplete.

- [ ] **Step 4: Run the local fmt check**

```bash
cargo fmt --all --check
```

Expected: exit code 0, no diff. If fmt complains (likely on string indentation inside `query_as`), run `cargo fmt --all` and re-verify.

- [ ] **Step 5: Commit the implementation**

```bash
git add crates/spinbike-server/src/db/reports.rs
git commit -m "feat(reports): fix NAVSTEVY/ATTENDANCE to count only class visits (#23)

Replace the attendance CASE in day_report and range_report. New
formula: (service is Fitness or Spinning) AND (paid charge OR
logged €0 visit). The old formula counted any paid non-pass
transaction (snacks, card-activation-fee) and missed the free
pass-visit log rows."
```

---

## Task 4: Add NEW Playwright E2E test for the reports KPI

**Files:**
- Create: `e2e/tests/reports-attendance.spec.ts`

- [ ] **Step 1: Create the new Playwright test file**

Use the Write tool to create `e2e/tests/reports-attendance.spec.ts` with this EXACT content:

```typescript
import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

interface ServiceLookup {
    spinning: number;
    fitness: number;
    monthly_pass: number;
    refreshments: number;
    card_activation_fee: number;
}

async function fetchServiceIds(token: string): Promise<ServiceLookup> {
    const resp = await fetch(`${BASE_URL}/api/services/active`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`/api/services/active failed: ${resp.status}`);
    const services: { id: number; name_en: string }[] = await resp.json();
    const find = (n: string) => {
        const s = services.find((x) => x.name_en === n);
        if (!s) throw new Error(`service "${n}" not in /api/services/active`);
        return s.id;
    };
    return {
        spinning: find('Spinning'),
        fitness: find('Fitness'),
        monthly_pass: find('Monthly pass'),
        refreshments: find('Refreshments'),
        card_activation_fee: find('Card activation fee'),
    };
}

async function activateCard(token: string, suffix: string, credit: number): Promise<number> {
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode: `RA-${suffix}`,
            initial_credit: credit,
            first_name: 'RA',
            last_name: `Reports${suffix}`,
        }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    const card = await resp.json();
    return card.id;
}

async function postCharge(token: string, cardId: number, serviceId: number, amount: number) {
    const resp = await fetch(`${BASE_URL}/api/payments/charge`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ card_id: cardId, amount, service_id: serviceId }),
    });
    if (!resp.ok) throw new Error(`charge failed: ${resp.status} ${await resp.text()}`);
}

async function postSellPass(token: string, cardId: number, serviceId: number, price: number, validUntil: string) {
    const resp = await fetch(`${BASE_URL}/api/payments/sell-pass`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ card_id: cardId, service_id: serviceId, price, valid_until: validUntil }),
    });
    if (!resp.ok) throw new Error(`sell-pass failed: ${resp.status} ${await resp.text()}`);
}

async function postLogVisit(token: string, cardId: number, serviceId: number) {
    const resp = await fetch(`${BASE_URL}/api/payments/log-visit`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ card_id: cardId, service_id: serviceId }),
    });
    if (!resp.ok) throw new Error(`log-visit failed: ${resp.status} ${await resp.text()}`);
}

async function postTopup(token: string, cardId: number, amount: number) {
    const resp = await fetch(`${BASE_URL}/api/payments/topup`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ card_id: cardId, amount }),
    });
    if (!resp.ok) throw new Error(`topup failed: ${resp.status} ${await resp.text()}`);
}

test.describe('Reports — NAVSTEVY/ATTENDANCE KPI counts class visits only (#23)', () => {
    test('paid Fitness + paid Spinning + free pass-visits = 4; snacks/fees/passes/topups excluded', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Reports endpoints require admin role.
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const services = await fetchServiceIds(token);
        const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;

        // Card with enough credit to cover all the charges below (5+5+2.50+2.50+3+35 = 53)
        // plus a 35 € pass sale; 100 keeps the math comfortably positive.
        const cardId = await activateCard(token, suffix, 100.0);

        // 4 rows that SHOULD count toward attendance.
        await postCharge(token, cardId, services.fitness, 5.0);   // paid Fitness
        await postCharge(token, cardId, services.spinning, 5.0);  // paid Spinning
        // log-visit only requires the card to have an active pass for the UI button to show,
        // but the API itself accepts a log-visit on any card. Sell a pass first so the
        // backend state is consistent with the staff workflow that produced these rows.
        const tomorrow = new Date(Date.now() + 24 * 3600_000).toISOString().slice(0, 10);
        await postSellPass(token, cardId, services.monthly_pass, 35.0, tomorrow); // counts against passes_sold, NOT attendance
        await postLogVisit(token, cardId, services.fitness);   // free Fitness visit
        await postLogVisit(token, cardId, services.spinning);  // free Spinning visit

        // 4 more rows that should NOT count toward attendance (in addition to the pass-sale above).
        await postCharge(token, cardId, services.refreshments, 2.50);  // snack #1
        await postCharge(token, cardId, services.refreshments, 2.50);  // snack #2 — discriminator
        await postCharge(token, cardId, services.card_activation_fee, 3.0); // card fee
        await postTopup(token, cardId, 10.0);                           // topup, no service

        // Today's date — every transaction above lands on `datetime('now')`.
        const today = new Date().toISOString().slice(0, 10);

        // Hit the day report endpoint directly to assert the shape, then drive the UI.
        const dayResp = await fetch(`${BASE_URL}/api/reports/day?date=${today}`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(dayResp.ok).toBe(true);
        const dayJson = await dayResp.json();
        expect(dayJson.kpi.attendance).toBe(4);

        // Now drive the UI: navigate to /reports and read the kpi-attendance tile.
        await page.goto('/reports');
        const kpiAttendance = page.locator('[data-testid="kpi-attendance"]');
        await expect(kpiAttendance).toBeVisible();
        // The tile renders the integer value; it MAY be inside a child .kpi-card__value div.
        // Use textContent and assert the number 4 appears as the displayed value.
        await expect(kpiAttendance).toContainText('4');

        assertCleanConsole(consoleMessages);
    });
});
```

- [ ] **Step 2: Verify the file looks right**

```bash
wc -l e2e/tests/reports-attendance.spec.ts
head -5 e2e/tests/reports-attendance.spec.ts
```

Expected: ~115 lines; first 5 lines are the imports + BASE_URL constant matching the template.

DO NOT run Playwright locally — same reason as Task 2 (project rule + needs a live server). CI is authoritative.

- [ ] **Step 3: Commit the new E2E test**

```bash
git add e2e/tests/reports-attendance.spec.ts
git commit -m "test(e2e): add reports-attendance.spec for #23

Seeds 4 should-count + 5 should-not-count transactions via the
payments API, hits /api/reports/day directly to assert the JSON
shape, then loads /reports and asserts the [data-testid=
\"kpi-attendance\"] tile shows '4'. Zero console errors."
```

---

## Task 5: Push and monitor CI to terminal state

**Files:** none (push action only)

- [ ] **Step 1: Push to dev**

```bash
git push origin dev
```

This pushes commits from Tasks 1-4 (version bump + RED test + GREEN SQL + new E2E test) plus the pre-existing unpushed commit `6ef1a37` (the spec doc for #23).

- [ ] **Step 2: Identify the CI runs**

```bash
sleep 12 && gh run list --branch dev --limit 4 --json databaseId,status,conclusion,headSha,event,createdAt
```

Both a `push` event and a `pull_request` event run will start (the `pull_request` run gates PR #25). Match by `headSha` against `git rev-parse HEAD`. Note both run IDs.

- [ ] **Step 3: Monitor the push run to terminal state**

```bash
sleep 900 && gh run view <push-run-id> --json status,conclusion,jobs
```

Run via the Bash tool with `run_in_background: true`. When it completes, read the result. Expected end state: `status: completed`, `conclusion: success`. All jobs green: Test Integrity, Lint, Build WASM, Test, E2E, Deploy (dev), Smoke (dev). Mutation Testing and Version Bump Check skip on push events; they run on the `pull_request` event.

- [ ] **Step 4: If a job fails, investigate and fix in ONE commit**

```bash
gh run view <run-id> --log-failed
```

Common fail modes for this PR:

- `cargo fmt --check`: run `cargo fmt --all` locally and recommit.
- `cargo test` (the new unit test): if it returns `attendance = 5` instead of `4`, the SQL fix didn't apply correctly to one of the two query strings — check both `day_report` and `range_report`.
- `cargo test` (other tests): the new test pool helper might have name collisions with existing test helpers. Inspect the error and rename if needed.
- `clippy -D warnings`: the new test code may need `#[allow(clippy::too_many_arguments)]` if `create_transaction` has 7+ params; check the existing transactions.rs tests for precedent.
- E2E flake (#24, SQLITE_BUSY) on a test UNRELATED to `reports-attendance.spec.ts`: per `ci-monitoring.md`, "ONE rerun is acceptable to rule out transient issues". If a single E2E test fails on `(code: 5) database is locked` and a rerun passes, accept the rerun. If it fails twice, escalate.
- E2E flake on `reports-attendance.spec.ts` itself: this is a real bug — investigate. Possible cause: the `today` date computed in the test uses local time on the runner, and a transaction's `datetime('now')` is UTC. If timezone differs from UTC the date comparison can drop a second. Re-read project memory for date handling; the fix is likely to use `date('now')` server-side (i.e. backend code) rather than ISO-locale date in the test.

After fix, push and re-monitor.

- [ ] **Step 5: All green — proceed to Task 6**

No commit in this task.

---

## Task 6: Update PR #25 title and body to reflect bundled scope, verify mergeable

**Files:** none (PR metadata only — PR #25 already exists)

- [ ] **Step 1: Update PR #25 title and body via the REST API**

`gh pr edit` hits a deprecated GraphQL projects-classic codepath in this repo's setup, so use the REST API directly. Write the new body to a temp file first to avoid shell-escaping pain with the markdown backticks:

```bash
cat > /tmp/pr25-body-v0138.md <<'BODY_EOF'
## Summary

This PR bundles three staff-facing changes plus the CI cache + E2E diagnostics work that landed earlier on the branch.

### #13 — Charge/Topup button order, Fitness/Spinning visit order, soft-sibling colors (v0.13.6)

- **Charge** moves left, keeps `.btn--primary` (solid blue, most-used action).
- **Topup** moves right, uses `.btn--primary-soft` (same blue hue, low saturation).
- **Visit Fitness** is left of **Visit Spinning** in the visit row.
- **Visit Fitness** keeps `.btn--info`, **Visit Spinning** uses `.btn--info-soft`.
- Three new CSS modifiers added (`.btn--info`, `.btn--primary-soft`, `.btn--info-soft`).
- New Playwright test `e2e/tests/dashboard-button-layout.spec.ts` asserts DOM order + class names + zero console errors.

### #17 — No predefined prices on staff dashboard (v0.13.7)

- Staff types the price every time when charging a service or selling a monthly pass. No more auto-fill from `default_price`.
- Service dropdown labels show only the name ("Spinning", "Fitness", "Monthly pass"). The `(5.00 €)` annotation is gone.
- Admin services CRUD page is unchanged. The 4-hour Spinning class auto-charger keeps using `default_price` to bill booked classes — no functional impact on automatic billing.
- New Playwright test `e2e/tests/no-predefined-prices.spec.ts` asserts dropdown labels without prices, empty input on service change, empty-submit inline error, and the typed-amount path end-to-end with zero console errors.
- Seven existing E2E tests updated (3 in original Task 4, 4 caught when CI surfaced auto-fill assertions in unrelated tests).

### #23 — Fix NAVSTEVY / ATTENDANCE visit-count KPI on reports (v0.13.8)

- Today's reports KPI counted ANY paid non-pass transaction (Refreshments, Supplements, Card-activation-fee inflate the count) AND missed the €0 pass-visit log rows.
- Per CEO direction, attendance now counts only `(Fitness | Spinning)` AND `(paid charge | logged €0 visit)`.
- Backend-only fix: two CASE-expression replacements in `crates/spinbike-server/src/db/reports.rs` (`day_report` + `range_report`).
- New Rust unit test seeds a discriminating fixture (4 should-count + 5 should-not-count, with 2× Refreshments so the OLD SQL returns 5 and NEW SQL returns 4).
- New Playwright test `e2e/tests/reports-attendance.spec.ts` drives the UI and asserts the KPI tile.
- No schema, no migration, no API contract change. Historical reports retroactively show the corrected number — that's the right behavior because the bug was in the calc, not the data.

### CI / E2E diagnostics (carried over from earlier on the branch)

- All 3 E2E jobs cache npm + Playwright browsers across runs.
- Server stdout/stderr captured to `/tmp/spinbike-server.log` and uploaded as artifact on E2E failure.
- `RUST_LOG=spinbike_server=info` on the E2E server invocation so `internal_error` lines surface.
- These diagnostics already paid off — flake #24 (`SQLITE_BUSY` writer race) was root-caused with concrete server-log evidence on this branch.

Closes #13. Closes #17. Closes #23.

## Test plan

- [x] CI green on push and PR runs — all jobs (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing, Deploy (dev), Smoke (dev)).
- [x] Existing E2E tests still pass.
- [x] New Rust unit test for attendance KPI (`db::reports`) passes.
- [x] New Playwright test for button layout, no-predefined-prices, and reports-attendance.
- [ ] Post-deploy on dev frontend: pick each service, confirm input stays empty, type 7.50 + Charge to confirm typed-amount path. Open /reports and confirm attendance KPI counts only class visits. Read DOM version label, confirm v0.13.8.
- [ ] Auto-charger regression: existing `charger_*` unit tests remain green without modification.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
BODY_EOF

gh api -X PATCH repos/zbynekdrlik/spinbike/pulls/25 \
  -f title="feat: button layout + no predefined prices + reports visit count (#13 #17 #23, v0.13.8)" \
  -F body=@/tmp/pr25-body-v0138.md \
  --jq '{title, mergeable_state: .mergeable_state, head_sha: .head.sha}'
```

- [ ] **Step 2: Confirm the title and body updated**

```bash
gh pr view 25 --json title,body | jq -r '.title, "---", (.body | .[:300])'
```

Expected: title contains `#13`, `#17`, `#23`, and `v0.13.8`; body opens with the new Summary section.

- [ ] **Step 3: Wait for the latest CI run on PR #25 to complete**

The push from Task 5 retriggered PR #25's CI. Poll the latest:

```bash
sleep 10 && gh pr view 25 --json statusCheckRollup -q '.statusCheckRollup[] | {name, status, conclusion}'
```

Expected end state: every check `conclusion: SUCCESS`.

- [ ] **Step 4: Verify PR #25 is mergeable + clean**

```bash
gh api repos/zbynekdrlik/spinbike/pulls/25 --jq '{mergeable: .mergeable, mergeable_state: .mergeable_state}'
```

Expected:
```json
{"mergeable":true,"mergeable_state":"clean"}
```

If `mergeable_state` is anything other than `"clean"` (e.g. `unstable`, `behind`, `blocked`, `dirty`), STOP and investigate. Per `autonomous-quality-discipline.md`, UNSTABLE is not mergeable. Fix the cause before reporting done.

- [ ] **Step 5: Report PR URL to the controller**

Output: `https://github.com/zbynekdrlik/spinbike/pull/25` — mergeable, clean. Awaiting user "merge it".

**Per `pr-merge-policy.md`, do NOT merge. The user merges explicitly. Task 6 ends here.**

---

## Task 7: Post-deploy verification (after user merges)

**Files:** none (verification only)

This task runs ONLY after the user explicitly tells the agent to merge (or merges manually). Until then, the implementation is not "done".

- [ ] **Step 1: Wait for main branch CI + deploy job to complete**

```bash
# Identify the main run triggered by the merge.
sleep 60 && gh run list --branch main --limit 3 --json databaseId,status,conclusion,headSha,name
# Then monitor:
sleep 600 && gh run view <main-run-id> --json status,conclusion,jobs
```

Expected: deploy-prod and smoke-prod jobs succeed. The deploy-dev path was already exercised on the dev push.

- [ ] **Step 2: Verify the dev frontend reads v0.13.8 from the DOM**

Per `version-on-dashboard.md` and `post-deploy-verification.md`: open the dev frontend in Playwright and read the version label.

```typescript
// Inline Playwright session via mcp__plugin_playwright_playwright__* tools:
// 1. browser_navigate to https://spinbike-dev.newlevel.media
// 2. browser_evaluate: document.querySelector('[aria-label="Application version"], [data-testid="version"], .app-version')?.textContent?.trim()
// 3. assert it matches /^v0\.13\.8(-dev\.\d+)?(\s\([0-9a-f]{7}.*\))?$/
// 4. Cross-check backend: curl https://spinbike-dev.newlevel.media/api/version → {"version":"0.13.8"}
```

If the version label does NOT match v0.13.8, the deploy failed silently. Investigate (CDN cache, build skipped, wrong host) before reporting done.

- [ ] **Step 3: Functional verification on dev — drive /reports through Playwright**

```typescript
// 1. Read the admin token from the existing browser session (the dev frontend
//    keeps the JWT in localStorage as 'spinbike_token' if the user is logged in).
//    If not logged in, login as the CEO via the UI form (admin role required).
// 2. Seed transactions via the same payments API the unit test uses:
//    - 1 paid Fitness charge, 1 paid Spinning charge
//    - 1 sell-pass (Monthly pass)
//    - 1 log-visit Fitness, 1 log-visit Spinning
//    - 2 paid Refreshments (the discriminator)
//    - 1 paid Card-activation-fee
//    - 1 topup
// 3. Navigate to /reports
// 4. browser_evaluate: read [data-testid="kpi-attendance"] textContent
//    Confirm it equals "4" (or whatever number reflects the seeded subset of class visits FOR TODAY).
//    NOTE: the dev DB has historical data, so the KPI may include more than just our seed —
//    instead, hit /api/reports/day?date=<today> with admin token and assert the JSON's kpi.attendance == 4 + (any pre-seeded class visits today).
//    Cleanest: run the seed against a date filter that's "yesterday" relative to the reports view, but that requires backend support for inserting at past dates which the test fixture lacks.
//    Practical: read the KPI value BEFORE seeding (capture as `before`), seed exactly 4 class-visit rows + 5 non-class rows, read the KPI value AFTER, assert (after - before) == 4.
// 5. Read browser_console_messages — confirm zero errors / zero warnings (Chrome integrity preload warnings are not app-level and acceptable).
```

- [ ] **Step 4: Send the completion report**

Per `completion-report.md`, send the EXACT template:

```
## ✅ Work Complete

**Audits & deploy:**
✅ CI: green (all jobs on push and PR runs, including Mutation Testing)
✅ /plan-check: 7/7 fulfilled
✅ /review: clean — 0 🔴 0 🟡 0 🔵
✅ Deploy: dev frontend shows v0.13.8 (matches backend /api/version); reports KPI delta == 4 after seeding 4 class-visit + 5 non-class transactions; console clean.

**Plan steps:**
- Bumped VERSION to 0.13.8
- Added Rust unit test asserting attendance counts only Fitness/Spinning paid+visit rows
- Replaced attendance CASE in db::reports::day_report and range_report
- Added Playwright e2e/tests/reports-attendance.spec.ts (seed + UI assertion)
- Updated PR #25 title/body to reflect bundled v0.13.8 scope
- Verified deploy on dev: KPI delta of 4 after seeding the discriminating fixture

---

**Goal:** Make the reports page's NAVSTEVY tile count only real class visits (paid Fitness + paid Spinning + free Fitness on pass + free Spinning on pass).
**What changed:** Snack purchases, supplement charges, and card-activation fees no longer inflate the visit count, and free pass-visits are now included. Other reports KPIs (revenue, passes, cash-in) and the activity feed are unchanged.

🌐 Dev:  https://spinbike-dev.newlevel.media
🌐 Prod: <prod URL from project CLAUDE.md or admin>

**[spinbike] PR #25: feat: button layout + no predefined prices + reports visit count (#13 #17 #23, v0.13.8)**
https://github.com/zbynekdrlik/spinbike/pull/25 — merged
```

If the deploy verification surfaces any console error, broken behavior, or version mismatch — DO NOT send the report. Investigate and fix first.

---

## Self-review (the planner's checklist, ran inline)

**Spec coverage:** Every spec section maps to a task —

- "Goal" / "Bug" / "Fix" → Task 3 (the SQL replacement).
- "What does NOT change" → enforced by the closed file list in File Structure (no schema, no API, no UI).
- "Files affected" — every file in the spec table has at least one task step. The `VERSION` bump is Task 1, the Rust SQL is Task 3, the Rust unit test is Task 2, the Playwright test is Task 4.
- "Implementation details" — the SQL strings in Task 3 reproduce the spec's CASE expressions verbatim.
- "Rust unit tests" — Task 2 implements the test, including the discriminating-fixture note.
- "NEW Playwright test" — Task 4 implements the test. The skeleton in the spec mapped to a full file in Task 4 with concrete API calls and assertions.
- "Acceptance criteria" — covered by Task 2's assertions (4 buckets count, 4+ excluded buckets don't, adjacent KPIs unchanged), Task 4's assertions (UI value), Task 5's CI gate, and Task 7's deploy verification.
- "Versioning" — Task 1.

**Placeholder scan:** None. Every step has the actual command or the actual code.

**Type consistency:** All identifiers used in later tasks match earlier definitions:

- `[data-testid="kpi-attendance"]` (matches `spinbike-ui/src/pages/reports/kpi_cards.rs:15`).
- `loginViaAPI`, `setupConsoleCheck`, `assertCleanConsole` (all in `e2e/tests/helpers.ts`).
- `create_transaction`, `create_transaction_with_valid_until` (both in `crates/spinbike-server/src/db/transactions.rs`).
- `create_memory_pool`, `run_migrations` (in `crates/spinbike-server/src/db/mod.rs`).
- Service `name_en` strings (`"Spinning"`, `"Fitness"`, `"Monthly pass"`, `"Refreshments"`, `"Card activation fee"`) match the V8-migration seed.
- `KpiSummary.attendance: i64` (in `crates/spinbike-core/src/reports.rs`).

**No gaps found.**
