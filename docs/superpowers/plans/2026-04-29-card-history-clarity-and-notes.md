# Card History Clarity + Per-Transaction Notes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make card history and report activity feed show ONE consistent set of CEO-friendly Slovak labels for every transaction movement, and let staff record an editable free-text note on every movement, visible on both surfaces.

**Architecture:** Single PR, four parts — (1) unified i18n labels driven by a shared `classify(action, amount, valid_until) -> EventKind` core function; (2) `EventKind::Visit` variant + classifier rewrite (pass-sale > visit > charge > topup > other); (3) incremental migration v10 adding `note TEXT` column with full plumbing through DB rows, API responses, and UI structs; (4) note input on every create flow + inline pencil edit on card history + read-only display on report. New endpoint `PATCH /api/transactions/{id}/note` for edits with 200-char cap and voided-rejection.

**Tech Stack:** Rust (Axum 0.8, Leptos 0.7 CSR/WASM, sqlx + SQLite WAL), Trunk, Playwright E2E, GitHub Actions CI.

**Spec:** [`docs/superpowers/specs/2026-04-29-card-history-clarity-and-notes-design.md`](../specs/2026-04-29-card-history-clarity-and-notes-design.md)

**Branch / PR:** Working on `dev` (= `main = c86b8d4`); after CI green, open NEW PR `dev` → `main`.

---

## File map

**Modified:**

- `VERSION` (already at 0.13.9 from commit `0d5abd7`)
- `crates/spinbike-core/src/reports.rs` — Add `Visit` variant; rename `PassSold` → `PassSale`; extract `classify()` free function; rewrite `kind()` to call it; extend `ReportEvent` with `note: Option<String>`; new + updated unit tests.
- `crates/spinbike-server/src/db/migrations.rs` — Register v10 + define `V10_TRANSACTIONS_NOTE_COLUMN` + new test.
- `crates/spinbike-server/src/db/transactions.rs` — `TransactionRow` gets `note`; SELECT lists in 3 functions get `t.note`; `create_transaction*` get `note: Option<&str>` parameter; new `update_note()` helper.
- `crates/spinbike-server/src/db/reports.rs` — `DbEventRow` gets `note`; both SELECTs add `t.note`; `From<DbEventRow> for ReportEvent` populates note.
- `crates/spinbike-server/src/routes/payments.rs` — `ChargeRequest`, `SellPassRequest`, `LogVisitRequest` gain optional `note`; INSERT statements include `note`; pass to `create_transaction`.
- `crates/spinbike-server/src/routes/cards.rs` — `TopupRequest` gains optional `note`; `TransactionResponse` gains `note`; `card_transactions` mapping includes it.
- `crates/spinbike-server/src/routes/transactions.rs` — Register `PATCH /api/transactions/{id}/note` route; new handler with 200-char cap + 409 on voided.
- `spinbike-ui/src/i18n.rs` — Add 4 `tx_label_*` keys + `tx_note_placeholder` + `tx_note_edit_save` + `tx_note_edit_cancel`; remove 6 obsolete keys (`tx_action_topup`, `tx_action_charge`, `tx_action_visit`, `event_charge`, `event_topup`, `event_pass`).
- `spinbike-ui/src/pages/dashboard/mod.rs` — `TxnInfo` gains `note`; new `TxnInfo::kind()` method calling `spinbike_core::reports::classify`.
- `spinbike-ui/src/pages/dashboard/transactions_list.rs` — Switch to EventKind-based label resolution; add note display + inline pencil edit.
- `spinbike-ui/src/pages/dashboard/action_form.rs` — Add shared `txn-note-input` + plumb into `do_topup`, `do_charge`, sell-pass branch, `visit_click_for`.
- `spinbike-ui/src/pages/reports/activity_feed.rs` — Switch to EventKind-based label resolution; add read-only note display below subtitle.

**Created:**

- `crates/spinbike-server/tests/transactions_note.rs` — 6 integration tests for create-with-note + PATCH note flows.
- `e2e/tests/txn-note.spec.ts` — 6 Playwright E2E cases.

---

## Task 1: Verify VERSION is at 0.13.9

**Files:**
- Read: `VERSION`

- [ ] **Step 1: Confirm version state**

```bash
cat VERSION
git log --oneline -1 VERSION
```

Expected: `VERSION` contains `0.13.9` and the most recent commit touching `VERSION` is `0d5abd7 chore: bump VERSION to 0.13.9 (#26 card history clarity + notes)`.

If both check out: nothing to do. If `VERSION` shows anything else, write `0.13.9\n` to `VERSION`, run `bash scripts/sync-version.sh`, commit `chore: bump VERSION to 0.13.9 (#26 card history clarity + notes)`. Move on.

---

## Task 2: Core EventKind precedence fix (RED → GREEN)

**Files:**
- Modify: `crates/spinbike-core/src/reports.rs`
- Modify: `spinbike-ui/src/pages/reports/activity_feed.rs:155-160` (kind_class match) — to add the new `Visit` arm so the WASM crate still builds after the variant is added.

- [ ] **Step 1: Update existing tests that match `EventKind::PassSold`**

In `crates/spinbike-core/src/reports.rs`, the four existing `kind_*` tests use `EventKind::PassSold`. Rename to `PassSale` everywhere in the test bodies. Don't touch the `kind()` function yet — this is a pre-rename so the test body will compile after the variant rename in step 3.

- [ ] **Step 2: Add new failing tests for `Visit` variant + classify-by-action precedence**

Append to the existing `mod tests` in `crates/spinbike-core/src/reports.rs`. The `ev()` helper currently doesn't take an `action`; extend it. The new tests reference `EventKind::Visit` (not yet defined) and the rewritten `kind()` (not yet rewritten) — they will fail to compile, which IS the RED state for this step:

```rust
fn ev_with_action(action: &str, amount: f64, valid_until: Option<chrono::NaiveDate>) -> ReportEvent {
    ReportEvent {
        id: 1,
        card_id: None,
        card_name: None,
        barcode: None,
        action: action.to_string(),
        amount,
        service_name_sk: None,
        service_name_en: None,
        service_kind: None,
        created_at: "2026-04-29 12:00:00".into(),
        valid_until,
        voided: false,
        note: None,
    }
}

#[test]
fn kind_visit_when_action_is_visit_zero_amount_no_pass() {
    // The bug fix from issue #26: today this lands in EventKind::Other.
    assert_eq!(
        ev_with_action("visit", 0.0, None).kind(),
        EventKind::Visit,
    );
}

#[test]
fn kind_visit_overrides_charge_when_action_is_visit_and_amount_negative() {
    // Defensive: if a future bug lets a visit have a negative amount, the
    // action='visit' should still win over the amount<0 charge classification.
    assert_eq!(
        ev_with_action("visit", -1.0, None).kind(),
        EventKind::Visit,
    );
}

#[test]
fn kind_passsale_overrides_visit_when_valid_until_set() {
    // valid_until still wins over action='visit' (defensive — should never
    // happen in practice, but the precedence must be deterministic).
    let d = chrono::NaiveDate::from_ymd_opt(2026, 5, 24).unwrap();
    assert_eq!(
        ev_with_action("visit", 0.0, Some(d)).kind(),
        EventKind::PassSale,
    );
}

#[test]
fn kind_charge_when_action_charge_amount_negative_no_pass() {
    // Preserves Charge for non-visit non-pass charges.
    assert_eq!(
        ev_with_action("charge", -5.0, None).kind(),
        EventKind::Charge,
    );
}

#[test]
fn kind_other_when_amount_zero_action_neither_visit_nor_charge_no_pass() {
    // Guards Other as the residual bucket for a 'storno' or unknown action
    // with amount=0.
    assert_eq!(
        ev_with_action("storno", 0.0, None).kind(),
        EventKind::Other,
    );
}
```

- [ ] **Step 3: Confirm tests fail (RED)**

Just commit at the end of this task — local cargo runs are forbidden by project policy. The push at Task 12 will surface compile errors if the tests don't actually fail at this point. If you want a sanity check, run only `cargo fmt --all --check`. The critical assertion is: the compile-once on push must show `EventKind::Visit` undefined (which is exactly what we're about to fix).

- [ ] **Step 4: Add `Visit` variant + rename `PassSold` → `PassSale` + add `note` field + extract `classify()` + rewrite `kind()`**

