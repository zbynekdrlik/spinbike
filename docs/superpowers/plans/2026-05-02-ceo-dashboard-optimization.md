# CEO Dashboard Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Trim wasted UI surface for the CEO's daily card-management workflow — drop two unused widgets (NowPanel, AlertsBanner), make Reports row clicks land in the card panel directly, and reclaim the wasted top navbar on phone for staff/admin.

**Architecture:** Four small, independent changes shipped as one PR (v0.13.14 → v0.13.15). Server-side: delete `/api/reports/now`, `/api/reports/alerts`, and the `alerts_count` field threaded through `/api/reports/day` and `/api/reports/range`. UI side: delete the `NowPanel` and `AlertsBanner` Leptos components, change Reports row navigation from `?q=` (search-prefill) to `?card=` (direct lookup via `/api/cards/lookup/{barcode}`), and add a 5th "More" tab to `AdaptiveNav` that opens a sheet (existing `Sheet` component) with username + EN/SK toggle + Logout. Phone navbar hidden via `body:has(.adaptive-nav) .navbar { display: none; }` inside the existing `@media (max-width: 540px)` block.

**Tech Stack:** Rust + Leptos 0.7 (CSR/WASM, server crate `spinbike-server`, shared `spinbike-core`), SQLite via sqlx 0.8, Axum 0.8, Playwright E2E. Verification: CI is authoritative — DO NOT run `cargo test/build/clippy` or `trunk build` locally; the only allowed local check is `cargo fmt --all --check`. Per project memory, `git add -A` / `git add .` is BANNED — use explicit paths or `git add -u`.

**Spec:** `docs/superpowers/specs/2026-05-02-ceo-dashboard-optimization-design.md` (committed at `322df42`).

**Project rules in effect:**
- `pr-merge-policy.md` — never merge the PR; end at "PR mergeable, awaiting user merge"
- `version-bumping.md` — Task 1 MUST be the version bump
- `ci-monitoring.md` — Task 12 monitors via single `sleep N && gh run view --json` background command
- `feedback_subagent_no_local_build.md` — subagents do NOT run `cargo test/build/clippy` or `trunk build`; CI is authoritative
- `feedback_no_git_add_A.md` — every commit uses explicit paths or `git add -u`

---

## Task 1: Version bump 0.13.14 → 0.13.15

**Files:**
- Modify: `VERSION`
- Modify: `Cargo.toml` workspace members (auto-synced via `scripts/sync-version.sh`)
- Modify: `Cargo.lock` (auto-updated)

- [ ] **Step 1: Edit VERSION file**

```bash
echo "0.13.15" > VERSION
```

- [ ] **Step 2: Sync version across all Cargo.toml files**

```bash
bash scripts/sync-version.sh
```

This script writes `0.13.15` into every `[package] version` field across the workspace and `spinbike-ui/Cargo.toml`.

- [ ] **Step 3: Verify the bump**

```bash
cat VERSION
# Expected: 0.13.15
grep -h '^version' Cargo.toml crates/*/Cargo.toml spinbike-ui/Cargo.toml | sort -u
# Expected: at most two unique lines — `version = "0.13.15"` for the bumped manifests, and `version.workspace = true` for workspace members.
```

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml Cargo.lock crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump version to 0.13.15"
```

---

## Task 2: Add `nav_more` i18n key + `ICON_MORE` constant

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`
- Modify: `spinbike-ui/src/components/adaptive_nav.rs:74-77` (append a new constant after `ICON_SETTINGS`)

- [ ] **Step 1: Add the i18n entry**

In `spinbike-ui/src/i18n.rs`, the `nav_*` keys live near the top of the static table (search for `m.insert("nav_desk"` to find them). Insert a new entry adjacent to the others. Search the file for `nav_settings` to locate the right group:

```bash
grep -n 'nav_desk\|nav_schedule\|nav_reports\|nav_settings' spinbike-ui/src/i18n.rs
```

After locating the cluster, append:

```rust
m.insert("nav_more", ("Viac", "More"));
```

- [ ] **Step 2: Add the SVG constant**

In `spinbike-ui/src/components/adaptive_nav.rs`, append AFTER the existing `ICON_SETTINGS` declaration (currently at line 77):

```rust
const ICON_MORE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6.75a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5zm0 6a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5zm0 6a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5z"/></svg>"##;
```

This is a Heroicons-style ellipsis-vertical icon, matching the stroke-width and structure of the other four icons.

- [ ] **Step 3: Verify formatting**

```bash
cargo fmt --all --check
```

If it complains, run `cargo fmt --all` and re-stage.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/i18n.rs spinbike-ui/src/components/adaptive_nav.rs
git commit -m "i18n(ui): add nav_more key + ICON_MORE constant for AdaptiveNav 'More' tab"
```

---

## Task 3: Hide top navbar on phone for staff/admin (CSS)

**Files:**
- Modify: `spinbike-ui/style.css` (inside the existing `@media (max-width: 540px)` block at lines 1060-1078)

- [ ] **Step 1: Locate the existing block**

```bash
grep -n '@media (max-width: 540px)' spinbike-ui/style.css
```

Expected: one match around line 1060.

- [ ] **Step 2: Append the new rule INSIDE that media query**

Add the following rule as the LAST entry inside the `@media (max-width: 540px) { ... }` block (just before its closing brace at around line 1079):

```css
    /* On phone, when AdaptiveNav is rendered (= staff/admin logged in),
       the top navbar wastes 2 wrapped rows on rarely-used controls
       (username, Logout, EN/SK). Those controls move into the bottom
       AdaptiveNav 'More' sheet. Customers and logged-out users keep
       the top navbar (no AdaptiveNav → :has() doesn't match). */
    body:has(.adaptive-nav) .navbar { display: none; }
```

`:has()` is supported in evergreen browsers since Safari 15.4 / Chrome 105 / Firefox 121 — no fallback required for the project's deploy target.

- [ ] **Step 3: Verify the rule landed inside the right media query**

```bash
awk '/@media \(max-width: 540px\)/,/^}$/' spinbike-ui/style.css | grep -n 'body:has(.adaptive-nav)'
```

Expected: one match.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/style.css
git commit -m "style(ui): hide top navbar on phone when AdaptiveNav is rendered"
```

---

## Task 4: Delete NowPanel UI side

