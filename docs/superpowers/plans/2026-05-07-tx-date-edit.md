# Edit transaction date Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let staff backdate a non-voided transaction's `created_at` to a date within the last 30 days via a row pencil + bottom sheet, with a backend `PATCH /api/transactions/{id}/created-at` endpoint that preserves the time-of-day component.

**Architecture:** New backend handler `patch_created_at` in `crates/spinbike-server/src/routes/transactions.rs` (mirrors `patch_note` / `patch_valid_until`), validates 30-day window, splits the existing `created_at` on `' '` to keep the time portion, overwrites the date portion. New Leptos `EditTxDateSheet` mirrors `EditPassDateSheet`; per-row 📅 pencil added to `transactions_list.rs` next to existing ✎ / ✕ icons (hidden on voided rows). Three new Slovak/English i18n keys; one new Playwright E2E. No DB migration. No audit trail (per spec).

**Tech Stack:** Axum 0.8 + sqlx (SQLite), Leptos 0.7 CSR/WASM, chrono::NaiveDate. Playwright + TypeScript for E2E.

**Spec:** `docs/superpowers/specs/2026-05-07-tx-date-edit-design.md` (commit `4c47085`).
**Issue:** [#76](https://github.com/zbynekdrlik/spinbike/issues/76).

---

## File structure

| File | New / Modify | Responsibility |
|---|---|---|
| `VERSION` + `Cargo.toml` + `spinbike-ui/Cargo.toml` | Modify | Bump 0.13.26 → 0.13.27 (via `scripts/sync-version.sh`). |
| `spinbike-ui/src/i18n.rs` | Modify | Add 3 new keys. |
| `crates/spinbike-server/src/routes/transactions.rs` | Modify | Add `patch_created_at` handler + register route; extend `TxMini` with `created_at`. |
| `crates/spinbike-server/tests/transactions_date.rs` | Create | 6 integration tests for the new endpoint. |
| `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs` | Create | New `EditTxDateSheet` component (mirrors `EditPassDateSheet`). |
| `spinbike-ui/src/pages/dashboard/sheets/mod.rs` | Modify | Export `EditTxDateSheet`. |
| `spinbike-ui/src/pages/dashboard/transactions_list.rs` | Modify | Add per-row date-edit signal, 📅 button, sheet open/close, refresh wiring. |
| `e2e/tests/edit-tx-date.spec.ts` | Create | One Playwright test covering the full pencil → sheet → PATCH → render flow. |

---

## Project guard rails (apply to every task)

- Working directory: `/home/newlevel/devel/spinbike`. Branch: `dev`.
- **No local cargo build / test / clippy / trunk build.** Only `cargo fmt --all --check` is allowed locally. CI is authoritative.
- **Never use `git add -A` or `git add .`.** Stage explicit paths or `git add -u` for tracked-file changes.
- **Never** add `wasm_bindgen_test_configure!(run_in_browser);` — silently skips tests under `wasm-pack test --node`.
- Commit messages use Conventional Commits and end with the trailer:

  ```
  Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
  ```

- One commit per task.
- Slovak strings are unaccented (`Zmenit`, `datum`, `zaznamu`, `dnoch` — no diacritics).

---

## Task 1: Bump VERSION 0.13.26 → 0.13.27 (CONTROLLER-RUN)

**Files:**
- Modify: `VERSION`, `Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Confirm current versions match (dev should equal main right now)**

```bash
git fetch origin
echo "VERSION: $(cat VERSION)"
echo "main VERSION: $(git show origin/main:VERSION)"
```

Expected: both report `0.13.26`.

- [ ] **Step 2: Edit VERSION file**

Replace the contents of `VERSION` with exactly:

```
0.13.27
```

- [ ] **Step 3: Sync version into Cargo manifests**

```bash
bash scripts/sync-version.sh
```

This script reads `VERSION` and writes the same value into the workspace `Cargo.toml` and `spinbike-ui/Cargo.toml`.

- [ ] **Step 4: Commit**

```bash
git add VERSION Cargo.toml spinbike-ui/Cargo.toml
git commit -m "$(cat <<'EOF'
chore: bump version to 0.13.27 for #76 (edit transaction date)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: i18n keys (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/src/i18n.rs:474` (existing `tx_note_edit` block region) and `:560` (existing `edit_pass_date` line region) — add new keys near related entries.

- [ ] **Step 1: Add the three new keys**

Open `spinbike-ui/src/i18n.rs` and add the following three `m.insert` lines anywhere in the i18n table block (near other `tx_*` / `edit_*` entries — e.g. just after `m.insert("edit_pass_date", ...)` at line 560 is the cleanest spot):

```rust
m.insert("edit_tx_date", ("Zmenit datum zaznamu", "Change entry date"));
m.insert("tx_date_edit_tooltip", ("Zmenit datum", "Change date"));
m.insert("tx_date_window_error", ("Datum musi byt v poslednych 30 dnoch", "Date must be within last 30 days"));
```

Slovak strings are intentionally unaccented (project convention).

- [ ] **Step 2: Verify no duplicate key**

```bash
grep -nE 'm\.insert\("(edit_tx_date|tx_date_edit_tooltip|tx_date_window_error)"' spinbike-ui/src/i18n.rs
```

Expected: exactly one line per key. If `modal_date` is needed later (it is referenced as a label in the sheet), check first:

```bash
grep -n 'm\.insert\("modal_date"' spinbike-ui/src/i18n.rs
```

If absent, add `m.insert("modal_date", ("Datum", "Date"));` near the other `modal_*` entries (`modal_valid_until` is at line 486). If already present, skip.

- [ ] **Step 3: Format check**

```bash
cargo fmt --all --check
```

Expected: clean (no diff).

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "$(cat <<'EOF'
feat(i18n): add tx-date-edit strings (#76)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Backend handler `patch_created_at` + integration tests (subagent, sonnet)

**Files:**
- Modify: `crates/spinbike-server/src/routes/transactions.rs:14-30` (route registration + `TxMini` struct), and append handler near existing `patch_note` (around line 161).
- Create: `crates/spinbike-server/tests/transactions_date.rs`

### Behavior

- Route: `PATCH /api/transactions/{id}/created-at`
- Body: `{ "created_at_date": "YYYY-MM-DD" }` typed as `chrono::NaiveDate` so serde rejects malformed dates automatically.
- Role gate: `claims.role.can_manage_cards()` (same as `patch_note`).
- Validation order:
  1. Role gate → 403 with body `{"error": "Staff access required"}`.
  2. Look up the row. Missing → 404 `{"error": "Transaction not found"}`.
  3. Row voided (`deleted_at IS NOT NULL`) → 409 `{"error": "Cannot edit date on a voided transaction"}`.
  4. New date out of `[today − 30 days, today]` (inclusive both ends) → 400 `{"error": "Date must be within last 30 days"}`.
- Update: split existing `created_at` on the first `' '`. Build `format!("{} {}", new_date, time_part)`; if there is no space (paranoia path), use `format!("{} 12:00:00", new_date)`. Bind into `UPDATE transactions SET created_at = ? WHERE id = ?`.
- Response: `200 OK` with `{ "id": <i64>, "created_at_date": <NaiveDate> }`.

- [ ] **Step 1: Extend `TxMini` to carry `created_at`**

Edit `crates/spinbike-server/src/routes/transactions.rs:24-30`:

```rust
#[derive(sqlx::FromRow)]
struct TxMini {
    amount: f64,
    user_id: Option<i64>,
    deleted_at: Option<String>,
    valid_until: Option<String>,
    created_at: String,
}
```

Then in the existing two SELECTs that build `TxMini` (`void_transaction` at line 70, `patch_valid_until` at line 128, `patch_note` at line 187), add `, created_at` to the column list. Each becomes:

```rust
"SELECT amount, user_id, deleted_at, valid_until, created_at FROM transactions WHERE id = ?"
```

(The order inside the column list doesn't matter for `FromRow` derivation, but keep the existing field order on the type definition consistent.)

- [ ] **Step 2: Add request/response types and route registration**

Append the following types near the existing `PatchNoteResp` (around line 50), and add the route in the `routes()` function (line 14):

```rust
#[derive(Deserialize)]
struct PatchCreatedAtReq {
    created_at_date: chrono::NaiveDate,
}

#[derive(serde::Serialize)]
struct PatchCreatedAtResp {
    id: i64,
    created_at_date: chrono::NaiveDate,
}
```

In `routes()`:

```rust
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/transactions/{id}", delete(void_transaction))
        .route(
            "/api/transactions/{id}/valid-until",
            patch(patch_valid_until),
        )
        .route("/api/transactions/{id}/note", patch(patch_note))
        .route(
            "/api/transactions/{id}/created-at",
            patch(patch_created_at),
        )
}
```

- [ ] **Step 3: Add the handler**

Append at the end of `crates/spinbike-server/src/routes/transactions.rs` (after `patch_note`):

```rust
async fn patch_created_at(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchCreatedAtReq>,
) -> Result<Json<PatchCreatedAtResp>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, user_id, deleted_at, valid_until, created_at FROM transactions WHERE id = ?",
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
            Json(serde_json::json!({"error": "Cannot edit date on a voided transaction"})),
        ));
    }

    // 30-day window check (inclusive). Future dates are also rejected — same
    // single error message covers both branches per spec.
    let today = chrono::Local::now().date_naive();
    let earliest = today - chrono::Duration::days(30);
    if body.created_at_date < earliest || body.created_at_date > today {
        return Err(super::bad_request("Date must be within last 30 days"));
    }

    // Preserve the existing time-of-day. SQLite's default is "YYYY-MM-DD HH:MM:SS".
    // Paranoia: if there's no space, fall back to noon.
    let time_part = row
        .created_at
        .split_once(' ')
        .map(|(_, t)| t.to_string())
        .unwrap_or_else(|| "12:00:00".to_string());
    let new_value = format!("{} {}", body.created_at_date, time_part);

    sqlx::query("UPDATE transactions SET created_at = ? WHERE id = ?")
        .bind(&new_value)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(PatchCreatedAtResp {
        id,
        created_at_date: body.created_at_date,
    }))
}
```

- [ ] **Step 4: Create the integration test file**

Create `crates/spinbike-server/tests/transactions_date.rs` with the following content (mirrors `transactions_note.rs` helper usage):

```rust
//! Integration tests for #76 — PATCH /api/transactions/{id}/created-at.
//! Covers happy path, time-portion preservation, 30-day window enforcement,
//! 404, 409 (voided), and 403 (non-staff).

mod helpers;

use helpers::{TestApp, delete, patch_json, post_json};
use serde_json::json;

async fn seed_charge(app: &TestApp, code: &str) -> i64 {
    let card_id = app.seed_card(code, 50.0, None, None, None, None).await;
    let spinning_id = app.spinning_service_id().await;
    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"user_id": card_id, "amount": 1.0, "service_id": spinning_id}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    resp.get("transaction_id").unwrap().as_i64().unwrap()
}

#[tokio::test]
async fn patch_created_at_happy_path_preserves_time() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-OK").await;

    // Fetch the original time portion so we can assert it survived.
    let original: String = sqlx::query_scalar("SELECT created_at FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let original_time = original.split_once(' ').unwrap().1.to_string();

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(3);
    let target_str = target.format("%Y-%m-%d").to_string();

    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target_str}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        resp.get("created_at_date").unwrap().as_str(),
        Some(target_str.as_str())
    );

    let stored: String = sqlx::query_scalar("SELECT created_at FROM transactions WHERE id = ?")
        .bind(tx_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let (date_part, time_part) = stored.split_once(' ').unwrap();
    assert_eq!(date_part, target_str);
    assert_eq!(
        time_part, original_time,
        "time portion of created_at must be preserved across edit"
    );
}

#[tokio::test]
async fn patch_created_at_31_days_back_rejected() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-31").await;

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(31);
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("30 days"),
        "error message must mention the 30-day window"
    );
}

#[tokio::test]
async fn patch_created_at_future_date_rejected() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-FUT").await;

    let target = chrono::Local::now().date_naive() + chrono::Duration::days(1);
    let (status, resp) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        resp.get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("30 days"),
        "error message must mention the 30-day window"
    );
}

#[tokio::test]
async fn patch_created_at_missing_id_returns_404() {
    let app = TestApp::new().await;
    let target = chrono::Local::now().date_naive();
    let (status, _) = app
        .request(patch_json(
            "/api/transactions/9999999/created-at",
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_created_at_voided_returns_409() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-VOID").await;

    // Void the transaction first.
    let (void_status, _) = app
        .request(delete(
            &format!("/api/transactions/{tx_id}"),
            &app.staff_token,
        ))
        .await;
    assert_eq!(void_status, axum::http::StatusCode::NO_CONTENT);

    let target = chrono::Local::now().date_naive() - chrono::Duration::days(1);
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.staff_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn patch_created_at_non_staff_returns_403() {
    let app = TestApp::new().await;
    let tx_id = seed_charge(&app, "DATE-403").await;

    let target = chrono::Local::now().date_naive();
    let (status, _) = app
        .request(patch_json(
            &format!("/api/transactions/{tx_id}/created-at"),
            &app.customer_token,
            &json!({"created_at_date": target.format("%Y-%m-%d").to_string()}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
```

- [ ] **Step 5: Format check**

```bash
cargo fmt --all --check
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/routes/transactions.rs \
        crates/spinbike-server/tests/transactions_date.rs
git commit -m "$(cat <<'EOF'
feat(routes): PATCH /api/transactions/{id}/created-at (#76)

Backdate non-voided transactions to a date within the last 30 days.
Preserves the time-of-day so list ordering by created_at stays stable.
Voided rows return 409; out-of-window dates return 400; non-staff 403.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `EditTxDateSheet` component (subagent, sonnet)

**Files:**
- Create: `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs`
- Modify: `spinbike-ui/src/pages/dashboard/sheets/mod.rs`

### Behavior

- Component props:
  - `show: RwSignal<bool>` — visibility toggle (parent controls open/close).
  - `tx_id: i64` — transaction id to PATCH.
  - `current_date: chrono::NaiveDate` — pre-fills the date input.
  - `on_saved: Callback<()>` — called by the sheet after a successful save so the parent can refresh.
- Pre-flight validation in the sheet: if `draft < today − 30d || draft > today`, show the i18n string `tx_date_window_error` as an inline alert and DO NOT send the request.
- On success, set `show.set(false)` then invoke `on_saved.run(())`.

- [ ] **Step 1: Create the sheet file**

Create `spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs`:

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::{DateInput, Sheet};
use crate::i18n::{self, Lang};

#[component]
pub fn EditTxDateSheet(
    /// Whether the sheet is visible.
    show: RwSignal<bool>,
    /// Transaction id to PATCH.
    tx_id: i64,
    /// Current created_at date (pre-fills the date input).
    current_date: chrono::NaiveDate,
    /// Invoked after a successful save so the parent can refresh.
    on_saved: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            // Per-mount form state — each open of the sheet starts fresh from `current_date`.
            let (draft, set_draft) = signal(current_date);
            let (err, set_err) = signal(String::new());
            let (saving, set_saving) = signal(false);

            let on_save = move |_| {
                let new_date = draft.get();
                let today = chrono::Local::now().date_naive();
                let earliest = today - chrono::Duration::days(30);
                if new_date < earliest || new_date > today {
                    set_err.set(i18n::t(lang.get_untracked(), "tx_date_window_error").to_string());
                    return;
                }
                set_err.set(String::new());
                set_saving.set(true);
                spawn_local(async move {
                    #[derive(serde::Serialize)]
                    struct Req {
                        created_at_date: chrono::NaiveDate,
                    }
                    match api::patch::<Req, serde_json::Value>(
                        &format!("/api/transactions/{tx_id}/created-at"),
                        &Req { created_at_date: new_date },
                    )
                    .await
                    {
                        Ok(_) => {
                            show.set(false);
                            on_saved.run(());
                        }
                        Err(e) => set_err.set(e),
                    }
                    set_saving.set(false);
                });
            };

            let on_cancel = move |_| {
                set_err.set(String::new());
                show.set(false);
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| show.set(false))
                    title=i18n::t(lang.get(), "edit_tx_date").to_string()
                    testid="sheet-edit-tx-date"
                >
                    <div class="form-group">
                        <label>{i18n::t(lang.get(), "modal_date")}</label>
                        <DateInput value=draft set_value=set_draft testid="tx-date-input" />
                    </div>
                    {move || {
                        let e = err.get();
                        if e.is_empty() {
                            view! { <div></div> }.into_any()
                        } else {
                            view! { <div class="alert alert-error" data-testid="tx-date-error">{e}</div> }.into_any()
                        }
                    }}
                    <div class="sheet__actions">
                        <button
                            class="btn btn--ghost"
                            disabled=move || saving.get()
                            on:click=on_cancel
                        >
                            {i18n::t(lang.get(), "cancel")}
                        </button>
                        <button
                            class="btn btn--primary"
                            data-testid="tx-date-save"
                            disabled=move || saving.get()
                            on:click=on_save
                        >
                            {i18n::t(lang.get(), "save")}
                        </button>
                    </div>
                </Sheet>
            }.into_any()
        }}
    }
}
```

- [ ] **Step 2: Export from sheets module**

Edit `spinbike-ui/src/pages/dashboard/sheets/mod.rs`. Current contents:

```rust
pub mod edit_pass_date;
pub use edit_pass_date::EditPassDateSheet;
```

Replace with:

```rust
pub mod edit_pass_date;
pub mod edit_tx_date;

pub use edit_pass_date::EditPassDateSheet;
pub use edit_tx_date::EditTxDateSheet;
```

- [ ] **Step 3: Format check**

```bash
cargo fmt --all --check
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/sheets/edit_tx_date.rs \
        spinbike-ui/src/pages/dashboard/sheets/mod.rs
git commit -m "$(cat <<'EOF'
feat(ui): EditTxDateSheet component (#76)

Bottom-sheet for editing a transaction's created_at date. Mirrors
EditPassDateSheet. Inline 30-day window check before PATCH; success
calls on_saved so the parent can refresh the txn list.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Wire 📅 pencil into transaction rows + E2E (subagent, sonnet)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/transactions_list.rs`
- Create: `e2e/tests/edit-tx-date.spec.ts`

### Behavior

- Add a per-row `editing_date: RwSignal<bool>` next to the existing `(editing, set_editing)` for note edits.
- Add a 📅 (`\u{1F4C5}`) `<button class="btn btn--compact btn--ghost">` next to the existing ✎ (note-edit) and ✕ (void) icons, with `data-testid="txn-date-edit"` and `title=i18n::t(lang.get(), "tx_date_edit_tooltip")`.
- The button is **only rendered when `is_voided == false`** (it lives inside the same `if !is_voided { … }` branch that already wraps the ✎/✕ buttons).
- Clicking the button sets `editing_date.set(true)`. The sheet is mounted alongside the row; its `show` is `editing_date`, `tx_id` is `tx.id`, `current_date` is parsed from `tx.created_at` (substring before the first `' '`, parsed as `NaiveDate`).
- `on_saved` callback bumps `txn_refresh` (the list re-fetches and re-renders the row with the new date).

- [ ] **Step 1: Add the import for `EditTxDateSheet`**

Edit `spinbike-ui/src/pages/dashboard/transactions_list.rs:1-8`. Add a single line under the existing `use crate::i18n::{self, Lang};`:

```rust
use crate::pages::dashboard::sheets::EditTxDateSheet;
```

- [ ] **Step 2: Add the per-row date-edit signal**

Inside the `t.iter().map(|tx| { … })` closure, after the existing per-row signal `let (editing, set_editing) = signal(false);` (around line 100), add:

```rust
let editing_date = RwSignal::new(false);
```

Right after the existing `let (note_value, set_note_value) = signal(note_initial.clone());` line is the right place (line 101 + 1).

Also parse the current date from `tx.created_at` into a `NaiveDate` for the sheet's `current_date` prop. Add right after the `editing_date` line:

```rust
let current_date = tx
    .created_at
    .split_once(' ')
    .map(|(d, _)| d)
    .unwrap_or(&tx.created_at);
let current_date = chrono::NaiveDate::parse_from_str(current_date, "%Y-%m-%d")
    .unwrap_or_else(|_| chrono::Local::now().date_naive());
```

- [ ] **Step 3: Add the 📅 button next to ✎ / ✕**

Edit the `if !is_voided { view! { <div class="list-row__end list-row__end--column"> … </div> } } else { … }` block (around lines 199-218). Inside the `<div class="list-row__end list-row__end--column">`, add a new button BETWEEN the existing ✎ and ✕ buttons:

```rust
<button
    class="btn btn--compact btn--ghost"
    data-testid="txn-date-edit"
    title=move || i18n::t(lang.get(), "tx_date_edit_tooltip")
    on:click=move |_| editing_date.set(true)
>"\u{1F4C5}"</button>
```

Final order in that column: ✎ (note-edit) → 📅 (date-edit) → ✕ (void).

- [ ] **Step 4: Mount the sheet inside the row view**

After the existing `<div class=row_class data-testid="transaction-row"> … </div>` closing tag, BEFORE the closing of the `view!` block in the row map, add the sheet mount as a sibling element. Concretely, change the row return from:

```rust
view! {
    <div class=row_class data-testid="transaction-row">
        … existing children …
    </div>
}
```

to:

```rust
let on_saved = Callback::new(move |()| txn_refresh.update(|n| *n += 1));
view! {
    <div class=row_class data-testid="transaction-row">
        … existing children unchanged …
    </div>
    <EditTxDateSheet
        show=editing_date
        tx_id=tx_id
        current_date=current_date
        on_saved=on_saved
    />
}
```

(Leptos accepts a fragment of two siblings inside `view!{}` — the parent `<div class="group">` already wraps the whole list, so an extra sheet sibling is fine. The sheet renders nothing while `editing_date.get() == false`.)

- [ ] **Step 5: Format check**

```bash
cargo fmt --all --check
```

Expected: clean.

- [ ] **Step 6: Create the Playwright E2E**

Create `e2e/tests/edit-tx-date.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    createUniqueUser,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Edit transaction date (#76)', () => {
    test('staff can backdate a charge by 3 days via row pencil', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const { user_id, card_code } = await createUniqueUser(token, 50.0, 'TXD');

        // Look up the Spinning service so we can post a charge.
        const svcResp = await fetch(`${BASE_URL}/api/services`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        if (!svcResp.ok) throw new Error(`services GET failed: ${svcResp.status}`);
        const services = (await svcResp.json()) as Array<{
            id: number;
            kind: string;
            active: boolean;
        }>;
        const spinning = services.find((s) => s.kind === 'spinning' && s.active);
        if (!spinning) throw new Error('No active spinning service found');

        // Create a charge so there is a row in the txn list.
        const chargeResp = await fetch(`${BASE_URL}/api/payments/charge`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                Authorization: `Bearer ${token}`,
            },
            body: JSON.stringify({
                user_id,
                amount: 1.0,
                service_id: spinning.id,
            }),
        });
        if (!chargeResp.ok) throw new Error(`charge POST failed: ${chargeResp.status}`);

        // Open the card via search.
        await page.goto('/staff');
        const search = page.locator('input[type="search"]');
        await search.waitFor();
        await search.focus();
        await page.keyboard.type(card_code, { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // The list should have one row; click the date-edit pencil.
        const list = page.locator('[data-testid="transactions-list"]');
        await expect(list).toBeVisible();
        const row = list.locator('[data-testid="transaction-row"]').first();
        await row.locator('[data-testid="txn-date-edit"]').click();

        // The sheet appears.
        const sheet = page.locator('[data-testid="sheet-edit-tx-date"]');
        await expect(sheet).toBeVisible();

        // Set the date input to today − 3 days.
        const target = new Date();
        target.setDate(target.getDate() - 3);
        const dd = String(target.getDate()).padStart(2, '0');
        const mm = String(target.getMonth() + 1).padStart(2, '0');
        const yyyy = target.getFullYear();
        // English DateInput formats as YYYY-MM-DD; Slovak as DD.MM.YYYY. setEnglishLanguage()
        // is called inside loginViaAPI, so we type the ISO form.
        const isoTarget = `${yyyy}-${mm}-${dd}`;
        const input = page.locator('[data-testid="tx-date-input"]');
        await input.fill(isoTarget);
        await input.blur();

        await page.locator('[data-testid="tx-date-save"]').click();

        // Sheet closes and the row re-renders. Pull the row again and assert
        // its visible date column now reflects the new day.
        await expect(sheet).not.toBeVisible();
        const updatedRow = list.locator('[data-testid="transaction-row"]').first();
        // Date is rendered via i18n::fmt_datetime_str on tx.created_at, which
        // shows the full datetime. Asserting the date portion is enough.
        const ddSk = `${dd}.${mm}.${yyyy}`;
        await expect(updatedRow).toContainText(new RegExp(`${ddSk}|${isoTarget}`));

        assertCleanConsole(msgs);
    });
});
```

- [ ] **Step 7: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/transactions_list.rs \
        e2e/tests/edit-tx-date.spec.ts
git commit -m "$(cat <<'EOF'
feat(ui,e2e): wire EditTxDateSheet into txn list rows (#76)

Adds 📅 pencil between existing ✎ and ✕ icons (hidden on voided
rows). Click opens EditTxDateSheet; on save, txn_refresh bumps and
the row re-renders with the new date. Playwright covers the full
flow (login → seed charge → open card → edit date → assert UI).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Push + monitor CI to terminal state + open PR (CONTROLLER-RUN)

- [ ] **Step 1: Confirm clean working tree**

```bash
git status -s
```

Expected: empty (or `??` only for ignored / IDE files; never `M` / `A` / `D` after the per-task commits).

- [ ] **Step 2: Push dev**

```bash
git push origin dev
```

- [ ] **Step 3: Monitor CI to terminal state**

Identify the latest run on `dev`:

```bash
RUN_ID=$(gh run list --branch dev --limit 1 --json databaseId -q '.[0].databaseId')
echo "RUN_ID=$RUN_ID"
```

Start a single background poll. **No `/loop`. No custom monitor scripts.**

```bash
sleep 300 && gh run view "$RUN_ID" --json status,conclusion,jobs
```

Run that with `run_in_background: true` via the Bash tool. When the poll returns:

- If `status == "completed"` and `conclusion == "success"` for ALL jobs (incl. Deploy (dev) and Smoke (dev)) → proceed to step 4.
- If any job failed → run `gh run view "$RUN_ID" --log-failed` and fix in a single follow-up commit + push, then re-poll.
- If still in progress → repeat the same `sleep 300 && gh run view` background poll.

- [ ] **Step 4: Open the PR (only when CI is green)**

```bash
gh pr create --base main --head dev \
  --title "v0.13.27: edit transaction date (#76)" \
  --body "$(cat <<'EOF'
## Summary

- Adds the ability to backdate a non-voided transaction's `created_at` to a date within the last 30 days
- New `PATCH /api/transactions/{id}/created-at` endpoint preserves time-of-day when changing the date portion
- New `EditTxDateSheet` opens from a 📅 pencil in each transaction row (hidden on voided rows)

## Test plan

- [x] Backend: 6 integration tests in `crates/spinbike-server/tests/transactions_date.rs` (happy path with time preservation, 31-days-back rejected, future date rejected, 404, 409 voided, 403 non-staff)
- [x] Frontend: Playwright E2E in `e2e/tests/edit-tx-date.spec.ts` covers full pencil → sheet → PATCH → re-render flow with a clean console assertion
- [x] Mutation testing on diff via existing CI gate

Closes #76

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Verify PR is mergeable + clean**

```bash
PR_NUMBER=$(gh pr view --json number -q .number)
REPO=$(gh repo view --json owner,name -q '.owner.login + "/" + .name')
gh api "repos/$REPO/pulls/$PR_NUMBER" \
  --jq '{mergeable: .mergeable, mergeable_state: .mergeable_state}'
```

Expected: `mergeable: true` AND `mergeable_state: "clean"`. If `behind`, sync dev with main and push again. If `dirty` / `blocked`, investigate and fix.

- [ ] **Step 6: STOP**

Per `pr-merge-policy.md`: NEVER merge. Report the green PR URL and wait for the user's explicit "merge it" instruction.

---

## Task 7: Post-deploy verification on prod (CONTROLLER-RUN, only after user merges)

This task ONLY runs after the user has explicitly said "merge it" AND the merge has been performed AND main CI (incl. Deploy (prod) + Smoke (prod)) has reached terminal state ✅.

- [ ] **Step 1: Confirm main CI is green**

```bash
MAIN_RUN=$(gh run list --branch main --limit 1 --json databaseId -q '.[0].databaseId')
gh run view "$MAIN_RUN" --json status,conclusion,jobs
```

Expected: `status: "completed"`, `conclusion: "success"` for every job, especially Deploy (prod) and Smoke (prod).

- [ ] **Step 2: Verify version on prod via Playwright**

Use the Playwright MCP tools (NOT curl-only) to:

1. Navigate to `https://spinbike.newlevel.media/staff` (cache-bust query if needed: `?cb=<random>`).
2. Clear service workers + caches if a stale build is shown.
3. Read `[data-testid="version"]` from the DOM.
4. Confirm the value equals `v0.13.27`.

- [ ] **Step 3: Exercise the edit-date pencil flow**

In the same Playwright session:

1. Find any non-voided transaction row in any user history (open a card via search, pick a card with at least one txn).
2. Click `[data-testid="txn-date-edit"]`.
3. Confirm `[data-testid="sheet-edit-tx-date"]` is visible.
4. Set the date input to `today − 1 day`.
5. Click `[data-testid="tx-date-save"]`.
6. Confirm the sheet closes and the row's date column reflects the new day.
7. Capture browser console — must show 0 errors / 0 warnings (after `helpers.ts` filters).

- [ ] **Step 4: Send the completion report**

Per `airuleset/modules/core/completion-report.md`. Include:
- ✅ CI green (main run id)
- ✅ /plan-check fulfilled
- ✅ /review clean
- ✅ Deploy: prod shows `v0.13.27`; pencil flow exercised; row date moved as expected; 0 console errors
- E2E test coverage table row pointing at `e2e/tests/edit-tx-date.spec.ts`
- 🌐 Dev: https://spinbike-dev.newlevel.media
- 🌐 Prod: https://spinbike.newlevel.media
- PR ref with full title

---

## Spec coverage check (controller self-review)

| Spec section | Plan task |
|---|---|
| Goal — backdate within 30 days | Task 3 (handler), Task 4 (sheet), Task 5 (row pencil) |
| User workflow (record today → edit on row) | Task 5 (no entry-time picker; pencil only on existing row) |
| Out of scope items | All preserved (no entry picker, no audit, no future, voided hidden, no bulk) |
| Backend `PATCH /api/transactions/{id}/created-at` | Task 3 |
| Validation 1: out-of-window 400 | Task 3 step 3 + test cases |
| Validation 2: 404 not found | Task 3 step 3 + test |
| Validation 3: 409 voided | Task 3 step 3 + test |
| Time-portion preservation | Task 3 step 3 (split-on-space) + happy-path test asserts time survives |
| Frontend sheet `EditTxDateSheet` | Task 4 |
| Sheet inline window error | Task 4 step 1 (`tx_date_window_error`) |
| Row 📅 affordance, hidden on voided | Task 5 step 3 (inside `if !is_voided` branch) |
| testid contract (`txn-date-edit`, `sheet-edit-tx-date`, `tx-date-input`, `tx-date-save`) | Task 4 + Task 5 |
| i18n keys | Task 2 |
| E2E one-test flow | Task 5 step 6 |
| Backend integration tests (6) | Task 3 step 4 |
| No migration / no audit | No tasks needed; explicitly preserved by overwriting `created_at` directly |
| Version bump first | Task 1 |
