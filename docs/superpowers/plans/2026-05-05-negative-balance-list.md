# Negative-Balance List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface every card with `credit < 0` on the staff Desk — as a proactive list under the search box when idle, and as a row-level highlight inside the existing search dropdown.

**Architecture:** New backend endpoint `GET /api/cards/negative-balance` returns sorted negative-credit cards with `last_visit_at` and `last_payment_at`. New Leptos component `NegativeBalanceList` mounts on the Dashboard between the alerts and the action panel, hidden when a card is selected or the search box has any text. Existing search-result row gains a `search-result--negative` modifier class via a small helper for testability and mutation pressure.

**Tech Stack:** Axum 0.8, sqlx + SQLite, Leptos 0.7 CSR (WASM), gloo-net via the existing `api::get` wrapper, Playwright for E2E.

---

## Spec

`docs/superpowers/specs/2026-05-05-negative-balance-list-design.md` (committed at `cec3a5e`).

## File map

| File | Change | Responsibility |
|---|---|---|
| `VERSION` + `*/Cargo.toml` (synced) | edit | Bump 0.13.20 → 0.13.21. |
| `spinbike-ui/src/i18n.rs` | edit | Three new keys + wasm-bindgen tests. |
| `crates/spinbike-server/src/db/cards.rs` | edit | New `NegativeBalanceRow` struct + `list_negative_balance` query + unit tests. |
| `crates/spinbike-server/src/routes/cards.rs` | edit | New `negative_balance` handler + `NegativeBalanceCardResponse` + route registration + 200/403 route test. |
| `crates/spinbike-server/src/routes/test_fixtures.rs` | edit | New `seed-credit` fixture so Playwright can set `cards.credit` directly. |
| `spinbike-ui/src/pages/dashboard/helpers.rs` | edit | New `result_row_class(highlighted, credit) -> &'static str` helper + 4 wasm-bindgen tests. |
| `spinbike-ui/src/pages/dashboard/mod.rs` | edit | Switch the search-result row's class to use the helper; mount `<NegativeBalanceList />` between alerts and `CardActionPanel`. |
| `spinbike-ui/src/pages/dashboard/negative_balance_list.rs` | create | The new presentational component. |
| `spinbike-ui/style.css` | edit | `.search-result--negative` rule + list-card styles. |
| `e2e/tests/negative-balance.spec.ts` | create | Idle list + search highlight + click-row → card panel + clean console. |

---

## Workflow rules (read once, apply throughout)