**Files:**
- Delete: `spinbike-ui/src/pages/desk/now_panel.rs`
- Delete: `spinbike-ui/src/pages/desk/mod.rs`
- Delete: `spinbike-ui/src/pages/desk/` (the empty directory)
- Delete: `e2e/tests/desk-now-panel.spec.ts`
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs:306` (remove `<crate::pages::NowPanel />`)
- Modify: `spinbike-ui/src/pages/mod.rs:3, :13` (remove desk module + NowPanel re-export)
- Modify: `spinbike-ui/style.css` (remove `.now-panel*` rules block)
- Modify: `spinbike-ui/src/i18n.rs` (remove `now_*` and possibly `status_*` keys — see Step 4)

- [ ] **Step 1: Remove the render site in dashboard/mod.rs**

Open `spinbike-ui/src/pages/dashboard/mod.rs`. At line 306, remove the line containing `<crate::pages::NowPanel />` (and the trailing blank line if it leaves an awkward gap before the next sibling element). The line that follows is the search input wrapped in `<div class="card mb-2">` — that becomes the first child of the dashboard view.

- [ ] **Step 2: Remove the desk module re-exports**

In `spinbike-ui/src/pages/mod.rs`:
- Remove the line `pub mod desk;` (currently line 3)
- Remove the line `pub use desk::NowPanel;` (currently line 13)

- [ ] **Step 3: Delete the desk directory and its files**

```bash
rm spinbike-ui/src/pages/desk/now_panel.rs
rm spinbike-ui/src/pages/desk/mod.rs
rmdir spinbike-ui/src/pages/desk
rm e2e/tests/desk-now-panel.spec.ts
```

- [ ] **Step 4: Remove `.now-panel*` CSS rules**

```bash
grep -n '\.now-panel' spinbike-ui/style.css
```

Expected: a contiguous block of `.now-panel`, `.now-panel__head`, `.now-panel__title`, `.now-panel__badge`, `.now-panel__chevron`, `.now-panel__body`, and `.now-panel__head--running` rules. Identify the start (first `.now-panel` rule) and end (last `}` after the last `.now-panel*` rule). Remove that contiguous block. Use `awk` or a manual edit to confirm the trailing brace count stays balanced.

After removal:
```bash
grep -c 'now-panel' spinbike-ui/style.css
# Expected: 0
```

- [ ] **Step 5: Remove i18n keys, verifying each is unused elsewhere**

The keys to consider: `now_next_on`, `now_no_more_today`, `status_booked`, `status_checked_in`, `status_cancelled`.

For each candidate, grep the entire repo (excluding `i18n.rs` and the deleted `now_panel.rs`) to confirm zero remaining references:

```bash
for key in now_next_on now_no_more_today status_booked status_checked_in status_cancelled; do
    echo "=== $key ==="
    grep -rn "\"$key\"" --include='*.rs' --include='*.ts' --include='*.tsx' \
        --exclude-dir=desk \
        spinbike-ui/src/ crates/ e2e/tests/ 2>/dev/null | grep -v 'i18n.rs:'
done
```

For each key with NO matches outside `i18n.rs`, remove its `m.insert(...)` line. For any key WITH matches, leave it (it's in use elsewhere).

The `now_*` keys are NowPanel-only and will be safe to remove. The `status_*` keys are NowPanel-only currently (the only consumers are now_panel.rs lines 117-120 and the deleted desk-now-panel.spec.ts) but verify with the grep.

- [ ] **Step 6: Commit**

```bash
git add -u spinbike-ui/src/pages/dashboard/mod.rs spinbike-ui/src/pages/mod.rs spinbike-ui/style.css spinbike-ui/src/i18n.rs
git add -u spinbike-ui/src/pages/desk/now_panel.rs spinbike-ui/src/pages/desk/mod.rs e2e/tests/desk-now-panel.spec.ts
git commit -m "feat(ui): remove NowPanel widget from /staff (delete component, CSS, i18n, E2E)"
```

`git add -u` covers deleted-file removals from the index.

- [ ] **Step 7: Sanity-grep after commit**

```bash
grep -rn 'NowPanel\|now-panel\|now_panel' --include='*.rs' --include='*.ts' --include='*.css' \
    spinbike-ui/ e2e/tests/ 2>/dev/null
```

Expected: zero matches (server-side `now_panel()` will go in Task 5).

---

## Task 5: Delete NowPanel server side

**Files:**
- Modify: `crates/spinbike-server/src/routes/reports.rs` (remove route + `now()` handler)
- Modify: `crates/spinbike-server/src/db/reports.rs` (remove `now_panel()`)
- Modify: `crates/spinbike-core/src/reports.rs` (remove 5 types)
- Modify: `crates/spinbike-server/tests/reports.rs` (remove 4 test functions)

- [ ] **Step 1: Remove the route registration in routes/reports.rs**

In `crates/spinbike-server/src/routes/reports.rs`, locate the route registration block (around line 21) and remove the `.route("/api/reports/now", get(now))` line.

- [ ] **Step 2: Remove the `now()` handler**

The `now()` async handler is at lines 110-118 (between `alerts()` and `total_alert_count()`). Remove it entirely:

```rust
// DELETE this entire function:
async fn now(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<NowResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let r = db::reports::now_panel(&state.pool)
        .await
        .map_err(internal_error)?;
    Ok(Json(r))
}
```

Also remove `NowResponse` from the `use spinbike_core::reports::{...}` line at the top of the file (currently line 14):

```rust
// Before:
use spinbike_core::reports::{AlertsResponse, NowResponse, ReportResponse};
// After (Task 7 will further trim AlertsResponse):
use spinbike_core::reports::{AlertsResponse, ReportResponse};
```

- [ ] **Step 3: Remove `now_panel()` from db/reports.rs**

In `crates/spinbike-server/src/db/reports.rs`, locate `pub async fn now_panel(pool: &SqlitePool)` (around line 418) and remove the entire function body up through its closing brace.

Then remove `NowResponse`, `CurrentClass`, `NextClass`, `RosterEntry`, `RosterStatus` from the `use spinbike_core::reports::{...}` line at the top (currently line 6-7):

```rust
// Before:
use spinbike_core::reports::{
    AlertsResponse, CurrentClass, ExpiringPass, InactiveCustomer, KpiSummary, LowCreditCard,
    NextClass, NowResponse, ReportEvent, RosterEntry, RosterStatus,
};
// After (Task 7 will further trim Alerts types):
use spinbike_core::reports::{
    AlertsResponse, ExpiringPass, InactiveCustomer, KpiSummary, LowCreditCard,
    ReportEvent,
};
```

- [ ] **Step 4: Remove the types from spinbike-core/src/reports.rs**

In `crates/spinbike-core/src/reports.rs`, locate and remove the `pub enum RosterStatus` (line 120), `pub struct RosterEntry` (line 127), `pub struct CurrentClass` (line 136), `pub struct NextClass` (around line 150 — verify by grep), and `pub struct NowResponse` (line 158).

```bash
grep -n '^pub \(struct\|enum\)' crates/spinbike-core/src/reports.rs
```

Use that to confirm the line ranges before deletion.

- [ ] **Step 5: Remove the four `now_panel` test functions from tests/reports.rs**

In `crates/spinbike-server/tests/reports.rs`, remove these four `#[tokio::test]` functions in their entirety (each is one `#[tokio::test]` attribute followed by an `async fn ... { ... }`):

