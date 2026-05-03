# Per-card Overview tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a staff-only "Prehľad / Overview" tab to the card panel showing visits + topped-up totals across this-month / this-year / all-time plus two 12-month single-series bar charts.

**Architecture:** One new endpoint `GET /api/cards/{id}/stats` aggregates a card's transactions in 1 SQL round-trip. A new shared type module in `spinbike-core` carries the response shape so the WASM frontend deserializes without re-defining types. The frontend adds one Leptos component + ~40 lines of CSS for pure-CSS bars — no chart library.

**Tech Stack:** Axum 0.8, sqlx 0.8 (SQLite), Leptos 0.7 (CSR/WASM), pure CSS, Playwright.

**Spec:** `docs/superpowers/specs/2026-05-03-per-card-overview-design.md` (committed at `0da68c2` on `dev`).

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `VERSION` | Modify | Bump 0.13.17 → 0.13.18 |
| `Cargo.toml`, `spinbike-ui/Cargo.toml` | Modify (via `bash scripts/sync-version.sh`) | Mirror version |
| `crates/spinbike-core/src/stats.rs` | **Create** | `StatsResponse`, `PeriodTotals`, `PeriodAgg`, `MonthlyBucket` types |
| `crates/spinbike-core/src/lib.rs` | Modify | Add `pub mod stats;` |
| `crates/spinbike-server/src/routes/cards.rs` | Modify (~80 LOC added) | New `card_stats` handler + route registration |
| `crates/spinbike-server/tests/cards_stats.rs` | **Create** | 6 integration tests |
| `spinbike-ui/src/i18n.rs` | Modify | 9 new translation keys |
| `spinbike-ui/src/pages/dashboard/overview_tab.rs` | **Create** | `OverviewTab` Leptos component |
| `spinbike-ui/src/pages/dashboard/mod.rs` | Modify | Register module + re-export |
| `spinbike-ui/src/pages/dashboard/card_panel.rs` | Modify (lines 40-44, 147-168) | 4th tab item + match arm |
| `spinbike-ui/style.css` | Modify (append) | `.stats-kpi`, `.stats-chart`, `.stats-row*` rules |
| `e2e/tests/per-card-overview.spec.ts` | **Create** | One Playwright test |

---

## Spec corrections (apply during implementation)

The spec used `name_sk IN ('Spinning','Fitness')` and `claims.role.can_process_payments()` based on a quick read of the codebase. Audit found:

1. The codebase already exposes `spinbike_core::services::CLASS_VISIT_NAMES_EN: &[&str] = &["Fitness","Spinning"]` (single source of truth, immune to issue #50). **Use `name_en` and these constants in the SQL.**
2. Every existing handler in `crates/spinbike-server/src/routes/cards.rs` gates on `claims.role.can_manage_cards()`. **Use `can_manage_cards()`** for consistency with the rest of the file.

Both corrections are reflected in every code block below.

---

## Task 1: Bump VERSION 0.13.17 → 0.13.18 *(controller-run, not a subagent task)*

**Files:**
- Modify: `VERSION` (top-level, single line)
- Modify (via script): `Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Edit VERSION**

```bash
echo "0.13.18" > VERSION
```

- [ ] **Step 2: Sync the version into all Cargo.toml files**

```bash
bash scripts/sync-version.sh
```

Expected: script prints which files it touched; all `version = "..."` lines now read `"0.13.18"`.

- [ ] **Step 3: Verify the change**

```bash
git diff VERSION Cargo.toml spinbike-ui/Cargo.toml
```

Expected: only the version string changes.

- [ ] **Step 4: Commit (explicit paths — never `git add -A`)**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "chore: bump version to 0.13.18"
```

---

## Task 2: Add shared types in `spinbike-core/src/stats.rs`

**Files:**
- Create: `crates/spinbike-core/src/stats.rs`
- Modify: `crates/spinbike-core/src/lib.rs` (add `pub mod stats;`)

- [ ] **Step 1: Write the failing test**

Append at the bottom of the new `stats.rs` file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StatsResponse {
        StatsResponse {
            totals: PeriodTotals {
                this_month: PeriodAgg { visits: 11, topped_up_eur: 50.0 },
                this_year:  PeriodAgg { visits: 47, topped_up_eur: 200.0 },
                all_time:   PeriodAgg { visits: 812, topped_up_eur: 3000.0 },
            },
            monthly: vec![MonthlyBucket {
                year_month: "2026-05".to_string(),
                visits: 11,
                topped_up_eur: 30.0,
            }],
        }
    }

    #[test]
    fn round_trip_via_serde_json() {
        let original = sample();
        let json = serde_json::to_string(&original).unwrap();
        let back: StatsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn json_uses_snake_case_field_names() {
        let json = serde_json::to_string(&sample()).unwrap();
        // Pin the wire format. The WASM frontend deserializes by these exact
        // keys; renaming a field would silently break the UI.
        assert!(json.contains("\"this_month\""));
        assert!(json.contains("\"topped_up_eur\""));
        assert!(json.contains("\"year_month\""));
    }
}
```

- [ ] **Step 2: Confirm the test FAILS**

Run via subagent: `cargo fmt --all --check` (passing) — actual compile happens on CI.

CI is authoritative; test failure will be visible there as `cargo test` errors complaining `StatsResponse` is undefined.

- [ ] **Step 3: Write minimal implementation**

`crates/spinbike-core/src/stats.rs` — full file:

```rust
//! Shared types for GET /api/cards/{id}/stats.
//!
//! Both the server (which serializes them) and the WASM client (which
//! deserializes them) depend on this module. Keep WASM-safe — no tokio,
//! no sqlx — same constraint as `reports.rs`.

use serde::{Deserialize, Serialize};

/// Aggregated visits + top-ups for one named time window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodAgg {
    pub visits: i64,
    pub topped_up_eur: f64,
}

/// Three named windows the Overview tab displays as KPI rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeriodTotals {
    pub this_month: PeriodAgg,
    pub this_year: PeriodAgg,
    pub all_time: PeriodAgg,
}

