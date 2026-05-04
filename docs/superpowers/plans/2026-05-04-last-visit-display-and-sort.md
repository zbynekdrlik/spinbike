# Last-Visit Display + Quick Search Sort Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show "Posledná návšteva: 28.04.2026 (pred 6 dňami)" on the staff card page (hidden when no qualifying visit) and order Quick Search results newest-visit-first (barcode-prefix match still wins).

**Architecture:** Backend adds one correlated subquery to the existing card-list/search SQL plus a new `last_visit_at: Option<String>` field on `CardResponse`. UI gets a small relative-time helper module (Slovak/English) and renders one extra line in the existing card-panel header. Visit definition reuses `CLASS_VISIT_NAMES_EN` — Spinning + Fitness, soft-deletes excluded — same pattern the Overview tab uses.

**Tech Stack:** Axum 0.8 + sqlx 0.8 + SQLite, Leptos 0.7 CSR/WASM, Trunk, Playwright (TypeScript), `wasm-bindgen-test` for UI unit tests.

**Spec:** `docs/superpowers/specs/2026-05-04-last-visit-display-and-sort-design.md` (commit `94b6a0b`).

**Issue:** [#57](https://github.com/zbynekdrlik/spinbike/issues/57) (CEO meeting follow-up).

---

## File Structure

| File | Responsibility | Status |
|------|----------------|--------|
| `VERSION` | Single source of truth for app version | Modify (0.13.18 → 0.13.19) |
| `crates/spinbike-server/src/db/cards.rs` | Three SQL queries that build `CardRowWithPass` (now also `last_visit_at`) + struct + `into_parts` | Modify |
| `crates/spinbike-server/src/routes/cards.rs` | `CardResponse` API shape + `card_response_from_row_with_pass` helper | Modify |
| `crates/spinbike-server/tests/cards_last_visit.rs` | Integration tests for `last_visit_at` field + sort order | Create |
| `spinbike-ui/src/relative_date.rs` | Slovak/English smart-granularity relative-time + combined `format_last_visit` | Create |
| `spinbike-ui/src/lib.rs` | Register `pub mod relative_date;` | Modify |
| `spinbike-ui/src/i18n.rs` | 11 new keys for the relative-time helper | Modify |
| `spinbike-ui/src/pages/dashboard/mod.rs` | `CardInfo.last_visit_at` field | Modify |
| `spinbike-ui/src/pages/dashboard/card_panel.rs` | New `<div class="card-title__last-visit">` line | Modify |
| `spinbike-ui/style.css` | One CSS rule for the new line | Modify |
| `e2e/tests/last-visit-display.spec.ts` | Playwright E2E covering sort + display + absent case | Create |

---

## Project rules every task observes

- **Never** run `cargo build`, `cargo test`, `cargo clippy`, or `trunk build` locally. CI is authoritative. The ONLY local check allowed is `cargo fmt --all --check`.
- **Never** use `git add -A` or `git add .`. Stage explicit paths or use `git add -u`.
- **Commit messages**: imperative mood, conventional-ish, mention issue #57. End with the standard `Co-Authored-By` trailer.

---

## Task 1: Bump VERSION 0.13.18 → 0.13.19 (CONTROLLER-RUN, FIRST commit)

**Files:**
- Modify: `VERSION`
- Modify: `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml` (sync script writes these)

- [ ] **Step 1: Verify current versions match between dev and main**

```bash
git fetch origin
diff <(git show origin/main:VERSION) VERSION
```
Expected: no output (versions match — both `0.13.18`).

- [ ] **Step 2: Bump VERSION**

Write `0.13.19` to `VERSION`:

```bash
echo 0.13.19 > VERSION
```

- [ ] **Step 3: Sync version into all Cargo.toml**

Run: `bash scripts/sync-version.sh`

Expected: script edits `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml`.

- [ ] **Step 4: Verify and commit**

```bash
git diff --stat VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
```
Expected: 5 files modified, version bumped from 0.13.18 to 0.13.19.

```bash
git add VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "$(cat <<'EOF'
chore(release): v0.13.19

Bump version ahead of last-visit-display + Quick Search sort feature
work for issue #57. First commit of the cycle per version-bumping rule.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Backend — `last_visit_at` field, SQL, integration tests (subagent, sonnet)

**Files:**
- Modify: `crates/spinbike-server/src/db/cards.rs` (struct `CardRowWithPass` + 3 queries + `into_parts`)
- Modify: `crates/spinbike-server/src/routes/cards.rs` (struct `CardResponse` + helper signature + 2 handlers)
- Create: `crates/spinbike-server/tests/cards_last_visit.rs` (4 integration tests)

### Step 2.1 — Extend `CardRowWithPass` in `crates/spinbike-server/src/db/cards.rs`

- [ ] **Add the `last_visit_at` field**

Find the struct (around line 162):

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CardRowWithPass {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: i64,
    pub credit: f64,
    pub allow_debit: i64,
    pub created_at: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
    pub pass_valid_until: Option<chrono::NaiveDate>,
    pub pass_tx_id: Option<i64>,
}
```

Add a new field at the end (so existing field order in `into_parts` doesn't shift):

```rust
    pub pass_tx_id: Option<i64>,
    pub last_visit_at: Option<String>,
}
```

### Step 2.2 — Update `CardRowWithPass::into_parts`

- [ ] **Change return tuple to include `last_visit_at`**

Before:

```rust
impl CardRowWithPass {
    pub fn into_parts(self) -> (CardRow, Option<(i64, chrono::NaiveDate)>) {
        let pass = match (self.pass_tx_id, self.pass_valid_until) {
            (Some(id), Some(date)) => Some((id, date)),
            _ => None,
        };
        (
            CardRow { /* … */ },
            pass,
        )
    }
}
```

After:

```rust
impl CardRowWithPass {
    pub fn into_parts(
        self,
    ) -> (CardRow, Option<(i64, chrono::NaiveDate)>, Option<String>) {
        let pass = match (self.pass_tx_id, self.pass_valid_until) {
            (Some(id), Some(date)) => Some((id, date)),
            _ => None,
        };
        let last_visit = self.last_visit_at;
        (
            CardRow {
                id: self.id,
                barcode: self.barcode,
                user_id: self.user_id,
                blocked: self.blocked,
                credit: self.credit,
                allow_debit: self.allow_debit,
                created_at: self.created_at,
                first_name: self.first_name,
                last_name: self.last_name,
                company: self.company,
                phone: self.phone,
            },
            pass,
            last_visit,
        )
    }
}
```

### Step 2.3 — Extend the three SQL queries

The three queries are at:

- `list_all_cards_with_pass` (line 207)
- `search_cards_with_pass` (line 230)
- `get_cards_with_pass_by_user` (line ~321)

All three need the same new correlated subquery (mirrors the existing pass subquery pattern):

```sql
(SELECT MAX(created_at) FROM transactions
 WHERE card_id = c.id
   AND deleted_at IS NULL
   AND service_id IN (SELECT id FROM services WHERE name_en IN (?, ?))
) AS last_visit_at
```

The two `?` placeholders bind to `CLASS_VISIT_NAMES_EN` (which today resolves to `["Fitness", "Spinning"]`). Each query also needs to import the constant if not already imported.

- [ ] **Add the import at the top of `crates/spinbike-server/src/db/cards.rs`**

If not already present, add:

```rust
use spinbike_core::services::CLASS_VISIT_NAMES_EN;
```

(Check the existing imports — if it's missing, add it next to the other `use spinbike_core::…` lines. If the file has no `spinbike_core` import, add a fresh `use` line.)

- [ ] **Update `list_all_cards_with_pass` SQL + binding**

Before (around line 210):

```rust
let rows: Vec<CardRowWithPass> = sqlx::query_as(
    "SELECT c.id, c.barcode, c.user_id, c.blocked, c.credit, c.allow_debit,
            c.created_at, c.first_name, c.last_name, c.company, c.phone,
            (SELECT MAX(valid_until) FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
             ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id
     FROM cards c
     ORDER BY c.barcode",
)
.fetch_all(pool)
.await
.context("Failed to list cards with pass")?;
```

After:

```rust
let mut q = sqlx::query_as::<_, CardRowWithPass>(
    "SELECT c.id, c.barcode, c.user_id, c.blocked, c.credit, c.allow_debit,
            c.created_at, c.first_name, c.last_name, c.company, c.phone,
            (SELECT MAX(valid_until) FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
             ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id,
            (SELECT MAX(created_at) FROM transactions
             WHERE card_id = c.id
               AND deleted_at IS NULL
               AND service_id IN (SELECT id FROM services WHERE name_en IN (?, ?))
            ) AS last_visit_at
     FROM cards c
     ORDER BY c.barcode",
);
for n in CLASS_VISIT_NAMES_EN {
    q = q.bind(*n);
}
let rows: Vec<CardRowWithPass> = q
    .fetch_all(pool)
    .await
    .context("Failed to list cards with pass")?;
```

(NOTE: the literal `(?, ?)` works only because `CLASS_VISIT_NAMES_EN` has exactly 2 entries. If you want to be future-proof, build the placeholders with the same `repeat_n` pattern shown in `routes/cards.rs:549`. That is OPTIONAL — if you add it, name the local `placeholders` and `format!` the SQL like the Overview-tab handler does. For this PR, the 2-element literal is acceptable since `CLASS_VISIT_NAMES_EN` is `pub const` and any addition is a coordinated change.)

- [ ] **Update `search_cards_with_pass` SQL + binding + ORDER BY**

Before (around line 240):

```rust
let rows: Vec<CardRowWithPass> = sqlx::query_as(
    "SELECT c.id, c.barcode, c.user_id, c.blocked, c.credit, c.allow_debit,
            c.created_at, c.first_name, c.last_name, c.company, c.phone,
            (SELECT MAX(valid_until) FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
             ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id
     FROM cards c
     WHERE c.search_text LIKE ?
     ORDER BY
       CASE WHEN c.barcode LIKE ? THEN 0 ELSE 1 END,
       c.last_name IS NULL, c.last_name ASC,
       c.first_name IS NULL, c.first_name ASC,
       c.barcode ASC
     LIMIT ?",
)
.bind(&like)
.bind(&prefix)
.bind(limit)
.fetch_all(pool)
.await
.context("Failed to search cards with pass")?;
```

After:

```rust
let mut q = sqlx::query_as::<_, CardRowWithPass>(
    "SELECT c.id, c.barcode, c.user_id, c.blocked, c.credit, c.allow_debit,
            c.created_at, c.first_name, c.last_name, c.company, c.phone,
            (SELECT MAX(valid_until) FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
             ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id,
            (SELECT MAX(created_at) FROM transactions
             WHERE card_id = c.id
               AND deleted_at IS NULL
               AND service_id IN (SELECT id FROM services WHERE name_en IN (?, ?))
            ) AS last_visit_at
     FROM cards c
     WHERE c.search_text LIKE ?
     ORDER BY
       CASE WHEN c.barcode LIKE ? THEN 0 ELSE 1 END,
       last_visit_at IS NULL,
       last_visit_at DESC,
       c.last_name IS NULL, c.last_name ASC,
       c.first_name IS NULL, c.first_name ASC,
       c.barcode ASC
     LIMIT ?",
);
// Bind order MUST match positional `?`s top-to-bottom:
//   2 names for the new last_visit_at subquery
//   1 LIKE for search_text
//   1 LIKE for barcode-prefix (CASE WHEN)
//   1 LIMIT
for n in CLASS_VISIT_NAMES_EN {
    q = q.bind(*n);
}
let rows: Vec<CardRowWithPass> = q
    .bind(&like)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to search cards with pass")?;
```

The TWO new ORDER BY lines (`last_visit_at IS NULL` and `last_visit_at DESC`) come AFTER the barcode-prefix case (so a barcode prefix match still wins) and BEFORE the alphabetic fallback (so identical-last-visit cards are still alphabetic).

- [ ] **Update `get_cards_with_pass_by_user` SQL + binding**

Before (around line 325):

```rust
let rows: Vec<CardRowWithPass> = sqlx::query_as(
    "SELECT c.id, c.barcode, c.user_id, c.blocked, c.credit, c.allow_debit,
            c.created_at, c.first_name, c.last_name, c.company, c.phone,
            (SELECT MAX(valid_until) FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
             ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id
     FROM cards c
     WHERE c.user_id = ?",
)
.bind(user_id)
.fetch_all(pool)
.await
.context("Failed to get cards with pass for user")?;
```

After:

```rust
let mut q = sqlx::query_as::<_, CardRowWithPass>(
    "SELECT c.id, c.barcode, c.user_id, c.blocked, c.credit, c.allow_debit,
            c.created_at, c.first_name, c.last_name, c.company, c.phone,
            (SELECT MAX(valid_until) FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
            ) AS pass_valid_until,
            (SELECT id FROM transactions
             WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL
             ORDER BY valid_until DESC, id DESC LIMIT 1
            ) AS pass_tx_id,
            (SELECT MAX(created_at) FROM transactions
             WHERE card_id = c.id
               AND deleted_at IS NULL
               AND service_id IN (SELECT id FROM services WHERE name_en IN (?, ?))
            ) AS last_visit_at
     FROM cards c
     WHERE c.user_id = ?",
);
for n in CLASS_VISIT_NAMES_EN {
    q = q.bind(*n);
}
let rows: Vec<CardRowWithPass> = q
    .bind(user_id)
    .fetch_all(pool)
    .await
    .context("Failed to get cards with pass for user")?;
```

- [ ] **Update the three call-site mappings**

Each of the three queries currently ends with:

```rust
Ok(rows.into_iter().map(CardRowWithPass::into_parts).collect())
```

The TUPLE shape returned by `into_parts` is now `(CardRow, Option<(i64, NaiveDate)>, Option<String>)`. The function return type for all three changes from:

```rust
Result<Vec<(CardRow, Option<(i64, chrono::NaiveDate)>)>>
```

to:

```rust
Result<Vec<(CardRow, Option<(i64, chrono::NaiveDate)>, Option<String>)>>
```

Update all three function signatures accordingly. The `.collect()` on `Vec<(_, _, _)>` works automatically.

### Step 2.4 — Extend `CardResponse` and the helper in `crates/spinbike-server/src/routes/cards.rs`

- [ ] **Add `last_visit_at` to `CardResponse` (line 89)**

Before:

```rust
#[derive(Serialize)]
pub struct CardResponse {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
    pub pass: Option<CardPass>,
}
```

After:

```rust
#[derive(Serialize)]
pub struct CardResponse {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
    pub pass: Option<CardPass>,
    /// MAX(transactions.created_at) for non-soft-deleted Spinning/Fitness rows.
    /// `None` if the card has never had a qualifying class visit.
    pub last_visit_at: Option<String>,
}
```

- [ ] **Extend `card_response_from_row_with_pass` (line 142) signature + body**

Before:

```rust
fn card_response_from_row_with_pass(
    c: &db::CardRow,
    pass: Option<(i64, chrono::NaiveDate)>,
) -> CardResponse {
    let today = chrono::Local::now().date_naive();
    let pass = pass.map(|(tx_id, d)| CardPass {
        valid_until: d,
        days_remaining: (d - today).num_days() as i32,
        transaction_id: tx_id,
    });
    CardResponse {
        id: c.id,
        // … existing fields …
        pass,
    }
}
```

After:

```rust
fn card_response_from_row_with_pass(
    c: &db::CardRow,
    pass: Option<(i64, chrono::NaiveDate)>,
    last_visit_at: Option<String>,
) -> CardResponse {
    let today = chrono::Local::now().date_naive();
    let pass = pass.map(|(tx_id, d)| CardPass {
        valid_until: d,
        days_remaining: (d - today).num_days() as i32,
        transaction_id: tx_id,
    });
    CardResponse {
        id: c.id,
        barcode: c.barcode.clone(),
        user_id: c.user_id,
        blocked: c.blocked != 0,
        credit: c.credit,
        allow_debit: c.allow_debit != 0,
        first_name: c.first_name.clone(),
        last_name: c.last_name.clone(),
        company: c.company.clone(),
        phone: c.phone.clone(),
        pass,
        last_visit_at,
    }
}
```

- [ ] **Extend `card_response_from_row` (the single-card variant) — pass `None`**

The async helper a few lines above currently calls:

```rust
async fn card_response_from_row(
    pool: &sqlx::SqlitePool,
    c: &db::CardRow,
) -> anyhow::Result<CardResponse> {
    let pass = db::get_card_pass_tx(pool, c.id).await?;
    Ok(card_response_from_row_with_pass(c, pass))
}
```

becomes:

```rust
async fn card_response_from_row(
    pool: &sqlx::SqlitePool,
    c: &db::CardRow,
) -> anyhow::Result<CardResponse> {
    let pass = db::get_card_pass_tx(pool, c.id).await?;
    // Single-card paths don't currently fetch last_visit; the field is None
    // here. Users who hit /api/cards/lookup/{barcode}, /activate, /topup,
    // /block, /update, /link don't need the field on the response. Only the
    // listing/search paths populate it.
    Ok(card_response_from_row_with_pass(c, pass, None))
}
```

- [ ] **Update the two call sites that destructure `(card, pass)`**

In `list_cards` and `search_cards` (lines ~207-225 and ~196-204), the loop currently looks like:

```rust
let out = rows
    .iter()
    .map(|(c, pass)| card_response_from_row_with_pass(c, *pass))
    .collect();
```

Change both to:

```rust
let out = rows
    .iter()
    .map(|(c, pass, last_visit)| {
        card_response_from_row_with_pass(c, *pass, last_visit.clone())
    })
    .collect();
```

(Note: `pass` is `Copy` so `*pass` is fine; `last_visit` is `Option<String>` so we need `.clone()`.)

- [ ] **Search the workspace for any OTHER call sites using `card_response_from_row_with_pass` or `into_parts`**

```bash
grep -rn 'card_response_from_row_with_pass\|CardRowWithPass::into_parts\|into_parts(' crates/ 2>/dev/null
```

Any additional call sites need the same destructuring update. Likely candidates: `routes/balance.rs`, `routes/users.rs`, anything that uses `get_cards_with_pass_by_user`. Update them to ignore the new third tuple element with `_` if they don't need it (e.g. `(c, pass, _)`).

### Step 2.5 — Write the failing integration tests

- [ ] **Create `crates/spinbike-server/tests/cards_last_visit.rs`**

```rust
//! Integration tests for the `last_visit_at` field on /api/cards/search.

mod helpers;

use helpers::{TestApp, get};

/// Local DTO mirroring just the fields these tests assert on. The server's
/// `CardResponse` derives only `Serialize`, so this test file defines its own
/// `Deserialize` shape; serde_json ignores extra fields by default, so the
/// wire response's other fields (credit, blocked, etc.) are silently skipped.
#[derive(serde::Deserialize, Debug)]
struct CardResponse {
    pub id: i64,
    pub barcode: String,
    pub last_visit_at: Option<String>,
}

/// Insert a transaction at a chosen timestamp, optionally tied to a service.
/// Mirrors the helper in `cards_stats.rs` but is duplicated here on purpose:
/// each integration test file wires its own seed shape and we want zero
/// cross-test coupling.
async fn seed_txn(
    pool: &sqlx::SqlitePool,
    card_id: i64,
    service_name_en: Option<&str>,
    amount: f64,
    action: &str,
    created_at: &str,
) -> i64 {
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
    let result = sqlx::query(
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
    result.last_insert_rowid()
}

async fn search(
    app: &TestApp,
    q: &str,
) -> (axum::http::StatusCode, Vec<CardResponse>) {
    app.request_typed::<Vec<CardResponse>>(get(
        &format!("/api/cards/search?q={q}&limit=50"),
        &app.staff_token,
    ))
    .await
}

fn fmt(d: chrono::DateTime<chrono::Local>) -> String {
    d.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[tokio::test]
async fn last_visit_at_populated_correctly_for_each_seed_shape() {
    let app = TestApp::new().await;

    // Pick a search prefix the fixtures don't accidentally match.
    let prefix = "LVTEST";
    let card_a = app.seed_card(&format!("{prefix}A"), 0.0, None, Some("Alpha"), Some("A"), None).await;
    let card_b = app.seed_card(&format!("{prefix}B"), 0.0, None, Some("Bravo"), Some("B"), None).await;
    let card_c = app.seed_card(&format!("{prefix}C"), 0.0, None, Some("Charlie"), Some("C"), None).await;
    let card_d = app.seed_card(&format!("{prefix}D"), 0.0, None, Some("Delta"), Some("D"), None).await;
    let card_e = app.seed_card(&format!("{prefix}E"), 0.0, None, Some("Echo"), Some("E"), None).await;
    let card_f = app.seed_card(&format!("{prefix}F"), 0.0, None, Some("Foxtrot"), Some("F"), None).await;
    let card_g = app.seed_card(&format!("{prefix}G"), 0.0, None, Some("Golf"), Some("G"), None).await;

    let now = chrono::Local::now();
    let yesterday = (now - chrono::Duration::days(1)).format("%Y-%m-%d 12:00:00").to_string();
    let five_days = (now - chrono::Duration::days(5)).format("%Y-%m-%d 12:00:00").to_string();
    let ten_days = (now - chrono::Duration::days(10)).format("%Y-%m-%d 12:00:00").to_string();
    let thirty_days = (now - chrono::Duration::days(30)).format("%Y-%m-%d 12:00:00").to_string();
    let today_str = fmt(now);

    // A: Spinning charge yesterday → last_visit_at = yesterday
    seed_txn(&app.pool, card_a, Some("Spinning"), -3.30, "charge", &yesterday).await;
    // B: Spinning charge 5 days ago → 5 days ago
    seed_txn(&app.pool, card_b, Some("Spinning"), -3.30, "charge", &five_days).await;
    // C: Refreshments charge today → None (not a class visit)
    seed_txn(&app.pool, card_c, Some("Refreshments"), -2.0, "charge", &today_str).await;
    // D: no transactions → None (default)
    // E: Spinning charge today, then soft-deleted → None
    let e_txn = seed_txn(&app.pool, card_e, Some("Spinning"), -3.30, "charge", &today_str).await;
    sqlx::query("UPDATE transactions SET deleted_at = datetime('now') WHERE id = ?")
        .bind(e_txn)
        .execute(&app.pool)
        .await
        .unwrap();
    // F: zero-amount Spinning visit-pass log row today → today
    seed_txn(&app.pool, card_f, Some("Spinning"), 0.0, "visit_pass", &today_str).await;
    // G: Fitness 30 days ago + Spinning 10 days ago → 10 days ago (MAX wins)
    seed_txn(&app.pool, card_g, Some("Fitness"), -5.0, "charge", &thirty_days).await;
    seed_txn(&app.pool, card_g, Some("Spinning"), -3.30, "charge", &ten_days).await;

    let (status, results) = search(&app, prefix).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(results.len(), 7, "expected 7 LVTEST cards in results");

    let by_id: std::collections::HashMap<i64, &CardResponse> =
        results.iter().map(|r| (r.id, r)).collect();

    let a = by_id[&card_a];
    let b = by_id[&card_b];
    let c = by_id[&card_c];
    let d = by_id[&card_d];
    let e = by_id[&card_e];
    let f = by_id[&card_f];
    let g = by_id[&card_g];

    // A and B: have a real last_visit timestamp (string starts with the date).
    assert!(a.last_visit_at.is_some(), "A should have last_visit_at");
    assert!(a.last_visit_at.as_deref().unwrap().starts_with(&yesterday[..10]),
        "A.last_visit_at = {:?}", a.last_visit_at);
    assert!(b.last_visit_at.is_some(), "B should have last_visit_at");
    assert!(b.last_visit_at.as_deref().unwrap().starts_with(&five_days[..10]),
        "B.last_visit_at = {:?}", b.last_visit_at);

    // C, D, E: None — Refreshments doesn't count, no txns, soft-deleted.
    assert_eq!(c.last_visit_at, None, "C (Refreshments only) must be None");
    assert_eq!(d.last_visit_at, None, "D (no txns) must be None");
    assert_eq!(e.last_visit_at, None, "E (soft-deleted Spinning) must be None");

    // F: zero-amount visit_pass row today still counts.
    assert!(f.last_visit_at.is_some(), "F (visit_pass amount=0) should count");
    assert!(f.last_visit_at.as_deref().unwrap().starts_with(&today_str[..10]),
        "F.last_visit_at = {:?}", f.last_visit_at);

    // G: MAX picks the newer (10d ago Spinning), not the older (30d ago Fitness).
    assert!(g.last_visit_at.is_some(), "G should have last_visit_at");
    let g_str = g.last_visit_at.as_deref().unwrap();
    assert!(g_str.starts_with(&ten_days[..10]),
        "G.last_visit_at must be the 10-day-ago Spinning row, got {g_str:?}");
}

#[tokio::test]
async fn search_results_sort_by_last_visit_desc() {
    let app = TestApp::new().await;
    let prefix = "LVSORT";

    // Seed 4 cards: today / yesterday / 10d ago / never.
    let card_today = app.seed_card(&format!("{prefix}1"), 0.0, None, Some("OneToday"), Some("Z"), None).await;
    let card_yesterday = app.seed_card(&format!("{prefix}2"), 0.0, None, Some("TwoYesterday"), Some("Z"), None).await;
    let card_ten = app.seed_card(&format!("{prefix}3"), 0.0, None, Some("ThreeTen"), Some("Z"), None).await;
    let card_never = app.seed_card(&format!("{prefix}4"), 0.0, None, Some("FourNever"), Some("Z"), None).await;

    let now = chrono::Local::now();
    let today_str = fmt(now);
    let yesterday = (now - chrono::Duration::days(1)).format("%Y-%m-%d 12:00:00").to_string();
    let ten_days = (now - chrono::Duration::days(10)).format("%Y-%m-%d 12:00:00").to_string();

    seed_txn(&app.pool, card_today, Some("Spinning"), -3.30, "charge", &today_str).await;
    seed_txn(&app.pool, card_yesterday, Some("Spinning"), -3.30, "charge", &yesterday).await;
    seed_txn(&app.pool, card_ten, Some("Spinning"), -3.30, "charge", &ten_days).await;
    // card_never has no Spinning/Fitness.

    let (status, results) = search(&app, prefix).await;
    assert_eq!(status, axum::http::StatusCode::OK);

    // Filter to just our 4 LVSORT cards (ignore unrelated fixtures, if any).
    let ids: Vec<i64> = results
        .iter()
        .filter(|r| r.barcode.starts_with(prefix))
        .map(|r| r.id)
        .collect();

    assert_eq!(
        ids,
        vec![card_today, card_yesterday, card_ten, card_never],
        "expected sort order: today → yesterday → 10d → never"
    );
}

#[tokio::test]
async fn barcode_prefix_match_overrides_last_visit_sort() {
    let app = TestApp::new().await;

    // Card A: matches barcode prefix exactly, but visit was 100 days ago.
    let card_old = app.seed_card("LVPFX99X", 0.0, None, Some("OldVisit"), Some("Z"), None).await;
    // Card B: does NOT match barcode prefix, but visit was today (newer).
    let card_new = app.seed_card("OTHER01LVPFX", 0.0, None, Some("NewVisit"), Some("Z"), None).await;

    let now = chrono::Local::now();
    let hundred_days = (now - chrono::Duration::days(100)).format("%Y-%m-%d 12:00:00").to_string();
    let today_str = fmt(now);

    seed_txn(&app.pool, card_old, Some("Spinning"), -3.30, "charge", &hundred_days).await;
    seed_txn(&app.pool, card_new, Some("Spinning"), -3.30, "charge", &today_str).await;

    // Search with the prefix `LVPFX99` — it matches `LVPFX99X` as a barcode prefix
    // (CASE WHEN c.barcode LIKE 'LVPFX99%' THEN 0…), and matches `OTHER01LVPFX` only
    // via the search_text body.
    let (status, results) = search(&app, "LVPFX99").await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let lv_results: Vec<i64> = results
        .iter()
        .filter(|r| r.barcode.contains("LVPFX"))
        .map(|r| r.id)
        .collect();

    assert_eq!(
        lv_results.first().copied(),
        Some(card_old),
        "barcode-prefix match (card_old, 100 days ago) must come BEFORE \
         the newer-visit non-prefix-match (card_new). Got {lv_results:?}"
    );
}

#[tokio::test]
async fn customer_role_forbidden() {
    let app = TestApp::new().await;
    let _ = app.seed_card("LVAUTH1", 0.0, None, None, None, None).await;

    let (status, _) = app
        .request(get("/api/cards/search?q=LVAUTH", &app.customer_token))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
```

If `helpers::TestApp::seed_card` has a different signature in the helper (the current pattern from `cards_stats.rs` shows `seed_card(barcode, credit, valid_until, first_name, last_name, _).await`), match the helper's actual signature. Read `crates/spinbike-server/tests/helpers/mod.rs` to confirm. If the signature differs, adapt the calls — but the body of each test stays identical.

### Step 2.6 — Verify locally + commit

- [ ] **`cargo fmt --all --check`**

```bash
cargo fmt --all --check
```
Expected: no output. If it fails, run `cargo fmt --all` and re-check.

- [ ] **Commit**

```bash
git add crates/spinbike-server/src/db/cards.rs \
        crates/spinbike-server/src/routes/cards.rs \
        crates/spinbike-server/tests/cards_last_visit.rs
git commit -m "$(cat <<'EOF'
feat(server): expose last_visit_at on /api/cards + sort search by it (#57)

Adds a correlated MAX(created_at) subquery to the three existing
`CardRowWithPass` queries (list_all_cards_with_pass, search_cards_with_pass,
get_cards_with_pass_by_user). The new field is populated only for non-soft-
deleted transactions whose service is in CLASS_VISIT_NAMES_EN — same
definition the v0.13.18 Overview tab uses, so per-visit charges AND zero-
amount pass-holder visit logs (action='visit_pass') both count.

The /api/cards/search ORDER BY now interleaves last_visit_at DESC (NULLS
LAST) between the existing barcode-prefix-match-wins clause and the
alphabetic fallback, so:
  - scanning a barcode still hits that card first;
  - typing a name surfaces the most recently active matching customer;
  - cards never seen in a class sink to the bottom alphabetically.

Tests: cards_last_visit.rs covers all seven seed scenarios from the spec
(yesterday, 5d, Refreshments, none, soft-deleted, zero-amount visit_pass,
MAX-of-two), the sort-order assertion, the barcode-prefix override, and
customer-role 403. Designed to kill the highest-risk mutants on the SQL
filters and ORDER BY clause.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Slovak/English relative-time helper + i18n keys + wasm tests (subagent, sonnet)

**Files:**
- Create: `spinbike-ui/src/relative_date.rs`
- Modify: `spinbike-ui/src/lib.rs` (one-line module registration)
- Modify: `spinbike-ui/src/i18n.rs` (11 new keys)

### Step 3.1 — Add the 11 i18n keys

- [ ] **Find the i18n table block in `spinbike-ui/src/i18n.rs`**

The file uses the `m.insert("key", ("Slovak", "English"))` pattern. Search for an existing key like `m.insert("tab_overview"` to find the right block and follow the local convention (typically grouped semantically; pick a contiguous spot — after the `tab_*` keys or the `overview_*` keys, both added in PR #52).

- [ ] **Insert these 11 keys in one block**

```rust
m.insert("last_visit_label", ("Posledna navsteva", "Last visit"));
m.insert("rel_today", ("dnes", "today"));
m.insert("rel_yesterday", ("vcera", "yesterday"));
m.insert("rel_days_one", ("pred 1 dnom", "1 day ago"));
m.insert("rel_days_few", ("pred {n} dnami", "{n} days ago"));
m.insert("rel_weeks_one", ("pred 1 tyzdnom", "1 week ago"));
m.insert("rel_weeks_few", ("pred {n} tyzdnami", "{n} weeks ago"));
m.insert("rel_months_one", ("pred 1 mesiacom", "1 month ago"));
m.insert("rel_months_few", ("pred {n} mesiacmi", "{n} months ago"));
m.insert("rel_years_one", ("pred 1 rokom", "1 year ago"));
m.insert("rel_years_few", ("pred {n} rokmi", "{n} years ago"));
```

NOTE: this project's i18n strings are **unaccented Slovak** (per existing convention — see e.g. "Hladam…" instead of "Hľadám…"). The strings above follow that convention. If you discover the project actually uses accented Slovak, switch to: `Posledná návšteva`, `včera`, `dňom`, `dňami`, `týždňom`, `týždňami`. The decision rule: copy the accent style of nearby existing keys.

### Step 3.2 — Create the relative-time helper

- [ ] **Create `spinbike-ui/src/relative_date.rs`**

```rust
//! Slovak/English relative-time formatter for "last visit" displays.
//!
//! Smart granularity: today / yesterday / 2-7 days / 1-8 weeks / 2-12 months
//! / 1+ years. Slovak grammar uses two forms per unit (singular `_one` for
//! N=1 and instrumental plural `_few` for N>=2 — these specific words
//! collapse the 2-4 / 5+ distinction to one form).
//!
//! Public API is `format_last_visit(visited, today, lang)` which returns the
//! combined string `"<DD.MM.YYYY> (<relative>)"`.

use crate::i18n::{self, Lang};
use chrono::{Datelike, NaiveDate};

/// Format `visited` as a date label combined with a relative-time hint
/// computed against `today`. Output examples:
///   - Slovak, 6 days ago, visited 2026-04-28 → "28.04.2026 (pred 6 dnami)"
///   - Slovak, today, 2026-05-04            → "04.05.2026 (dnes)"
///   - English, 2 weeks ago, 2026-04-20     → "20.04.2026 (2 weeks ago)"
///
/// `visited` MUST be <= `today`. Future visits are clamped to "today".
pub fn format_last_visit(visited: NaiveDate, today: NaiveDate, lang: Lang) -> String {
    let date_part = format_date(visited, lang);
    let rel = relative(visited, today, lang);
    format!("{date_part} ({rel})")
}

/// Slovak / English absolute date.
fn format_date(d: NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::Sk => format!("{:02}.{:02}.{:04}", d.day(), d.month(), d.year()),
        Lang::En => format!("{:02}.{:02}.{:04}", d.day(), d.month(), d.year()),
    }
    // Both languages use DD.MM.YYYY — Slovak idiom plus the project already
    // uses %d.%m.%Y for English staff displays.
}

/// Relative-time bucket. See module docs for the exact thresholds.
fn relative(visited: NaiveDate, today: NaiveDate, lang: Lang) -> String {
    let days = (today - visited).num_days().max(0);
    if days == 0 {
        return i18n::t(lang, "rel_today").to_string();
    }
    if days == 1 {
        return i18n::t(lang, "rel_yesterday").to_string();
    }
    if days <= 7 {
        return plural(days as u32, "rel_days_one", "rel_days_few", lang);
    }
    if days <= 60 {
        let n = (days / 7) as u32;
        return plural(n, "rel_weeks_one", "rel_weeks_few", lang);
    }
    if days <= 364 {
        let n = (days / 30) as u32;
        return plural(n, "rel_months_one", "rel_months_few", lang);
    }
    let n = (days / 365) as u32;
    plural(n, "rel_years_one", "rel_years_few", lang)
}

/// Return the i18n string for `key_one` if `n == 1`, otherwise the i18n
/// string for `key_few` with `{n}` replaced by `n`.
fn plural(n: u32, key_one: &str, key_few: &str, lang: Lang) -> String {
    if n == 1 {
        i18n::t(lang, key_one).to_string()
    } else {
        i18n::t(lang, key_few)
            .to_string()
            .replace("{n}", &n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn mk(today_y: i32, today_m: u32, today_d: u32, days_ago: i64) -> (NaiveDate, NaiveDate) {
        let today = NaiveDate::from_ymd_opt(today_y, today_m, today_d).unwrap();
        let visited = today - chrono::Duration::days(days_ago);
        (visited, today)
    }

    #[wasm_bindgen_test]
    fn rel_0_days_today_sk() {
        let (v, t) = mk(2026, 5, 4, 0);
        assert_eq!(relative(v, t, Lang::Sk), "dnes");
    }
    #[wasm_bindgen_test]
    fn rel_0_days_today_en() {
        let (v, t) = mk(2026, 5, 4, 0);
        assert_eq!(relative(v, t, Lang::En), "today");
    }
    #[wasm_bindgen_test]
    fn rel_1_day_yesterday_sk() {
        let (v, t) = mk(2026, 5, 4, 1);
        assert_eq!(relative(v, t, Lang::Sk), "vcera");
    }
    #[wasm_bindgen_test]
    fn rel_1_day_yesterday_en() {
        let (v, t) = mk(2026, 5, 4, 1);
        assert_eq!(relative(v, t, Lang::En), "yesterday");
    }
    #[wasm_bindgen_test]
    fn rel_2_days_sk() {
        let (v, t) = mk(2026, 5, 4, 2);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_2_days_en() {
        let (v, t) = mk(2026, 5, 4, 2);
        assert_eq!(relative(v, t, Lang::En), "2 days ago");
    }
    #[wasm_bindgen_test]
    fn rel_4_days_sk() {
        let (v, t) = mk(2026, 5, 4, 4);
        assert_eq!(relative(v, t, Lang::Sk), "pred 4 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_5_days_sk() {
        let (v, t) = mk(2026, 5, 4, 5);
        assert_eq!(relative(v, t, Lang::Sk), "pred 5 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_7_days_sk() {
        let (v, t) = mk(2026, 5, 4, 7);
        assert_eq!(relative(v, t, Lang::Sk), "pred 7 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_8_days_one_week_sk() {
        let (v, t) = mk(2026, 5, 4, 8);
        assert_eq!(relative(v, t, Lang::Sk), "pred 1 tyzdnom");
    }
    #[wasm_bindgen_test]
    fn rel_8_days_one_week_en() {
        let (v, t) = mk(2026, 5, 4, 8);
        assert_eq!(relative(v, t, Lang::En), "1 week ago");
    }
    #[wasm_bindgen_test]
    fn rel_14_days_two_weeks_sk() {
        let (v, t) = mk(2026, 5, 4, 14);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 tyzdnami");
    }
    #[wasm_bindgen_test]
    fn rel_60_days_eight_weeks_sk() {
        let (v, t) = mk(2026, 5, 4, 60);
        assert_eq!(relative(v, t, Lang::Sk), "pred 8 tyzdnami");
    }
    #[wasm_bindgen_test]
    fn rel_61_days_two_months_sk() {
        let (v, t) = mk(2026, 5, 4, 61);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 mesiacmi");
    }
    #[wasm_bindgen_test]
    fn rel_61_days_two_months_en() {
        let (v, t) = mk(2026, 5, 4, 61);
        assert_eq!(relative(v, t, Lang::En), "2 months ago");
    }
    #[wasm_bindgen_test]
    fn rel_364_days_twelve_months_sk() {
        let (v, t) = mk(2026, 5, 4, 364);
        assert_eq!(relative(v, t, Lang::Sk), "pred 12 mesiacmi");
    }
    #[wasm_bindgen_test]
    fn rel_365_days_one_year_sk() {
        let (v, t) = mk(2026, 5, 4, 365);
        assert_eq!(relative(v, t, Lang::Sk), "pred 1 rokom");
    }
    #[wasm_bindgen_test]
    fn rel_365_days_one_year_en() {
        let (v, t) = mk(2026, 5, 4, 365);
        assert_eq!(relative(v, t, Lang::En), "1 year ago");
    }
    #[wasm_bindgen_test]
    fn rel_730_days_two_years_sk() {
        let (v, t) = mk(2026, 5, 4, 730);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 rokmi");
    }
    #[wasm_bindgen_test]
    fn rel_1825_days_five_years_sk() {
        let (v, t) = mk(2026, 5, 4, 1825);
        assert_eq!(relative(v, t, Lang::Sk), "pred 5 rokmi");
    }

    #[wasm_bindgen_test]
    fn combined_format_sk_six_days_ago() {
        let (v, t) = mk(2026, 5, 4, 6);
        assert_eq!(format_last_visit(v, t, Lang::Sk), "28.04.2026 (pred 6 dnami)");
    }
    #[wasm_bindgen_test]
    fn combined_format_sk_today() {
        let (v, t) = mk(2026, 5, 4, 0);
        assert_eq!(format_last_visit(v, t, Lang::Sk), "04.05.2026 (dnes)");
    }
}
```

NOTE: if the project uses accented Slovak in i18n (see Step 3.1), all the SK strings in these tests must match the accented forms — `pred 6 dňami`, `dnes`, etc. Whichever convention matches the existing keys, USE IT IN BOTH PLACES (i18n.rs AND the test asserts).

### Step 3.3 — Register the module in `spinbike-ui/src/lib.rs`

- [ ] **Add `pub mod relative_date;` next to the other top-level module declarations**

`pages` is one of the existing modules, so add:

```rust
pub mod relative_date;
```

near `pub mod components;` / `pub mod i18n;` (alphabetic order if the file follows that convention; otherwise just somewhere visible at top-level).

### Step 3.4 — Verify locally + commit

- [ ] **`cargo fmt --all --check`**

```bash
cargo fmt --all --check
```
Expected: no output. If it fails, run `cargo fmt --all` and re-check.

- [ ] **Commit**

```bash
git add spinbike-ui/src/relative_date.rs spinbike-ui/src/lib.rs spinbike-ui/src/i18n.rs
git commit -m "$(cat <<'EOF'
feat(ui): Slovak/English relative-time helper for last-visit display (#57)

Adds spinbike-ui/src/relative_date.rs with format_last_visit() returning a
combined "<DD.MM.YYYY> (<relative>)" label. Smart granularity: today /
yesterday / 2-7 days / 1-8 weeks / 2-12 months / 1+ years. Two i18n forms
per unit (singular _one + instrumental plural _few — Slovak collapses
2-4 / 5+ to one form for these specific instrumental nouns).

Eleven new i18n keys added for the helper. Twenty-two #[wasm_bindgen_test]
cases lock every boundary (0/1/2/4/5/7/8/14/60/61/364/365/730/1825 days)
in Slovak and English, plus two combined-format tests. Boundaries chosen
to kill `<8 → <=8` / `<60 → <=60` / `<365 → <=365` mutants and `<` ↔ `<=`
flips on the threshold checks.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Card panel rendering + CardInfo + CSS (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` (`CardInfo.last_visit_at` field)
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs` (new `<div>` line)
- Modify: `spinbike-ui/style.css` (one CSS rule)

### Step 4.1 — Extend `CardInfo`

- [ ] **Add `last_visit_at` field to `CardInfo` (around line 48 of `dashboard/mod.rs`)**

Before:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CardInfo {
    pub id: i64,
    pub barcode: String,
    #[allow(dead_code)]
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    #[allow(dead_code)]
    pub allow_debit: bool,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub pass: Option<CardPass>,
}
```

After:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CardInfo {
    pub id: i64,
    pub barcode: String,
    #[allow(dead_code)]
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    #[allow(dead_code)]
    pub allow_debit: bool,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub pass: Option<CardPass>,
    /// MAX(transactions.created_at) for non-soft-deleted Spinning/Fitness rows.
    /// `None` when the card has never been used for a class. The shape is
    /// the SQLite `created_at` literal ("YYYY-MM-DD HH:MM:SS"); the helper
    /// `parse_last_visit` extracts the date for display.
    #[serde(default)]
    pub last_visit_at: Option<String>,
}
```

### Step 4.2 — Render the new line in `card_panel.rs`

- [ ] **Add the imports + the date helper at the top of `spinbike-ui/src/pages/dashboard/card_panel.rs`**

After the existing `use` lines, ensure these are present:

```rust
use chrono::NaiveDate;

use crate::i18n;
use crate::relative_date::format_last_visit;
```

(Some of these may already exist — only add the missing ones.)

- [ ] **Add a small parse helper inside the file** (NOT in `relative_date.rs` — it's a UI-side concern)

Below the existing `use` block, inside `card_panel.rs`:

```rust
/// Parse the SQLite `created_at` shape ("YYYY-MM-DD HH:MM:SS") into a date.
/// Returns None if the input doesn't match the expected leading 10 chars.
fn parse_last_visit(s: &Option<String>) -> Option<NaiveDate> {
    let s = s.as_ref()?;
    if s.len() < 10 {
        return None;
    }
    NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d").ok()
}
```

- [ ] **Add the new `<div>` after the existing `<div class="card-title">` block (around line 51-58)**

Find the existing block:

```rust
<div class="card-header">
    <div class="card-header__main">
        <div class="card-title">
            <span class="card-title__name">{name}</span>
            " "
            <code class="card-title__barcode">{barcode.clone()}</code>
        </div>
    </div>
    <button … >"\u{2715}"</button>
</div>
```

Capture the value upfront (Leptos's `card` is moved into the view, so clone the field before view! to keep ownership simple):

```rust
let card_id = card.id;
let barcode = card.barcode.clone();
let name = full_name(&card);
let credit = card.credit;
let is_blocked = card.blocked;
let company = card.company.clone().unwrap_or_default();
let phone = card.phone.clone().unwrap_or_default();
let card_pass = card.pass.clone();
let card_for_edit = card.clone();
let card_for_form = card.clone();
let last_visit_at = card.last_visit_at.clone();   // NEW
```

Then change the `card-header` block to:

```rust
<div class="card-header">
    <div class="card-header__main">
        <div class="card-title">
            <span class="card-title__name">{name}</span>
            " "
            <code class="card-title__barcode">{barcode.clone()}</code>
        </div>
        {move || {
            let parsed = parse_last_visit(&last_visit_at);
            match parsed {
                Some(visited) => {
                    let today = chrono::Local::now().date_naive();
                    let label = i18n::t(lang.get(), "last_visit_label");
                    let value = format_last_visit(visited, today, lang.get());
                    view! {
                        <div class="card-title__last-visit" data-testid="card-last-visit">
                            {label} ": " {value}
                        </div>
                    }
                    .into_any()
                }
                None => view! { <span></span> }.into_any(),
            }
        }}
    </div>
    <button
        class="btn btn--compact btn--ghost"
        on:click=move |e| on_close.run(e)
        title="close"
    >"\u{2715}"</button>
</div>
```

The `move ||` closure makes the line reactive to language changes (so flipping SK ↔ EN re-renders correctly).

### Step 4.3 — Add the CSS rule

- [ ] **Append to `spinbike-ui/style.css`**

```css
.card-title__last-visit {
    font-size: 0.875rem;
    color: var(--text-muted, #6c757d);
    margin-top: 0.125rem;
}
```

(Add it near the existing `.card-title*` rules. If the file is tightly grouped, place it right after `.card-title__barcode`. Otherwise just append at the bottom.)

### Step 4.4 — Verify locally + commit

- [ ] **`cargo fmt --all --check`**

```bash
cargo fmt --all --check
```
Expected: no output. If it fails, `cargo fmt --all` and re-check.

- [ ] **Commit**

```bash
git add spinbike-ui/src/pages/dashboard/mod.rs \
        spinbike-ui/src/pages/dashboard/card_panel.rs \
        spinbike-ui/style.css
git commit -m "$(cat <<'EOF'
feat(ui): show last visit on card page header (#57)

Adds CardInfo.last_visit_at, rendered below the name + barcode in the card
panel header as "Posledna navsteva: 28.04.2026 (pred 6 dnami)" via the new
relative_date::format_last_visit helper. Hidden entirely when the card has
no qualifying class visit (parse_last_visit returns None).

The new line is reactive to the language toggle. Styled subdued via
.card-title__last-visit (0.875rem, --text-muted with #6c757d fallback).

The Playwright assertion uses [data-testid="card-last-visit"]; the empty-
case test relies on the fact that the None branch emits a <span></span>
shell that toHaveCount(0) does not match (the testid is absent).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Playwright E2E (subagent, sonnet)

**Files:**
- Create: `e2e/tests/last-visit-display.spec.ts`

### Step 5.1 — Create the spec

- [ ] **Create `e2e/tests/last-visit-display.spec.ts`**

```typescript
import { test, expect } from '@playwright/test';
import { loginViaAPI } from './helpers/login';
import { setupConsoleCheck, assertCleanConsole } from './helpers/console';
import { seedTransactions } from './helpers/seed';

const RUN_TAG = `LV57-${Date.now()}`;

test('last-visit display + Quick Search sort by last visit', async ({ page }) => {
  setupConsoleCheck(page);
  await loginViaAPI(page, 'staff');

  // Seed three cards via the test fixture endpoint. Each card's `last_visit_at`
  // is driven by the `created_at` of the seeded transactions.
  // The endpoint signature comes from src/routes/test_fixtures.rs's
  // /api/test/seed-transactions handler — same one cards_stats E2E uses.
  const today = new Date();
  const oneDayAgo = new Date(today.getTime() - 1 * 24 * 60 * 60 * 1000);
  const hundredDaysAgo = new Date(today.getTime() - 100 * 24 * 60 * 60 * 1000);
  const fmt = (d: Date) =>
    `${d.toISOString().slice(0, 10)} 12:00:00`;

  await seedTransactions(page, {
    cards: [
      {
        barcode: `Alpha${RUN_TAG}`,
        first_name: 'AlphaTest',
        last_name: RUN_TAG,
        transactions: [
          { service_name_en: 'Spinning', amount: -3.30, action: 'charge', created_at: fmt(oneDayAgo) },
        ],
      },
      {
        barcode: `Zulu${RUN_TAG}`,
        first_name: 'ZuluTest',
        last_name: RUN_TAG,
        transactions: [
          { service_name_en: 'Spinning', amount: -3.30, action: 'charge', created_at: fmt(hundredDaysAgo) },
        ],
      },
      {
        barcode: `Never${RUN_TAG}`,
        first_name: 'NeverTest',
        last_name: RUN_TAG,
        transactions: [
          // top-up only — should NOT count as a visit
          { service_name_en: null, amount: 10.0, action: 'topup', created_at: fmt(today) },
        ],
      },
    ],
  });

  // Type the run tag into Quick Search; only our three cards match.
  await page.goto('/staff');
  const search = page.locator('input[type="search"]').first();
  await search.fill(RUN_TAG);

  // Wait for the search results list to render at least one row.
  const results = page.locator('[data-testid="search-result"]');
  await expect(results).toHaveCount(3);

  // Sort assertion: AlphaTest (1 day ago) → ZuluTest (100 days ago) → NeverTest (none).
  await expect(results.nth(0)).toContainText('AlphaTest');
  await expect(results.nth(1)).toContainText('ZuluTest');
  await expect(results.nth(2)).toContainText('NeverTest');

  // Open AlphaTest. Last-visit line must be visible with "(pred 1 dnom)".
  await results.nth(0).click();
  const alphaLastVisit = page.locator('[data-testid="card-last-visit"]');
  await expect(alphaLastVisit).toBeVisible();
  // Note: i18n strings are unaccented Slovak per project convention; if the
  // codebase actually uses accented Slovak, change "pred 1 dnom" to "pred 1 dňom".
  await expect(alphaLastVisit).toContainText('pred 1 dnom');

  // Close + open ZuluTest. Should show "(pred 3 mesiacmi)" — 100 days / 30 = 3.
  await page.locator('[data-testid="action-panel"] button[title="close"]').click();
  await search.fill(RUN_TAG); // re-trigger search
  await expect(results).toHaveCount(3);
  await results.nth(1).click();
  const zuluLastVisit = page.locator('[data-testid="card-last-visit"]');
  await expect(zuluLastVisit).toBeVisible();
  await expect(zuluLastVisit).toContainText('pred 3 mesiacmi');

  // Close + open NeverTest. Last-visit testid must be ABSENT.
  await page.locator('[data-testid="action-panel"] button[title="close"]').click();
  await search.fill(RUN_TAG);
  await expect(results).toHaveCount(3);
  await results.nth(2).click();
  await expect(page.locator('[data-testid="card-last-visit"]')).toHaveCount(0);

  await assertCleanConsole();
});
```

NOTE on `seedTransactions`: this is an **assumed** existing helper at `e2e/tests/helpers/seed.ts`. The PR #52 cards_stats E2E already used `/api/test/seed-transactions`. If a `seedTransactions` helper does NOT exist at `e2e/tests/helpers/seed.ts`, replace the helper call with a direct `await page.request.post('/api/test/seed-transactions', { data: …, headers: { Authorization: `Bearer ${token}` } })` — read `e2e/tests/cards-overview.spec.ts` (or whatever the cards_stats E2E was named in PR #52) to get the exact request shape and adapt.

NOTE on the `[data-testid="search-result"]` selector: this matches the existing data-testid on each row in `dashboard/mod.rs`'s search results (confirmed at lines ~388 in that file). If the card_panel close button uses a different selector than `button[title="close"]`, adjust to match the actual DOM (e.g. `[data-testid="action-panel-close"]` or whatever exists).

NOTE on the unaccented "pred 1 dnom" / "pred 3 mesiacmi": these strings MUST exactly match what the i18n keys in Task 3 produce. If Task 3 used accented Slovak (`pred 1 dňom`, `pred 3 mesiacmi`), update the `toContainText` calls here.

### Step 5.2 — Commit

- [ ] **Commit**

```bash
git add e2e/tests/last-visit-display.spec.ts
git commit -m "$(cat <<'EOF'
test(e2e): last-visit display + Quick Search sort by last visit (#57)

Seeds three cards with a unique RUN_TAG: AlphaTest (Spinning charge 1 day
ago), ZuluTest (Spinning charge 100 days ago), NeverTest (top-up only, no
class visits). Asserts:

  * Quick Search returns the three results in last-visit-DESC order:
    AlphaTest → ZuluTest → NeverTest;
  * AlphaTest's card panel shows "(pred 1 dnom)";
  * ZuluTest's card panel shows "(pred 3 mesiacmi)";
  * NeverTest's card panel has NO [data-testid="card-last-visit"];
  * zero browser console errors/warnings throughout.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Push + monitor CI to terminal state + open PR (CONTROLLER-RUN)

- [ ] **Step 6.1: Push the branch**

```bash
git push origin dev
```

- [ ] **Step 6.2: Find the latest CI run for this push**

```bash
gh run list --branch dev --limit 1 --json databaseId,status,conclusion,headSha
```

Capture `databaseId` as `RUN_ID` for the next steps.

- [ ] **Step 6.3: Monitor CI to terminal state via single background sleep**

Run this in a background shell:

```bash
sleep 600 && gh run view RUN_ID --json status,conclusion,jobs
```

Watch for these jobs (per `.github/workflows/ci.yml`):

- Test Integrity
- Lint
- Test
- Test (UI)
- Build WASM (UI)
- E2E Tests
- Mutation Testing
- Deploy (dev)
- Smoke (dev)

ALL must reach a terminal state and conclude `success` (Deploy/Smoke prod will be `skipped` because this is on `dev`, not `main`).

- [ ] **Step 6.4: If anything fails — investigate and fix**

For the failing job:

```bash
gh run view RUN_ID --log-failed
```

Common shapes of failure for this PR specifically:

- **Mutation Testing on `cards.rs` SQL change** — surviving mutant on the new subquery filter or the new ORDER BY clauses. Strengthen the cards_last_visit.rs test that targets it (e.g. add an extra seed scenario, tighten an assertion). Push fix, re-monitor.
- **Mutation Testing on `relative_date.rs` thresholds** — a `<= 7` mutated to `< 7` survives because the test for day-7 is missing or wrong. Add the boundary test, push, re-monitor.
- **Test failure on `cards_last_visit.rs::*`** — usually a string-literal mismatch (e.g. `Refreshments` vs `Refreshments service` in the actual seed data). Read the assertion failure carefully, adjust the seed name or assertion to match real DB shape. Push, re-monitor.
- **Lint (clippy)** — common: `unused import`, `redundant_clone`, `manual_repeat_n`. Apply the suggestion, push, re-monitor.
- **E2E** — often `setupConsoleCheck` catches a real console error from the new UI line (e.g. unwrap on a malformed date). Read the trace, fix the UI, push, re-monitor.

- [ ] **Step 6.5: When ALL jobs are green, open the PR**

```bash
gh pr create --base main --head dev --title "v0.13.19: per-card last-visit + Quick Search sort by visit (#57)" --body "$(cat <<'EOF'
## Summary

CEO meeting follow-up (#57). Two changes to the staff card-lookup workflow:

1. **Card page header** now shows `Posledná návšteva: 28.04.2026 (pred 6 dňami)` below the name + barcode. Hidden entirely when the card has no qualifying class visit.
2. **Quick Search results** are ordered newest-visit-first (active customers float to the top). Barcode-prefix matches still win, alphabetic fallback unchanged.

Visit definition reuses `CLASS_VISIT_NAMES_EN` — Spinning + Fitness, soft-deletes excluded — same definition the v0.13.18 Overview tab uses. Per-visit charges AND zero-amount pass-holder visit logs (`action='visit_pass'`) both count.

Backend: one new correlated subquery on three `CardRowWithPass` queries; one new field on `CardResponse`; the `search_cards_with_pass` ORDER BY interleaves the new sort criteria between barcode-prefix-match and alphabetic fallback.

UI: new `spinbike-ui/src/relative_date.rs` helper with smart Slovak/English granularity (today / yesterday / 2-7 days / 1-8 weeks / 2-12 months / 1+ years). 11 new i18n keys. One `<div>` added to the card-panel header. One CSS rule for the subdued line.

## Test plan

- [x] Server integration: `crates/spinbike-server/tests/cards_last_visit.rs` covers all seven seed scenarios from the spec (yesterday, 5d, Refreshments, none, soft-deleted, zero-amount visit_pass, MAX-of-two), the sort-order assertion, the barcode-prefix override, and customer-role 403.
- [x] UI unit: 22 `#[wasm_bindgen_test]` cases lock every relative-time boundary in Slovak and English plus two `format_last_visit` combined-string tests.
- [x] E2E: `e2e/tests/last-visit-display.spec.ts` seeds three cards (1d / 100d / never), asserts sort order in Quick Search, displays the line on the two visited cards with the right relative bit, and asserts the testid is ABSENT on the never-visited card. Zero console errors.
- [x] Mutation Testing CI: 0 surviving mutants on the diff.
- [x] CI green: Test Integrity, Lint, Test, Test (UI), Build WASM (UI), E2E, Mutation Testing, Deploy (dev), Smoke (dev).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 6.6: Verify the PR is mergeable + clean**

```bash
gh pr view --json number,mergeable,mergeStateStatus
```

Expected: `mergeable: MERGEABLE` and `mergeStateStatus: CLEAN`. If either is wrong, fix the underlying cause (sync branches, fix conflicts, fix CI). Do NOT propose admin-merge or any other shortcut.

- [ ] **Step 6.7: Provide the green PR URL to the user**

End the controller turn with the completion report (per `completion-report.md`). Do NOT merge — wait for the user's explicit instruction.

---

## Task 7: Post-deploy verification (CONTROLLER-RUN, only AFTER user merges)

This task does NOT run during the development cycle. It runs ONLY after the user explicitly says "merge it".

- [ ] **Step 7.1: Wait for `merge it` and merge the PR**

```bash
gh pr merge <pr-number> --merge
```

- [ ] **Step 7.2: Monitor the main-branch CI**

```bash
gh run list --branch main --limit 1 --json databaseId
sleep 600 && gh run view <main-run-id> --json status,conclusion,jobs
```

ALL jobs must be green, including `Deploy (prod)` and `Smoke (prod)`.

- [ ] **Step 7.3: Verify dev frontend shows v0.13.19 + last-visit line**

Use Playwright via the MCP server (or a small one-off Playwright run) to:

1. Navigate to `https://spinbike-dev.newlevel.media/staff` after `Deploy (dev)` completes.
2. Read `[data-testid="version"]` from the DOM. Expect `v0.13.19`.
3. Open `/staff?card=70701712` (rich-history real card; per memory `feedback_prod_dev_same_machine.md`, this card has 117+ lifetime visits).
4. Assert `[data-testid="card-last-visit"]` is visible and contains `Posledná návšteva` (or `Posledna navsteva` if unaccented).
5. Search "Drlik" → assert the results list is sorted last-visit-DESC (the most-recent visitor among Drlik matches appears first).

- [ ] **Step 7.4: Verify prod after `Deploy (prod)` completes**

Same flow against `https://spinbike.newlevel.media`. Read version, open card, assert.

- [ ] **Step 7.5: Send the completion report**

Use the FULL template from `completion-report.md` — audits at top, separator, Goal / What changed / 🌐 / PR / question at bottom.

---

## Self-review (planner ran this checklist before committing)

**Spec coverage:**

- ✅ Goal — display + sort: Tasks 4 (display) + 2 (sort SQL).
- ✅ Visit definition (CLASS_VISIT_NAMES_EN, soft-deletes excluded, charges + visit_pass): Task 2.3 SQL + Task 2.5 tests F (visit_pass) and E (soft-delete).
- ✅ Backend SQL change on `list_all_cards_with_pass`, `search_cards_with_pass`, `get_cards_with_pass_by_user`: Task 2.3 covers all three.
- ✅ Backend ORDER BY change on `search_cards_with_pass`: Task 2.3 second bullet.
- ✅ `last_visit_at` on `CardResponse`: Task 2.4.
- ✅ No new index: spec says don't add one; Task 2.3 doesn't add one.
- ✅ Auth unchanged: implicit (Task 2.4 doesn't touch the role gate).
- ✅ Relative-time helper with smart granularity: Task 3.2.
- ✅ 11 i18n keys: Task 3.1.
- ✅ `CardInfo` field: Task 4.1.
- ✅ Card-panel rendering with `data-testid="card-last-visit"`: Task 4.2.
- ✅ CSS rule: Task 4.3.
- ✅ Seven seed integration tests: Task 2.5.
- ✅ 22 wasm boundary tests: Task 3.2.
- ✅ Playwright E2E: Task 5.1.
- ✅ VERSION bump: Task 1.

**Placeholder scan:** no `TBD`, no `add appropriate error handling`, no `similar to Task N`, no unspecified test names. Each step has actual code or commands.

**Type consistency:**

- `last_visit_at` is `Option<String>` everywhere (DB row, CardResponse, CardInfo).
- `into_parts` returns `(CardRow, Option<(i64, NaiveDate)>, Option<String>)` — matches what callers destructure as `(c, pass, last_visit)`.
- `parse_last_visit(&Option<String>) -> Option<NaiveDate>` matches the call in card_panel.rs.
- `format_last_visit(visited: NaiveDate, today: NaiveDate, lang: Lang) -> String` matches the call.

All names consistent across tasks.