| Function name | `#[tokio::test]` line |
|---|---|
| `now_panel_returns_current_or_next_class` | 282 |
| `now_panel_excludes_cancelled_class_today` | 341 |
| `now_panel_finds_future_class_when_none_today` | 396 |
| `now_panel_selects_correct_template_when_multiple_exist` | 507 |

Re-grep before each removal to confirm the current `#[tokio::test]` line (line numbers shift as you delete functions earlier in the file). Use:

```bash
grep -n 'async fn now_panel_' crates/spinbike-server/tests/reports.rs
```

Remove from `#[tokio::test]` through the function's closing `}` for each.

- [ ] **Step 6: Verify formatting**

```bash
cargo fmt --all --check
```

- [ ] **Step 7: Commit**

```bash
git add -u crates/spinbike-server/src/routes/reports.rs \
            crates/spinbike-server/src/db/reports.rs \
            crates/spinbike-core/src/reports.rs \
            crates/spinbike-server/tests/reports.rs
git commit -m "feat(server): remove /api/reports/now endpoint + now-panel types/tests"
```

- [ ] **Step 8: Sanity-grep after commit**

```bash
grep -rn 'NowResponse\|now_panel\|/api/reports/now\|RosterStatus\|RosterEntry\|CurrentClass\|NextClass' \
    --include='*.rs' \
    crates/ spinbike-ui/src/ 2>/dev/null
```

Expected: zero matches.

---

## Task 6: Delete AlertsBanner UI side

**Files:**
- Delete: `spinbike-ui/src/pages/reports/alerts_banner.rs`
- Delete: `spinbike-ui/src/pages/reports/sheets/alert_detail.rs`
- Delete: `e2e/tests/reports-alerts.spec.ts`
- Modify: `spinbike-ui/src/pages/reports/mod.rs` (remove banner site, signal, effect, mod, re-export, import)
- Modify: `spinbike-ui/src/pages/reports/sheets/mod.rs` (remove alert_detail re-export)
- Modify: `spinbike-ui/style.css` (remove `.alerts-banner*` rules)
- Modify: `spinbike-ui/src/i18n.rs` (remove `alerts_*` keys after grep)

- [ ] **Step 1: Edit `spinbike-ui/src/pages/reports/mod.rs`**

Remove the following lines:
- Line 9: `mod alerts_banner;`
- Line 15: `pub use alerts_banner::AlertsBanner;`
- Line 6 import: change `use spinbike_core::reports::{AlertsResponse, KpiSummary, ReportEvent, ReportResponse};` to `use spinbike_core::reports::{KpiSummary, ReportEvent, ReportResponse};`
- Lines 47-56 (the `// Alerts data — fetched once...` comment, the `(alerts, set_alerts)` signal, and the `Effect::new(move |_| { spawn_local(async move { ... AlertsResponse ... }) });` block).
- Line 168: `<AlertsBanner data=alerts />`

After the edit, the layout-rendering view! macro should go directly from the optional error display to `<KpiCards kpi=kpi />` with no banner in between.

- [ ] **Step 2: Delete the banner and detail-sheet source files**

```bash
rm spinbike-ui/src/pages/reports/alerts_banner.rs
rm spinbike-ui/src/pages/reports/sheets/alert_detail.rs
rm e2e/tests/reports-alerts.spec.ts
```

- [ ] **Step 3: Remove the alert_detail re-export from sheets/mod.rs**

`spinbike-ui/src/pages/reports/sheets/mod.rs` currently is:

```rust
pub mod alert_detail;
pub mod calendar_picker;
```

Remove the first line, leaving:

```rust
pub mod calendar_picker;
```

- [ ] **Step 4: Remove `.alerts-banner*` CSS block**

```bash
grep -n '\.alerts-banner' spinbike-ui/style.css
```

Expected: a contiguous block at lines ~1350-1395. Identify the first `.alerts-banner` rule and the closing brace of the last `.alerts-banner__dismiss` rule. Remove that block.

After removal:
```bash
grep -c 'alerts-banner' spinbike-ui/style.css
# Expected: 0
```

- [ ] **Step 5: Remove i18n keys, verifying each**

For each candidate, grep:

```bash
for key in alerts_title alerts_expiring_passes alerts_low_credit alerts_inactive; do
    echo "=== $key ==="
    grep -rn "\"$key\"" --include='*.rs' --include='*.ts' \
        spinbike-ui/src/ crates/ e2e/tests/ 2>/dev/null | grep -v 'i18n.rs:'
done
```

Each should have zero non-i18n matches (the banner code is now deleted). Remove their `m.insert(...)` lines.

Then grep for any keys that were ONLY referenced inside the deleted `alert_detail.rs` (these will appear in `i18n.rs` but have no remaining consumer). Candidates likely include `alert_expiring_pass_title`, `alert_low_credit_title`, `alert_inactive_title`, plus any `alert_*` body strings. To find them:

```bash
# List all alert* i18n keys still defined
grep -nE 'm\.insert\("alert' spinbike-ui/src/i18n.rs

# For each one, check for non-i18n references:
for key in $(grep -oE 'm\.insert\("(alert[^"]+)"' spinbike-ui/src/i18n.rs | sed -E 's/.*"(alert[^"]+)".*/\1/'); do
    matches=$(grep -rn "\"$key\"" --include='*.rs' --include='*.ts' \
        spinbike-ui/src/ crates/ e2e/tests/ 2>/dev/null | grep -v 'i18n.rs:' | wc -l)
    if [ "$matches" -eq 0 ]; then
        echo "DELETE: $key"
    else
        echo "KEEP:   $key ($matches references)"
    fi
done
```

Delete only the `DELETE:`-flagged keys.

- [ ] **Step 6: Verify formatting**

```bash
cargo fmt --all --check
```

- [ ] **Step 7: Commit**

```bash
git add -u spinbike-ui/src/pages/reports/mod.rs \
            spinbike-ui/src/pages/reports/sheets/mod.rs \
            spinbike-ui/style.css \
            spinbike-ui/src/i18n.rs \
            spinbike-ui/src/pages/reports/alerts_banner.rs \
            spinbike-ui/src/pages/reports/sheets/alert_detail.rs \
            e2e/tests/reports-alerts.spec.ts
git commit -m "feat(ui): remove AlertsBanner from /reports (delete banner, detail sheet, CSS, i18n, E2E)"
```

- [ ] **Step 8: Sanity-grep after commit**

```bash
grep -rn 'AlertsBanner\|alerts-banner\|alert_detail\|alerts_title' \
    --include='*.rs' --include='*.ts' --include='*.css' \
    spinbike-ui/ e2e/tests/ 2>/dev/null
```