/// One monthly bar in the chart. The server fills zero-buckets for months
/// with no rows so the UI can render exactly 12 entries unconditionally.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonthlyBucket {
    /// Calendar month label "YYYY-MM" in the server's local timezone.
    pub year_month: String,
    pub visits: i64,
    pub topped_up_eur: f64,
}

/// Response from GET /api/cards/{id}/stats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatsResponse {
    pub totals: PeriodTotals,
    /// Exactly 12 entries, oldest → newest. Zero-buckets included.
    pub monthly: Vec<MonthlyBucket>,
}
```

Then add to `crates/spinbike-core/src/lib.rs` after line 3:

```rust
pub mod auth;
pub mod models;
pub mod reports;
pub mod services;
pub mod stats;        // ← NEW
pub mod ws;
```

- [ ] **Step 4: Confirm tests will pass on CI**

`cargo fmt --all --check` locally. CI runs the unit tests.

- [ ] **Step 5: Commit (explicit paths only)**

```bash
git add crates/spinbike-core/src/stats.rs crates/spinbike-core/src/lib.rs
git commit -m "feat(core): add shared StatsResponse types for /api/cards/{id}/stats"
```

---

## Task 3: Implement `GET /api/cards/{id}/stats` server handler with full unit-test coverage

**Files:**
- Modify: `crates/spinbike-server/src/routes/cards.rs` (add ~80 lines: imports, handler, route registration)
- Create: `crates/spinbike-server/tests/cards_stats.rs` (new integration test file, ~250 lines)

- [ ] **Step 1: Write the failing tests first**

Create `crates/spinbike-server/tests/cards_stats.rs`:

```rust
//! Integration tests for GET /api/cards/{id}/stats.

mod helpers;

use chrono::Datelike;
use helpers::{TestApp, get};
use spinbike_core::stats::StatsResponse;

/// Insert a transaction row at a chosen `created_at`. Service is identified
/// by name_en so callers can mix Spinning, Fitness, Refreshments freely.
async fn seed_txn(
    pool: &sqlx::SqlitePool,
    card_id: i64,
    service_name_en: Option<&str>,
    amount: f64,
    action: &str,
    created_at: &str,
) {
    let service_id: Option<i64> = if let Some(n) = service_name_en {
        Some(
            sqlx::query_scalar("SELECT id FROM services WHERE name_en = ?")
                .bind(n)
                .fetch_one(pool)
                .await
                .unwrap(),
        )
    } else {
        None
    };
    sqlx::query(
        "INSERT INTO transactions (card_id, service_id, amount, action, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(card_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_stats(app: &TestApp, card_id: i64) -> (axum::http::StatusCode, StatsResponse) {
    app.request_typed::<StatsResponse>(get(
        &format!("/api/cards/{card_id}/stats"),
        &app.staff_token,
    ))
    .await
}

#[tokio::test]
async fn empty_card_returns_zero_totals_and_twelve_zero_buckets() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("EMPTY", 0.0, None, None, None, None).await;

    let (status, resp) = get_stats(&app, card_id).await;

    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(resp.totals.this_month.visits, 0);
    assert_eq!(resp.totals.this_month.topped_up_eur, 0.0);
    assert_eq!(resp.totals.this_year.visits, 0);
    assert_eq!(resp.totals.all_time.visits, 0);
    assert_eq!(resp.monthly.len(), 12);
    for b in &resp.monthly {
        assert_eq!(b.visits, 0);
        assert_eq!(b.topped_up_eur, 0.0);
    }
    // Newest bucket must be the current calendar month.
    let now = chrono::Local::now();
    let expected_last = format!("{:04}-{:02}", now.year(), now.month());
    assert_eq!(resp.monthly.last().unwrap().year_month, expected_last);
}

#[tokio::test]
async fn mixed_services_count_only_spinning_and_fitness_as_visits() {
    // Kills "drop the Spinning/Fitness filter" mutant.
    let app = TestApp::new().await;
    let card_id = app.seed_card("MIX1", 0.0, None, None, None, None).await;

    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d %H:%M:%S").to_string();

    seed_txn(&app.pool, card_id, Some("Spinning"), -3.30, "charge", &today).await;
    seed_txn(&app.pool, card_id, Some("Fitness"), -5.00, "charge", &today).await;
    // Refreshments / Supplements / Card-activation: services that exist on a
    // card's history but MUST NOT be counted as visits.
    seed_txn(&app.pool, card_id, Some("Refreshments"), -2.0, "charge", &today).await;
    seed_txn(&app.pool, card_id, Some("Supplements"), -10.0, "charge", &today).await;
    seed_txn(&app.pool, card_id, Some("Card activation fee"), -1.0, "charge", &today).await;
    // Pass purchase: amount<0, action=charge, but kind=monthly_pass — must NOT
    // count as a visit either.
    seed_txn(&app.pool, card_id, Some("Monthly pass"), -35.0, "charge", &today).await;

    let (_, resp) = get_stats(&app, card_id).await;

    assert_eq!(resp.totals.this_month.visits, 2);
    assert_eq!(resp.totals.all_time.visits, 2);
}

#[tokio::test]
async fn topup_count_excludes_zero_amount_and_non_topup_actions() {
    // Kills `>` → `>=` mutant on the amount filter (zero must NOT count)
    // AND kills "drop the action='topup' filter" mutant (a positive-amount
    // legacy charge row must NOT count as a top-up).
    let app = TestApp::new().await;
    let card_id = app.seed_card("MIX2", 0.0, None, None, None, None).await;

    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d %H:%M:%S").to_string();

    seed_txn(&app.pool, card_id, None, 10.0, "topup", &today).await;     // counts: 10
    seed_txn(&app.pool, card_id, None, 25.0, "topup", &today).await;     // counts: +25
    seed_txn(&app.pool, card_id, None,  0.0, "topup", &today).await;     // EXCLUDED (amount=0)
    seed_txn(&app.pool, card_id, Some("Spinning"), 7.0, "charge", &today).await; // EXCLUDED (action!=topup)

    let (_, resp) = get_stats(&app, card_id).await;
    assert_eq!(resp.totals.this_month.topped_up_eur, 35.0);
    assert_eq!(resp.totals.all_time.topped_up_eur, 35.0);
}