Replace the entire `EventKind` enum, the `ReportEvent` struct, and the `kind()` impl in `crates/spinbike-core/src/reports.rs`:

```rust
/// One row in the activity feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEvent {
    pub id: i64,
    pub card_id: Option<i64>,
    pub card_name: Option<String>,
    pub barcode: Option<String>,
    pub action: String,
    pub amount: f64,
    /// Slovak label for the service (NULL when the transaction has no service).
    pub service_name_sk: Option<String>,
    /// English label for the service (NULL when the transaction has no service).
    pub service_name_en: Option<String>,
    /// Stable kind enum: `"generic"` or `"monthly_pass"`. NULL when service is NULL.
    pub service_kind: Option<String>,
    pub created_at: String,
    pub valid_until: Option<chrono::NaiveDate>,
    pub voided: bool,
    /// Free-text staff note (≤200 chars). NULL when no note was recorded.
    #[serde(default)]
    pub note: Option<String>,
}

/// Classification for UI colour/icon logic. Derived from action + amount + valid_until.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    PassSale, // valid_until IS NOT NULL  (highest precedence)
    Visit,    // action == "visit"        (next — covers €0 pass attendance)
    Charge,   // amount < 0
    TopUp,    // amount > 0
    Other,    // residual (e.g. action='storno' with amount=0)
}

/// Free function so both ReportEvent (server-side reports) and TxnInfo
/// (UI dashboard) can derive the same EventKind from the same fields.
/// Precedence (top-down, first match wins):
///   1. valid_until.is_some() → PassSale
///   2. action == "visit"     → Visit
///   3. amount < 0.0          → Charge
///   4. amount > 0.0          → TopUp
///   5. else                  → Other
pub fn classify(action: &str, amount: f64, valid_until: Option<chrono::NaiveDate>) -> EventKind {
    if valid_until.is_some() {
        EventKind::PassSale
    } else if action == "visit" {
        EventKind::Visit
    } else if amount < 0.0 {
        EventKind::Charge
    } else if amount > 0.0 {
        EventKind::TopUp
    } else {
        EventKind::Other
    }
}

impl ReportEvent {
    pub fn kind(&self) -> EventKind {
        classify(&self.action, self.amount, self.valid_until)
    }
}
```

Inside the existing `mod tests`, the `ev()` helper must also gain the `note` field. Update it:

```rust
fn ev(amount: f64, valid_until: Option<chrono::NaiveDate>) -> ReportEvent {
    ReportEvent {
        id: 1,
        card_id: None,
        card_name: None,
        barcode: None,
        action: "x".into(),
        amount,
        service_name_sk: None,
        service_name_en: None,
        service_kind: None,
        created_at: "2026-04-24 12:00:00".into(),
        valid_until,
        voided: false,
        note: None,
    }
}
```

The existing `kind_pass_sold_*` test currently uses action `"x"`; under the new precedence, valid_until still wins, so the test passes as-is — only the variant name changed (already renamed in step 1). Confirm by re-reading the test bodies after the rename.

- [ ] **Step 5: Update consumers of the renamed variant in spinbike-ui**

`spinbike-ui/src/pages/reports/activity_feed.rs` matches on `EventKind::PassSold` at lines 158 and 164. Replace the `PassSold` arms with the new variant set:

```rust
let kind_class = match kind {
    EventKind::Charge => "feed-dot feed-dot--charge",
    EventKind::TopUp => "feed-dot feed-dot--topup",
    EventKind::PassSale => "feed-dot feed-dot--pass",
    EventKind::Visit => "feed-dot feed-dot--visit",
    EventKind::Other => "feed-dot feed-dot--voided",
};
```

The `event_label_key` match block (lines 161-166) is also affected — but its full rewrite happens in Task 7 (label switch). For THIS task, just keep the file compiling: replace the match block temporarily as:

```rust
let event_label_key = match kind {
    EventKind::Charge => "event_charge",
    EventKind::TopUp => "event_topup",
    EventKind::PassSale => "event_pass",
    EventKind::Visit => "event_charge", // placeholder — Task 7 replaces all keys
    EventKind::Other => "event_other",
};
```

(Yes this temporarily mis-labels Visit as a Charge; Task 7 fixes it. We do this so Task 2 produces a clean compile-and-test pass for the core change without dragging i18n into scope.)

- [ ] **Step 6: Add CSS class for `feed-dot--visit`**

In `spinbike-ui/src/styles.scss` (or whichever stylesheet declares the existing `feed-dot--*` classes — grep first to find the file). Locate the existing `.feed-dot--charge` rule and add a sibling using the same hue family as `--info` (the activity_feed already uses info-blue for charges):

```scss
.feed-dot--visit {
  background: var(--info-soft, #b8d4f0);  // soft-blue: free pass attendance, secondary to charge
}
```

If the existing pattern uses different tokens, mirror those exactly — DO NOT introduce new design tokens.

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-core/src/reports.rs spinbike-ui/src/pages/reports/activity_feed.rs spinbike-ui/src/styles.scss
git commit -m "feat(core): add EventKind::Visit + classify() with action precedence (#26)"
```

(If the SCSS file path is different, swap the path in the `git add` line. Use explicit paths — never `git add -A`.)

---

## Task 3: Migration v10 — note column on transactions

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs`

- [ ] **Step 1: Add v10 migration constant**

In `crates/spinbike-server/src/db/migrations.rs`, add after `V9_TRANSACTIONS_LEGACY_BACKFILL_MARKER` (around line 257):

```rust
const V10_TRANSACTIONS_NOTE_COLUMN: &str = r#"
ALTER TABLE transactions ADD COLUMN note TEXT;
"#;
```

- [ ] **Step 2: Register v10 in MIGRATIONS array**

Add the new entry to `MIGRATIONS` (in the same file, around line 30):

```rust
pub(crate) static MIGRATIONS: &[(i64, &str, &str)] = &[
    // ... existing entries 1..=9 ...
    (
        10,
        "transactions: free-text note column",
        V10_TRANSACTIONS_NOTE_COLUMN,
    ),
];
```

(Append as the last entry. Keep the comma after the closing `)`.)

- [ ] **Step 3: Add migration unit test**

Append to `mod tests` in the same file:

```rust
#[tokio::test]
async fn v10_adds_note_column_to_transactions() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let cols: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM pragma_table_info('transactions')")
            .fetch_all(&pool)
            .await
            .unwrap();
    let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
    assert!(
        names.contains(&"note"),
        "transactions.note column missing; found: {names:?}"
    );
}

#[tokio::test]
async fn v10_note_defaults_to_null_for_existing_rows() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    // Inserting a row without a note column read should yield NULL.
    let card_id: i64 = sqlx::query_scalar(
        "INSERT INTO cards (barcode) VALUES ('NOTE-TEST') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transactions (card_id, amount, action) VALUES (?, ?, ?)",
    )
    .bind(card_id)
    .bind(1.0_f64)
    .bind("topup")
    .execute(&pool)
    .await
    .unwrap();
    let note: Option<String> = sqlx::query_scalar(
        "SELECT note FROM transactions WHERE card_id = ?",
    )
    .bind(card_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(note.is_none(), "fresh row's note must be NULL");
}
```