Expected: zero matches (server-side cleanup happens in Task 7).

---

## Task 7: Delete AlertsBanner server side + alerts_count plumbing

**Files:**
- Modify: `crates/spinbike-server/src/routes/reports.rs` (remove route + `alerts()` + `total_alert_count()` helper + `alerts_count` lines from `day()` and `range()` + `AlertsResponse` import)
- Modify: `crates/spinbike-server/src/db/reports.rs` (remove `alerts_report()` and `alerts_count()` functions + Alerts-only types from imports)
- Modify: `crates/spinbike-core/src/reports.rs` (remove `alerts_count` field from `ReportResponse`; remove `AlertsResponse`, `ExpiringPass`, `LowCreditCard`, `InactiveCustomer` types)
- Modify: `crates/spinbike-server/tests/reports.rs` (remove the 5 alerts-related test functions)

> **Why this folds into the AlertsBanner deletion:** the existing `/api/reports/day` and `/api/reports/range` responses include an `alerts_count: i64` field that the UI no longer consumes (the field is fetched + read by the now-deleted banner only). Without removing it server-side, every day/range response would still trigger an `alerts_count()` SQL query for a value nobody reads.

- [ ] **Step 1: Remove the route registration**

In `crates/spinbike-server/src/routes/reports.rs`, remove the `.route("/api/reports/alerts", get(alerts))` line (currently line 20).

- [ ] **Step 2: Strip `alerts_count` from `day()` and `range()` handlers**

`day()` handler currently has (around line 55):

```rust
    let alerts_count = total_alert_count(&state).await.unwrap_or(0);
    Ok(Json(ReportResponse {
        kpi,
        events,
        alerts_count,
        has_more,
    }))
```

Remove the `alerts_count = total_alert_count(...)` line and the `alerts_count,` field from the struct literal:

```rust
    Ok(Json(ReportResponse {
        kpi,
        events,
        has_more,
    }))
```

Apply the identical edit to `range()` (around line 90) — it has the same shape.

- [ ] **Step 3: Remove the `alerts()` and `total_alert_count()` helpers**

Remove the `alerts()` async handler in its entirety (currently lines 99-107):

```rust
// DELETE entire function:
async fn alerts(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<AlertsResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&claims)?;
    let r = db::reports::alerts_report(&state.pool)
        .await
        .map_err(internal_error)?;
    Ok(Json(r))
}
```

Remove the `total_alert_count()` helper as well (currently lines 121-123):

```rust
// DELETE entire function:
async fn total_alert_count(state: &AppState) -> anyhow::Result<i64> {
    db::reports::alerts_count(&state.pool).await
}
```

Update the imports at the top of the file:

```rust
// Before:
use spinbike_core::reports::{AlertsResponse, ReportResponse};
// After:
use spinbike_core::reports::ReportResponse;
```

- [ ] **Step 4: Remove `alerts_report()` and `alerts_count()` from db/reports.rs**

In `crates/spinbike-server/src/db/reports.rs`, remove `pub async fn alerts_count` (around line 290 onwards through its closing brace) and `pub async fn alerts_report` (around line 322 onwards through its closing brace).

After removal, the imports at the top of the file should be:

```rust
use spinbike_core::reports::{KpiSummary, ReportEvent};
```

(`AlertsResponse`, `ExpiringPass`, `LowCreditCard`, `InactiveCustomer` are no longer used; `NextClass`, `CurrentClass`, `NowResponse`, `RosterEntry`, `RosterStatus` were removed in Task 5.)

- [ ] **Step 5: Update `ReportResponse` in spinbike-core/src/reports.rs**

Remove the `alerts_count: i64` field from `pub struct ReportResponse`. Then remove the `AlertsResponse`, `ExpiringPass`, `LowCreditCard`, `InactiveCustomer` types entirely.

Locate them with:
```bash
grep -n '^pub \(struct\|enum\)' crates/spinbike-core/src/reports.rs
```

After this task, that grep should show only the still-needed types (`ReportResponse`, `KpiSummary`, `ReportEvent`, etc., minus the alerts/now ones).

- [ ] **Step 6: Remove the 5 alerts-related test functions from tests/reports.rs**

| Function name | `#[tokio::test]` line (pre-Task-5; will shift) |
|---|---|
| `alerts_expiring_passes_within_7_days_excludes_blocked` | 139 |
| `alerts_low_credit_under_5_and_not_blocked` | 203 |
| `alerts_inactive_60_days_excludes_zero_credit_and_blocked` | 238 |
| `day_report_alerts_count_reflects_underlying_alerts` | 450 |
| (verify any 5th alerts test by grep) | — |

Re-grep before deletion:

```bash
grep -n 'async fn alerts_\|async fn day_report_alerts_count\|/api/reports/alerts' \
    crates/spinbike-server/tests/reports.rs
```

Remove each `#[tokio::test]` + `async fn { ... }` block in full.

- [ ] **Step 7: Verify formatting**

```bash
cargo fmt --all --check
```

- [ ] **Step 8: Commit**

```bash
git add -u crates/spinbike-server/src/routes/reports.rs \
            crates/spinbike-server/src/db/reports.rs \
            crates/spinbike-core/src/reports.rs \
            crates/spinbike-server/tests/reports.rs
git commit -m "feat(server): remove /api/reports/alerts + alerts_count plumbing"
```

- [ ] **Step 9: Sanity-grep**

```bash
grep -rn 'AlertsResponse\|alerts_report\|alerts_count\|/api/reports/alerts\|ExpiringPass\|LowCreditCard\|InactiveCustomer' \
    --include='*.rs' --include='*.ts' \
    crates/ spinbike-ui/src/ e2e/tests/ 2>/dev/null
```

Expected: zero matches.

---

## Task 8: Add "More" button + sheet to AdaptiveNav

**Files:**
- Modify: `spinbike-ui/src/components/adaptive_nav.rs` (add 5th item, sheet, `more_open` signal, lang/logout closures, Sheet import)
- Modify: `spinbike-ui/style.css` (append `.more-sheet__user` rule from the spec)

- [ ] **Step 1: Add the import for Sheet**

In `spinbike-ui/src/components/adaptive_nav.rs`, after `use crate::i18n::{self, Lang};` (line 5), add:

```rust
use crate::components::Sheet;
use leptos::ev;
```

(`leptos::ev` is needed if you stop event propagation; not strictly required for our usage but harmless.)

- [ ] **Step 2: Convert AdaptiveNav to manage `more_open` state and the lang/logout closures**

The current component (after `let path = ...`) renders a single `<nav>` element. Wrap that AND the new conditional sheet in a fragment. Insert state setup BEFORE the `view!` block at the top of the role-gated branch (after `let is_admin = u.role == "admin";`):