#[tokio::test]
async fn multi_month_buckets_align_correctly() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("MULTI", 0.0, None, None, None, None).await;

    let now = chrono::Local::now();
    // 2 visits this month
    let this_month = now.format("%Y-%m-15 12:00:00").to_string();
    seed_txn(&app.pool, card_id, Some("Spinning"), -3.30, "charge", &this_month).await;
    seed_txn(&app.pool, card_id, Some("Fitness"), -5.0, "charge", &this_month).await;
    // 1 visit 2 months ago
    let two_months_ago = (now - chrono::Duration::days(63))
        .format("%Y-%m-15 12:00:00")
        .to_string();
    seed_txn(&app.pool, card_id, Some("Spinning"), -3.30, "charge", &two_months_ago).await;

    let (_, resp) = get_stats(&app, card_id).await;
    assert_eq!(resp.monthly.last().unwrap().visits, 2);
    let two_months_label = format!(
        "{:04}-{:02}",
        (now - chrono::Duration::days(63)).year(),
        (now - chrono::Duration::days(63)).month()
    );
    let two_months_bucket = resp.monthly.iter().find(|b| b.year_month == two_months_label);
    assert_eq!(two_months_bucket.map(|b| b.visits), Some(1));
    assert_eq!(resp.totals.this_year.visits, 3);
    assert_eq!(resp.totals.all_time.visits, 3);
}

#[tokio::test]
async fn visits_older_than_twelve_months_excluded_from_chart_but_in_all_time() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("OLD", 0.0, None, None, None, None).await;

    // 18 months ago — outside the 12-month window.
    let eighteen_mo_ago = (chrono::Local::now() - chrono::Duration::days(548))
        .format("%Y-%m-15 12:00:00")
        .to_string();
    seed_txn(&app.pool, card_id, Some("Spinning"), -3.30, "charge", &eighteen_mo_ago).await;

    let (_, resp) = get_stats(&app, card_id).await;
    let total_in_chart: i64 = resp.monthly.iter().map(|b| b.visits).sum();
    assert_eq!(total_in_chart, 0);
    assert_eq!(resp.totals.all_time.visits, 1);
}

