# Spinning Visits KPI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the REVENUE card on `/reports` with a SPINNING card showing the count of spinning class entries (paid-from-credit + monthly-pass visits) for the selected day or range.

**Architecture:** One-card UI swap, one-field swap on `KpiSummary`, two SQL aggregates updated (day + range), one shared i18n key swap. Backed by the existing Spinning service tag (`spinbike_core::services::SPINNING_NAME_EN`).

**Tech Stack:** Rust + Axum 0.8 server, sqlx + SQLite, Leptos 0.7 CSR frontend, Playwright E2E.

---

## File map

| File | Change |
|---|---|
| `crates/spinbike-core/src/reports.rs` | `KpiSummary`: drop `revenue_eur`, add `spinning_visits` as field 1 |
| `crates/spinbike-server/src/db/reports.rs` | Both `day_report` + `range_report` SQL aggregations; `DbKpiRow`; fixture extended |
| `crates/spinbike-server/tests/reports.rs` | Update existing `revenue_eur` JSON assertions to `spinning_visits` |
| `spinbike-ui/src/pages/reports/kpi_cards.rs` | Remove `kpi-revenue` card, add `kpi-spinning-visits` card in slot 1 |
| `spinbike-ui/src/pages/reports/mod.rs` | `KpiSummary` initial-state default: `revenue_eur: 0.0` → `spinning_visits: 0` |
| `spinbike-ui/src/i18n.rs` | Remove `kpi_revenue`, add `kpi_spinning_visits → ("SPINNING", "SPINNING")` |
| `e2e/tests/reports-page.spec.ts` | Swap `kpi-revenue` for `kpi-spinning-visits`; assert old absent |
| `e2e/tests/reports-range.spec.ts` | Swap `kpi-revenue` for `kpi-spinning-visits`; assert old absent |

---

### Task 0: Pre-flight grep — confirm no external `revenue_eur` consumers

**Files:** none (read-only check)

**Owner:** CONTROLLER (no subagent).

- [ ] **Step 1: Grep for any consumers outside the in-tree files**

```bash
grep -rn "revenue_eur\|kpi-revenue\|kpi_revenue" \
  /home/newlevel/devel/spinbike \
  --include="*.rs" --include="*.ts" --include="*.tsx" \
  --include="*.toml" --include="*.json" --include="*.md" \
  2>/dev/null | grep -v -E "(docs/superpowers/(specs|plans))"
```

Expected: ONLY the files listed in the File map above. Any other hit (different SDK consumer, exported type, etc.) → STOP and update the plan before proceeding.

---

### Task 1: Core struct + server SQL + server tests

**Owner:** Subagent — model **opus** (couples three Rust files that compile together).

**Files:**
- Modify: `crates/spinbike-core/src/reports.rs:7-13`
- Modify: `crates/spinbike-server/src/db/reports.rs` (day_report ~lines 60-95, range_report ~lines 190-230, DbKpiRow ~lines 99-105, fixture asserts ~lines 414-419)
- Modify: `crates/spinbike-server/tests/reports.rs:64-68, 122`

**Hard rules for the subagent:**
- ONLY local command allowed: `cargo fmt --all --check`. Do NOT run `cargo test|build|clippy|run`. CI is authoritative.
- Use `git add <explicit-paths>` — NEVER `git add -A` or `git add .`.
- Single commit at the end.

- [ ] **Step 1: Swap the `KpiSummary` field**