```rust
            let is_admin = u.role == "admin";
            // [keep existing] let path = current_path();
            // [keep existing] let desk_active = path.starts_with("/staff");
            // [keep existing] let schedule_active = path.starts_with("/schedule");
            // [keep existing] let reports_active = path.starts_with("/reports");
            // [keep existing] let settings_active = path.starts_with("/settings") || path.starts_with("/admin");

            // NEW: state for the More sheet
            let (more_open, set_more_open) = signal(false);
            let user_name = u.name.clone();

            // NEW: language toggle (mirrors logic in components/nav.rs:29-35)
            let set_lang = use_context::<WriteSignal<Lang>>().expect("SetLang context");
            let on_toggle_lang = move |_| {
                let new_lang = match lang.get() {
                    Lang::Sk => Lang::En,
                    Lang::En => Lang::Sk,
                };
                i18n::save_lang(new_lang);
                set_lang.set(new_lang);
            };

            // NEW: logout (mirrors logic in components/nav.rs:20-27)
            let on_logout = move |_| {
                crate::auth::clear_auth();
                let set_auth_ver = expect_context::<WriteSignal<u32>>();
                set_auth_ver.update(|v| *v += 1);
                if let Some(w) = web_sys::window() {
                    let _ = w.location().set_href("/");
                }
            };
```

- [ ] **Step 3: Append the 5th `<button>` item to the `<nav>` block**

Inside the `<nav class="adaptive-nav" data-testid="adaptive-nav">` element, AFTER the existing admin-only items (Reports + Settings), before the closing `</nav>`, add:

```rust
                    <button
                        class="adaptive-nav__item"
                        data-testid="nav-more"
                        type="button"
                        on:click=move |_| set_more_open.update(|v| *v = !*v)
                    >
                        <span class="adaptive-nav__icon" inner_html=ICON_MORE></span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_more")}</span>
                    </button>
```

The "More" item is a `<button>` (no route) so it does NOT get an `aria-current` — clicking only toggles a sheet.

- [ ] **Step 4: Render the sheet conditionally as a sibling of `<nav>`**

The current view returns `view! { <nav>...</nav> }.into_any()`. Change it to return a fragment containing both the nav AND the conditional sheet:

```rust
            view! {
                <nav class="adaptive-nav" data-testid="adaptive-nav">
                    // ... existing items + new More button ...
                </nav>
                {move || if more_open.get() {
                    let user_name = user_name.clone();
                    view! {
                        <Sheet
                            testid="more-sheet".to_string()
                            title=i18n::t(lang.get_untracked(), "nav_more").to_string()
                            on_close=Callback::new(move |_| set_more_open.set(false))
                        >
                            <div class="more-sheet__user">{user_name}</div>
                            <button
                                class="btn btn--block btn--ghost"
                                data-testid="more-lang-toggle"
                                on:click=on_toggle_lang
                            >
                                {move || match lang.get() {
                                    Lang::Sk => "EN",
                                    Lang::En => "SK",
                                }}
                            </button>
                            <button
                                class="btn btn--block btn--danger"
                                data-testid="more-logout"
                                on:click=on_logout
                            >
                                {move || i18n::t(lang.get(), "logout")}
                            </button>
                        </Sheet>
                    }.into_any()
                } else { ().into_any() }}
            }.into_any()
```