- [ ] **Step 4: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): migration v10 adds note column to transactions (#26)"
```

---

## Task 4: Plumb note column through DB rows + structs (read side)

**Files:**
- Modify: `crates/spinbike-server/src/db/transactions.rs`
- Modify: `crates/spinbike-server/src/db/reports.rs`
- Modify: `crates/spinbike-server/src/routes/cards.rs` (TransactionResponse + card_transactions mapping)
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs` (TxnInfo)

**No new tests in this task** — the existing tests already exercise the SELECT lists; if `note` plumbing breaks any column ordering, those tests fail. Test additions for create-with-note happen in Task 5.

- [ ] **Step 1: Add `note` to `TransactionRow`**

In `crates/spinbike-server/src/db/transactions.rs`, after `pub deleted_at: Option<String>` (line 24):

```rust
    pub deleted_at: Option<String>,
    pub note: Option<String>,
}
```

- [ ] **Step 2: Add `t.note` to all three SELECT lists in transactions.rs**

In `list_transactions_for_card` (line 86), `list_transactions_for_card_paginated` (lines 118 and 134), and `list_transactions_for_user` (line 157), each SELECT lists transaction columns ending in `t.deleted_at`. Append `, t.note` after `t.deleted_at` in EACH of the four SELECTs (the paginated function has two SELECTs — one with cursor, one without).

Example for `list_transactions_for_card`:

```rust
    let txns = sqlx::query_as::<_, TransactionRow>(
        "SELECT t.id, t.user_id, t.card_id, t.staff_id, t.service_id,
                t.amount, t.action, t.created_at, t.valid_until,
                s.name_sk AS service_name_sk, s.name_en AS service_name_en, s.kind AS service_kind,
                t.deleted_at, t.note
         FROM transactions t
         LEFT JOIN services s ON s.id = t.service_id
         WHERE t.card_id = ?
         ORDER BY t.created_at DESC",
    )
```

Apply the same `, t.note` append to the other three SELECTs.

- [ ] **Step 3: Add `note` to `DbEventRow` + both report SELECTs + From impl**

In `crates/spinbike-server/src/db/reports.rs`, append `note: Option<String>,` to `DbEventRow` (after `deleted_at` at line 175):

```rust
#[derive(sqlx::FromRow)]
struct DbEventRow {
    id: i64,
    card_id: Option<i64>,
    card_name: Option<String>,
    barcode: Option<String>,
    action: String,
    amount: f64,
    service_name_sk: Option<String>,
    service_name_en: Option<String>,
    service_kind: Option<String>,
    created_at: String,
    valid_until: Option<chrono::NaiveDate>,
    deleted_at: Option<String>,
    note: Option<String>,
}
```

In both `day_report` (around line 85) and `range_report` (around line 213), the events SELECT lists `t.id, t.card_id, t.amount, t.action, t.created_at, t.valid_until, t.deleted_at, ...`. Append `, t.note` after `t.deleted_at` in BOTH.

In the `From<DbEventRow> for ReportEvent` impl (around line 178):

```rust
impl From<DbEventRow> for ReportEvent {
    fn from(r: DbEventRow) -> Self {
        ReportEvent {
            id: r.id,
            card_id: r.card_id,
            card_name: r.card_name.filter(|s| !s.trim().is_empty()),
            barcode: r.barcode,
            action: r.action,
            amount: r.amount,
            service_name_sk: r.service_name_sk,
            service_name_en: r.service_name_en,
            service_kind: r.service_kind,
            created_at: r.created_at,
            valid_until: r.valid_until,
            voided: r.deleted_at.is_some(),
            note: r.note,
        }
    }
}
```

- [ ] **Step 4: Add `note` to `TransactionResponse` + `card_transactions` mapping**

In `crates/spinbike-server/src/routes/cards.rs`, locate `TransactionResponse` (search for `pub struct TransactionResponse` — it's around line 80-100). Add `pub note: Option<String>` after the existing fields (mirror what's there — keep the same `#[serde(skip_serializing_if = "Option::is_none")]` attributes if they're applied).

Then locate the `.map(|t| TransactionResponse { ... })` block in `card_transactions` (around line 500). Add `note: t.note,` to the constructed value.

Same treatment for `my_balance` (around line 525-545) — if it constructs `TransactionResponse`, add `note: t.note`. If it returns a different type, ignore.

- [ ] **Step 5: Add `note` to `TxnInfo` in spinbike-ui**

In `spinbike-ui/src/pages/dashboard/mod.rs`, add to `TxnInfo` after `pub deleted_at: Option<String>` (line 132):

```rust
#[serde(default)]
pub note: Option<String>,
```

- [ ] **Step 6: Add `TxnInfo::kind()` method**

In the same file, in the existing `impl TxnInfo` block (line 135), add a new method:

```rust
impl TxnInfo {
    pub fn service_label(&self, lang: crate::i18n::Lang) -> Option<&str> {
        match lang {
            crate::i18n::Lang::Sk => self.service_name_sk.as_deref(),
            crate::i18n::Lang::En => self.service_name_en.as_deref(),
        }
    }

    /// EventKind classification — same precedence as ReportEvent::kind().
    /// Sourced from spinbike-core so card history and report stay in sync.
    pub fn kind(&self) -> spinbike_core::reports::EventKind {
        spinbike_core::reports::classify(&self.action, self.amount, self.valid_until)
    }
}
```

- [ ] **Step 7: Commit**

```bash
git add crates/spinbike-server/src/db/transactions.rs crates/spinbike-server/src/db/reports.rs crates/spinbike-server/src/routes/cards.rs spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "feat: plumb note through TransactionRow + ReportEvent + TxnInfo (#26)"
```

---

## Task 5: Create endpoints accept optional note (4 flows + integration tests)

**Files:**
- Modify: `crates/spinbike-server/src/db/transactions.rs` (extend `create_transaction*` signatures)
- Modify: `crates/spinbike-server/src/routes/payments.rs` (3 endpoints + 3 INSERTs)
- Modify: `crates/spinbike-server/src/routes/cards.rs` (`topup_card` handler + `TopupRequest` + initial-credit topup in `create_card`)
- Create: `crates/spinbike-server/tests/transactions_note.rs`

- [ ] **Step 1: Extend `create_transaction` and `create_transaction_with_valid_until` with note param**

In `crates/spinbike-server/src/db/transactions.rs`, replace both function signatures + their INSERT statements:

```rust
#[allow(clippy::too_many_arguments)]
pub async fn create_transaction(
    pool: &SqlitePool,
    user_id: Option<i64>,
    card_id: Option<i64>,
    staff_id: Option<i64>,
    service_id: Option<i64>,
    amount: f64,
    action: &str,
    note: Option<&str>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, note)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(user_id)
    .bind(card_id)
    .bind(staff_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .bind(note)
    .fetch_one(pool)
    .await
    .context("Failed to create transaction")?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_transaction_with_valid_until(
    pool: &SqlitePool,
    user_id: Option<i64>,
    card_id: Option<i64>,
    staff_id: Option<i64>,
    service_id: Option<i64>,
    amount: f64,
    action: &str,
    valid_until: Option<chrono::NaiveDate>,
    note: Option<&str>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, valid_until, note)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(user_id)
    .bind(card_id)
    .bind(staff_id)
    .bind(service_id)
    .bind(amount)
    .bind(action)
    .bind(valid_until)
    .bind(note)
    .fetch_one(pool)
    .await
    .context("Failed to create transaction with valid_until")?;
    Ok(id)
}
```

- [ ] **Step 2: Update existing call sites in `db/transactions.rs::tests`**

The `mod tests` calls `create_transaction(...)` in five places (`create_and_list_transactions` calls it twice, plus `transaction_without_valid_until_reads_back_as_none`, `soft_delete_sets_deleted_at`, `list_transactions_returns_deleted_at_flag`). Each call needs a trailing `None` argument. Same for the one `create_transaction_with_valid_until` call in `transaction_stores_and_retrieves_valid_until`.

Quick search-replace pattern: every existing call ends `..., "charge")` or `..., "topup")` or similar — change each to `..., "charge", None)` etc.

- [ ] **Step 3: Update call sites in `crates/spinbike-server/src/routes/cards.rs`**

`crates/spinbike-server/src/routes/cards.rs` has two existing calls to `create_transaction`:

- In `create_card` (line ~327) for the initial-credit topup. Append `, None` as the final arg.
- In `topup_card` (line ~379). Replace with the note plumbing — see step 4 below for the full handler rewrite.

- [ ] **Step 4: Add `note` to `TopupRequest` + plumb into `topup_card`**

Locate `TopupRequest` in `crates/spinbike-server/src/routes/cards.rs` (search `struct TopupRequest`). Add `pub note: Option<String>`:

```rust
#[derive(Deserialize)]
pub struct TopupRequest {
    pub card_id: i64,
    pub amount: f64,
    #[serde(default)]
    pub note: Option<String>,
}
```

In `topup_card` (around line 355), add note validation + pass to `create_transaction`:

```rust
async fn topup_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<TopupRequest>,
) -> Result<Json<CardResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    if body.amount <= 0.0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Amount must be greater than zero"})),
        ));
    }
    if let Some(n) = body.note.as_deref() {
        if n.chars().count() > 200 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Note must be 200 characters or fewer"})),
            ));
        }
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    db::update_credit(&state.pool, body.card_id, body.amount)
        .await
        .map_err(internal_error)?;

    transactions::create_transaction(
        &state.pool,
        None,
        Some(body.card_id),
        Some(claims.sub),
        None,
        body.amount,
        "topup",
        note_for_db,
    )
    .await
    .map_err(internal_error)?;

    let card = sqlx::query_as::<_, db::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        card_response_from_row(&state.pool, &card)
            .await
            .map_err(internal_error)?,
    ))
}
```

The 200-char check uses `.chars().count()` (NOT `.len()`) so multi-byte Slovak characters (š, č, ž etc.) count as one each, matching the client-side `maxlength` attribute. Empty/whitespace-only notes are stored as NULL.

- [ ] **Step 5: Add `note` to `ChargeRequest`, `SellPassRequest`, `LogVisitRequest`**

In `crates/spinbike-server/src/routes/payments.rs`:

```rust
#[derive(Deserialize)]
pub struct ChargeRequest {
    pub card_id: i64,
    pub amount: f64,
    pub service_id: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct LogVisitRequest {
    pub card_id: i64,
    pub service_id: i64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct SellPassRequest {
    pub card_id: i64,
    pub price: f64,
    pub valid_until: chrono::NaiveDate,
    #[serde(default)]
    pub note: Option<String>,
}
```

(Leave `StornoRequest` alone — no UI flow for staff-typed notes on storno, and storno already has a `reason` field; we don't conflate.)

- [ ] **Step 6: Plumb note into the 3 INSERT statements + log_visit's `create_transaction` call**

In `charge` (around line 138):

```rust
    if let Some(n) = body.note.as_deref() {
        if n.chars().count() > 200 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Note must be 200 characters or fewer"})),
            ));
        }
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());
```

(Insert this validation block right after the `if body.amount <= 0.0` check, before `let amount = cards::round_cents(body.amount);`.)

Then update the INSERT to include `note`:

```rust
    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, note)
         VALUES (?, ?, ?, ?, ?, 'charge', ?)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind(body.service_id)
    .bind(-amount)
    .bind(note_for_db)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;
```

In `sell_pass` (around line 291), apply the same note-validation block (after the existing valid_until check, before `let price = cards::round_cents(body.price);`) and update the INSERT:

```rust
    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, valid_until, note)
         VALUES (?, ?, ?, ?, ?, 'charge', ?, ?)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind(Some(service_id))
    .bind(-price)
    .bind(body.valid_until)
    .bind(note_for_db)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;
```

In `log_visit` (around line 361), apply the validation block (after the service_exists check) and update the call:

```rust
    let tx_id = crate::db::transactions::create_transaction(
        &state.pool,
        None,
        Some(body.card_id),
        Some(claims.sub),
        Some(body.service_id),
        0.0,
        "visit",
        note_for_db,
    )
    .await
    .map_err(internal_error)?;
```

In `storno` (around line 207), the existing INSERT has no note — append a literal `NULL` to the column list and append `None::<String>` to the binds, matching the new signature shape:

```rust
    let tx_id: i64 = sqlx::query_scalar(
        "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action, note)
         VALUES (?, ?, ?, ?, ?, 'storno', NULL)
         RETURNING id",
    )
    .bind(card.user_id)
    .bind(Some(body.card_id))
    .bind(Some(claims.sub))
    .bind::<Option<i64>>(None)
    .bind(amount)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;
```

(No `body.note` reference because we're not adding `note` to `StornoRequest`. Hardcoded `NULL` keeps the column list explicit.)

- [ ] **Step 7: Update `legacy_backfill` and `migrate_legacy.rs` call sites**

Search for all remaining call sites of `create_transaction(` and `create_transaction_with_valid_until(` outside `routes/` and `db/transactions.rs`:

```bash
grep -rn 'create_transaction\(\|create_transaction_with_valid_until\(' crates/spinbike-server/src/ --include='*.rs' | grep -v 'fn create_transaction'
```

For every call, append `, None` (or `, None,` if there are more args after). Notably:

- `crates/spinbike-server/src/db/backfill.rs` — legacy import inserts. None of these get notes.
- `crates/spinbike-server/src/bin/migrate_legacy.rs` — same.
- The existing topup call in `routes/cards.rs::create_card` for initial credit (around line 327) — append `, None`.

- [ ] **Step 8: Create integration test file `crates/spinbike-server/tests/transactions_note.rs`**

Mirror the existing `crates/spinbike-server/tests/payments.rs` setup pattern (helpers, `setup_app()`, `staff_token()` etc. — read it first to match signatures). Write the file:

```rust
//! Integration tests for #26 — per-transaction note support
//! covers: create endpoints accepting note + PATCH /api/transactions/{id}/note.

mod helpers;
use helpers::{StaffSession, ResponseExt, TestApp};

use serde_json::json;

#[tokio::test]
async fn charge_persists_note() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("NOTE-CHARGE", 50.0).await;

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 2.50, "note": "Proteinová tyčinka"}),
            &session.token,
        )
        .await;
    assert_eq!(resp.status, 200);
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some("Proteinová tyčinka"));
}

#[tokio::test]
async fn topup_persists_note() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("NOTE-TOPUP", 0.0).await;

    let resp = app
        .post_json("/api/cards/topup",
            json!({"card_id": card_id, "amount": 30.0, "note": "Platil v hotovosti"}),
            &session.token,
        )
        .await;
    assert_eq!(resp.status, 200);

    let note: Option<String> = sqlx::query_scalar(
        "SELECT note FROM transactions WHERE card_id = ? ORDER BY id DESC LIMIT 1"
    )
    .bind(card_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(note.as_deref(), Some("Platil v hotovosti"));
}

#[tokio::test]
async fn sell_pass_persists_note() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("NOTE-PASS", 50.0).await;
    let valid_until = (chrono::Local::now().date_naive() + chrono::Duration::days(30))
        .format("%Y-%m-%d").to_string();

    let resp = app
        .post_json("/api/payments/sell-pass",
            json!({"card_id": card_id, "price": 35.0, "valid_until": valid_until,
                   "note": "Zľava 10%"}),
            &session.token,
        )
        .await;
    assert_eq!(resp.status, 200);
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(note.as_deref(), Some("Zľava 10%"));
}

#[tokio::test]
async fn empty_note_stored_as_null() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("NOTE-EMPTY", 50.0).await;

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0, "note": "   "}),
            &session.token,
        )
        .await;
    assert_eq!(resp.status, 200);
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(note.is_none(), "whitespace-only note must store as NULL");
}

#[tokio::test]
async fn note_over_200_chars_rejected() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("NOTE-LONG", 50.0).await;
    let long = "x".repeat(201);

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0, "note": long}),
            &session.token,
        )
        .await;
    assert_eq!(resp.status, 400);
    assert!(resp.body.get("error").unwrap().as_str().unwrap()
        .contains("200 characters"));
}

#[tokio::test]
async fn missing_note_field_works_unchanged() {
    // Legacy clients send no note field; default deserializer must keep working.
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("NOTE-MISSING", 50.0).await;

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0}),
            &session.token,
        )
        .await;
    assert_eq!(resp.status, 200);
}
```

Read `crates/spinbike-server/tests/helpers/` first to match the existing TestApp/StaffSession API exactly. If `activate_card` doesn't exist, follow whatever pattern `crates/spinbike-server/tests/payments.rs` uses for card setup (e.g., direct sqlx INSERT into `cards` + `update_credit`). The point is to seed a card with credit and exercise the four create endpoints.

- [ ] **Step 9: Commit**

```bash
git add crates/spinbike-server/src/db/transactions.rs crates/spinbike-server/src/routes/payments.rs crates/spinbike-server/src/routes/cards.rs crates/spinbike-server/src/db/backfill.rs crates/spinbike-server/src/bin/migrate_legacy.rs crates/spinbike-server/tests/transactions_note.rs
git commit -m "feat(api): create endpoints accept optional note (≤200 chars) for #26"
```

---

## Task 6: PATCH /api/transactions/{id}/note endpoint

**Files:**
- Modify: `crates/spinbike-server/src/routes/transactions.rs`
- Modify: `crates/spinbike-server/tests/transactions_note.rs`

- [ ] **Step 1: Add the PATCH route + handler**

In `crates/spinbike-server/src/routes/transactions.rs`, register the new route in `pub fn routes()`:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/transactions/{id}", delete(void_transaction))
        .route(
            "/api/transactions/{id}/valid-until",
            patch(patch_valid_until),
        )
        .route(
            "/api/transactions/{id}/note",
            patch(patch_note),
        )
}
```

Add the request/response structs after `PatchValidUntilResp`:

```rust
#[derive(Deserialize)]
struct PatchNoteReq {
    /// New note. `None` (or absent) → clear the column.
    #[serde(default)]
    note: Option<String>,
}

#[derive(serde::Serialize)]
struct PatchNoteResp {
    id: i64,
    note: Option<String>,
}
```

Add the handler after `patch_valid_until`:

```rust
async fn patch_note(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchNoteReq>,
) -> Result<Json<PatchNoteResp>, (StatusCode, Json<serde_json::Value>)> {
    // Same role gate as void / valid-until edit — staff only.
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // 200-char cap, counted in characters (not bytes) so Slovak diacritics
    // don't count double. Empty/whitespace becomes NULL.
    let normalized: Option<String> = match body.note.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            if s.chars().count() > 200 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Note must be 200 characters or fewer"})),
                ));
            }
            Some(s.to_string())
        }
        _ => None,
    };

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, card_id, deleted_at, valid_until FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Transaction not found"})),
        ));
    };
    if row.deleted_at.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Cannot edit note on a voided transaction"})),
        ));
    }

    sqlx::query("UPDATE transactions SET note = ? WHERE id = ?")
        .bind(normalized.as_deref())
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(PatchNoteResp {
        id,
        note: normalized,
    }))
}
```

- [ ] **Step 2: Append PATCH integration tests to `crates/spinbike-server/tests/transactions_note.rs`**

```rust
#[tokio::test]
async fn patch_note_updates_existing_row() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("PATCH-1", 50.0).await;

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0, "note": "first"}),
            &session.token,
        )
        .await;
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let patch = app
        .patch_json(&format!("/api/transactions/{tx_id}/note"),
            json!({"note": "edited"}),
            &session.token,
        )
        .await;
    assert_eq!(patch.status, 200);
    assert_eq!(patch.body.get("note").unwrap().as_str(), Some("edited"));

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id).fetch_one(&app.pool).await.unwrap();
    assert_eq!(note.as_deref(), Some("edited"));
}