Edit `crates/spinbike-core/src/reports.rs` lines 7-13. Replace the existing struct body with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KpiSummary {
    pub spinning_visits: i64,
    pub attendance: i64,
    pub passes_sold: i64,
    pub cash_in_eur: f64,
}
```

- [ ] **Step 2: Update `DbKpiRow` in the server query module**

Edit `crates/spinbike-server/src/db/reports.rs` lines 99-105. Replace with:

```rust
#[derive(sqlx::FromRow)]
struct DbKpiRow {
    spinning_visits: i64,
    attendance: i64,
    passes_sold: i64,
    cash_in_eur: f64,
}
```

- [ ] **Step 3: Update `day_report` SQL + struct mapping**

In `crates/spinbike-server/src/db/reports.rs` the `day_report` query starts at line 65 (`sqlx::query_as::<_, DbKpiRow>(\n"SELECT ...`). Replace the aggregation SQL string AND the `let kpi = KpiSummary { ... }` block immediately below it. The new SQL must use `SPINNING_NAME_EN` as bind index `?2` and `date_str` as `?1`:

```rust
let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
    "SELECT
        COALESCE(SUM(
          CASE
            WHEN service_id IN (SELECT id FROM services WHERE name_en = ?2)
             AND (
               (action = 'charge' AND amount < 0 AND valid_until IS NULL)
               OR action = 'visit'
             )
            THEN 1 ELSE 0
          END
        ), 0) AS spinning_visits,
        COALESCE(SUM(
          CASE
            WHEN service_id IN (SELECT id FROM services WHERE name_en IN (?2, ?3))
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
)
.bind(&date_str)
.bind(spinbike_core::services::SPINNING_NAME_EN)
.bind(spinbike_core::services::FITNESS_NAME_EN)
.fetch_one(pool)
.await?;

let kpi = KpiSummary {
    spinning_visits: kpi_row.spinning_visits,
    attendance: kpi_row.attendance,
    passes_sold: kpi_row.passes_sold,
    cash_in_eur: kpi_row.cash_in_eur,
};
```

Note: bind order matters. `?1` = date, `?2` = `SPINNING_NAME_EN`, `?3` = `FITNESS_NAME_EN`. The attendance subquery uses `?2, ?3` so Spinning + Fitness both count; the new spinning_visits subquery uses ONLY `?2`. Also remove the obsolete code comment about "(?2, ?3)" that referenced `FITNESS_NAME_EN` first (just rephrase to "Class-visit names bound from spinbike_core::services constants").

- [ ] **Step 4: Update `range_report` SQL + struct mapping**

In `crates/spinbike-server/src/db/reports.rs` the `range_report` aggregation starts around line 195. Replace the SQL string AND the `KpiSummary { ... }` literal at the end (lines ~218-226). Bind index map: `?1` = from, `?2` = to, `?3` = `SPINNING_NAME_EN`, `?4` = `FITNESS_NAME_EN`.

```rust
let kpi_row: DbKpiRow = sqlx::query_as::<_, DbKpiRow>(
    "SELECT
        COALESCE(SUM(
          CASE
            WHEN service_id IN (SELECT id FROM services WHERE name_en = ?3)
             AND (
               (action = 'charge' AND amount < 0 AND valid_until IS NULL)
               OR action = 'visit'
             )
            THEN 1 ELSE 0
          END
        ), 0) AS spinning_visits,
        COALESCE(SUM(
          CASE
            WHEN service_id IN (SELECT id FROM services WHERE name_en IN (?3, ?4))
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
)
.bind(&from_str)
.bind(&to_str)
.bind(spinbike_core::services::SPINNING_NAME_EN)
.bind(spinbike_core::services::FITNESS_NAME_EN)
.fetch_one(pool)
.await?;

Ok((
    KpiSummary {
        spinning_visits: kpi_row.spinning_visits,
        attendance: kpi_row.attendance,
        passes_sold: kpi_row.passes_sold,
        cash_in_eur: kpi_row.cash_in_eur,
    },
    events,
    has_more,
))
```

Also rewrite the leading code comment that said "see day_report for the rationale" if needed — keep the binding rationale concise. Do NOT remove the comment entirely; replace its text with a one-liner that references the SPINNING/FITNESS bind order.

- [ ] **Step 5: Extend the fixture in `db/reports.rs` to assert `spinning_visits`**

In the test module at the bottom of `crates/spinbike-server/src/db/reports.rs`, the fixture `attendance_counts_only_fitness_and_spinning_visits` currently asserts revenue/passes/cash_in around lines 413-419. Remove the stale `revenue_eur` assertion and add `spinning_visits == 2` for both day and range:

```rust
let (day_kpi, _, _) = super::day_report(&pool, today, 50, None).await.unwrap();
assert_eq!(
    day_kpi.attendance, 4,
    "day_report attendance must count only Fitness/Spinning paid+visit rows"
);
assert_eq!(
    day_kpi.spinning_visits, 2,
    "day_report spinning_visits = 1 paid Spinning charge + 1 zero-amount Spinning visit"
);

let (range_kpi, _, _) = super::range_report(&pool, today, today, 50, None)
    .await
    .unwrap();
assert_eq!(
    range_kpi.attendance, 4,
    "range_report attendance must agree with day_report on the same date"
);
assert_eq!(
    range_kpi.spinning_visits, 2,
    "range_report spinning_visits must agree with day_report on the same date"
);

// Sanity: adjacent KPIs aren't disturbed by the change.
// passes_sold counts valid_until-set rows: exactly 1.
assert_eq!(day_kpi.passes_sold, 1);
// cash_in_eur sums positive-amount rows: just the topup.
assert!((day_kpi.cash_in_eur - 10.00).abs() < 0.001);
```

(The `revenue_eur` line is deleted.)

- [ ] **Step 6: Update the HTTP-level integration tests in `tests/reports.rs`**

In `crates/spinbike-server/tests/reports.rs`:

**Day-report test, line 64-68** — the fixture inserts ONE Spinning -5 charge, ONE pass sale with no service tag, and one voided -5. So `spinning_visits` for the day is `1`. Replace:

```rust
let kpi = &body["kpi"];
assert_eq!(
    kpi["spinning_visits"].as_i64().unwrap(),
    1,
    "one paid Spinning charge counts as one spinning visit"
);
assert_eq!(
    kpi["attendance"].as_i64().unwrap(),
    1,
    "only one regular charge counts as a visit"
);
assert_eq!(kpi["passes_sold"].as_i64().unwrap(), 1);
assert_eq!(kpi["cash_in_eur"].as_f64().unwrap(), 20.0);
```

**Range-report test, line 122** — fixture inserts 2 Spinning -5 charges across two days + 1 topup. Replace the single `revenue_eur` line with:

```rust
assert_eq!(body["kpi"]["spinning_visits"].as_i64().unwrap(), 2);
```

Keep the existing `attendance` and `cash_in_eur` assertions on lines 121 and 123 unchanged.

- [ ] **Step 7: Run local formatter check**

```bash
cd /home/newlevel/devel/spinbike && cargo fmt --all --check
```

Expected: clean, exit 0. If any formatting drift, run `cargo fmt --all` and re-check.

- [ ] **Step 8: Stage + commit**

```bash
git add crates/spinbike-core/src/reports.rs \
        crates/spinbike-server/src/db/reports.rs \
        crates/spinbike-server/tests/reports.rs
git commit -m "$(cat <<'EOF'
feat(reports): replace revenue_eur with spinning_visits in KpiSummary

CEO finds REVENUE unusable (double-counts vs CASH IN top-ups). Replace
the metric with a count of Spinning-service entries (paid-from-credit
charges + monthly-pass zero-amount visits) over the selected day or
range. Server-side SQL changes for both day_report and range_report;
DbKpiRow + KpiSummary updated; fixtures assert spinning_visits == 2 in
db/reports.rs and HTTP integration tests in tests/reports.rs.

Refs docs/superpowers/specs/2026-05-12-spinning-visits-kpi-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: UI KPI card swap

**Owner:** Subagent — model **sonnet**.

**Files:**
- Modify: `spinbike-ui/src/pages/reports/kpi_cards.rs:11-14`
- Modify: `spinbike-ui/src/pages/reports/mod.rs:42-47` (initial-state default)

**Hard rules for the subagent:**
- Do NOT run `cargo build`, `trunk build`, or any compile command. Only `cargo fmt --all --check` is allowed.
- Explicit paths in `git add`. Single commit.

- [ ] **Step 1: Replace the revenue card in `kpi_cards.rs`**

Edit `spinbike-ui/src/pages/reports/kpi_cards.rs` lines 11-14 (the `<div class="kpi-card" data-testid="kpi-revenue">` block). Replace with:

```rust
<div class="kpi-card" data-testid="kpi-spinning-visits">
    <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_spinning_visits")}</div>
    <div class="kpi-card__value">{move || format!("{}", kpi.get().spinning_visits)}</div>
</div>
```

The remaining three cards (`kpi-attendance`, `kpi-passes`, `kpi-cash-in`) stay unchanged in their current positions.

- [ ] **Step 2: Update the initial-state `KpiSummary` in `mod.rs`**

Edit `spinbike-ui/src/pages/reports/mod.rs:42-47`. Replace the literal with:

```rust
let (kpi, set_kpi) = signal(KpiSummary {
    spinning_visits: 0,
    attendance: 0,
    passes_sold: 0,
    cash_in_eur: 0.0,
});
```

- [ ] **Step 3: Run local formatter check**

```bash
cd /home/newlevel/devel/spinbike && cargo fmt --all --check
```

Expected: clean.

- [ ] **Step 4: Stage + commit**

```bash
git add spinbike-ui/src/pages/reports/kpi_cards.rs \
        spinbike-ui/src/pages/reports/mod.rs
git commit -m "$(cat <<'EOF'
feat(reports-ui): swap REVENUE card for SPINNING visits card

Card 1 on /reports now shows the spinning_visits count served by the
server. Removes the kpi-revenue testid (so the old assertion fails
loudly until the E2E specs are updated). Initial KpiSummary default
in mod.rs updated to match the new field set.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: i18n keys

**Owner:** Subagent — model **sonnet**.

**Files:**
- Modify: `spinbike-ui/src/i18n.rs:593` (remove `kpi_revenue`; add `kpi_spinning_visits`)

**Hard rules for the subagent:**
- Slovak strings UNACCENTED. The new key uses `SPINNING` for both Slovak and English — already unaccented.
- `cargo fmt --all --check` only. Single commit. Explicit `git add`.

- [ ] **Step 1: Edit the i18n table**

In `spinbike-ui/src/i18n.rs` find the line:

```rust
    m.insert("kpi_revenue", ("TRZBA", "REVENUE"));
```

Replace with:

```rust
    m.insert("kpi_spinning_visits", ("SPINNING", "SPINNING"));
```

Keep all other entries untouched.

- [ ] **Step 2: Run local formatter check**

```bash
cd /home/newlevel/devel/spinbike && cargo fmt --all --check
```

Expected: clean.

- [ ] **Step 3: Stage + commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "$(cat <<'EOF'
feat(i18n): replace kpi_revenue with kpi_spinning_visits

Slovak + English label both "SPINNING" (unaccented per project i18n
convention). Drops the old TRZBA/REVENUE key entirely.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Playwright E2E assertion

**Owner:** Subagent — model **sonnet**.

**Files:**
- Modify: `e2e/tests/reports-page.spec.ts:13, 43, 45, 47`
- Modify: `e2e/tests/reports-range.spec.ts:13, 16, 19`

**Hard rules for the subagent:**
- Do NOT add `wasm_bindgen_test_configure!(run_in_browser);` anywhere — irrelevant here (TS Playwright, not WASM), but the rule applies project-wide.
- Reuse the existing `loginViaAPI`, `setupConsoleCheck`, `assertCleanConsole` helpers from `./helpers`. Do not create new helpers.
- Explicit `git add` for the two files; single commit.

- [ ] **Step 1: Update `e2e/tests/reports-page.spec.ts`**

In the first test ("loads with KPI cards, feed, filters"), replace the line at 13:

```ts
        await expect(page.locator('[data-testid="kpi-revenue"]')).toBeVisible();
```

With:

```ts
        await expect(page.locator('[data-testid="kpi-spinning-visits"]')).toBeVisible();
        await expect(page.locator('[data-testid="kpi-spinning-visits"] .kpi-card__value')).toHaveText(/^\d+$/);
        await expect(page.locator('[data-testid="kpi-revenue"]')).toHaveCount(0);
```

In the third test ("Week/Month range buttons toggle"), replace each of the three `kpi-revenue` assertions on lines 43, 45, 47:

```ts
        await expect(page.locator('[data-testid="kpi-spinning-visits"]')).toBeVisible();
```

(Three identical replacements — one per Week / Month / quick-today click. Keep the surrounding click + console assertions intact.)

- [ ] **Step 2: Update `e2e/tests/reports-range.spec.ts`**

In the first test ("Week / Month UI modes load without error"), replace each of the three `kpi-revenue` assertions on lines 13, 16, 19 with:

```ts
        await expect(page.locator('[data-testid="kpi-spinning-visits"]')).toBeVisible();
```

(The other tests in this file already use `fetch` to call the API directly without referencing `kpi-revenue` — leave them unchanged.)

- [ ] **Step 3: Stage + commit**

```bash
git add e2e/tests/reports-page.spec.ts e2e/tests/reports-range.spec.ts
git commit -m "$(cat <<'EOF'
test(e2e): assert kpi-spinning-visits card replaces kpi-revenue

Updates the Playwright reports specs to use the new testid. First test
also asserts integer rendering and absence of the old kpi-revenue
testid (catches accidental regression of the card swap).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Push + monitor CI

**Owner:** CONTROLLER. Per `ci-monitoring.md`: ONE background `sleep N && gh run view` call, no `/loop`, no custom monitor scripts.

- [ ] **Step 1: Push the branch**

```bash
git push origin dev
```

- [ ] **Step 2: Identify the latest run**

```bash
gh run list --branch dev --limit 1 --json databaseId,status,headSha
```

Capture `databaseId` as `$RUN_ID`.

- [ ] **Step 3: Monitor with a single background command**

```bash
sleep 600 && gh run view "$RUN_ID" --json status,conclusion,jobs
```

Run with `run_in_background: true`. When the result lands, inspect: every job must end with `conclusion: success`.

- [ ] **Step 4: On failure, investigate the root cause**

```bash
gh run view "$RUN_ID" --log-failed | head -200
```

Fix the underlying issue in ONE commit (e.g. SQL bind mismatch, fixture math, formatter drift). Push again, monitor the new run. Do NOT bump timeouts; do NOT add `continue-on-error`; do NOT blindly rerun.

---

### Task 6: Validate on the synced-from-prod dev DB

**Owner:** CONTROLLER. The dev deploy job syncs prod DB to dev on every deploy (per memory `feedback_dev_ci_sync_prod_db.md`), so a recent dev deploy gives us a prod-shaped DB to query.

- [ ] **Step 1: Find a recent date with activity**

```bash
sqlite3 /var/lib/spinbike-dev/spinbike.db \
  "SELECT date(created_at) AS d, COUNT(*) FROM transactions WHERE deleted_at IS NULL GROUP BY d ORDER BY d DESC LIMIT 5;"
```

Pick a date with ≥1 transaction (call it `$D`).

- [ ] **Step 2: Hit `/api/reports/day` against dev and inspect KPIs**

```bash
TOKEN=$(curl -s -X POST https://spinbike-dev.newlevel.media/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"admin@test.com","password":"admin123"}' | jq -r .token)
curl -s -H "Authorization: Bearer $TOKEN" \
  "https://spinbike-dev.newlevel.media/api/reports/day?date=$D" | jq .kpi
```

Expected: a JSON object containing `spinning_visits`, `attendance`, `passes_sold`, `cash_in_eur` — NO `revenue_eur` key. `spinning_visits` ≥ 0 and ≤ `attendance` (Spinning is a subset of Fitness+Spinning, so spinning_visits ≤ attendance must hold for every date).

- [ ] **Step 3: Cross-check the raw SQL against the synced DB**

```bash
sqlite3 /var/lib/spinbike-dev/spinbike.db \
  "SELECT
     SUM(CASE
           WHEN service_id IN (SELECT id FROM services WHERE name_en='Spinning')
            AND ((action='charge' AND amount<0 AND valid_until IS NULL) OR action='visit')
           THEN 1 ELSE 0 END) AS spinning,
     SUM(CASE
           WHEN service_id IN (SELECT id FROM services WHERE name_en IN ('Spinning','Fitness'))
            AND ((action='charge' AND amount<0 AND valid_until IS NULL) OR action='visit')
           THEN 1 ELSE 0 END) AS attendance
   FROM transactions
   WHERE date(created_at)='$D' AND deleted_at IS NULL;"
```

Expected: returned `spinning` matches the API response's `spinning_visits`; `spinning <= attendance`.

If the sanity check fails — STOP, investigate the SQL.

---

### Task 7: Open the PR and send the completion report

**Owner:** CONTROLLER. Per `pr-merge-policy.md`: never merge; only the user merges.

- [ ] **Step 1: Open PR `dev` → `main`**

```bash
gh pr create --base main --head dev \
  --title "feat(reports): replace REVENUE card with SPINNING visits count (v0.15.0)" \
  --body "$(cat <<'EOF'
## Summary

CEO finds the REVENUE card on /reports unusable: it sums credit charges, which is money already received (and double-counted vs CASH IN). Replaces card 1 with SPINNING — the count of spinning class entries on the selected day or range, combining paid-from-credit charges and zero-amount monthly-pass visits.

## Scope

- `KpiSummary`: drop `revenue_eur`, add `spinning_visits: i64` (first field).
- `day_report` + `range_report` SQL: same filter shape as ATTENDANCE but scoped to `services.name_en = 'Spinning'`.
- UI: card 1 testid swap to `kpi-spinning-visits`, label `SPINNING / SPINNING`.
- i18n: drop `kpi_revenue`, add `kpi_spinning_visits`.
- Tests: server fixture asserts `spinning_visits == 2`; HTTP integration tests updated; Playwright assertions on both reports spec files swap testids and add `kpi-revenue` absence check.

## Out of scope

- ATTENDANCE / PASSES / CASH IN cards unchanged.
- Door entries (`single_entry` service, retag from Fitness) are NOT counted — only the `Spinning` service.

Spec: docs/superpowers/specs/2026-05-12-spinning-visits-kpi-design.md
Plan: docs/superpowers/plans/2026-05-12-spinning-visits-kpi.md

## Test plan

- [x] Server unit fixture (`attendance_counts_only_fitness_and_spinning_visits`) asserts `spinning_visits == 2` for both day_report and range_report.
- [x] HTTP integration tests (`tests/reports.rs`) assert `spinning_visits` in JSON for day + range.
- [x] Playwright assertions in `reports-page.spec.ts` and `reports-range.spec.ts` confirm the new testid renders an integer and the old testid is absent.
- [x] Validated against the synced-from-prod dev DB: `spinning_visits <= attendance` holds for a recent date.

EOF
)"
```

- [ ] **Step 2: Verify the PR is mergeable + clean**

```bash
PR=$(gh pr view --json number -q .number)
gh api repos/zbynekdrlik/spinbike/pulls/$PR --jq '{mergeable: .mergeable, mergeable_state: .mergeable_state}'
```

Expected: `mergeable: true` AND `mergeable_state: "clean"`. If `unstable` / `behind` / `dirty`, fix the cause first (sync `dev` with `main`, fix any failing check). NEVER bypass branch protection.

- [ ] **Step 3: Send the completion report**

Use the EXACT template from `~/devel/airuleset/modules/core/completion-report.md`. Include:

- `✅ Audits & deploy` block with `✅ CI: green`, `✅ /plan-check: 8/8 fulfilled`, `✅ /review: clean — 0 🔴 0 🟡 0 🔵`, `✅ Deploy: dev shows v0.15.0-dev.N (matches /api/version)` with version read from the live DOM.
- This is NOT a bug-fix PR (no `fix:` prefix, no `Closes #bug`); OMIT the `✅ Regression test:` line.
- `🌐 Dev:` https://spinbike-dev.newlevel.media
- `🌐 Prod:` https://spinbike.newlevel.media
- PR title + URL.
- A `❓ Question:` asking whether to proceed to prod merge.

---

### Task 8: Post-merge prod verification

**Owner:** CONTROLLER. ONLY run after the user explicitly says "merge it". Per `approval-scope.md`, "merge it" is approval to merge only — NOT to deploy/verify autonomously. But once merged, the main CI auto-deploys to prod; this task verifies prod after that deploy completes.

- [ ] **Step 1: Wait for the main CI run to finish deploying to prod**

```bash
gh run list --branch main --limit 1 --json databaseId,status,conclusion
```

Wait until terminal state. Inspect deploy job specifically.

- [ ] **Step 2: Read the version from prod DOM via Playwright**

Open https://spinbike.newlevel.media in Playwright, log in as `admin@test.com`, navigate to `/reports`. Assert:

- `[data-testid="version"]` text matches `^v0\.15\.0(-dev\.\d+)?` (or higher).
- `[data-testid="kpi-spinning-visits"]` is visible and `.kpi-card__value` matches `/^\d+$/`.
- `[data-testid="kpi-revenue"]` has count 0.
- Browser console: 0 errors, 0 warnings.

Use the same `loginViaAPI` + `setupConsoleCheck` + `assertCleanConsole` helpers used in the E2E specs.

- [ ] **Step 3: Final completion report**

Send a second completion report confirming prod deploy verified, all 🌐 URLs live, version match observed in DOM.

---

## Self-review notes

- **Spec coverage:** spec §"Data model change" → Task 1; §"Server SQL" → Task 1; §"UI change" → Task 2; §"i18n" → Task 3; §"Tests (server)" → Task 1 steps 5-6; §"Tests (UI/E2E)" → Task 4; §"Risks (external consumer of revenue_eur)" → Task 0; §"Acceptance criteria" → Tasks 5-7 (CI green) + Task 6 (synced-DB validation) + Task 8 (prod DOM verification).
- **Placeholder scan:** clean. No TBD / TODO / "similar to Task N" placeholders. Every code-changing step has a complete code block; every command has expected output.
- **Type consistency:** `spinning_visits: i64` named identically everywhere — `KpiSummary` (core), `DbKpiRow` (server), JSON key (`tests/reports.rs`), UI testid (`kpi-spinning-visits`), i18n key (`kpi_spinning_visits`).

## Pre-answered questions (do not re-ask)

- "Subagent or sequential?" → Subagent (already answered).
- "Should I review the plan first?" → No (already answered — dispatch now).
- "Monitor CI?" → Yes (always).
- "Verify with Playwright?" → Yes (always).
- "Bundle these into one PR?" → Yes — single PR per feature per `autonomous-batch-issue-development.md`.