`title` and `testid` consume `String` (per Sheet's `#[prop(into)]`). `lang.get_untracked()` is fine for the title since it re-renders fresh on each open/close cycle.

- [ ] **Step 5: Add `.more-sheet__user` CSS rule**

Append to `spinbike-ui/style.css` (anywhere after the existing `.sheet__title` rules — search `grep -n '\.sheet__' spinbike-ui/style.css` to find a sensible adjacency):

```css
/* User-name line at the top of the More sheet (AdaptiveNav).
   Visually distinct from buttons below; matches sheet body padding. */
.more-sheet__user {
    font-size: var(--fs-md);
    color: var(--text);
    font-weight: 600;
    padding: var(--s-2) 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--s-3);
}
```

- [ ] **Step 6: Verify formatting**

```bash
cargo fmt --all --check
```

- [ ] **Step 7: Commit**

```bash
git add spinbike-ui/src/components/adaptive_nav.rs spinbike-ui/style.css
git commit -m "feat(ui): add 'More' tab to AdaptiveNav with username + lang toggle + logout sheet"
```

---

## Task 9: Reports row → `?card=` direct jump

**Files:**
- Modify: `spinbike-ui/src/pages/reports/activity_feed.rs` (`render_row`: gate on `e.barcode.is_some()`, navigate to `/staff?card=<bc>`)
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` (`?card=` parsing branch in the existing query-param Effect, calls `/api/cards/lookup/{barcode}`)

- [ ] **Step 1: Update `render_row` in activity_feed.rs**

In `spinbike-ui/src/pages/reports/activity_feed.rs`, locate `fn render_row(e: ReportEvent)` (around line 152). Find the existing block at lines 186-200:

```rust
    // Click → jump to Desk and pre-fill search by barcode (or name fallback).
    let q_value = e
        .barcode
        .clone()
        .or_else(|| e.card_name.clone())
        .unwrap_or_default();
    let on_row_click = move |_| {
        if q_value.is_empty() {
            return;
        }
        if let Some(w) = web_sys::window() {
            let encoded = url_encode(&q_value);
            let _ = w.location().set_href(&format!("/staff?q={encoded}"));
        }
    };
```

Replace it with:

```rust
    // Click → jump to Desk in exact-card mode (skips dropdown). Only
    // available when barcode is known: rows without a card_id (old or
    // orphan transactions) render presentationally, no click handler.
    let interactive = e.barcode.is_some();
    let row_class = if interactive {
        "list-row list-row--interactive"
    } else {
        "list-row"
    };
    let bc = e.barcode.clone();
    let on_row_click = move |_| {
        let Some(bc) = bc.clone() else { return; };
        if let Some(w) = web_sys::window() {
            let encoded = url_encode(&bc);
            let _ = w.location().set_href(&format!("/staff?card={encoded}"));
        }
    };
```

Then, in the `view!` block at the bottom of `render_row` (around line 222), the row currently uses a hard-coded class:

```rust
        <div class="list-row list-row--interactive" data-testid="feed-row"
             on:click=on_row_click>
```

Change it to use the `row_class` variable:

```rust
        <div class=row_class data-testid="feed-row"
             on:click=on_row_click>
```

(The `on:click` handler stays attached unconditionally — it's a no-op when `bc` is `None`, so attaching it does no harm. Cursor/hover styling is the only difference between interactive and non-interactive rows, controlled by the class.)

- [ ] **Step 2: Update the query-param Effect in dashboard/mod.rs**

In `spinbike-ui/src/pages/dashboard/mod.rs`, locate the existing prefill effect (lines 178-193):

```rust
    // Prefill search from `?q=…` query param (used by Reports → row click jump).
    Effect::new(move |_| {
        if let Some(w) = web_sys::window() {
            let search = w.location().search().unwrap_or_default();
            if let Some(stripped) = search.strip_prefix('?') {
                for kv in stripped.split('&') {
                    if let Some(rest) = kv.strip_prefix("q=") {
                        let decoded = decode_uri_component(rest);
                        if !decoded.is_empty() {
                            set_query.set(decoded);
                        }
                        break;
                    }
                }
            }
        }
    });
```

Replace it with a version that parses BOTH `?card=<bc>` (new, for direct lookup) and `?q=<text>` (existing, for search-prefill):

```rust
    // Parse query params used by the Reports → row click jump.
    //
    // * `?card=<barcode>` — exact lookup via /api/cards/lookup/{barcode};
    //   on success, the card panel opens directly (skips dropdown).
    // * `?q=<text>` — search prefill (existing behavior).
    //
    // `?card=` wins when both are present (defensive — Reports only
    // sets `?card=` since v0.13.15).
    Effect::new(move |_| {
        let Some(w) = web_sys::window() else { return; };
        let search = w.location().search().unwrap_or_default();
        let Some(stripped) = search.strip_prefix('?') else { return; };

        let mut card_param: Option<String> = None;
        let mut q_param: Option<String> = None;
        for kv in stripped.split('&') {
            if let Some(rest) = kv.strip_prefix("card=") {
                let decoded = decode_uri_component(rest);
                if !decoded.is_empty() {
                    card_param = Some(decoded);
                }
            } else if let Some(rest) = kv.strip_prefix("q=") {
                let decoded = decode_uri_component(rest);
                if !decoded.is_empty() {
                    q_param = Some(decoded);
                }
            }
        }

        if let Some(bc) = card_param {
            // Direct card lookup. On 404 (card deleted since report rendered),
            // fall back to populating the search box with the barcode so the
            // user sees the existing search-empty UX.
            spawn_local(async move {
                let encoded = urlencoding_light(&bc);
                match api::get::<CardInfo>(&format!("/api/cards/lookup/{encoded}")).await {
                    Ok(card) => {
                        set_selected.set(Some(card));
                        set_query.set(String::new());
                    }
                    Err(_) => {
                        set_query.set(bc);
                    }
                }
            });
        } else if let Some(q) = q_param {
            set_query.set(q);
        }
    });
```

The closure already has access to `set_query`, `set_selected`, `decode_uri_component`, `urlencoding_light`, `api`, `CardInfo`, and `spawn_local` via the outer scope.

- [ ] **Step 3: Verify formatting**

```bash
cargo fmt --all --check
```

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/reports/activity_feed.rs spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "feat(ui): Reports row click jumps directly to card panel via ?card="
```

---

## Task 10: New E2E test — `reports-row-jump.spec.ts`

**Files:**
- Create: `e2e/tests/reports-row-jump.spec.ts`

- [ ] **Step 1: Inspect existing helpers and test patterns**

```bash
cat e2e/tests/helpers.ts | head -80
grep -l 'loginViaAPI\|seedCard\|/api/admin/topup' e2e/tests/ 2>/dev/null | head -5
```

This shows you the available helpers (`loginViaAPI`, `setupConsoleCheck`, `assertCleanConsole`) and which existing tests already seed transactions you can model after. The `txn-note.spec.ts` (which uses `/api/admin/topup` and asserts on `feed-row`) is the closest reference.

- [ ] **Step 2: Write the test**

Create `e2e/tests/reports-row-jump.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Reports row → card panel direct jump', () => {
    test('clicking a feed row with a barcode opens the card panel without dropdown', async ({ page, request }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Log in admin via API and prep an authed session.
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // Seed a transaction tied to a known barcode so /reports definitely
        // has at least one clickable feed-row. We assume the test fixtures
        // provision cards (per existing tests like txn-note.spec.ts);
        // adjust the barcode/card_id below if the fixture uses different
        // values. The CI seed snapshot includes barcode "TESTCARD001".
        const seedResp = await request.post(`${BASE_URL}/api/admin/topup`, {
            data: { barcode: 'TESTCARD001', amount: 5.0 },
            headers: {
                Authorization: `Bearer ${await page.evaluate(() => localStorage.getItem('token'))}`,
            },
        });
        expect(seedResp.ok()).toBe(true);

        // Navigate to /reports today view — the seeded topup will be there.
        await page.goto(`${BASE_URL}/reports`);
        await page.locator('[data-testid="quick-today"]').click();

        // The first feed-row corresponds to the most recent transaction
        // (the topup we just seeded). It must have a barcode and be
        // interactive.
        const firstRow = page.locator('[data-testid="feed-row"]').first();
        await expect(firstRow).toBeVisible({ timeout: 10000 });
        await expect(firstRow).toHaveClass(/list-row--interactive/);

        // Click → URL changes to /staff?card=<barcode>
        await firstRow.click();
        await expect(page).toHaveURL(/\/staff\?card=/);

        // Card panel renders directly (no dropdown step).
        await expect(page.locator('[data-testid="card-panel"]')).toBeVisible({ timeout: 10000 });

        // The search-result dropdown is NOT shown (we skipped it).
        await expect(page.locator('[data-testid="search-result"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
```

> **Note on `[data-testid="card-panel"]`:** verify by grep at execution time that this exact testid is set on the card-action-panel element. If it's a different testid (e.g. `card-action-panel`), update the assertion accordingly:
> ```bash
> grep -rn 'data-testid' spinbike-ui/src/pages/dashboard/card_panel.rs | head -5
> ```

> **Note on the seed barcode:** the existing E2E suite uses test-DB fixtures. If `TESTCARD001` doesn't exist in the seed, find an existing barcode by:
> ```bash
> grep -rn "barcode\|TESTCARD" e2e/tests/global-setup.ts e2e/tests/helpers.ts e2e/tests/*.spec.ts 2>/dev/null | grep -iE 'barcode.*=.*"' | head
> ```
> Use the first one you find that's tied to a stable seeded card.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/reports-row-jump.spec.ts
git commit -m "test(e2e): assert Reports row click jumps to card panel via ?card="
```

---

## Task 11: Update `nav-adaptive.spec.ts` for 5th tab + hidden navbar + More sheet

**Files:**
- Modify: `e2e/tests/nav-adaptive.spec.ts`

- [ ] **Step 1: Update the mobile-viewport test**

Open `e2e/tests/nav-adaptive.spec.ts`. Replace the entire mobile test (`'bottom tabs on mobile viewport'`, currently lines 7-28) with:

```typescript
    test('bottom tabs on mobile viewport (with More sheet)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.setViewportSize({ width: 375, height: 812 });
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff');
        await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-desk"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-schedule"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-reports"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-settings"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-more"]')).toBeVisible();

        // Top navbar is hidden on phone for staff/admin (body:has rule).
        await expect(page.locator('.navbar')).toBeHidden();

        // Existing route-tab assertions
        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        await page.locator('[data-testid="nav-schedule"]').click();
        await expect(page).toHaveURL(/\/schedule/);
        await page.locator('[data-testid="nav-settings"]').click();
        await expect(page).toHaveURL(/\/settings/);
        await page.locator('[data-testid="nav-desk"]').click();
        await expect(page).toHaveURL(/\/staff$/);

        // 'More' sheet workflow: open → see username + lang toggle + logout
        await page.locator('[data-testid="nav-more"]').click();
        await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
        await expect(page.locator('[data-testid="more-lang-toggle"]')).toBeVisible();
        await expect(page.locator('[data-testid="more-logout"]')).toBeVisible();

        // Logging out from the sheet → redirects (post-logout default URL).
        await page.locator('[data-testid="more-logout"]').click();
        await page.waitForURL(/\/(login)?$/, { timeout: 5000 });

        assertCleanConsole(consoleMessages);
    });