#[tokio::test]
async fn patch_note_clears_with_null_or_empty() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("PATCH-2", 50.0).await;

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0, "note": "to clear"}),
            &session.token,
        )
        .await;
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let patch = app
        .patch_json(&format!("/api/transactions/{tx_id}/note"),
            json!({"note": null}),
            &session.token,
        )
        .await;
    assert_eq!(patch.status, 200);

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM transactions WHERE id = ?")
        .bind(tx_id).fetch_one(&app.pool).await.unwrap();
    assert!(note.is_none());
}

#[tokio::test]
async fn patch_note_rejects_voided_409() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("PATCH-VOID", 50.0).await;

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0}),
            &session.token,
        )
        .await;
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let void = app
        .delete(&format!("/api/transactions/{tx_id}"), &session.token)
        .await;
    assert_eq!(void.status, 204);

    let patch = app
        .patch_json(&format!("/api/transactions/{tx_id}/note"),
            json!({"note": "after void"}),
            &session.token,
        )
        .await;
    assert_eq!(patch.status, 409);
}

#[tokio::test]
async fn patch_note_rejects_over_200_chars() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let card_id = session.activate_card("PATCH-LONG", 50.0).await;

    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0}),
            &session.token,
        )
        .await;
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let long = "x".repeat(201);
    let patch = app
        .patch_json(&format!("/api/transactions/{tx_id}/note"),
            json!({"note": long}),
            &session.token,
        )
        .await;
    assert_eq!(patch.status, 400);
}