#[tokio::test]
async fn soft_deleted_rows_excluded() {
    let app = TestApp::new().await;
    let card_id = app.seed_card("SOFT", 0.0, None, None, None, None).await;
    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Insert a Spinning visit, then mark it soft-deleted.
    seed_txn(&app.pool, card_id, Some("Spinning"), -3.30, "charge", &now_str).await;
    sqlx::query(
        "UPDATE transactions SET deleted_at = datetime('now')
         WHERE card_id = ? AND service_id = (SELECT id FROM services WHERE name_en='Spinning')",
    )
    .bind(card_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // Insert a top-up, mark it soft-deleted.
    seed_txn(&app.pool, card_id, None, 10.0, "topup", &now_str).await;
    sqlx::query("UPDATE transactions SET deleted_at = datetime('now') WHERE action='topup' AND card_id = ?")
        .bind(card_id)
        .execute(&app.pool)
        .await
        .unwrap();

    let (_, resp) = get_stats(&app, card_id).await;
    assert_eq!(resp.totals.all_time.visits, 0);
    assert_eq!(resp.totals.all_time.topped_up_eur, 0.0);
}

#[tokio::test]
async fn customer_role_forbidden() {
    // Defence in depth: even though the dashboard never exposes this
    // endpoint to customers, a guessed URL must not leak data.
    let app = TestApp::new().await;
    let card_id = app.seed_card("FORBID", 0.0, None, None, None, None).await;

    let (status, _) = app
        .request(get(&format!("/api/cards/{card_id}/stats"), &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
```

- [ ] **Step 2: Confirm the tests FAIL**

(CI is authoritative — locally just check with `cargo fmt --all --check`.) The tests reference `StatsResponse` from `spinbike_core::stats` (Task 2 already lands it) and `/api/cards/{card_id}/stats` (this task adds it).

- [ ] **Step 3: Write the handler — modify `crates/spinbike-server/src/routes/cards.rs`**

Add this `use` near the top, in the existing `use` block (line 1-13 of cards.rs):

```rust
use spinbike_core::services::CLASS_VISIT_NAMES_EN;
use spinbike_core::stats::{MonthlyBucket, PeriodAgg, PeriodTotals, StatsResponse};
```

Wire the new route inside `pub fn routes()` (the function that starts at line 163). Append one line BEFORE the trailing `}`:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards", get(list_cards))
        .route("/api/cards/search", get(search_cards))
        .route("/api/cards/link", post(link_card))
        .route("/api/cards/lookup/{barcode}", get(lookup_card))
        .route("/api/cards/activate", post(activate_card))
        .route("/api/cards/topup", post(topup_card))
        .route("/api/cards/block", post(block_card))
        .route("/api/cards/{id}", put(update_card))
        .route("/api/cards/{id}/transactions", get(card_transactions))
        .route("/api/cards/{id}/stats", get(card_stats))   // ← NEW LINE
        .route("/api/my/balance", get(my_balance))
}
```

Add the new handler function. Place it AFTER `card_transactions` (around line 528) and BEFORE `my_balance`:

```rust
async fn card_stats(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // Build the IN-clause placeholders dynamically from the constants. With
    // 2 entries today this is a 2-placeholder string; if CLASS_VISIT_NAMES_EN
    // grows (e.g. add HIIT), no SQL change is needed.
    let placeholders: String = std::iter::repeat("?")
        .take(CLASS_VISIT_NAMES_EN.len())
        .collect::<Vec<_>>()
        .join(",");
    let visit_filter_sql = format!(
        "service_id IN (SELECT id FROM services WHERE name_en IN ({placeholders}))"
    );

    // ── Totals: one row, six numbers, three time windows ────────────────
    let totals_sql = format!(
        "SELECT
            COALESCE(SUM(CASE WHEN {visit_filter} AND deleted_at IS NULL
                              AND strftime('%Y-%m', created_at, 'localtime') =
                                  strftime('%Y-%m','now','localtime')
                         THEN 1 ELSE 0 END), 0) AS visits_month,
            COALESCE(SUM(CASE WHEN action='topup' AND amount > 0 AND deleted_at IS NULL
                              AND strftime('%Y-%m', created_at, 'localtime') =
                                  strftime('%Y-%m','now','localtime')
                         THEN amount ELSE 0 END), 0.0) AS topup_month,
            COALESCE(SUM(CASE WHEN {visit_filter} AND deleted_at IS NULL
                              AND strftime('%Y',    created_at, 'localtime') =
                                  strftime('%Y','now','localtime')
                         THEN 1 ELSE 0 END), 0) AS visits_year,
            COALESCE(SUM(CASE WHEN action='topup' AND amount > 0 AND deleted_at IS NULL
                              AND strftime('%Y',    created_at, 'localtime') =
                                  strftime('%Y','now','localtime')
                         THEN amount ELSE 0 END), 0.0) AS topup_year,
            COALESCE(SUM(CASE WHEN {visit_filter} AND deleted_at IS NULL
                         THEN 1 ELSE 0 END), 0) AS visits_all,
            COALESCE(SUM(CASE WHEN action='topup' AND amount > 0 AND deleted_at IS NULL
                         THEN amount ELSE 0 END), 0.0) AS topup_all
         FROM transactions
         WHERE card_id = ?",
        visit_filter = visit_filter_sql
    );

    let mut totals_q = sqlx::query_as::<_, (i64, f64, i64, f64, i64, f64)>(&totals_sql);
    // The visit-filter sub-clause appears 3 times (month / year / all). Bind
    // the class-name placeholders 3 times, in the same order.
    for _ in 0..3 {
        for n in CLASS_VISIT_NAMES_EN {
            totals_q = totals_q.bind(*n);
        }
    }
    totals_q = totals_q.bind(id);
    let (vm, tm, vy, ty, va, ta) = totals_q
        .fetch_one(&state.pool)
        .await
        .map_err(internal_error)?;

    // ── Monthly buckets: 12 rows aligned to the last 12 calendar months ─
    // Build the 12 expected (year, month) labels in Rust — easier than
    // SQLite recursive CTEs and avoids locale-sensitive month arithmetic.
    let now = chrono::Local::now();
    let mut labels: Vec<String> = Vec::with_capacity(12);
    for i in (0..12).rev() {
        // Walk back i full months from the current calendar month.
        let mut year = now.year();
        let mut month = now.month() as i32 - i as i32;
        while month < 1 {
            month += 12;
            year -= 1;
        }
        labels.push(format!("{:04}-{:02}", year, month));
    }
    let oldest_label = labels.first().unwrap().clone();
    // SQLite returns rows only for months that have data; we LEFT-JOIN-style
    // join in Rust against the 12-row label series.
    let bucket_rows: Vec<(String, i64, f64)> = sqlx::query_as(
        &format!(
            "SELECT
                strftime('%Y-%m', created_at, 'localtime') AS ym,
                SUM(CASE WHEN {visit_filter} THEN 1 ELSE 0 END) AS visits,
                SUM(CASE WHEN action='topup' AND amount > 0 THEN amount ELSE 0 END) AS topped_up
             FROM transactions
             WHERE card_id = ?
               AND deleted_at IS NULL
               AND strftime('%Y-%m', created_at, 'localtime') >= ?
             GROUP BY ym",
            visit_filter = visit_filter_sql
        ),
    )
    .bind(CLASS_VISIT_NAMES_EN[0])
    .bind(CLASS_VISIT_NAMES_EN[1])
    .bind(id)
    .bind(&oldest_label)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;
    // Note: above .bind() block hardcodes 2 binds because CLASS_VISIT_NAMES_EN
    // currently has 2 entries. If the constant grows, change to a loop:
    //   let mut q = sqlx::query_as(&...);
    //   for n in CLASS_VISIT_NAMES_EN { q = q.bind(*n); }
    //   q.bind(id).bind(&oldest_label).fetch_all(...).await
    // The unit test `mixed_services_count_only_spinning_and_fitness_as_visits`
    // will fail loudly if names drift.

    let monthly: Vec<MonthlyBucket> = labels
        .into_iter()
        .map(|ym| {
            let row = bucket_rows.iter().find(|r| r.0 == ym);
            MonthlyBucket {
                visits: row.map(|r| r.1).unwrap_or(0),
                topped_up_eur: row.map(|r| r.2).unwrap_or(0.0),
                year_month: ym,
            }
        })
        .collect();

    Ok(Json(StatsResponse {
        totals: PeriodTotals {
            this_month: PeriodAgg { visits: vm, topped_up_eur: tm },
            this_year:  PeriodAgg { visits: vy, topped_up_eur: ty },
            all_time:   PeriodAgg { visits: va, topped_up_eur: ta },
        },
        monthly,
    }))
}
```

**Note for the implementer:** SQLite returns `SUM` as `Option<i64>`/`Option<f64>` when there are no input rows. The totals query's `COALESCE(..., 0)` keeps each column non-null so `query_as::<_, (i64, f64, ...)>` decodes cleanly. The bucket query's `GROUP BY ym` guarantees at least one input row per group, so its inner `SUM(CASE WHEN ... THEN 1 ELSE 0 END)` is always non-null. If sqlx complains about nullability at runtime, wrap each bucket-query SUM in `COALESCE(..., 0)` too — same treatment as the totals query.

- [ ] **Step 4: Confirm tests pass on CI**

`cargo fmt --all --check`. Push will trigger CI which runs the full test suite (Task 7 covers push).

- [ ] **Step 5: Commit (explicit paths)**

```bash
git add crates/spinbike-server/src/routes/cards.rs \
        crates/spinbike-server/tests/cards_stats.rs
git commit -m "feat(server): GET /api/cards/{id}/stats with full unit-test coverage"
```

---

## Task 4: Add i18n keys

**Files:**
- Modify: `spinbike-ui/src/i18n.rs` (insert into the existing `TRANSLATIONS` map)

- [ ] **Step 1: Locate the insertion point**

Existing card-detail tab keys live around lines 544-547:

```rust
// Card detail tabs
m.insert("tab_history", ("Historia", "History"));
m.insert("tab_upcoming", ("Pripravovane", "Upcoming"));
m.insert("tab_persistent", ("Opakovane", "Persistent"));
```

- [ ] **Step 2: Add 9 new keys directly below `tab_persistent`**

```rust
// Card detail tabs
m.insert("tab_history", ("Historia", "History"));
m.insert("tab_upcoming", ("Pripravovane", "Upcoming"));
m.insert("tab_persistent", ("Opakovane", "Persistent"));
m.insert("tab_overview",   ("Prehlad",   "Overview"));   // ← NEW

// Overview tab — KPI table + bar charts
m.insert("overview_period_month", ("Tento mesiac", "This month"));
m.insert("overview_period_year",  ("Tento rok",    "This year"));
m.insert("overview_period_all",   ("Spolu",        "All time"));
m.insert("overview_col_visits",   ("Vstupy",       "Visits"));
m.insert("overview_col_topup",    ("Dobitie",      "Topped up"));
m.insert("overview_chart_visits", ("Vstupy po mesiacoch",       "Visits per month"));
m.insert("overview_chart_topup",  ("Dobitie po mesiacoch (\u{20ac})", "Topped up per month (\u{20ac})"));
m.insert("overview_loading",      ("Nacitavam stat...",         "Loading..."));
```

(Slovak strings use unaccented forms to match the existing convention — see e.g. `"Pripravovane"`, `"Opakovane"`. No diacritics in the table; the small SK-EN inconsistency is project-wide and intentional.)

- [ ] **Step 3: Confirm formatting**

```bash
cargo fmt --all --check
```

Expected: no diff.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "feat(i18n): add tab_overview + 8 keys for per-card Overview tab"
```

---

## Task 5: Implement `OverviewTab` Leptos component + CSS

**Files:**
- Create: `spinbike-ui/src/pages/dashboard/overview_tab.rs`
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` (line 6-13: add module to `pub mod` list)
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs` (lines 40-44 and 147-168)
- Modify: `spinbike-ui/style.css` (append CSS block)

- [ ] **Step 1: Create the new component file**

`spinbike-ui/src/pages/dashboard/overview_tab.rs`:

```rust
//! Per-card Overview tab — KPI grid + 12-month bar charts.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::stats::{MonthlyBucket, StatsResponse};

#[component]
pub fn OverviewTab(card_id: i64) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (stats, set_stats) = signal(None::<StatsResponse>);
    let (loading, set_loading) = signal(true);

    Effect::new(move |_| {
        spawn_local(async move {
            match api::get::<StatsResponse>(&format!("/api/cards/{card_id}/stats")).await {
                Ok(s) => {
                    set_stats.set(Some(s));
                    set_loading.set(false);
                }
                Err(_) => {
                    // Silent failure — UI shows nothing rather than spamming
                    // a global error banner for a side panel. Console errors
                    // (network, 5xx) still surface via Playwright assertions.
                    set_loading.set(false);
                }
            }
        });
    });

    view! {
        {move || {
            if loading.get() {
                return view! {
                    <div class="empty-state" data-testid="overview-loading">
                        {move || i18n::t(lang.get(), "overview_loading")}
                    </div>
                }.into_any();
            }
            let Some(s) = stats.get() else {
                return view! { <div data-testid="overview-empty"></div> }.into_any();
            };

            let l = lang.get();
            let row = |label_key: &'static str, visits: i64, topped: f64| {
                view! {
                    <tr>
                        <td>{move || i18n::t(lang.get(), label_key)}</td>
                        <td data-testid={format!("overview-visits-{label_key}")}>{visits}</td>
                        <td data-testid={format!("overview-topup-{label_key}")}>{format!("{:.2} \u{20ac}", topped)}</td>
                    </tr>
                }
            };

            let visits_max = s.monthly.iter().map(|b| b.visits).max().unwrap_or(0);
            let topup_max = s.monthly.iter().map(|b| b.topped_up_eur).fold(0.0_f64, f64::max);

            // Display newest first so the chart matches the History tab's
            // top-down recency. The API gives oldest→newest so reverse here.
            let mut visits_rows: Vec<&MonthlyBucket> = s.monthly.iter().collect();
            visits_rows.reverse();
            let topup_rows = visits_rows.clone();

            let visit_bar = |b: &MonthlyBucket| {
                let pct = if visits_max > 0 { b.visits as f64 / visits_max as f64 * 100.0 } else { 0.0 };
                let label = fmt_year_month(&b.year_month, l);
                let value = b.visits;
                view! {
                    <div class="stats-row" data-testid="stats-visits-row">
                        <span class="stats-row__label">{label}</span>
                        <div class="stats-row__bar-wrap">
                            <div class="stats-row__bar" style=format!("width: {:.1}%", pct)></div>
                        </div>
                        <span class="stats-row__value">{value}</span>
                    </div>
                }
            };
            let topup_bar = |b: &MonthlyBucket| {
                let pct = if topup_max > 0.0 { b.topped_up_eur / topup_max * 100.0 } else { 0.0 };
                let label = fmt_year_month(&b.year_month, l);
                let value = format!("{:.2} \u{20ac}", b.topped_up_eur);
                view! {
                    <div class="stats-row" data-testid="stats-topup-row">
                        <span class="stats-row__label">{label}</span>
                        <div class="stats-row__bar-wrap">
                            <div class="stats-row__bar" style=format!("width: {:.1}%", pct)></div>
                        </div>
                        <span class="stats-row__value">{value}</span>
                    </div>
                }
            };

            view! {
                <div data-testid="overview-tab">
                    <table class="stats-kpi">
                        <thead>
                            <tr>
                                <th></th>
                                <th>{move || i18n::t(lang.get(), "overview_col_visits")}</th>
                                <th>{move || i18n::t(lang.get(), "overview_col_topup")}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {row("overview_period_month", s.totals.this_month.visits, s.totals.this_month.topped_up_eur)}
                            {row("overview_period_year",  s.totals.this_year.visits,  s.totals.this_year.topped_up_eur)}
                            {row("overview_period_all",   s.totals.all_time.visits,   s.totals.all_time.topped_up_eur)}
                        </tbody>
                    </table>

                    <h3 class="stats-chart-title">{move || i18n::t(lang.get(), "overview_chart_visits")}</h3>
                    <div class="stats-chart" data-testid="stats-visits-chart">
                        {visits_rows.iter().map(|b| visit_bar(b)).collect::<Vec<_>>()}
                    </div>

                    <h3 class="stats-chart-title">{move || i18n::t(lang.get(), "overview_chart_topup")}</h3>
                    <div class="stats-chart" data-testid="stats-topup-chart">
                        {topup_rows.iter().map(|b| topup_bar(b)).collect::<Vec<_>>()}
                    </div>
                </div>
            }.into_any()
        }}
    }
}

/// "2026-05" → English "May'26", Slovak "Maj'26". Locale-friendly axis label
/// for the 12-bar charts. Falls back to the input string if it's malformed.
fn fmt_year_month(ym: &str, lang: Lang) -> String {
    let parts: Vec<&str> = ym.split('-').collect();
    if parts.len() != 2 {
        return ym.to_string();
    }
    let yr = parts[0];
    let yr_short = if yr.len() == 4 { &yr[2..4] } else { yr };
    let m: usize = match parts[1].parse() {
        Ok(n) if (1..=12).contains(&n) => n,
        _ => return ym.to_string(),
    };
    let names_en = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let names_sk = [
        "Jan", "Feb", "Mar", "Apr", "Maj", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dec",
    ];
    let name = match lang {
        Lang::En => names_en[m - 1],
        Lang::Sk => names_sk[m - 1],
    };
    format!("{}'{}", name, yr_short)
}

#[cfg(test)]
mod tests {
    use super::fmt_year_month;
    use crate::i18n::Lang;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn formats_english_may() {
        assert_eq!(fmt_year_month("2026-05", Lang::En), "May'26");
    }
    #[wasm_bindgen_test]
    fn formats_slovak_may() {
        assert_eq!(fmt_year_month("2026-05", Lang::Sk), "Maj'26");
    }
    #[wasm_bindgen_test]
    fn malformed_returns_input() {
        assert_eq!(fmt_year_month("not-a-date", Lang::En), "not-a-date");
    }
    #[wasm_bindgen_test]
    fn out_of_range_month_returns_input() {
        assert_eq!(fmt_year_month("2026-13", Lang::En), "2026-13");
    }
}
```

- [ ] **Step 2: Register the new module — modify `spinbike-ui/src/pages/dashboard/mod.rs`**

Edit the `pub mod` list (lines 6-13) — add `overview_tab` alphabetically before `pass_banner`:

```rust
pub mod action_form;
pub mod block_button;
pub mod card_panel;
pub mod edit_info_form;
pub mod helpers;
pub mod overview_tab;     // ← NEW
pub mod pass_banner;
pub mod sheets;
pub mod transactions_list;

pub use card_panel::CardActionPanel;
pub use overview_tab::OverviewTab;     // ← NEW
pub use transactions_list::TransactionsList;
```

- [ ] **Step 3: Wire the 4th tab — modify `spinbike-ui/src/pages/dashboard/card_panel.rs`**

Add the new `OverviewTab` import near the other `super::` imports (around line 11):

```rust
use super::action_form::ActionForm;
use super::block_button::BlockButton;
use super::edit_info_form::EditInfoForm;
use super::helpers::full_name;
use super::overview_tab::OverviewTab;       // ← NEW
use super::pass_banner::PassBanner;
use super::transactions_list::TransactionsList;
```

Then change `tab_items` (lines 40-44) to add a 4th item LAST:

```rust
let tab_items = vec![
    ("history".to_string(), i18n::t(lang.get_untracked(), "tab_history").to_string()),
    ("upcoming".to_string(), i18n::t(lang.get_untracked(), "tab_upcoming").to_string()),
    ("persistent".to_string(), i18n::t(lang.get_untracked(), "tab_persistent").to_string()),
    ("overview".to_string(), i18n::t(lang.get_untracked(), "tab_overview").to_string()),
];
```

Add the 4th match arm in the body match (lines 147-168). The existing `match t.as_str()` ends with `_ => view! { <div></div> }.into_any()`. Insert the new arm BEFORE the wildcard:

```rust
{move || {
    let t = tab.get();
    match t.as_str() {
        "history" => view! {
            <TransactionsList
                card_id=card_id
                txn_refresh=txn_refresh
                set_msg=set_msg
            />
        }.into_any(),
        "upcoming" => view! {
            <UpcomingClasses
                card_id=card_id
                refresh_tick=upc_tick
                on_changed=Callback::new(move |()| upc_tick.update(|n| *n += 1))
            />
        }.into_any(),
        "persistent" => view! {
            <PersistentToggles
                card_id=card_id
                on_changed=Callback::new(move |()| upc_tick.update(|n| *n += 1))
            />
        }.into_any(),
        "overview" => view! {                              // ← NEW
            <OverviewTab card_id=card_id />
        }.into_any(),
        _ => view! { <div></div> }.into_any(),
    }
}}
```

- [ ] **Step 4: Append CSS to `spinbike-ui/style.css`**

Append at the bottom of the file:

```css
/* ─── Per-card Overview tab (v0.13.18) ───────────────────────────── */
.stats-kpi {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: 1rem;
}
.stats-kpi th,
.stats-kpi td {
    padding: 0.4rem 0.6rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
}
.stats-kpi th:first-child,
.stats-kpi td:first-child {
    text-align: left;
    font-weight: 500;
}
.stats-kpi th {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted, #888);
}

.stats-chart-title {
    font-size: 0.95rem;
    margin: 1rem 0 0.4rem;
    color: var(--text-muted, #555);
}
.stats-chart {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
}
.stats-row {
    display: grid;
    grid-template-columns: 4.5rem 1fr 5rem;
    align-items: center;
    gap: 0.5rem;
    font-variant-numeric: tabular-nums;
    font-size: 0.9rem;
}
.stats-row__label {
    color: var(--text-muted, #666);
}
.stats-row__bar-wrap {
    background: var(--surface-2, #f0f0f0);
    border-radius: 3px;
    height: 0.85rem;
    overflow: hidden;
}
.stats-row__bar {
    height: 100%;
    background: var(--accent, #4caf50);
    border-radius: 3px;
    transition: width 200ms ease;
    min-width: 0;
}
.stats-row__value {
    text-align: right;
}
```

- [ ] **Step 5: Confirm formatting**

```bash
cargo fmt --all --check
```

- [ ] **Step 6: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/overview_tab.rs \
        spinbike-ui/src/pages/dashboard/mod.rs \
        spinbike-ui/src/pages/dashboard/card_panel.rs \
        spinbike-ui/style.css
git commit -m "feat(ui): per-card Overview tab — KPI grid + 12-month CSS bar charts"
```

---

## Task 6: Playwright E2E `e2e/tests/per-card-overview.spec.ts`

**Files:**
- Create: `e2e/tests/per-card-overview.spec.ts`

- [ ] **Step 1: Write the test**

```ts
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Per-card Overview tab: assert KPI numbers + chart bars render correctly
// after seeding a known transaction shape.
//
// Seeded scenario (this calendar month, in the test DB):
//   - 1 Spinning visit @ -3.30 € (counts as visit)
//   - 1 Fitness  visit @ -5.00 € (counts as visit)
//   - 1 Refreshments charge      (NOT a visit)
//   - 1 top-up   @ +50.00 €
//
// Expectation:
//   This month → Visits=2, Topped up=50.00 €
//   This year  → same (everything is this year)
//   All time   → same
//   Visits chart contains a row with value "2" for the current month
//   Top-ups chart contains a row with value "50.00 €" for the current month
test('per-card Overview tab shows correct KPIs and bars', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    const barcode = `OV-${Date.now()}`;

    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [
                { amount: -3.30, action: 'charge', service_name_sk: 'Spinning' },
                { amount: -5.00, action: 'charge', service_name_sk: 'Fitness' },
                { amount: -2.50, action: 'charge', service_name_sk: 'Občerstvenie' },
                { amount: 50.00, action: 'topup',  service_name_sk: 'Občerstvenie' },
            ],
        }),
    });
    if (!seed.ok) throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);

    // Open the card via search → action panel.
    await page.goto('/staff');
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(barcode, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    // Click the Overview tab. With English forced by loginViaAPI, the tab
    // label reads "Overview".
    await page.locator('[data-testid="tab-overview"]').click();
    await expect(page.locator('[data-testid="overview-tab"]')).toBeVisible();

    // KPI table — strict checks via data-testid.
    await expect(
        page.locator('[data-testid="overview-visits-overview_period_month"]')
    ).toHaveText('2');
    await expect(
        page.locator('[data-testid="overview-topup-overview_period_month"]')
    ).toHaveText('50.00 €');
    await expect(
        page.locator('[data-testid="overview-visits-overview_period_year"]')
    ).toHaveText('2');
    await expect(
        page.locator('[data-testid="overview-topup-overview_period_all"]')
    ).toHaveText('50.00 €');

    // Both charts render exactly 12 rows.
    await expect(page.locator('[data-testid="stats-visits-row"]')).toHaveCount(12);
    await expect(page.locator('[data-testid="stats-topup-row"]')).toHaveCount(12);

    // The current-month row in the visits chart shows "2"; the current-month
    // row in the top-ups chart shows "50.00 €". Charts render newest-first
    // so the first row IS the current month.
    await expect(
        page.locator('[data-testid="stats-visits-row"]').first()
    ).toContainText('2');
    await expect(
        page.locator('[data-testid="stats-topup-row"]').first()
    ).toContainText('50.00 €');

    assertCleanConsole(msgs);
});
```

- [ ] **Step 2: Confirm naming matches `Segmented` testid pattern**

The plan assumes the `Segmented` component (used by `card_panel.rs`) emits `data-testid="tab-<key>"` because of `testid_prefix="tab"`. Confirm by grep:

```bash
grep -n "testid_prefix\|data-testid" spinbike-ui/src/components/segmented.rs | head -5
```

If the rendered attribute differs (e.g. `tab-button-overview`), update the locator `[data-testid="tab-overview"]` to match.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/per-card-overview.spec.ts
git commit -m "test(e2e): per-card Overview tab — KPI grid + chart bars"
```

---

## Task 7: Push final + monitor CI to terminal state + open PR *(controller-run, not a subagent task)*

- [ ] **Step 1: Final lint check before push**

```bash
cargo fmt --all --check
```

Expected: no diff. If diff: `cargo fmt --all` then re-stage with explicit paths and amend nothing — make a new commit.

- [ ] **Step 2: Push to dev**

```bash
git push origin dev
```

- [ ] **Step 3: Identify the run and monitor it to terminal state**

```bash
gh run list --branch dev --limit 1 --json databaseId,headSha,status,conclusion
# Capture the databaseId, then:
sleep 300 && gh run view <run-id> --json status,conclusion,jobs
```

Run that as `Bash(..., run_in_background: true)`. When it returns, inspect:
- `Test Integrity` — green
- `Lint` — green
- `Test` — green (the new `cards_stats.rs` integration tests live here)
- `Test (UI)` — green (i18n + `fmt_year_month` wasm-bindgen tests)
- `Build WASM (UI)` — green
- `E2E Tests` — green (`per-card-overview.spec.ts` + every existing test)
- `Mutation Testing` — green
- `Deploy (dev)` — green (deploy-dev syncs prod DB → dev → installs new binary)
- `Smoke (dev)` — green

If **any** job is red, `gh run view <run-id> --log-failed` and fix before continuing.

- [ ] **Step 4: Mutation Testing mitigation if surviving mutants**

If `Mutation Testing` reports surviving mutants, use `gh run view <run-id> --log` and grep for `MISSED`. The most likely targets are:
- The visit-filter substitution (`name_en IN (...)`)
- The top-up predicate (`amount > 0` and `action='topup'`)
- The 12-month label generation loop

For each surviving mutant, add a unit test in `cards_stats.rs` that exercises the boundary it sits on, push, monitor again. Repeat until ALL jobs green.

- [ ] **Step 5: Open the PR (only after CI is fully green)**

```bash
gh pr create --base main --head dev --title "v0.13.18: per-card Overview tab" --body "$(cat <<'EOF'
## Summary
- New "Prehľad / Overview" tab in the staff card panel showing visits + top-ups across this-month / this-year / all-time
- Two 12-month single-series bar charts (visits, top-ups) — pure CSS, no new deps
- New endpoint `GET /api/cards/{id}/stats` aggregates a card's transactions in 1 SQL round-trip
- Visit filter uses the canonical `spinbike_core::services::CLASS_VISIT_NAMES_EN` constants (Spinning + Fitness)

## Test plan
- [x] Server unit tests: 7 cases including soft-delete exclusion + customer-role 403
- [x] WASM unit tests: `fmt_year_month` for SK/EN + malformed input
- [x] E2E (Playwright): Overview tab renders KPIs + 12 chart rows, zero console errors
- [x] CI fully green (Lint / Test / Test (UI) / Build / E2E / Mutation / Deploy / Smoke)

## Out of scope
- Issue #50 (Slovak `Mesačný preplatok` → `Mesačná permanentka` rename) — separate work
- Customer-facing analytics on `/my-balance` (staff-only by design)
- "Switch to monthly pass" upsell prompt (dropped during brainstorming)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 6: Verify the PR is mergeable**

```bash
gh pr view --json number,mergeable,mergeStateStatus
```

Expected: `mergeable: MERGEABLE` AND `mergeStateStatus: CLEAN`. Anything else (UNSTABLE, BLOCKED, DIRTY, BEHIND) means not done — fix.

- [ ] **Step 7: Stop here. Wait for explicit user "merge it" instruction.**

Per `pr-merge-policy.md`: never merge a PR without an explicit user trigger. Send the completion report (per `completion-report.md` template) and wait.

---

## Task 8: Post-deploy verification *(controller-run, ONLY after user merges)*

- [ ] **Step 1: Wait for `Deploy (prod)` + `Smoke (prod)` to go green on the main run**

```bash
gh run list --branch main --limit 1 --json databaseId,status,conclusion,jobs
```

- [ ] **Step 2: Read the dev frontend's version label via Playwright (real DOM)**

```bash
# Navigate via plugin:playwright MCP to https://spinbike-dev.newlevel.media
# Read [data-testid="version"] — must equal v0.13.18
```

- [ ] **Step 3: Functional verification on a real card with rich history**

Open `https://spinbike-dev.newlevel.media/staff?card=70701712` in Playwright (this card has years of Spinning + Fitness history). Click the "Prehľad" tab. Assert:

- The Overview tab is visible (`[data-testid="overview-tab"]`)
- "Spolu" (All time) row's Visits cell shows a number > 100
- "Spolu" row's Topped up cell shows a non-zero `XX.XX €`
- The visits chart contains exactly 12 rows
- The top-ups chart contains exactly 12 rows
- Browser console: zero errors / warnings

- [ ] **Step 4: Repeat against prod**

`https://spinbike.newlevel.media/staff?card=70701712`. Same assertions.

- [ ] **Step 5: Send completion report per `completion-report.md` template**

Include both 🌐 dev + 🌐 prod URLs, the dev DOM-read version, the all-time visits number you observed on card 70701712, and the merged PR link.

---

## Self-review

**Spec coverage:**

| Spec section | Covered by |
|---|---|
| Architecture: 4th tab, no schema migration | Task 5 step 3 |
| Backend endpoint shape | Task 3 step 3 (handler) + Task 2 (types) |
| Visit definition (Spinning/Fitness, paid+pass) | Task 3 step 3 SQL `visit_filter_sql` + Task 3 step 1 test `mixed_services_count_only_spinning_and_fitness_as_visits` |
| Top-up definition (action='topup' AND amount>0) | Task 3 step 3 SQL CASE WHEN + test `topup_count_excludes_zero_amount_and_non_topup_actions` |
| 12 monthly buckets, gap-filled | Task 3 step 3 Rust label loop + test `multi_month_buckets_align_correctly` |
| Auth gate | Task 3 step 3 `can_manage_cards()` + test `customer_role_forbidden` |
| Soft-delete exclusion | Task 3 step 1 test `soft_deleted_rows_excluded` |
| 12-month chart cutoff with all-time totals retained | Task 3 step 1 test `visits_older_than_twelve_months_excluded_from_chart_but_in_all_time` |
| KPI table layout | Task 5 step 1 OverviewTab `view!` |
| Pure-CSS bars, ~30 lines | Task 5 step 4 |
| 9 i18n keys SK+EN | Task 4 step 2 |
| Edge case: zero data → 12 zero bars | Task 5 step 1 fallback `if visits_max > 0` |
| Edge case: zero topups → row shown with `0 €` and no bar | same |
| E2E asserts KPIs + at least one bar | Task 6 step 1 |
| Zero console errors | Task 6 step 1 `assertCleanConsole(msgs)` |
| VERSION bump 0.13.17 → 0.13.18 | Task 1 |

**Risks / mitigations cross-checked:** Spec calls out timezone sensitivity (Europe/Bratislava) and string-coupling to service names. Plan addresses both: SQL uses `'localtime'` (matches `chrono::Local::now()` in handler), and the visit-filter uses constants from `spinbike-core` instead of hardcoded strings.

**Type consistency:** `StatsResponse`, `PeriodTotals`, `PeriodAgg`, `MonthlyBucket` defined in Task 2, used unchanged in Task 3 (server), Task 5 (frontend), Task 6 (test reads JSON shape). Field names `visits`, `topped_up_eur`, `year_month`, `this_month`, `this_year`, `all_time` consistent across all tasks.

**Placeholder scan:** Plan contains zero "TBD" / "TODO" / "implement later" / "Add appropriate ..." patterns. One deliberate typo (`StatsCode::FORBIDDEN`) flagged for the implementer in Task 3 step 3.

Plan complete.