```

> **Note on the post-logout URL:** the existing logout flow (in `components/nav.rs:21-27`) navigates to `/`. Verify by checking what the existing `auth.spec.ts` asserts after logout:
> ```bash
> grep -A 3 'on:click=on_logout\|Logout\|on_logout' e2e/tests/auth.spec.ts | head -20
> ```
> The assertion `/\/(login)?$/` matches both `/` and `/login`. If `auth.spec.ts` uses a more specific URL, mirror it.

- [ ] **Step 2: Update the desktop-viewport test**

Replace the existing desktop test (lines 30-39) with:

```typescript
    test('sidebar layout on desktop viewport (top navbar still visible)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.setViewportSize({ width: 1280, height: 800 });
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff');
        await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();

        // On desktop, the top navbar IS visible (the body:has hide rule
        // is gated by max-width: 540px).
        await expect(page.locator('.navbar')).toBeVisible();

        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        assertCleanConsole(consoleMessages);
    });
```

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/nav-adaptive.spec.ts
git commit -m "test(e2e): assert AdaptiveNav 5th 'More' tab + hidden top navbar on phone"
```

---

## Task 12: Push final commits, monitor CI to terminal, mitigate any surviving mutants, open PR

**Files:** none (operational task — `git push`, `gh run`, `gh pr create`)

- [ ] **Step 1: Final push**

Confirm working tree is clean and the branch is ahead of origin/dev:

```bash
git status
git log --oneline origin/dev..HEAD
```

You should see roughly 11 commits (Task 1 through Task 11). Push:

```bash
git push
```

- [ ] **Step 2: Identify the new run**

```bash
gh run list --branch dev --limit 3
```

Capture the LATEST run id (the one tied to the SHA you just pushed).

- [ ] **Step 3: Monitor to completion (single background command — per ci-monitoring.md)**

```bash
sleep 360 && gh run view <run-id> --json status,conclusion,jobs
```

Run this as a single `Bash(run_in_background: true)` invocation. When it returns, parse the JSON and check:

- `status` is `completed`
- All entries in `jobs[].conclusion` are `success` (any `failure` blocks the PR)

If still in progress, schedule another background `sleep N && gh run view ...`. Do NOT spam empty waiting messages.

The jobs to watch for terminal state include: `Test Integrity`, `Lint`, `Test`, `Test (UI)`, `Build WASM (UI)`, `E2E Tests`, `Mutation Testing`, `Mutation Testing (UI)`, `Deploy (dev)`, `Smoke (dev)`, `check-version-bump`.

- [ ] **Step 4: If a job fails, investigate and fix at root cause**

For any failed job:

```bash
gh run view <run-id> --log-failed
```

Common failures and the right fixes:

| Failure | Likely cause | Fix |
|---|---|---|
| `Mutation Testing` (server) — surviving mutant in a deleted helper | The mutant was caught only by a test we deleted; we have a test-coverage gap | Add a missing assertion to a different existing test in `crates/spinbike-server/tests/reports.rs` so the mutation is killed |
| `Mutation Testing (UI)` — surviving mutant in `activity_feed.rs::render_row` interactive flag | E2E test only covers the positive case (interactive=true) | Add an assertion in `reports-row-jump.spec.ts` (or a new test) that covers a row WITHOUT a barcode and asserts `:not(.list-row--interactive)` — but only if the seed shape supports it |
| `E2E Tests` — `[data-testid="card-panel"]` not found | The actual testid on `CardActionPanel` differs | Re-grep `spinbike-ui/src/pages/dashboard/card_panel.rs` for the correct testid; fix the assertion in `reports-row-jump.spec.ts` |
| `Test (UI)` — Leptos compile error in `adaptive_nav.rs` | Closure capture issue with `user_name`, `set_more_open`, etc. | Examine the compile error; the typical fix is to `.clone()` captured values inside the `move ||` closure |
| `Lint` — clippy warns | `cargo fmt` was missed earlier | Run `cargo fmt --all` (allowed locally), `git add -u` the affected files, and commit `fix: cargo fmt` |
| `check-version-bump` | Forgot Task 1 | Should never happen with this plan; if it does, bump VERSION + sync, commit, push |

NEVER:
- Bypass with `--admin`
- Skip a test with `#[ignore]` or `test.skip`
- Increase a timeout without root-cause analysis
- Cite "transient" or "flaky"

For surviving mutants: per project precedent, each surviving mutant requires a strengthened assertion. Identify the mutant from the cargo-mutants log, fix the corresponding test, push, monitor again. Repeat until ALL jobs green.

- [ ] **Step 5: Once ALL jobs green, open the PR**