- **Branch:** `dev`. Direct commits.
- **Local checks:** `cargo fmt --all --check` only. Do **not** run `cargo build`, `cargo test`, `cargo clippy`, or `trunk build` locally — CI is authoritative.
- **Staging:** explicit paths or `git add -u`. **Never** `git add -A` or `git add .`.
- **Commit messages:** Conventional Commits (`feat`, `fix`, `docs`, `chore`, etc.) with `(#49)` reference where applicable.
- **wasm-bindgen tests:** `wasm-pack test --node` runs them. **Do NOT** add `wasm_bindgen_test_configure!(run_in_browser);` — that line silently skips tests under `--node` (see PR #59 lessons learned).
- **PR policy:** never merge. End at "PR mergeable, awaiting user merge".

---

### Task 1: Bump VERSION 0.13.20 → 0.13.21 (CONTROLLER, NOT subagent)

**Files:**
- Modify: `VERSION`
- Modify (via script): `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Edit `VERSION`**

```
0.13.21
```

- [ ] **Step 2: Sync the version into all `Cargo.toml` files**

Run: `bash scripts/sync-version.sh`
Expected: script prints which Cargo.toml files were updated, exit 0.

- [ ] **Step 3: Stage and commit**

```bash
git add VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
# (sync-version.sh may also update Cargo.lock — include it if changed.)
git add -u Cargo.lock
git commit -m "chore(release): v0.13.21"
```

---

### Task 2: i18n keys + wasm-bindgen tests (SUBAGENT, sonnet)

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`

Three new keys + two test cases per key (Slovak + English render). The existing `last_visit_label = "Posledna navsteva"` is **already in the file** — do **not** redefine it.

- [ ] **Step 1: Add the three keys**

In `spinbike-ui/src/i18n.rs`, locate the existing block of `m.insert("last_visit_label", ...)` (around line 674) and add immediately after it:

```rust
    m.insert("negative_balance_heading", ("Karty s dlhom", "Cards with negative balance"));
    m.insert("last_payment_label", ("Posledna platba", "Last payment"));
    m.insert("never_label", ("nikdy", "never"));
```

Slovak strings are unaccented per project convention.

- [ ] **Step 2: Add wasm-bindgen tests for the three keys**

In `spinbike-ui/src/i18n.rs`, find the existing `mod tests` (already at end of file). Add the following six test functions at the bottom of that module:

```rust
    #[wasm_bindgen_test]
    fn negative_balance_heading_slovak() {
        assert_eq!(t(Lang::Sk, "negative_balance_heading"), "Karty s dlhom");
    }

    #[wasm_bindgen_test]
    fn negative_balance_heading_english() {
        assert_eq!(t(Lang::En, "negative_balance_heading"), "Cards with negative balance");
    }

    #[wasm_bindgen_test]
    fn last_payment_label_slovak() {
        assert_eq!(t(Lang::Sk, "last_payment_label"), "Posledna platba");
    }

    #[wasm_bindgen_test]
    fn last_payment_label_english() {
        assert_eq!(t(Lang::En, "last_payment_label"), "Last payment");
    }

    #[wasm_bindgen_test]
    fn never_label_slovak() {
        assert_eq!(t(Lang::Sk, "never_label"), "nikdy");
    }

    #[wasm_bindgen_test]
    fn never_label_english() {
        assert_eq!(t(Lang::En, "never_label"), "never");
    }
```

(If the existing module uses a different invocation form, e.g. `tf` instead of `t`, mirror it. Look at the existing tests in the same module before editing.)

**DO NOT** add `wasm_bindgen_test_configure!(run_in_browser);` anywhere in this file. CI runs `wasm-pack test --node`. Browser-mode silently skips tests under `--node`.

- [ ] **Step 3: Local format check**

Run: `cargo fmt --all --check`
Expected: exit 0, no diff.

- [ ] **Step 4: Stage and commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "i18n(neg-balance): add three keys + wasm tests (#49)"
```

---

### Task 3: Server endpoint `GET /api/cards/negative-balance` + test-fixture endpoint (SUBAGENT, sonnet)

**Files:**
- Modify: `crates/spinbike-server/src/db/cards.rs` — DB function + DB unit test.
- Modify: `crates/spinbike-server/src/routes/cards.rs` — route handler, response struct, registration.
- Modify: `crates/spinbike-server/src/routes/test_fixtures.rs` — `seed-credit` fixture.
- Modify: `crates/spinbike-server/tests/cards_routes.rs` — integration tests using the existing `TestApp` helper.

**Action vocabulary verified by grep:**
- `'visit'` — class entry / visit log
- `'topup'` — credit topup. **Real payments have `amount > 0`** (negative `topup` rows exist in legacy data as refunds; we exclude them, mirroring the existing pattern in `routes/cards.rs:574+`).
- `'charge'` — debit, monthly-pass charges, etc.

So **last payment** = `MAX(timestamp) WHERE action='topup' AND amount > 0 AND deleted_at IS NULL`.

Use `created_at` as the timestamp column (the legacy schema's column name; verify with `grep "created_at" crates/spinbike-server/src/db/transactions.rs` if uncertain — every existing query in this codebase uses `created_at`).

- [ ] **Step 1: Write the failing DB unit test**

Append to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/spinbike-server/src/db/cards.rs`. The existing tests in this module use `#[tokio::test]` + a `setup()` helper that returns a migrated in-memory pool — mirror that:

```rust
    #[tokio::test]
    async fn list_negative_balance_returns_only_negatives_sorted() {
        let pool = setup().await;

        // Three cards: positive, mildly negative, deeply negative.
        let pos = create_card(&pool, "POS-1").await.unwrap();
        let mid = create_card(&pool, "MID-1").await.unwrap();
        let deep = create_card(&pool, "DEEP-1").await.unwrap();

        // Drive credit balances: positive +5, mid -3.5, deep -10.0.
        update_credit(&pool, pos, 5.0).await.unwrap();
        update_credit(&pool, mid, -3.5).await.unwrap();
        update_credit(&pool, deep, -10.0).await.unwrap();

        // Seed a 'visit' for `mid` and a 'topup' for `deep` so we can verify
        // the two subqueries return the right values.
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (?, 0.0, 'visit', '2026-04-22 12:00:00')",
        )
        .bind(mid)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (?, 5.0, 'topup', '2026-03-05 09:00:00')",
        )
        .bind(deep)
        .execute(&pool)
        .await
        .unwrap();

        let rows = list_negative_balance(&pool).await.unwrap();
        assert_eq!(rows.len(), 2, "positive card must be excluded");
        // Most-negative-first: deep (-10.0) before mid (-3.5).
        assert_eq!(rows[0].id, deep);
        assert!((rows[0].credit - (-10.0)).abs() < f64::EPSILON);
        assert_eq!(rows[0].last_visit_at, None);
        assert_eq!(
            rows[0].last_payment_at.as_deref(),
            Some("2026-03-05 09:00:00"),
        );
        assert_eq!(rows[1].id, mid);
        assert!((rows[1].credit - (-3.5)).abs() < f64::EPSILON);
        assert_eq!(
            rows[1].last_visit_at.as_deref(),
            Some("2026-04-22 12:00:00"),
        );
        assert_eq!(rows[1].last_payment_at, None);
    }
```

- [ ] **Step 2: Add the struct and query**

In `crates/spinbike-server/src/db/cards.rs`, near the existing `Card` and `CardWithPass` definitions, add:

```rust
#[derive(Debug, sqlx::FromRow)]
pub struct NegativeBalanceRow {
    pub id: i64,
    pub barcode: String,
    pub credit: f64,
    pub blocked: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub last_payment_at: Option<String>,
}

/// Cards with `credit < 0`, sorted most-negative-first. Includes blocked
/// cards (still owe money). The two subqueries piggyback on the existing
/// `(card_id, created_at)` index on `transactions`.
pub async fn list_negative_balance(
    pool: &sqlx::SqlitePool,
) -> anyhow::Result<Vec<NegativeBalanceRow>> {
    let rows = sqlx::query_as::<_, NegativeBalanceRow>(
        "SELECT
             c.id, c.barcode, c.credit, c.blocked,
             c.first_name, c.last_name, c.company,
             (SELECT MAX(t.created_at) FROM transactions t
                  WHERE t.card_id = c.id
                    AND t.action = 'visit'
                    AND t.deleted_at IS NULL) AS last_visit_at,
             (SELECT MAX(t.created_at) FROM transactions t
                  WHERE t.card_id = c.id
                    AND t.action = 'topup'
                    AND t.amount > 0
                    AND t.deleted_at IS NULL) AS last_payment_at
         FROM cards c
         WHERE c.credit < 0
         ORDER BY c.credit ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

(If `cards.blocked` in your sqlx column model maps to a different Rust type, mirror the type used by the existing `Card` struct in this file — likely `bool` after sqlx's INTEGER→bool coercion, but match what's already there.)

- [ ] **Step 3: Write the failing route tests**

The route-level integration tests live in `crates/spinbike-server/tests/cards_routes.rs`, NOT inside `src/routes/cards.rs`. The file already uses the shared `TestApp` helper from `crates/spinbike-server/tests/helpers/mod.rs` — `app.staff_token`, `app.customer_token`, `app.seed_card(barcode, credit, first_name, last_name, company, phone)`, `app.request(get(uri, token))`. **Do not invent a new bootstrap pattern.** Append to `crates/spinbike-server/tests/cards_routes.rs`:

```rust
#[tokio::test]
async fn negative_balance_endpoint_returns_only_negatives_sorted() {
    let app = TestApp::new().await;
    // Two negatives, one positive. Most-negative-first sort should put NEG-A
    // before NEG-B in the response array.
    app.seed_card("NEG-A", -10.0, Some("Alpha"), None, None, None).await;
    app.seed_card("NEG-B", -3.5, Some("Bravo"), None, None, None).await;
    app.seed_card("POS-A", 5.0, Some("Charlie"), None, None, None).await;

    let (status, resp) = app
        .request(get("/api/cards/negative-balance", &app.staff_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    // The TestApp may have auto-seeded other cards too (e.g. CUST1 from
    // TestApp::new). Filter to ours by barcode prefix to avoid coupling
    // to that detail.
    let ours: Vec<_> = arr
        .iter()
        .filter(|r| {
            let b = r["barcode"].as_str().unwrap_or("");
            b == "NEG-A" || b == "NEG-B" || b == "POS-A"
        })
        .collect();
    assert_eq!(ours.len(), 2, "positive card must be excluded");
    assert_eq!(ours[0]["barcode"], "NEG-A", "most-negative first");
    assert_eq!(ours[1]["barcode"], "NEG-B");
}

#[tokio::test]
async fn negative_balance_endpoint_forbidden_for_customer() {
    let app = TestApp::new().await;
    app.seed_card("NEG-X", -1.0, None, None, None, None).await;
    let (status, _) = app
        .request(get("/api/cards/negative-balance", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
```

(`get`, `TestApp` are already imported at the top of `cards_routes.rs` — re-check the existing `use helpers::{TestApp, delete, get, post_json, put_json};` line and add `get` if missing.)

- [ ] **Step 4: Add the route handler and register the route**

In `crates/spinbike-server/src/routes/cards.rs`:

Add the response struct near the other `*Response` definitions:

```rust
#[derive(Serialize)]
pub struct NegativeBalanceCardResponse {
    pub id: i64,
    pub barcode: String,
    pub credit: f64,
    pub blocked: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub last_payment_at: Option<String>,
}
```

Add the handler near `search_cards`:

```rust
async fn negative_balance(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<NegativeBalanceCardResponse>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff only"})),
        ));
    }
    let rows = db::list_negative_balance(&state.pool)
        .await
        .map_err(internal_error)?;
    let out = rows
        .into_iter()
        .map(|r| NegativeBalanceCardResponse {
            id: r.id,
            barcode: r.barcode,
            credit: r.credit,
            blocked: r.blocked,
            first_name: r.first_name,
            last_name: r.last_name,
            company: r.company,
            last_visit_at: r.last_visit_at,
            last_payment_at: r.last_payment_at,
        })
        .collect();
    Ok(Json(out))
}
```

Register the route in the existing `routes()` builder in this file. Find the chain that ends with `.route("/api/cards/{id}/stats", get(card_stats))` etc. and add:

```rust
        .route("/api/cards/negative-balance", get(negative_balance))
```

Order matters in axum only across overlapping path patterns; `/api/cards/negative-balance` and `/api/cards/{id}` overlap, so register `negative-balance` BEFORE `{id}`. (Axum 0.8 picks the most specific match, but explicit ordering is safer and clearer.)

- [ ] **Step 5: Add the test-fixture `seed-credit` endpoint**

Why: `seed_transactions` does NOT update `cards.credit`, but the E2E test in Task 6 needs cards with a known negative credit. Add a small dedicated fixture.

In `crates/spinbike-server/src/routes/test_fixtures.rs`:

```rust
#[derive(Deserialize)]
pub struct SeedCreditRequest {
    pub barcode: String,
    pub credit: f64,
}
```

Register it in the existing `routes()` function:

```rust
        .route("/api/test/seed-credit", post(seed_credit))
```

Add the handler:

```rust
async fn seed_credit(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SeedCreditRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !claims.role.can_process_payments() {
        return Err((StatusCode::FORBIDDEN, "Staff required".into()));
    }
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM cards WHERE barcode = ?")
        .bind(&body.barcode)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let card_id = match existing {
        Some(id) => id,
        None => cards::create_card(&state.pool, &body.barcode)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };
    sqlx::query("UPDATE cards SET credit = ROUND(?, 2) WHERE id = ?")
        .bind(body.credit)
        .bind(card_id)
        .execute(&state.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "card_id": card_id })))
}
```

(`seed-credit` is registered only when `SPINBIKE_TEST_MODE=1`, same as the other fixtures, because it lives in the test_fixtures router.)

- [ ] **Step 6: Local format check**

Run: `cargo fmt --all --check`
Expected: exit 0, no diff.

- [ ] **Step 7: Stage and commit**

```bash
git add crates/spinbike-server/src/db/cards.rs \
        crates/spinbike-server/src/routes/cards.rs \
        crates/spinbike-server/src/routes/test_fixtures.rs \
        crates/spinbike-server/tests/cards_routes.rs