#[tokio::test]
async fn patch_note_returns_404_when_id_missing() {
    let app = TestApp::new().await;
    let session = StaffSession::login(&app).await;
    let patch = app
        .patch_json("/api/transactions/9999999/note",
            json!({"note": "x"}),
            &session.token,
        )
        .await;
    assert_eq!(patch.status, 404);
}

#[tokio::test]
async fn patch_note_requires_staff_role() {
    let app = TestApp::new().await;
    // Customer login — adapt this helper if it's named differently in helpers/.
    let customer = StaffSession::login_as_role(&app, "customer").await;
    let staff = StaffSession::login(&app).await;
    let card_id = staff.activate_card("PATCH-403", 50.0).await;
    let resp = app
        .post_json("/api/payments/charge",
            json!({"card_id": card_id, "amount": 1.0}),
            &staff.token,
        )
        .await;
    let tx_id = resp.body.get("transaction_id").unwrap().as_i64().unwrap();

    let patch = app
        .patch_json(&format!("/api/transactions/{tx_id}/note"),
            json!({"note": "x"}),
            &customer.token,
        )
        .await;
    assert_eq!(patch.status, 403);
}
```

If `helpers/` doesn't expose `patch_json`, `delete`, or `login_as_role`, add the missing methods to `helpers/mod.rs` (read the existing file first; mimic the pattern of whatever `post_json` looks like).

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/routes/transactions.rs crates/spinbike-server/tests/transactions_note.rs crates/spinbike-server/tests/helpers/
git commit -m "feat(api): PATCH /api/transactions/{id}/note with cap + voided rejection (#26)"
```

(Adjust the `helpers/` path if no helper changes were needed.)

---

## Task 7: Switch UI labels — i18n + EventKind-driven label resolution

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`
- Modify: `spinbike-ui/src/pages/dashboard/transactions_list.rs`
- Modify: `spinbike-ui/src/pages/reports/activity_feed.rs`

This task only swaps labels; note display/edit comes in Tasks 8–10.

- [ ] **Step 1: Add 4 new label keys + note-related keys to i18n.rs**

In `spinbike-ui/src/i18n.rs`, add these inserts in the same `m.insert(...)` block where `tx_action_*` lives (around line 504-509). Replace the three `tx_action_*` lines and add the four new ones:

```rust
    // Transaction labels — issue #26: identical card + report wording.
    // DB stores raw English action ("topup"/"charge"/"visit"/"storno"); UI
    // derives an EventKind via spinbike_core::reports::classify() and looks
    // up one of these four keys.
    m.insert("tx_label_topup",  ("Dobitie kreditu",     "Top-up"));
    m.insert("tx_label_charge", ("Výdaj z kreditu",     "Spent from credit"));
    m.insert("tx_label_visit",  ("Vstup s permanentkou", "Entry with pass"));
    m.insert("tx_label_pass",   ("Predaj permanentky",   "Sale of pass"));

    // Note input + inline-edit affordances (#26).
    m.insert("tx_note_placeholder",  ("Poznámka (nepovinné)", "Note (optional)"));
    m.insert("tx_note_edit",         ("Upraviť poznámku",     "Edit note"));
    m.insert("tx_note_save",         ("Uložiť",               "Save"));
    m.insert("tx_note_cancel",       ("Zrušiť",               "Cancel"));
```

In the same step, REMOVE the now-obsolete keys (search the file):

- `tx_action_topup`, `tx_action_charge`, `tx_action_visit` (lines ~505-507)
- `event_charge`, `event_topup`, `event_pass` (lines ~642-644)

KEEP: `event_other`, `tx_until_short`, `filters_event_topups`, `filters_event_passes` — those are still used.

- [ ] **Step 2: Switch `transactions_list.rs` to use EventKind-based label resolution**

In `spinbike-ui/src/pages/dashboard/transactions_list.rs`, replace the action-key lookup block (lines 55-65):

```rust
            let kind = tx.kind();
            let action_key = match kind {
                spinbike_core::reports::EventKind::PassSale => "tx_label_pass",
                spinbike_core::reports::EventKind::Visit    => "tx_label_visit",
                spinbike_core::reports::EventKind::Charge   => "tx_label_charge",
                spinbike_core::reports::EventKind::TopUp    => "tx_label_topup",
                spinbike_core::reports::EventKind::Other    => "event_other",
            };
            let action = i18n::t(l, action_key).to_string();
```

(`tx.kind()` was added in Task 4. The previous `if action_key.is_empty()` fallback to `tx.action.clone()` is gone — Other has its own label.)

Also: the `else if tx.action == "visit"` row-class branch (line 84) still works as-is, but for consistency replace with the kind enum:

```rust
            let row_class = if is_voided {
                "list-row txn-row--voided"
            } else if matches!(kind, spinbike_core::reports::EventKind::Visit) {
                "list-row txn-row-visit"
            } else {
                "list-row"
            };