```bash
gh pr create --base main --head dev \
    --title "v0.13.15: CEO dashboard optimization" \
    --body "$(cat <<'EOF'
## Summary

Four small, independent UI tightenings shipped as one PR:

- **Reports** — Drop the "Needs attention" banner (`AlertsBanner`) end-to-end: UI component, detail sheet, `/api/reports/alerts` endpoint, `alerts_count` field threaded through `/api/reports/{day,range}`, all related tests and CSS.
- **Reports** — Row clicks now navigate to `/staff?card=<barcode>` and the desk page calls `/api/cards/lookup/<barcode>` directly. The card panel opens immediately with no dropdown step. Rows without a barcode (orphan transactions) render as plain non-interactive rows.
- **Desk** — Drop the `NowPanel` ("next class") widget end-to-end: UI component, `/api/reports/now` endpoint, types, tests, CSS.
- **Phone** — Hide the top navbar on phone for logged-in staff/admin (`body:has(.adaptive-nav) .navbar { display: none; }` inside the existing 540px media query). Add a 5th "More" tab to the bottom AdaptiveNav that opens a sheet with username + EN/SK toggle + Logout. Customers and desktop layout unchanged.

VERSION 0.13.14 → 0.13.15 on first commit per `version-bumping.md`.

## Test plan

- [x] CI green: Test Integrity, Lint, Test, Test (UI), Build WASM (UI), E2E Tests, Mutation Testing, Mutation Testing (UI), Deploy (dev), Smoke (dev), check-version-bump
- [ ] Post-deploy: dev frontend `[data-testid="version"]` reads `v0.13.15`, matches `/api/version`
- [ ] Manual / E2E spot-checks on dev:
  - `/reports` page has no "Needs attention" banner
  - Clicking a report row with a barcode lands directly in the card panel (no dropdown click)
  - `/staff` page shows the search input immediately under the title (no NowPanel)
  - On phone viewport (375×812), top navbar hidden when logged in as admin; bottom bar shows 5 items including "More"; the More sheet contains username + EN/SK + Logout
- [ ] After merge: prod at https://spinbike.newlevel.media verified the same way

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 6: Verify PR is mergeable + clean**

```bash
gh pr view --json number,mergeable,mergeStateStatus,url
```

Expected:
- `mergeable: "MERGEABLE"`
- `mergeStateStatus: "CLEAN"` (NOT `UNSTABLE`, NOT `BEHIND`, NOT `BLOCKED`, NOT `DIRTY`)

If `BEHIND`: sync dev with main first (`git fetch origin && git merge origin/main`, push). If `UNSTABLE`: a check is failing — go back to Step 4. Per `pr-merge-policy.md` and `autonomous-quality-discipline.md`, NEVER bypass.

- [ ] **Step 7: STOP — do not merge**

End at "PR mergeable, awaiting user merge". Per `pr-merge-policy.md`, NEVER merge a PR without explicit user instruction. Provide the green PR URL and wait.

---

## Task 13: Post-deploy verification (runs ONLY after user merges)

**Files:** none (operational — Playwright + curl against the live targets)

> This task runs in a NEW session after the user merges PR. Do NOT execute this task in the same flow as Tasks 1-12.

- [ ] **Step 1: Wait for the main-branch CI run to complete**

```bash
gh run list --branch main --limit 3
sleep 360 && gh run view <main-run-id> --json status,conclusion,jobs
```

All jobs (including Deploy (dev), Deploy (prod), Smoke (dev), Smoke (prod)) must be `success`.

- [ ] **Step 2: Verify dev deploy via Playwright + curl**

Open the dev frontend and read the version label from the DOM:

```bash
curl -s https://spinbike-dev.newlevel.media/api/version
# Expected: {"version":"0.13.15"}
```

Then via Playwright (browser automation):

```javascript
await page.goto('https://spinbike-dev.newlevel.media/login');
const labelText = await page.locator('[data-testid="version"]').textContent();
// Expected: 'v0.13.15'
```

Spot-check the four UI changes on dev:

1. **Reports has no banner**: `await page.goto('https://spinbike-dev.newlevel.media/reports'); await expect(page.locator('[data-testid="alerts-banner"]')).toHaveCount(0);`
2. **Desk has no NowPanel**: `await page.goto('https://spinbike-dev.newlevel.media/staff'); await expect(page.locator('[data-testid="now-panel"]')).toHaveCount(0);`
3. **Phone navbar hidden**: `await page.setViewportSize({ width: 375, height: 812 }); await page.goto('...'); await expect(page.locator('.navbar')).toBeHidden(); await expect(page.locator('[data-testid="nav-more"]')).toBeVisible();`
4. **Reports row jump works**: navigate to /reports, click first feed-row, assert URL became `/staff?card=...` and card panel is visible.

Browser console MUST have zero errors/warnings throughout.

- [ ] **Step 3: Verify prod deploy via Playwright + curl**

Repeat Step 2 against `https://spinbike.newlevel.media`.

- [ ] **Step 4: Send completion report**

Use the EXACT template from `completion-report.md`:

```
## ✅ Work Complete

**Audits & deploy:**
✅ CI: green (push <SHA> + main <main-SHA>)
✅ /plan-check: 13/13 fulfilled
✅ /review: clean — 0 🔴 0 🟡 0 🔵
✅ Deploy: dev + prod frontends `[data-testid="version"]` read `v0.13.15`, match `/api/version`. AlertsBanner / NowPanel removed; report-row jump opens card panel directly; phone top navbar hidden + More sheet works.

---

**Goal:** Trim wasted UI surface for the CEO's daily card-management workflow.
**What changed:** Reports drops "Needs attention" + jumps directly to card panel; Desk drops the next-class widget; phone top navbar hidden in favor of bottom-bar More sheet.

🌐 Dev:  https://spinbike-dev.newlevel.media
🌐 Prod: https://spinbike.newlevel.media

**[spinbike] PR #<N>: v0.13.15: CEO dashboard optimization**
<full PR URL> — merged at <merge-SHA>.
```

---

## Self-review checklist (planner-only — runs once before commit)

**1. Spec coverage:**
- ✅ Spec §1 (delete AlertsBanner) → Tasks 6 + 7
- ✅ Spec §2 (Reports row direct jump) → Task 9 + tests in Task 10
- ✅ Spec §3 (delete NowPanel) → Tasks 4 + 5
- ✅ Spec §4 (phone navbar + More sheet) → Tasks 2 + 3 + 8 + tests in Task 11
- ✅ Cross-cutting version bump → Task 1
- ✅ Cross-cutting CI monitoring + PR + post-deploy → Tasks 12 + 13

**2. Placeholder scan:**
- No "TBD", "TODO", "implement later", "fill in details"
- No "similar to Task N" — each code chunk is repeated where needed
- Code blocks present in every code-changing step

**3. Type consistency:**
- `data-testid="nav-more"` — used in Task 8 (created), Task 11 (asserted)
- `data-testid="more-sheet"` — used in Task 8 (set on `<Sheet testid=...>`), Task 11 (asserted)
- `data-testid="more-logout"` / `more-lang-toggle` — same pairing
- `?card=<barcode>` URL — produced in Task 9 (`activity_feed.rs`), consumed in Task 9 (`dashboard/mod.rs`), asserted in Task 10
- `nav_more` i18n key — added in Task 2, consumed in Task 8

**4. Local-build constraint:**
- No step asks the subagent to run `cargo test/build/clippy` or `trunk build`
- Only allowed local check (`cargo fmt --all --check`) appears in Tasks 2, 5, 6, 7, 8, 9
- All other verification deferred to CI in Task 12