git commit -m "feat(server): GET /api/cards/negative-balance + seed-credit fixture (#49)"
```

---

### Task 4: `result_row_class` helper + apply to search dropdown + CSS (SUBAGENT, sonnet)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/helpers.rs` — add helper + 4 wasm-bindgen tests.
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` — replace the row-class closure (around line 382-389) with a call to the helper.
- Modify: `spinbike-ui/style.css` — add `.search-result--negative` rule.

- [ ] **Step 1: Write the failing helper tests**

Append to `spinbike-ui/src/pages/dashboard/helpers.rs`:

```rust
#[cfg(test)]
mod result_row_class_tests {
    use super::result_row_class;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn default_row() {
        assert_eq!(result_row_class(false, 1.0), "search-result-row");
    }

    #[wasm_bindgen_test]
    fn highlighted_row() {
        assert_eq!(result_row_class(true, 1.0), "search-result-row search-result-active");
    }

    #[wasm_bindgen_test]
    fn negative_row() {
        assert_eq!(result_row_class(false, -0.01), "search-result-row search-result--negative");
    }

    #[wasm_bindgen_test]
    fn highlighted_negative_row() {
        assert_eq!(
            result_row_class(true, -0.01),
            "search-result-row search-result-active search-result--negative",
        );
    }

    #[wasm_bindgen_test]
    fn zero_credit_is_not_negative() {
        // Boundary: 0.0 stays in the default class (kills `<= 0.0` mutant).
        assert_eq!(result_row_class(false, 0.0), "search-result-row");
    }
}
```

**DO NOT** add `wasm_bindgen_test_configure!(run_in_browser);` in this file. CI runs `wasm-pack test --node`.

- [ ] **Step 2: Add the helper**

In `spinbike-ui/src/pages/dashboard/helpers.rs`, after the existing helpers, add:

```rust
/// Class string for a search-result row, combining keyboard-highlight state
/// with negative-credit highlight. Pure function — kept here so wasm-bindgen
/// tests can pin all four branches without having to drive the Leptos view.
pub fn result_row_class(highlighted: bool, credit: f64) -> &'static str {
    match (highlighted, credit < 0.0) {
        (false, false) => "search-result-row",
        (true, false) => "search-result-row search-result-active",
        (false, true) => "search-result-row search-result--negative",
        (true, true) => "search-result-row search-result-active search-result--negative",
    }
}
```

- [ ] **Step 3: Use the helper in the search-dropdown render**

Open `spinbike-ui/src/pages/dashboard/mod.rs`. The current render block has (around line 382-389):

```rust
                        <div
                            class=move || {
                                if highlighted_idx.get() == idx {
                                    "search-result-row search-result-active"
                                } else {
                                    "search-result-row"
                                }
                            }