```

- [ ] **Step 3: Switch `activity_feed.rs` render_row to use the same key map**

In `spinbike-ui/src/pages/reports/activity_feed.rs`, replace the `event_label_key` block (lines 161-166) with the same mapping (Task 2 left this as a temporary placeholder):

```rust
    let event_label_key = match kind {
        EventKind::PassSale => "tx_label_pass",
        EventKind::Visit    => "tx_label_visit",
        EventKind::Charge   => "tx_label_charge",
        EventKind::TopUp    => "tx_label_topup",
        EventKind::Other    => "event_other",
    };
```

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/i18n.rs spinbike-ui/src/pages/dashboard/transactions_list.rs spinbike-ui/src/pages/reports/activity_feed.rs
git commit -m "feat(ui): unify card+report transaction labels via EventKind (#26)"
```

---

## Task 8: Card-history note display + inline pencil edit

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/transactions_list.rs`
- Modify: `spinbike-ui/src/styles.scss` (or whichever stylesheet has `.list-row__sub` — grep first)
- Modify: `spinbike-ui/src/api.rs` (if no `patch_json` exists yet — read first; add a small helper if missing)

- [ ] **Step 1: Confirm or add `api::patch_json` helper**

Read `spinbike-ui/src/api.rs`. If it has `pub async fn patch<Req, Res>(...)` or a JSON-PATCH helper, use it as-is. If only `post`/`get`/`delete_empty` exist, add:

```rust
pub async fn patch_json<Req: serde::Serialize, Res: serde::de::DeserializeOwned>(
    url: &str,
    body: &Req,
) -> Result<Res, String> {
    // mirror existing post() — same JSON content-type, same auth header propagation,
    // same status-class handling. Read the existing post() body for the exact code.
    // ...
}
```

If the project already has a generic `request_json` driving `post`, reuse it with `Method::PATCH`.

- [ ] **Step 2: Replace the row rendering in `transactions_list.rs` with note-aware version**

The full new `view!` block per row (replaces lines ~127-139):

```rust
                let kind = tx.kind();
                let action_key = match kind {
                    spinbike_core::reports::EventKind::PassSale => "tx_label_pass",
                    spinbike_core::reports::EventKind::Visit    => "tx_label_visit",
                    spinbike_core::reports::EventKind::Charge   => "tx_label_charge",
                    spinbike_core::reports::EventKind::TopUp    => "tx_label_topup",
                    spinbike_core::reports::EventKind::Other    => "event_other",
                };
                let action = i18n::t(l, action_key).to_string();
                let note_initial = tx.note.clone().unwrap_or_default();
                // Per-row signal so the editor opens independently for each row.
                let (editing, set_editing) = signal(false);
                let (note_value, set_note_value) = signal(note_initial.clone());

                let on_edit = move |_| set_editing.set(true);
                let on_cancel = move |_| {
                    set_note_value.set(note_initial.clone());
                    set_editing.set(false);
                };
                let on_save = move |_| {
                    let new_note = note_value.get_untracked();
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { note: Option<String> }
                        #[derive(serde::Deserialize)]
                        struct Resp { #[allow(dead_code)] id: i64, #[allow(dead_code)] note: Option<String> }
                        let body = Req {
                            note: if new_note.trim().is_empty() { None } else { Some(new_note) },
                        };
                        match api::patch_json::<Req, Resp>(
                            &format!("/api/transactions/{tx_id}/note"), &body
                        ).await {
                            Ok(_) => {
                                set_editing.set(false);
                                txn_refresh.update(|n| *n += 1);
                            }
                            Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
                        }
                    });
                };

                let on_input = move |ev: web_sys::Event| {
                    let v = ev.target()
                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                        .map(|el| el.value())
                        .unwrap_or_default();
                    set_note_value.set(v);
                };

                view! {
                    <div class=row_class>
                        <div class="list-row__main">
                            <div class="list-row__title">
                                {action}{until_suffix}
                                {voided_tag}
                            </div>
                            <div class="list-row__sub">{date}" · "{service}</div>
                            {move || if editing.get() {
                                view! {
                                    <div class="list-row__note-edit">
                                        <input
                                            type="text"
                                            maxlength="200"
                                            class="form-control form-control--inline"
                                            data-testid="txn-note-edit-input"
                                            prop:value=move || note_value.get()
                                            on:input=on_input
                                        />
                                        <button class="btn btn--compact btn--primary"
                                                data-testid="txn-note-save"
                                                on:click=on_save>
                                            {move || i18n::t(lang.get(), "tx_note_save")}
                                        </button>
                                        <button class="btn btn--compact btn--ghost"
                                                data-testid="txn-note-cancel"
                                                on:click=on_cancel>
                                            {move || i18n::t(lang.get(), "tx_note_cancel")}
                                        </button>
                                    </div>
                                }.into_any()
                            } else if !note_value.get().is_empty() {
                                view! {
                                    <div class="list-row__note" data-testid="txn-note-text">
                                        {move || note_value.get()}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                        </div>
                        <div class=amount_class>{amount_str}</div>
                        {if !is_voided {
                            view! {
                                <div class="list-row__end list-row__end--column">
                                    <button
                                        class="btn btn--compact btn--ghost"
                                        data-testid="txn-note-edit"
                                        title=move || i18n::t(lang.get(), "tx_note_edit")
                                        on:click=on_edit
                                    >"\u{270e}"</button>
                                    <button
                                        class="btn btn--compact btn--ghost"
                                        data-testid="txn-void"
                                        title=move || i18n::t(lang.get(), "void")
                                        on:click=on_void
                                    >"\u{2715}"</button>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div></div> }.into_any()
                        }}
                    </div>
                }
```

(Note the `wasm_bindgen::JsCast` import is needed for `.dyn_into::<web_sys::HtmlInputElement>()` — add `use wasm_bindgen::JsCast;` to the top of the file if not present, plus `use serde;` is already implicit. The existing void-button block is replaced by a column container that holds both pencil and X buttons; on voided rows, both buttons disappear.)

The `void_btn` variable from the old code is now inlined into the row view (since it shares a column with the edit button). Remove the old `let void_btn = ...` block. Read the file before editing to avoid conflicts with the existing `on_void` closure logic — preserve the confirm dialog and PATCH→refresh pattern.

- [ ] **Step 3: Add CSS for `.list-row__note`, `.list-row__note-edit`, `.list-row__end--column`**

In the same SCSS file as `.list-row__sub` (grep `list-row__sub` in `spinbike-ui/src/`):

```scss
.list-row__note {
  margin-top: 2px;
  font-size: 0.85em;
  color: var(--text-muted, #888);
  font-style: italic;
}

.list-row__note-edit {
  display: flex;
  gap: 4px;
  margin-top: 4px;
  align-items: center;

  .form-control--inline {
    flex: 1;
    min-width: 0;
  }
}

.list-row__end--column {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
```

(If existing button-stack patterns in the file use different layout primitives — flex columns vs grids — mirror them.)

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/transactions_list.rs spinbike-ui/src/styles.scss spinbike-ui/src/api.rs
git commit -m "feat(ui): note display + inline pencil edit on card history (#26)"
```

(Drop `api.rs` from the `git add` if no api helper change was needed.)

---

## Task 9: Report-feed read-only note display

**Files:**
- Modify: `spinbike-ui/src/pages/reports/activity_feed.rs`

- [ ] **Step 1: Render note inline below the existing subtitle**

In `render_row` (around lines 219-231, the final `view! { ... }` block), modify the `<div class="list-row__sub">{subtitle}</div>` line to also render the note when present:

```rust
    let note_str = e.note.clone().unwrap_or_default();
    // ... (existing code through `voided_badge`) ...

    view! {
        <div class="list-row list-row--interactive" data-testid="feed-row"
             on:click=on_row_click>
            <div class=kind_class></div>
            <div class="list-row__sub" style="min-width: 48px;">{time_only}</div>
            <div class="list-row__main">
                <div class="list-row__title">{name}</div>
                <div class="list-row__sub">{subtitle}</div>
                {if !note_str.is_empty() {
                    view! {
                        <div class="list-row__note" data-testid="feed-note">
                            {note_str}
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </div>
            <div class=amount_class>{amount_display}</div>
            {voided_badge}
        </div>
    }
```

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/pages/reports/activity_feed.rs
git commit -m "feat(ui): read-only note display on report activity feed (#26)"
```

---

## Task 10: Action-form note input on all 4 create flows

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs`

- [ ] **Step 1: Add a shared `note_ref` + `note_for_request()` helper**

In `crates/.../action_form.rs::ActionForm`, after the existing `let amount_ref = NodeRef::<leptos::html::Input>::new();` (line 27):

```rust
    let note_ref = NodeRef::<leptos::html::Input>::new();

    // Read the current note value, normalised (None if empty/whitespace).
    let read_note = move || -> Option<String> {
        note_ref.get().and_then(|el| {
            let el: &web_sys::HtmlInputElement = &el;
            let v = el.value();
            if v.trim().is_empty() { None } else { Some(v) }
        })
    };
    let clear_note = move || {
        if let Some(el) = note_ref.get() {
            let el: &web_sys::HtmlInputElement = &el;
            el.set_value("");
        }
    };
```

- [ ] **Step 2: Plumb note into the four request bodies**

In `do_topup` (line 74), update the inline `Req` struct:

```rust
        #[derive(serde::Serialize)]
        struct Req {
            card_id: i64,
            amount: f64,
            note: Option<String>,
        }
        let note = read_note();
        match api::post::<Req, CardInfo>(
            "/api/cards/topup",
            &Req { card_id, amount, note },
        ).await { /* … */ }
```

After the `Ok(c)` arm: call `clear_note();` so the input doesn't carry stale text into the next transaction.

In `do_charge` (line 110), apply the same pattern to BOTH branches:

- The `is_monthly_pass()` branch (sell-pass): add `note: Option<String>` to the inline `Req` struct, set `note: read_note()` on construction, `clear_note()` on Ok.
- The non-pass branch (charge): same.

In `visit_click_for` (line 215), update:

```rust
        #[derive(serde::Serialize)]
        struct Req {
            card_id: i64,
            service_id: i64,
            note: Option<String>,
        }
        // …
        match api::post::<Req, Resp>(
            "/api/payments/log-visit",
            &Req { card_id, service_id, note: read_note() },
        ).await { /* … */ }
```

`clear_note()` on Ok.

- [ ] **Step 3: Add the input element to the form view**

Insert this `<div class="form-group">` immediately after the existing amount input block (after line 322, before the `is_monthly_pass()` valid_until conditional):

```rust
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "tx_note_edit")}</label>
                <input
                    type="text"
                    maxlength="200"
                    class="form-control"
                    node_ref=note_ref
                    data-testid="txn-note-input"
                    placeholder=move || i18n::t(lang.get(), "tx_note_placeholder")
                />
            </div>
```

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): note input on charge / topup / sell-pass / visit (#26)"
```

---

## Task 11: E2E spec — txn-note.spec.ts (6 cases)

**Files:**
- Create: `e2e/tests/txn-note.spec.ts`

- [ ] **Step 1: Read existing helpers + a sample spec to match style**

```bash
cat e2e/tests/helpers.ts
cat e2e/tests/log-visit-class-only.spec.ts | head -30
```

The new spec must use `setupConsoleCheck`, `assertCleanConsole`, `loginViaAPI`, plus the same per-test card activation pattern.

- [ ] **Step 2: Write the spec file**

```typescript
import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `NOTE-${suffix}`;
    const lastName = `Note${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'NT', last_name: lastName }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    return { barcode, lastName };
}

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

async function chargeWithNote(page: Page, amount: string, note: string) {
    const refreshOption = page
        .locator('[data-testid="charge-service"] option')
        .filter({ hasText: /Refreshments|Občerstvenie/ })
        .first();
    await expect(refreshOption).toBeAttached();
    const value = await refreshOption.getAttribute('value');
    if (!value) throw new Error('Refreshments option had no value');
    await page.locator('[data-testid="charge-service"]').selectOption(value);
    await page.locator('[data-testid="charge-amount"]').fill(amount);
    if (note.length > 0) {
        await page.locator('[data-testid="txn-note-input"]').fill(note);
    }
    const chargeResp = page.waitForResponse(
        (r) => r.url().includes('/api/payments/charge') && r.request().method() === 'POST',
    );
    await page.locator('[data-testid="charge-submit"]').click();
    const resp = await chargeResp;
    expect(resp.ok()).toBe(true);
}

test.describe('Transaction notes — issue #26', () => {

    test('charge with note shows note inline on card history', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '2.50', 'Proteinová tyčinka');

        const noteRow = page.locator('[data-testid="txn-note-text"]').first();
        await expect(noteRow).toBeVisible();
        await expect(noteRow).toContainText('Proteinová tyčinka');

        assertCleanConsole(msgs);
    });

    test('note appears on report activity feed', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        const noteText = `feed-${Date.now()}`;
        await chargeWithNote(page, '1.00', noteText);

        const today = new Date().toISOString().slice(0, 10);
        await page.goto(`/reports?date=${today}`);
        const feedNote = page
            .locator('[data-testid="feed-row"]')
            .filter({ has: page.locator('[data-testid="feed-note"]', { hasText: noteText }) })
            .first();
        await expect(feedNote).toBeVisible();

        assertCleanConsole(msgs);
    });

    test('inline pencil edits an existing note', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', 'old note');

        // Edit the note on the most recent row.
        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        await firstRow.locator('[data-testid="txn-note-edit"]').click();
        const editInput = firstRow.locator('[data-testid="txn-note-edit-input"]');
        await expect(editInput).toBeVisible();
        await editInput.fill('new note');
        const patchResp = page.waitForResponse(
            (r) => r.url().match(/\/api\/transactions\/\d+\/note/) !== null && r.request().method() === 'PATCH',
        );
        await firstRow.locator('[data-testid="txn-note-save"]').click();
        const resp = await patchResp;
        expect(resp.ok()).toBe(true);

        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toContainText('new note');
        assertCleanConsole(msgs);
    });

    test('clearing a note removes the note line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', 'temporary');

        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        await firstRow.locator('[data-testid="txn-note-edit"]').click();
        await firstRow.locator('[data-testid="txn-note-edit-input"]').fill('');
        const patchResp = page.waitForResponse(
            (r) => r.url().match(/\/api\/transactions\/\d+\/note/) !== null && r.request().method() === 'PATCH',
        );
        await firstRow.locator('[data-testid="txn-note-save"]').click();
        await patchResp;

        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toHaveCount(0);
        assertCleanConsole(msgs);
    });

    test('charge without a note renders no note line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', '');

        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toHaveCount(0);
        // Pencil is still visible (lets staff add a note later).
        await expect(firstRow.locator('[data-testid="txn-note-edit"]')).toBeVisible();
        assertCleanConsole(msgs);
    });

    test('voided transaction hides the pencil but keeps the note text visible', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', 'doomed');

        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        page.once('dialog', (d) => d.accept());
        await firstRow.locator('[data-testid="txn-void"]').click();

        // After void: note text remains, pencil and X disappear.
        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toContainText('doomed');
        await expect(firstRow.locator('[data-testid="txn-note-edit"]')).toHaveCount(0);
        await expect(firstRow.locator('[data-testid="txn-void"]')).toHaveCount(0);
        assertCleanConsole(msgs);
    });
});
```

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/txn-note.spec.ts
git commit -m "test(e2e): per-transaction note flows (#26)"
```

---

## Task 12: Validate migration on dev DB → push → monitor CI → open PR

**Files:** none modified

- [ ] **Step 1: Validate migration on the prod-synced dev DB**

Per project memory `feedback_validate_against_real_data.md`, dry-run the migration's DDL against a copy of the prod database. The migration is a single `ALTER TABLE ADD COLUMN` (non-destructive, idempotent-able-via-fresh-copy) — apply it with `sqlite3` directly so we don't depend on a cargo build:

```bash
# Prod and dev share the same machine — copy the live DB to a throwaway path.
cp /var/lib/spinbike/spinbike.db /tmp/dev-validate.db

# Snapshot pre-state.
sqlite3 /tmp/dev-validate.db "PRAGMA table_info(transactions);" | grep -c note
# Expected: 0  (no note column yet)
sqlite3 /tmp/dev-validate.db "SELECT COUNT(*) FROM transactions;"
# Note this number — call it N.

# Apply v10 SQL exactly as the migration constant has it.
sqlite3 /tmp/dev-validate.db "ALTER TABLE transactions ADD COLUMN note TEXT;"

# Verify post-state.
sqlite3 /tmp/dev-validate.db "PRAGMA table_info(transactions);" | grep note
# Expected: a row "<n>|note|TEXT|0||0"
sqlite3 /tmp/dev-validate.db "SELECT COUNT(*) FROM transactions WHERE note IS NULL;"
# Expected: N (every pre-existing row has note=NULL)
sqlite3 /tmp/dev-validate.db "SELECT COUNT(*) FROM transactions;"
# Expected: N (no rows lost or duplicated)

# Cleanup.
rm /tmp/dev-validate.db
```

If the column count check shows it already has note (return 1, not 0), the file isn't actually the prod DB — re-copy. If the post-state row count differs from N, STOP and investigate before pushing — that's a destructive migration and would never reach prod.

- [ ] **Step 2: Pre-push local check**

```bash
cargo fmt --all --check
```

If anything is unformatted: run `cargo fmt --all`, stage with `git add -u`, amend… NO — never amend. Make a fixup commit:

```bash
git add -u
git commit -m "chore: cargo fmt"
```

- [ ] **Step 3: Push and identify the run**

```bash
git push origin dev
sleep 5
gh run list --branch dev --limit 3 --json databaseId,status,headSha,event
```

Identify the latest run id (`databaseId`) for the `push` event on the new HEAD sha.

- [ ] **Step 4: Monitor CI to terminal state (single background command)**

Per `~/devel/airuleset/modules/core/ci-monitoring.md`, ONE background command, no loops, no scheduled polling:

```bash
sleep 600 && gh run view <run-id> --json status,conclusion,jobs
# Run with run_in_background: true
```

When the result comes back:

- All jobs `success` → proceed to PR (Step 6).
- Lint, test, integrity, e2e, mutation, build-wasm, deploy(prod) all green; deploy(dev) green; smoke(dev) and/or smoke(prod) green for whichever side was deployed → ALL green → proceed.
- ANY failure → `gh run view <run-id> --log-failed | head -200`, fix the root cause, push again, re-monitor. Never `--rerun-failed` blindly.
- Specifically for the SQLITE_BUSY E2E flake (#24): if a SINGLE test fails on `(code: 5) database is locked` AND the unit/integration tests are clean, ONE rerun is acceptable per `ci-monitoring.md`. Two reruns of the same flake is real — fix it, don't rerun.

- [ ] **Step 5: Wait for the PR-event run too (if it appears)**

GitHub Actions fires both `push` and `pull_request` runs once a PR exists. Until the PR is open, only the `push` run exists. After step 6 below, monitor the PR run's terminal state with the same `sleep N && gh run view` pattern.

- [ ] **Step 6: Open the PR**

```bash
gh pr create --base main --head dev --title "feat: card history clarity + per-transaction notes (#26, v0.13.9)" --body "$(cat <<'EOF'
## Summary

Closes #26.

- Unified Slovak labels across card history and report activity feed:
  - Top-up → `Dobitie kreditu`
  - Charge → `Výdaj z kreditu`
  - Free pass-visit → `Vstup s permanentkou`
  - Pass sale → `Predaj permanentky`
- Fixes a long-standing bug where €0 pass-visits showed as `Iné` on the report (now classify by `action='visit'` first).
- Adds `note TEXT` column on `transactions` (migration v10), wired into all four create endpoints and a new `PATCH /api/transactions/{id}/note` for inline edits.
- Card history: free-text note appears below the date·service line; pencil icon opens an inline editor; voided rows lock the note.
- Report activity feed: read-only note display.

## Test plan

- [ ] Migration v10 applied on prod-synced dev DB without errors; existing rows have `note=NULL`.
- [ ] `cargo test -p spinbike-core -p spinbike-server` green on CI.
- [ ] New integration tests `transactions_note.rs` cover: create-with-note (4 endpoints), PATCH (200/400/409/404/403), empty/whitespace-as-NULL, missing-note-field-still-works.
- [ ] New Playwright spec `txn-note.spec.ts` covers: create+display, on-report, inline-edit, clear, no-note no-render, voided-locks-pencil.
- [ ] Browser console clean across all E2E tests.
- [ ] Mutation testing green.
- [ ] Deploy → post-deploy verification reads `v0.13.9` from dev frontend `[aria-label="Application version"]` and from `/api/version`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Capture the PR URL from the `gh pr create` output for the completion report.

- [ ] **Step 7: Confirm PR mergeability**

```bash
gh pr view <pr-number> --json mergeable,mergeStateStatus
```

Required: `mergeable: MERGEABLE` AND `mergeStateStatus: CLEAN`. UNSTABLE / BLOCKED / BEHIND = NOT done; investigate and fix.

End state for this task: PR is open, CI is green, mergeable status is CLEAN. Per `pr-merge-policy.md`, do NOT merge — wait for the user's explicit "merge it".

---

## Task 13: Post-deploy verification (only AFTER user merges)

**Files:** none

This task only runs AFTER the user merges the PR. It verifies that v0.13.9 is live on prod and that the note feature works end-to-end against the real deployment.

- [ ] **Step 1: Wait for main CI to deploy v0.13.9 to prod**

After merge to main, GitHub fires the main-branch CI run including `deploy(prod)` and `smoke(prod)`. Monitor with the same single-`sleep` pattern.

- [ ] **Step 2: Read the version label from the live prod DOM (Playwright)**

```javascript
// Open prod URL in Playwright, read the version label.
// Use browser_navigate + browser_snapshot from the playwright MCP server.
//   URL: https://spinbike.newlevel.media
//   Selector: [aria-label="Application version"]
//   Expected text: "v0.13.9"
// Cross-check with curl https://spinbike.newlevel.media/api/version
//   Expected JSON: {"version":"0.13.9"}
```

If they don't match → deploy stale → investigate (CDN cache, build skip, wrong target).

- [ ] **Step 3: Functional verification on prod**

In Playwright (against the prod URL):

1. Login as staff.
2. Pick a real test card (existing test card barcode known to staff — coordinate with user if needed).
3. Charge €0.50 with note `verify-#26-<timestamp>`.
4. Open the card history; assert the note line is visible.
5. Open `/reports?date=<today>`; assert the note line is visible there too.
6. Click the pencil; edit the note; save.
7. Reload the card; assert the new note text.
8. Void the test transaction (cleanup); assert pencil disappears, note text remains.
9. Browser console must be clean.

- [ ] **Step 4: Send completion report**

Per `~/devel/airuleset/modules/core/completion-report.md` — full template, audit lines (`✅ CI` / `✅ /plan-check` / `✅ /review` / `✅ Deploy: prod frontend shows v0.13.9 ...`), Goal + What changed in plain language, dev + prod URLs, PR URL, optional `❓ Question`. No "Future" / "Remaining" sections.

End state: PR merged, prod verified, completion report sent.

---

## Self-review

**Spec coverage:** Each acceptance criterion in the spec maps to a task —

1. Identical labels card↔report → Tasks 7+8+9.
2. €0 visit reads `Vstup s permanentkou` on both surfaces → Tasks 2+7.
3. Staff types note on all 4 create flows → Tasks 5+10.
4. Inline pencil edit on non-voided card row → Task 8.
5. >200 chars rejected server + client → Tasks 5+6 (server), Task 8/10 (client `maxlength`).
6. Voided transactions reject edits → Task 6 (409) + Task 8 (pencil hidden).
7. Migration v10 clean on prod-synced DB and CI test DB → Tasks 3 + 12.
8. CI green incl. mutation + deploy → Task 12.

**Type consistency:**

- `EventKind::PassSold` renamed to `PassSale` everywhere it appears (core enum, core tests, activity_feed.rs).
- `classify(action, amount, valid_until)` is the single source of truth, called from `ReportEvent::kind()` and `TxnInfo::kind()`.
- `note: Option<String>` field name used identically across `TransactionRow`, `DbEventRow`, `ReportEvent`, `TransactionResponse`, `TxnInfo`, all four request structs, and the database column.
- Endpoint paths used in tests match the routes registered in `routes()` blocks.

**Placeholder scan:** No "TBD"/"TODO"/"implement later" in the plan. The Task 2 Step 5 placeholder for activity_feed (`Visit → "event_charge"` temporarily) is explicitly fixed in Task 7 Step 3 — flagged in-line, not deferred indefinitely.