```

Replace with:

```rust
                        <div
                            class={
                                let credit_val = credit_val;
                                move || {
                                    crate::pages::dashboard::helpers::result_row_class(
                                        highlighted_idx.get() == idx,
                                        credit_val,
                                    )
                                }
                            }
```

`credit_val` is already defined a few lines above (line 377 `let credit_val = c.credit;`) — capture it by `Copy` (f64 is Copy) into the closure.

If your file uses `use super::helpers::result_row_class` already, drop the `crate::pages::dashboard::helpers::` prefix and call `result_row_class(...)` directly.

Remove the now-unused `let credit_class = if credit_val < 0.0 { "credit-negative" } else { "" };` line at line 379 IF it is unused after this edit. **Re-check line 408**: that line uses `credit_class` to colour the credit number red. Keep that line and that variable as-is — the helper handles the *row*, not the number. The number's red colour is a separate concern.

- [ ] **Step 4: Add the CSS rule**

In `spinbike-ui/style.css`, find the existing `.search-result-row` block (line ~1187) and add immediately after it:

```css
.search-result--negative {
    border-left: 3px solid var(--color-danger, #dc3545);
    background: rgba(220, 53, 69, 0.04);
}
```

(If the existing CSS uses a different danger-colour variable name, prefer that. Search for `--color-danger`, `--danger`, or `#dc3545` to confirm the canonical token in this stylesheet.)

- [ ] **Step 5: Local format check**

Run: `cargo fmt --all --check`
Expected: exit 0, no diff.

- [ ] **Step 6: Stage and commit**

```bash
git add spinbike-ui/src/pages/dashboard/helpers.rs spinbike-ui/src/pages/dashboard/mod.rs spinbike-ui/style.css
git commit -m "feat(ui): highlight negative-credit rows in Quick Search dropdown (#49)"
```

---

### Task 5: `NegativeBalanceList` component + Dashboard wiring (SUBAGENT, sonnet)

**Files:**
- Create: `spinbike-ui/src/pages/dashboard/negative_balance_list.rs`
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` — add `pub mod negative_balance_list;` near the top of the module, mount the component, pass props.
- Modify: `spinbike-ui/style.css` — add list-card styles.

- [ ] **Step 1: Create the component file**

`spinbike-ui/src/pages/dashboard/negative_balance_list.rs`:

```rust
//! Empty-state list of cards with `credit < 0`, rendered on the Desk under
//! the search box when no card is selected and the search box is empty.
//!
//! Source of truth: `GET /api/cards/negative-balance`. Refetches whenever
//! the parent's `txn_refresh` signal increments.

use chrono::NaiveDate;
use leptos::prelude::*;
use serde::Deserialize;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::dashboard::CardInfo;
use crate::relative_date::format_last_visit;

#[derive(Clone, Debug, Deserialize)]
pub struct NegativeBalanceCard {
    pub id: i64,
    pub barcode: String,
    pub credit: f64,
    pub blocked: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub last_payment_at: Option<String>,
}

#[component]
pub fn NegativeBalanceList(
    txn_refresh: ReadSignal<u32>,
    lang: ReadSignal<Lang>,
    set_selected: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    // Refetch whenever txn_refresh increments.
    let resource = Resource::new(
        move || txn_refresh.get(),
        |_| async {
            api::get::<Vec<NegativeBalanceCard>>("/api/cards/negative-balance")
                .await
                .unwrap_or_default()
        },
    );

    view! {
        <Suspense fallback=move || view! { <span></span> }>
            {move || {
                let rows = resource.get().unwrap_or_default();
                if rows.is_empty() {
                    return view! { <span></span> }.into_any();
                }
                let lang_now = lang.get();
                let heading = i18n::t(lang_now, "negative_balance_heading").to_string();
                let last_visit_label = i18n::t(lang_now, "last_visit_label").to_string();
                let last_payment_label = i18n::t(lang_now, "last_payment_label").to_string();
                let never_label = i18n::t(lang_now, "never_label").to_string();
                let today = today_local();

                let items = rows.into_iter().map(|r| {
                    let name = card_full_name(&r);
                    let credit = format!("{:.2} €", r.credit);
                    let last_visit = format_optional_date(&r.last_visit_at, today, lang_now, &never_label);
                    let last_payment = format_optional_date(&r.last_payment_at, today, lang_now, &never_label);
                    let card_for_pick = NegativeBalanceCard {
                        // Resource is single-shot per refresh; clone is fine.
                        ..r.clone()
                    };
                    view! {
                        <div
                            class="negative-balance-row"
                            data-testid="negative-balance-row"
                            on:click={
                                let card = card_for_pick.clone();
                                let label_ls = last_visit_label.clone();
                                move |_| {
                                    let _ = label_ls;
                                    set_selected.set(Some(neg_to_card_info(&card)));
                                }
                            }
                        >
                            <div class="negative-balance-row__main">
                                <div class="negative-balance-row__name">{name}</div>
                                <div class="negative-balance-row__meta">
                                    {format!("{}: {}", last_visit_label, last_visit)}
                                    {" · "}
                                    {format!("{}: {}", last_payment_label, last_payment)}
                                </div>
                            </div>
                            <div class="negative-balance-row__credit credit-negative">{credit}</div>
                        </div>
                    }
                }).collect_view();

                view! {
                    <div class="card mb-2 negative-balance-list" data-testid="negative-balance-list">
                        <div class="card__body">
                            <h3 class="negative-balance-list__heading">{heading}</h3>
                            {items}
                        </div>
                    </div>
                }.into_any()
            }}
        </Suspense>
    }
}

fn card_full_name(c: &NegativeBalanceCard) -> String {
    let f = c.first_name.clone().unwrap_or_default();
    let l = c.last_name.clone().unwrap_or_default();
    let combined = format!("{f} {l}").trim().to_string();
    if combined.is_empty() {
        c.company.clone().unwrap_or_else(|| c.barcode.clone())
    } else {
        combined
    }
}

fn format_optional_date(
    raw: &Option<String>,
    today: NaiveDate,
    lang: Lang,
    never_label: &str,
) -> String {
    match raw {
        None => never_label.to_string(),
        Some(s) => {
            // SQLite literal: "YYYY-MM-DD HH:MM:SS". Take the leading 10 chars.
            match NaiveDate::parse_from_str(&s[..s.len().min(10)], "%Y-%m-%d") {
                Ok(d) => format_last_visit(d, today, lang),
                Err(_) => never_label.to_string(),
            }
        }
    }
}

fn today_local() -> NaiveDate {
    // Use the same approach as card_panel.rs's last-visit display. If a
    // shared helper already exists (search for `today_local` or `today`
    // in the codebase), use that instead of duplicating logic.
    chrono::Local::now().date_naive()
}

/// Promote a `NegativeBalanceCard` into the parent `CardInfo` so clicking a
/// row opens the full action panel. We don't have all `CardInfo` fields here
/// (no pass), so the panel will refetch via its existing flow. The parent's
/// `set_selected` callback is wired the same way the search-dropdown wires
/// `pick_card` — see `pages/dashboard/mod.rs:305`.
fn neg_to_card_info(c: &NegativeBalanceCard) -> CardInfo {
    CardInfo {
        id: c.id,
        barcode: c.barcode.clone(),
        blocked: c.blocked,
        credit: c.credit,
        first_name: c.first_name.clone(),
        last_name: c.last_name.clone(),
        company: c.company.clone(),
        // Fields the search dropdown also doesn't populate immediately — the
        // action panel fetches the rest on mount. Initialise with neutral
        // defaults that the existing render code already handles.
        ..CardInfo::default()
    }
}
```

**IMPORTANT — verify before pasting:** `CardInfo` lives in `spinbike-ui/src/pages/dashboard/mod.rs` around line 48. Before writing `..CardInfo::default()`, confirm `CardInfo` derives `Default` — if it doesn't, manually initialise every required field with neutral values (None, false, 0.0, vec![], etc.). Do not rely on a derive that isn't there.

**Also verify the `pick_card` shape:** the current closure (line ~305) sets `selected.set(Some(card))`, then resets the search box, refocuses the input, etc. If clicking a negative-balance row should do all of those side effects (it should — we want the same UX), expose `pick_card` as a `Callback<CardInfo>` from the parent and pass it as a prop *instead of* `set_selected`. Look at how the current dropdown row dispatches `pick_card(card.clone())` and mirror that wiring (call it `on_pick: Callback<CardInfo>` or similar). Either approach is valid; the simpler one (raw `WriteSignal`) is fine if `pick_card` is just `set_selected` plus signal resets — read the actual closure body before deciding.

- [ ] **Step 2: Wire the component into Dashboard**

In `spinbike-ui/src/pages/dashboard/mod.rs`:

(a) Near the top of the file, add the module declaration alongside the existing `mod` lines (e.g. `pub mod helpers;` if present, otherwise wherever sibling modules are declared):

```rust
pub mod negative_balance_list;
```

(b) Inside the `view!` block in `DashboardPage`, after the success-message block (around line 444-447) and BEFORE the action-panel mount block (around line 450-451), add:

```rust
        // Idle-state proactive list: only when no card selected AND search is empty.
        {move || {
            if selected.get().is_none() && query.get().is_empty() {
                view! {
                    <negative_balance_list::NegativeBalanceList
                        txn_refresh=txn_refresh
                        lang=lang
                        set_selected=set_selected
                    />
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }
        }}
```

`txn_refresh`, `lang`, `selected`, `query`, and `set_selected` are all already in scope in `DashboardPage` (they're created near the top of the function). If any of them is named differently in your file, mirror the actual name; the *intent* is the gating rule above and the three props the component declares.

- [ ] **Step 3: Add CSS for the list card**

In `spinbike-ui/style.css`, near the `.search-result-row` rules, add:

```css
.negative-balance-list__heading {
    font-size: 0.95rem;
    margin: 0 0 0.5rem 0;
    color: var(--color-text-muted, #6c757d);
}
.negative-balance-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.5rem 0.25rem;
    cursor: pointer;
    border-bottom: 1px solid var(--color-border, #e0e0e0);
}
.negative-balance-row:last-child { border-bottom: none; }
.negative-balance-row:hover { background: var(--color-hover, rgba(0,0,0,0.03)); }
.negative-balance-row__main { min-width: 0; }
.negative-balance-row__name { font-weight: 600; }
.negative-balance-row__meta {
    font-size: 0.85rem;
    color: var(--color-text-muted, #6c757d);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.negative-balance-row__credit {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
}
```

(Use whatever CSS-variable tokens already exist in `style.css` for muted text, border, hover. The fallbacks above are safe defaults.)

- [ ] **Step 4: Local format check**

Run: `cargo fmt --all --check`
Expected: exit 0, no diff.

- [ ] **Step 5: Stage and commit**

```bash
git add spinbike-ui/src/pages/dashboard/negative_balance_list.rs spinbike-ui/src/pages/dashboard/mod.rs spinbike-ui/style.css
git commit -m "feat(ui): negative-balance list on Desk (#49)"
```

---

### Task 6: Playwright E2E `e2e/tests/negative-balance.spec.ts` (SUBAGENT, sonnet)

**Files:**
- Create: `e2e/tests/negative-balance.spec.ts`

The seed strategy uses both `seed-credit` (to set `cards.credit`) and `seed-transactions` (to add a `visit` and a `topup` row whose timestamps drive the row meta).

- [ ] **Step 1: Write the failing E2E test**

`e2e/tests/negative-balance.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #49: cards with credit < 0 must surface on the Desk in two ways:
// 1. Idle desk: a list under the search box (only when no card selected AND search empty).
// 2. Active search: dropdown rows for negative cards get the .search-result--negative class.
test('negative-balance list + search highlight', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `NBLA${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
    const fmtTs = (d: Date): string => `${d.toISOString().slice(0, 10)} 12:00:00`;
    const today = new Date();
    const yesterday = new Date(today.getTime() - 1 * 86400000);
    const lastWeek = new Date(today.getTime() - 7 * 86400000);
    const lastMonth = new Date(today.getTime() - 30 * 86400000);

    // Two negatives + one positive control.
    const cards = [
        { barcode: `Alpha${RUN_TAG}`, credit: -3.5, visitAt: fmtTs(yesterday), topupAt: fmtTs(lastWeek) },
        { barcode: `Bravo${RUN_TAG}`, credit: -10.0, visitAt: null, topupAt: fmtTs(lastMonth) },
        { barcode: `Charlie${RUN_TAG}`, credit: 5.0, visitAt: null, topupAt: fmtTs(yesterday) },
    ];

    for (const c of cards) {
        // 1. Set the credit (creates the card if missing).
        const credResp = await fetch(`${BASE_URL}/api/test/seed-credit`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ barcode: c.barcode, credit: c.credit }),
        });
        if (!credResp.ok) throw new Error(`seed-credit failed: ${credResp.status} ${await credResp.text()}`);

        // 2. Seed a visit (if any) — Spinning charge logs as 'visit' in normalised vocab,
        //    but the simplest direct seed is action='visit' with amount 0.
        const entries: Array<{
            amount: number; action: string; service_name_sk: string; created_at?: string;
        }> = [];
        if (c.visitAt) {
            entries.push({ amount: 0, action: 'visit', service_name_sk: 'Spinning', created_at: c.visitAt });
        }
        if (c.topupAt) {
            entries.push({ amount: 5.0, action: 'topup', service_name_sk: 'Občerstvenie', created_at: c.topupAt });
        }
        if (entries.length > 0) {
            const txResp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
                body: JSON.stringify({ barcode: c.barcode, entries }),
            });
            if (!txResp.ok) throw new Error(`seed-tx failed for ${c.barcode}: ${txResp.status} ${await txResp.text()}`);
        }
    }

    await page.goto('/staff');

    // ---- Surface 1: idle desk list -------------------------------------------------
    const list = page.locator('[data-testid="negative-balance-list"]');
    await expect(list).toBeVisible({ timeout: 5000 });

    const rows = list.locator('[data-testid="negative-balance-row"]');
    // Note: the list shows ALL cards with credit<0, not only our seeded ones —
    // the prod-synced dev DB already has 9 negatives. We assert that BOTH our
    // seeded negatives appear and the positive does NOT, regardless of how
    // many other rows are present.
    await expect(list.getByText(`Alpha${RUN_TAG}`, { exact: false })).toBeVisible();
    await expect(list.getByText(`Bravo${RUN_TAG}`, { exact: false })).toBeVisible();
    await expect(list.getByText(`Charlie${RUN_TAG}`, { exact: false })).toHaveCount(0);

    // Bravo is most-negative (-10.00) and must come BEFORE Alpha (-3.50) in DOM order.
    const bravoIdx = await rows.evaluateAll(
        (els, tag: string) => els.findIndex((e) => (e.textContent ?? '').includes(`Bravo${tag}`)),
        RUN_TAG,
    );
    const alphaIdx = await rows.evaluateAll(
        (els, tag: string) => els.findIndex((e) => (e.textContent ?? '').includes(`Alpha${tag}`)),
        RUN_TAG,
    );
    expect(bravoIdx).toBeGreaterThanOrEqual(0);
    expect(alphaIdx).toBeGreaterThan(bravoIdx);

    // ---- Surface 1b: clicking a row opens the action panel ------------------------
    const alphaRow = rows.filter({ hasText: `Alpha${RUN_TAG}` }).first();
    await alphaRow.click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible({ timeout: 2000 });
    // List hides because a card is now selected.
    await expect(list).toBeHidden();

    // Reset: clear selection by reloading.
    await page.goto('/staff');
    await expect(list).toBeVisible();

    // ---- Surface 2: search highlight ----------------------------------------------
    const search = page.locator('input[type="search"]').first();
    await search.fill(RUN_TAG);
    await expect(list).toBeHidden(); // search active hides idle list

    // Dropdown should have all three of our seeds.
    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(3, { timeout: 5000 });

    // Charlie (positive) — no negative class.
    const charlieRow = results.filter({ hasText: `Charlie${RUN_TAG}` });
    await expect(charlieRow).not.toHaveClass(/search-result--negative/);

    // Alpha & Bravo (negative) — must have the modifier class.
    const alphaRowSearch = results.filter({ hasText: `Alpha${RUN_TAG}` });
    const bravoRowSearch = results.filter({ hasText: `Bravo${RUN_TAG}` });
    await expect(alphaRowSearch).toHaveClass(/search-result--negative/);
    await expect(bravoRowSearch).toHaveClass(/search-result--negative/);

    // ---- Clean up: clear search → list reappears ----------------------------------
    await search.fill('');
    await expect(list).toBeVisible();

    assertCleanConsole(msgs);
});
```

**Notes for the implementer:**

- Auth header: the test uses `Authorization: Bearer <token>` — confirm by reading any of the existing tests (e.g. `last-visit-display.spec.ts:78`) that the project's auth pattern is the same.
- `data-testid="search-result"` already exists (mod.rs:390). `data-testid="action-panel"` already exists (card_panel.rs:64). The two new test-ids `negative-balance-list` and `negative-balance-row` are introduced by Task 5.
- Don't add a delete/cleanup step — the dev DB is reset by CI's "Test (UI)" job between runs and the test fixtures are tagged with the random `RUN_TAG` so collisions across runs are essentially impossible.

- [ ] **Step 2: Stage and commit**

```bash
git add e2e/tests/negative-balance.spec.ts
git commit -m "test(e2e): negative-balance list + search highlight (#49)"
```

---

### Task 7: Validate query against synced dev DB (CONTROLLER, NOT subagent)

Per memory `feedback_validate_against_real_data.md`: SQL changes get a real-data sanity check before merge.

- [ ] **Step 1: Run the new SQL against the live dev DB**

```bash
sqlite3 /opt/spinbike/dev/spinbike-dev.db "
SELECT
    c.id, c.barcode, ROUND(c.credit, 2) AS credit, c.first_name, c.last_name,
    (SELECT MAX(t.created_at) FROM transactions t
        WHERE t.card_id = c.id AND t.action='visit' AND t.deleted_at IS NULL) AS last_visit_at,
    (SELECT MAX(t.created_at) FROM transactions t
        WHERE t.card_id = c.id AND t.action='topup' AND t.amount > 0 AND t.deleted_at IS NULL) AS last_payment_at
FROM cards c
WHERE c.credit < 0
ORDER BY c.credit ASC;
"
```

Expected: ~9 rows (the prod-synced negative cards), most negative first. If the count diverges from the figure observed during brainstorming (9 unblocked negatives), debug before opening the PR.

---

### Task 8: Push + monitor CI + open PR (CONTROLLER, NOT subagent)

- [ ] **Step 1: Push the branch**

```bash
git push origin dev
```

- [ ] **Step 2: Find the run id**

```bash
gh run list --branch dev --limit 3
```

- [ ] **Step 3: Monitor in background until terminal**

```bash
RUN_ID=<id>; sleep 600 && gh run view "$RUN_ID" --json status,conclusion,jobs
```

(Per `ci-monitoring.md`: ONE background `sleep && gh run view` command — no loops, no `gh run watch`. Read the result via `BashOutput` when it completes.)

ALL jobs must be ✅ before opening the PR (Test Integrity, Lint, Build WASM (UI), Test, Test (UI), E2E Tests, Mutation Testing). Mutation testing should kill: server `<` boundary in `WHERE c.credit < 0`, server sort order in `ORDER BY c.credit ASC`, UI `< 0.0` boundary in `result_row_class`, UI `highlighted` boolean.

If a mutant survives, the test for that branch was too weak — strengthen the assertion (don't add a `#[ignore]`).

- [ ] **Step 4: Open the PR**

```bash
gh pr create --base main --head dev --title "v0.13.21: negative-balance list on Desk (#49)" --body "$(cat <<'EOF'
## Summary

- New `GET /api/cards/negative-balance` endpoint (admin/staff only).
- New `NegativeBalanceList` component on the staff Desk: empty-state list under the search box when no card is selected.
- Existing search-dropdown rows for cards with `credit < 0` get a red left border via the new `search-result--negative` modifier class.
- Three new i18n keys: `negative_balance_heading`, `last_payment_label`, `never_label`.
- New test fixture `POST /api/test/seed-credit` so Playwright can create cards with a known credit balance.

Closes #49.

## Test plan

- [ ] CI green: Test Integrity, Lint, Build WASM, Test, Test (UI), E2E, Mutation Testing
- [ ] Verify on `https://spinbike-dev.newlevel.media` after deploy: dashboard label reads `v0.13.21`, list appears under search box on `/staff`, real prod-synced negative cards visible
- [ ] Type a known-negative card prefix in search; assert highlighted row
EOF
)"
```

- [ ] **Step 5: Confirm mergeable**

```bash
PR=<number>
gh api "repos/zbynekdrlik/spinbike/pulls/$PR" --jq '{mergeable:.mergeable, mergeable_state:.mergeable_state}'
```

Expected: `{ "mergeable": true, "mergeable_state": "clean" }`. If anything else, fix the gate per `autonomous-quality-discipline.md` — never offer admin-merge.

---

### Task 9: Post-deploy verification (CONTROLLER, runs ONLY after user merges)

Do not start until the user explicitly says "merge it" and the merge completes.

- [ ] **Step 1: Watch main CI deploy**

```bash
gh run list --branch main --limit 3
RUN_ID=<id>; sleep 600 && gh run view "$RUN_ID" --json status,conclusion,jobs
```

ALL jobs ✅, including `Deploy (prod)` and `Smoke (prod)`.

- [ ] **Step 2: Verify dev frontend version**

Use Playwright MCP. Navigate to `https://spinbike-dev.newlevel.media`. Read `[data-testid="version"]` from the DOM. Expected: `v0.13.21`. Cross-check `https://spinbike-dev.newlevel.media/api/version` → `{"version":"0.13.21"}`. Frontend and backend must match.

- [ ] **Step 3: Verify the feature on dev**

Login as admin via the UI. Navigate to `/staff`. Confirm:
- The `[data-testid="negative-balance-list"]` card is visible under the search box.
- It shows ≥1 real prod-synced negative-credit row.
- Rows show name + credit + last visit + last payment (or "never").
- Type a known prefix that matches a negative card; the dropdown row has the `search-result--negative` class.
- Browser console clean.

- [ ] **Step 4: Verify on prod**

Repeat Step 2 and Step 3 against `https://spinbike.newlevel.media`. Per `no-destructive-remote-actions.md`: do NOT click any visit button or charge button on a real card. Read-only verification (open the dashboard, observe the list, type a search query, observe the highlight). The list itself is read-only — clicking a row only opens the panel; that is fine.

---

## Spec coverage check

| Spec section | Task |
|---|---|
| Surface 1 (idle desk list) | Task 5 (component + wiring) |
| Surface 2 (search highlight) | Task 4 (helper + CSS) |
| Definition `credit < 0`, includes blocked | Task 3 (SQL `WHERE c.credit < 0`, no `blocked` filter) |
| Idle-list row layout (name / credit / last visit / last payment) | Task 5 (component render) |
| Click row → action panel | Task 5 (set_selected) + Task 6 (assertion) |
| Backend endpoint + SQL | Task 3 |
| Frontend component | Task 5 |
| i18n keys | Task 2 |
| CSS rules | Task 4 (`.search-result--negative`) + Task 5 (list styles) |
| Server unit + route tests | Task 3 (steps 1, 3) |
| UI wasm-bindgen tests | Task 2 (i18n) + Task 4 (helper) |
| Playwright E2E | Task 6 |
| Validate against synced dev DB | Task 7 |
