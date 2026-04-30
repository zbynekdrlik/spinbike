# Staff Desk UX Cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bundle four desk-UX issues (#29 Fitness default, #30 Spinning quick-charge chip, #31 reject null-service charge, #32 dashboard layout cleanup) into v0.13.10 in a single PR.

**Architecture:** UI + CSS work in `spinbike-ui/`, plus one defense-in-depth check in `crates/spinbike-server/`. No DB migration. Off-band data fix to `services.default_price` for Spinning, done by the operator via `/admin/services` after deploy.

**Tech Stack:** Leptos 0.7 CSR/WASM (frontend), Axum 0.8 (backend), SQLite via sqlx, Playwright E2E.

**Spec:** `docs/superpowers/specs/2026-04-30-staff-desk-ux-cluster-design.md`

---

## File Structure

| File | Purpose | Tasks |
|---|---|---|
| `VERSION`, `Cargo.toml`, `spinbike-ui/Cargo.toml` | Version metadata, must be 0.13.10 before any non-doc commit | 1 |
| `crates/spinbike-server/src/routes/payments.rs` | Charge handler — add null-service guard | 2 |
| `crates/spinbike-server/tests/payments_charge_validation.rs` (NEW) | Integration tests for the new guard | 2 |
| `spinbike-ui/src/i18n.rs` | Drop unused pass/title keys, add new pass-oneline keys | 5 |
| `spinbike-ui/src/pages/dashboard/action_form.rs` | Default to Fitness, drop empty option, add Spinning quick chip, drop --compact on visit buttons | 3, 4, 9 |
| `spinbike-ui/src/pages/dashboard/mod.rs` | Remove `<h1>` page title | 6 |
| `spinbike-ui/src/pages/dashboard/card_panel.rs` | Card name + barcode on one line | 7 |
| `spinbike-ui/src/pages/dashboard/pass_banner.rs` | Collapse 2-line pass row to 1 line; pencil-icon edit button | 8 |
| `spinbike-ui/style.css` | New CSS for `.card-title*`, `.pass-banner__line`, `.quick-charge-row`, larger visit buttons, darker `--info` shades | 4, 7, 8, 9 |
| `e2e/tests/desk-ux.spec.ts` (NEW) | 8-case Playwright spec | 10 |

---

## Task 1: Verify VERSION bump

The bump from 0.13.9 → 0.13.10 is already at commit `4a99f07` on dev (first commit per `version-bumping.md`). This task is a no-op verification.

**Files:** `VERSION`, `Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Verify VERSION is 0.13.10 and Cargo.toml workspace + ui versions match**

```bash
test "$(cat VERSION)" = "0.13.10" || { echo "VERSION mismatch"; exit 1; }
grep -q '^version = "0.13.10"' Cargo.toml || { echo "Cargo.toml workspace version mismatch"; exit 1; }
grep -q '^version = "0.13.10"' spinbike-ui/Cargo.toml || { echo "spinbike-ui/Cargo.toml mismatch"; exit 1; }
echo "VERSION verified at 0.13.10"
```

Expected: `VERSION verified at 0.13.10`

- [ ] **Step 2: Confirm bump commit exists**

```bash
git log --oneline -5 | grep -q "0.13.10" || { echo "no 0.13.10 bump commit"; exit 1; }
echo "OK"
```

If either check fails, run `echo "0.13.10" > VERSION && bash scripts/sync-version.sh` then commit with explicit paths (NEVER `git add -A` per `feedback_no_git_add_A.md`).

---

## Task 2: Server-side null-service guard (#31 defense-in-depth)

**Files:**
- Modify: `crates/spinbike-server/src/routes/payments.rs:70-117` (charge handler)
- Create: `crates/spinbike-server/tests/payments_charge_validation.rs`

The pattern mirrors the existing note-cap guard at `payments.rs:109-116` and the test file `crates/spinbike-server/tests/transactions_note.rs` (use it as a template for `TestApp` + `post_json` setup).

- [ ] **Step 1: Write the failing test file**

Create `crates/spinbike-server/tests/payments_charge_validation.rs` with this exact content:

```rust
//! Integration tests for #31 — charge endpoint must reject null service_id
//! as defense-in-depth (UI already prevents this, but curl / future endpoints
//! must not slip past). Top-up is unaffected (service-independent).

mod helpers;

use helpers::{TestApp, post_json};
use serde_json::json;

#[tokio::test]
async fn charge_rejects_null_service_id_with_400() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("CHARGE-NULL-SVC", 50.0, None, None, None, None)
        .await;

    let (status, resp) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 1.50}),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let err = resp.get("error").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        err.contains("service_id"),
        "error message must mention service_id, got: {err}"
    );
}

#[tokio::test]
async fn charge_with_valid_service_id_still_succeeds() {
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("CHARGE-VALID-SVC", 50.0, None, None, None, None)
        .await;

    // Find the Fitness service id via /api/services or directly from DB.
    let fitness_id: i64 =
        sqlx::query_scalar("SELECT id FROM services WHERE name_en = 'Fitness'")
            .fetch_one(&app.pool)
            .await
            .unwrap();

    let (status, _) = app
        .request(post_json(
            "/api/payments/charge",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 5.00, "service_id": fitness_id}),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::OK);
}

#[tokio::test]
async fn topup_still_accepts_null_service_id() {
    // Top-up is service-independent — the new charge rule must NOT leak into top-up.
    let app = TestApp::new().await;
    let card_id = app
        .seed_card("TOPUP-NULL-SVC", 0.0, None, None, None, None)
        .await;

    let (status, _) = app
        .request(post_json(
            "/api/cards/topup",
            &app.staff_token,
            &json!({"card_id": card_id, "amount": 30.0}),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::OK);
}
```

- [ ] **Step 2: Implement the guard in `routes/payments.rs`**

In `crates/spinbike-server/src/routes/payments.rs`, locate the `charge` handler at line 70. Insert the new guard immediately after the role check at line 80 (BEFORE the existing monthly-pass check at line 84). The exact edit:

```rust
    if !claims.role.can_process_payments() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // #31: charge requires an explicit service_id (data integrity — untyped
    // charges pollute the activity feed and reports). Top-up stays
    // service-independent. UI also prevents this via removed empty <option>;
    // server enforces it as defense-in-depth.
    let service_id = match body.service_id {
        Some(sid) => sid,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "service_id required for charge"})),
            ));
        }
    };

    // Reject Monthly pass service_id via /charge — it requires valid_until,
    // which /charge doesn't set. Staff must use /sell-pass instead.
    let is_pass: bool =
        sqlx::query_scalar("SELECT kind = 'monthly_pass' FROM services WHERE id = ?")
            .bind(service_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_error)?
            .unwrap_or(false);
    if is_pass {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Use /api/payments/sell-pass for Monthly pass sales (requires valid_until)"
            })),
        ));
    }
```

That replaces the existing block at lines 82-100. The downstream code that referenced `body.service_id` (lines 162, etc.) keeps working because it reads from the request body — it just now sees a non-null value guaranteed by the new guard. The `INSERT` at line 162 will use `body.service_id` (still `Option<i64>`) — that stays as `Some(_)` and serializes fine.

Verify with `cargo fmt --all --check` (the only allowed local check per `feedback_subagent_no_local_build.md`).

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/routes/payments.rs \
        crates/spinbike-server/tests/payments_charge_validation.rs
git commit -m "feat(api): reject charge with null service_id (#31)" \
  -m "Defense-in-depth — UI already prevents this via the removed empty option," \
  -m "but a curl test, future endpoint, or regressed UI could slip past. Mirrors" \
  -m "the note-cap pattern from PR #27. Top-up unchanged (service-independent)."
```

CI's Test job (cargo test, --release) is the authoritative check; we do not run tests locally.

---

## Task 3: Default to Fitness + remove empty option (#29)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs:59` (initial signal value), `:326` (the `<option value="">…</option>` placeholder)

The Fitness service is identified by `spinbike_core::services::FITNESS_NAME_EN = "Fitness"` (already exists at `crates/spinbike-core/src/services.rs:26`).

- [ ] **Step 1: Initialize `selected_service_id` to Fitness's id, derived from the services signal**

In `spinbike-ui/src/pages/dashboard/action_form.rs`, replace line 59:

```rust
    let (selected_service_id, set_selected_service_id) = signal::<Option<i64>>(None);
```

with:

```rust
    // #29: open with Fitness preselected. Falls back to None only if the
    // Fitness service is missing from the seeded list (defensive only —
    // never happens in practice).
    let initial_fitness_id = services
        .get_untracked()
        .iter()
        .find(|s| s.name_en == spinbike_core::services::FITNESS_NAME_EN)
        .map(|s| s.id);
    let (selected_service_id, set_selected_service_id) =
        signal::<Option<i64>>(initial_fitness_id);
```

- [ ] **Step 2: Sync the `<select>` element to the initial value**

The `<select>` at `action_form.rs:320-337` uses `node_ref=service_ref` and re-reads on `change`. The initial DOM render needs to match `selected_service_id` so Fitness shows as picked.

Replace the current `<option value="">{select_service_text}</option>` line at `action_form.rs:326` AND remove the option entirely — the select will start with the Fitness option visually selected via `prop:value`.

Locate:

```rust
                <select
                    class="form-control"
                    node_ref=service_ref
                    on:change=on_service_change
                    data-testid="charge-service"
                >
                    <option value="">{move || i18n::t(lang.get(), "select_service")}</option>
                    {move || {
                        let lang_now = lang.get();
                        services.get().into_iter().map(|s| {
                            let val = s.id.to_string();
                            let kind = s.kind.clone();
                            let label = s.display_name(lang_now).to_string();
                            view! { <option value=val data-kind=kind>{label}</option> }
                        }).collect::<Vec<_>>()
                    }}
                </select>
```

Replace with:

```rust
                <select
                    class="form-control"
                    node_ref=service_ref
                    on:change=on_service_change
                    data-testid="charge-service"
                    prop:value=move || {
                        selected_service_id
                            .get()
                            .map(|id| id.to_string())
                            .unwrap_or_default()
                    }
                >
                    {move || {
                        let lang_now = lang.get();
                        services.get().into_iter().map(|s| {
                            let val = s.id.to_string();
                            let kind = s.kind.clone();
                            let label = s.display_name(lang_now).to_string();
                            view! { <option value=val data-kind=kind>{label}</option> }
                        }).collect::<Vec<_>>()
                    }}
                </select>
```

The `<option value="">` placeholder is gone, and `prop:value` reactively keeps the `<select>` in sync with the signal. When the form opens, Fitness is preselected.

- [ ] **Step 3: Remove the now-unused `select_service` placeholder rendering**

The `i18n::t("select_service")` call inside `<option value="">` was the placeholder — it's gone from the markup. The i18n key itself is still useful as the form's `<label>` text at line 319, so do NOT delete the key. Verify the label still uses it:

```bash
grep -n "select_service" spinbike-ui/src/pages/dashboard/action_form.rs
```

Expected: still present at the `<label>` line (319 area). If `select_service` only appears at the deleted `<option>` line, drop the i18n key in Task 5; otherwise keep it.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): preselect Fitness as default service (#29)" \
  -m "Form opens with Fitness picked, and the empty <option> placeholder is" \
  -m "removed entirely. The combo cannot send service_id=null any more."
```

---

## Task 4: Quick-charge chip row for Spinning (#30)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs` (new chip row above the service combo, around line 318 where the form-group begins)
- Modify: `spinbike-ui/style.css` (add `.quick-charge-row` styles; reuse `.chip-row` if appropriate)
- Modify: `spinbike-ui/src/i18n.rs` (covered in Task 5 if a label key is needed; for now, format inline in Rust)

The Spinning service is identified by `spinbike_core::services::SPINNING_NAME_EN = "Spinning"` (already at `crates/spinbike-core/src/services.rs:29`).

- [ ] **Step 1: Add the chip-row markup just before the service `<select>`'s form-group**

In `spinbike-ui/src/pages/dashboard/action_form.rs`, insert this block immediately after the closing `</div>` of the existing pass-active class buttons row (around line 316, the `}` after `view! { <div></div> }.into_any() }}`) AND before the `<div class="form-group"><label>` for the service combo at line 318:

```rust
            // #30: Spinning quick-charge chip — 1 click = charge from credit
            // at the live default_price. Always visible regardless of pass
            // status (a pass-holder may still want a paid spinning visit
            // outside their pass scope, and this avoids hiding the action).
            {move || {
                let svc_list = services.get();
                let spinning = svc_list
                    .iter()
                    .find(|s| s.name_en == spinbike_core::services::SPINNING_NAME_EN);
                match spinning {
                    None => view! { <div></div> }.into_any(),
                    Some(svc) => {
                        let svc_id = svc.id;
                        let price = svc.default_price;
                        let label = format!("Spinning {price:.2} €");
                        let card_id_for_click = card_id;
                        let on_quick_click = move |_ev: web_sys::MouseEvent| {
                            set_err.set(String::new());
                            set_loading.set(true);
                            spawn_local(async move {
                                #[derive(serde::Serialize)]
                                struct Req {
                                    card_id: i64,
                                    amount: f64,
                                    service_id: Option<i64>,
                                    note: Option<String>,
                                }
                                match api::post::<Req, PaymentResp>(
                                    "/api/payments/charge",
                                    &Req {
                                        card_id: card_id_for_click,
                                        amount: price,
                                        service_id: Some(svc_id),
                                        note: None,
                                    },
                                )
                                .await
                                {
                                    Ok(r) => {
                                        set_msg.set(i18n::tf(
                                            lang.get_untracked(),
                                            "charge_ok_format",
                                            &[&format!("{:.2}", r.new_credit)],
                                        ));
                                        set_selected.update(|s| {
                                            if let Some(c) = s {
                                                c.credit = r.new_credit;
                                            }
                                        });
                                        set_txn_refresh.update(|n| *n += 1);
                                    }
                                    Err(e) => set_err.set(e),
                                }
                                set_loading.set(false);
                            });
                        };
                        view! {
                            <div class="chip-row chip-row--spaced quick-charge-row">
                                <button
                                    class="btn btn--info"
                                    data-testid="quick-charge-spinning"
                                    on:click=on_quick_click
                                    disabled=move || loading.get()
                                >
                                    {label}
                                </button>
                            </div>
                        }.into_any()
                    }
                }
            }}
```

This block lives **inside** the outer `view! { <div class="stack-12" data-testid="action-form"> ...` (so it sits as a sibling to the existing pass class buttons + service form-group). Place it after the pass-active block, BEFORE the `<div class="form-group">` for the service combo.

The chip uses `.btn--info` (solid blue, matches Fitness class button so the visual pairs read together). Loading state is shared with the rest of the form via `loading` signal — a quick-charge in flight disables the regular submit too, preventing double charges.

Note: the new closure captures `services`, `card_id`, `set_err`, `set_loading`, `set_msg`, `set_selected`, `set_txn_refresh`, `lang` by `move`. All are already in scope at the form level. The closure must capture `card_id` as a fresh `let card_id_for_click = card_id;` because `card_id: i64` is `Copy` but the move closure inside `spawn_local` needs the value owned.

- [ ] **Step 2: Add CSS for `.quick-charge-row` (style.css)**

In `spinbike-ui/style.css`, append after the `.chip-row--spaced` rule (around line 1100):

```css
/* Quick-charge chip row (#30): always-visible 1-click charge buttons. */
.quick-charge-row {
    margin-bottom: var(--s-3);
}
.quick-charge-row .btn {
    font-size: 1.125rem;
    font-weight: 700;
    padding: var(--s-2) var(--s-4);
}
```

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs spinbike-ui/style.css
git commit -m "feat(ui): add Spinning quick-charge chip (#30)" \
  -m "New chip row above the service combo, always visible. Reads the live" \
  -m "default_price from the Spinning service so admin price edits propagate" \
  -m "without redeploy. One click = POST /api/payments/charge."
```

---

## Task 5: i18n drops + adds (#32 pass-row support)

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`

The drop-set is the keys that become unused when (a) the page-title is removed (Task 6) and (b) the pass banner is collapsed (Task 8). The add-set is the new format keys for the single-line pass row.

- [ ] **Step 1: Audit existing usage to confirm safe drops**

```bash
for key in card_dashboard pass_valid_until pass_days_remaining pass_expired pass_days_ago pass_last_valid_until; do
    echo "=== $key ==="
    grep -rn "\"$key\"" spinbike-ui/src/ 2>/dev/null
done
```

Expected: each key appears **only** at its definition site in `i18n.rs` and at the consumption site in either `mod.rs` (for `card_dashboard`) or `pass_banner.rs` (for the pass keys). If any key shows extra references, **do not delete it** in this task — note the cross-reference and adjust.

- [ ] **Step 2: Drop the 6 unused keys and add the 2 new format keys**

In `spinbike-ui/src/i18n.rs`, locate each existing key (around lines 355, 459-466, 475-476). Delete the `m.insert(...)` call that defines each of the 6 drop-set keys.

Then, add the new keys near the other `pass_*` keys (around the previous pass-keys block):

```rust
    // #32: collapsed single-line pass status (active + expired). Used by
    // pass_banner.rs. Placeholders are sequential `{}` per i18n::tf — first
    // `{}` is the date, second `{}` is the day count (active form).
    // For the expired form, first `{}` is days-ago count, second `{}` is the
    // last-valid date.
    m.insert(
        "pass_active_oneline_format",
        (
            "✓ Mesačný lístok do {} ({} dní)",
            "✓ Monthly pass valid until {} ({} days)",
        ),
    );
    m.insert(
        "pass_expired_oneline_format",
        (
            "⚠ Mesačný lístok vypršal pred {} dňami (do {})",
            "⚠ Monthly pass expired {} days ago (was valid until {})",
        ),
    );
```

**Slovak grammatical note:** Slovak uses `1 deň`, `2-4 dni`, `5+ dní`. The simplest stable form for both branches is the genitive plural (`dní` / `dňami`) which is grammatical for any count — acceptable for staff UI. Do NOT introduce a count-based pluralization helper; YAGNI.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "feat(i18n): replace pass banner & drop unused keys (#32)" \
  -m "Drops: card_dashboard, pass_valid_until, pass_days_remaining," \
  -m "pass_expired, pass_days_ago, pass_last_valid_until." \
  -m "Adds: pass_active_oneline_format, pass_expired_oneline_format."
```

CI will fail-fast on the WASM build if any dropped key is still consumed (compile error on `i18n::t(\"missing_key\")`); push and let CI catch any oversight.

---

## Task 6: Remove `<h1>` page title (#32a)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs:307`

- [ ] **Step 1: Delete the `<h1 class="page-title">` element**

In `spinbike-ui/src/pages/dashboard/mod.rs`, locate this line:

```rust
        <h1 class="page-title">{move || i18n::t(lang.get(), "card_dashboard")}</h1>
```

Delete the entire line. The `<crate::pages::NowPanel />` line above and the `<div class="card mb-2">` block below should now be adjacent.

- [ ] **Step 2: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/mod.rs
git commit -m "feat(ui): remove 'Cards — Quick Dashboard' page title (#32)" \
  -m "Cleans up vertical real estate on the desk view per Štefan's feedback."
```

---

## Task 7: Card name + barcode on one line (#32b)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs:48-53`
- Modify: `spinbike-ui/style.css` (`.card-title` + new `.card-title__name`, `.card-title__barcode`)

- [ ] **Step 1: Restructure the markup**

In `spinbike-ui/src/pages/dashboard/card_panel.rs`, locate:

```rust
        <div class="card mb-2" data-testid="action-panel">
            <div class="card-header">
                <div class="card-header__main">
                    <div class="card-title">{name}</div>
                    <div class="card-header__meta">
                        <code>{barcode.clone()}</code>
                    </div>
                </div>
```

Replace the inner `card-header__main` block with:

```rust
        <div class="card mb-2" data-testid="action-panel">
            <div class="card-header">
                <div class="card-header__main">
                    <div class="card-title">
                        <span class="card-title__name">{name}</span>
                        " "
                        <code class="card-title__barcode">{barcode.clone()}</code>
                    </div>
                </div>
```

The `card-header__meta` div is gone — both name and barcode now share the single `.card-title` row. The literal `" "` keeps a visible space between them.

Anywhere else in `card_panel.rs` that references `.card-header__meta` should be checked; the rest of the file does not reference it (verify with `grep -n "card-header__meta" spinbike-ui/src/pages/dashboard/card_panel.rs`).

- [ ] **Step 2: Update CSS for the new structure**

In `spinbike-ui/style.css`, replace the existing `.card-title` rule (around line 499) with:

```css
.card-title {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    flex-wrap: wrap;
}

.card-title__name {
    font-size: 1.75rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    line-height: 1.15;
}

.card-title__barcode {
    font-size: 1rem;
    font-weight: 500;
    color: var(--text-muted);
    font-family: var(--font-mono, ui-monospace, monospace);
}
```

Then locate the orphan `.card-header__meta` rule (around line 1117-1121) and **delete** it — the meta div no longer exists.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/card_panel.rs spinbike-ui/style.css
git commit -m "feat(ui): card name + barcode on one line, name larger (#32)" \
  -m "Replaces stacked card-title + card-header__meta with a single .card-title" \
  -m "row containing .card-title__name (28px bold) and .card-title__barcode" \
  -m "(16px monospace, muted)."
```

---

## Task 8: Pass banner one line (#32c)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/pass_banner.rs` (rewrite render block to single line, switch to new i18n keys, replace text edit button with pencil icon)
- Modify: `spinbike-ui/style.css` (`.pass-banner__line` replaces `.pass-banner-title` + `.pass-banner-sub`)

- [ ] **Step 1: Rewrite the active-pass and expired-pass render blocks**

In `spinbike-ui/src/pages/dashboard/pass_banner.rs`, replace the `title_view` and `sub_view` blocks (lines 39-72) and the `view! { … }` block (lines 74-90) with a single line block.

The full new file content (replacing lines 38-99):

```rust
    let line_view = if is_active {
        view! {
            <>
                {move || i18n::tf(
                    lang.get(),
                    "pass_active_oneline_format",
                    &[
                        &i18n::fmt_date(date_for_title, lang.get()),
                        &days.to_string(),
                    ],
                )}
            </>
        }
        .into_any()
    } else {
        view! {
            <>
                {move || i18n::tf(
                    lang.get(),
                    "pass_expired_oneline_format",
                    &[
                        &days_ago.to_string(),
                        &i18n::fmt_date(date_for_title, lang.get()),
                    ],
                )}
            </>
        }
        .into_any()
    };

    view! {
        <div class="group">
            <div class=format!("{banner_class} pass-banner--in-group") data-testid=banner_testid>
                <div class="pass-banner__line">
                    <span class="pass-banner__line-text">{line_view}</span>
                    <button
                        class="pass-banner__edit-btn"
                        data-testid="pass-date-edit"
                        title=move || i18n::t(lang.get(), "edit_pass_date")
                        on:click=move |_| show_edit_sheet.set(true)
                    >
                        "✏"
                    </button>
                </div>
            </div>
        </div>
        <EditPassDateSheet
            show=show_edit_sheet
            tx_id=tx_id
            current_date=current_date
            barcode=barcode.clone()
            set_selected=set_selected
        />
    }
    .into_any()
}
```

Key changes:
- Single `pass-banner__line` div replaces the two-div `pass-banner-title` + `pass-banner-sub` structure.
- New `pass-banner__edit-btn` button with the pencil glyph `✏` as text content, no surrounding `i18n::t` for label (the icon IS the label; `title` attribute provides screen-reader hint).
- The `data-testid="pass-date-edit"` attribute is preserved for E2E test stability.

- [ ] **Step 2: Update CSS in style.css**

In `spinbike-ui/style.css`, replace the existing `.pass-banner-title` and `.pass-banner-sub` rules (lines 886-896) with:

```css
.pass-banner__line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-2);
    font-size: var(--fs-md);
    font-weight: 600;
    letter-spacing: -0.005em;
}

.pass-banner__line-text {
    flex: 1;
    min-width: 0;
}

.pass-banner__edit-btn {
    background: transparent;
    border: 0;
    cursor: pointer;
    font-size: 1.25rem;
    line-height: 1;
    padding: var(--s-1);
    color: inherit;
    opacity: 0.7;
}
.pass-banner__edit-btn:hover { opacity: 1; }
```

Also delete the now-orphan `.pass-banner__title-row` and `.pass-banner__title-text` rules (around lines 1130-onwards in the post-Task-7 file) — they no longer have markup consumers. Grep first to confirm no other consumer:

```bash
grep -n "pass-banner__title-row\|pass-banner__title-text\|pass-banner-title\|pass-banner-sub" spinbike-ui/style.css spinbike-ui/src/
```

If only definitions remain (no usages outside `pass_banner.rs`), delete the obsolete rules.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/pass_banner.rs spinbike-ui/style.css
git commit -m "feat(ui): collapse monthly-pass banner to one line (#32)" \
  -m "Active: '✓ Mesačný lístok do 14.5.2026 (14 dní) ✏' on a single row." \
  -m "Expired: symmetric one-line form. Pencil icon (U+270F) replaces the" \
  -m "'Edit date' text button; data-testid=pass-date-edit preserved."
```

---

## Task 9: Log-visit class buttons — bigger, bolder, darker (#32d)

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/action_form.rs:301-310` (drop `--compact`, add a parent class for sizing)
- Modify: `spinbike-ui/style.css` (darker `--info`, larger font on the new container class)

- [ ] **Step 1: Drop `btn--compact` from log-visit buttons and add parent class**

In `spinbike-ui/src/pages/dashboard/action_form.rs`, locate the chip-row render around line 279-312. The current outer `<div class="chip-row chip-row--spaced">` should also gain the new modifier class `.chip-row--readable`:

```rust
                view! {
                    <div class="chip-row chip-row--spaced chip-row--readable">
```

And inside the buttons, change the `format!` line at 303 from `"btn btn--compact {color_cls}"` to `"btn {color_cls}"`:

```rust
                                let color_cls = if svc.name_en == spinbike_core::services::FITNESS_NAME_EN {
                                    "btn--info"
                                } else {
                                    "btn--info-soft"
                                };
                                view! {
                                    <button
                                        class=format!("btn {color_cls}")
                                        data-testid="log-visit-btn"
                                        on:click=visit_click_for(service_id)
                                    >
                                        {move || i18n::t(lang.get(), "log_visit")}" "{svc_name.clone()}
                                    </button>
                                }
```

- [ ] **Step 2: Add CSS for the readable chip-row**

In `spinbike-ui/style.css`, append after `.chip-row--spaced` (around line 1100):

```css
/* #32d: log-visit class buttons read bigger and bolder than the default
   --compact chip. Applied via .chip-row--readable on the parent so any
   future quick-action chip row can opt in. */
.chip-row--readable .btn {
    font-size: 1.125rem;
    font-weight: 700;
    padding: var(--s-2) var(--s-4);
}
```

- [ ] **Step 3: Darken `--info` and `--info-soft` for higher white-on-blue contrast**

Locate the `--info` and related variables in `spinbike-ui/style.css` at lines 62 and 84-88. The current values are:

```css
    --info:           var(--brand);
    --info-soft:      color-mix(in srgb, var(--info) 16%, var(--surface));
    --info-border:    color-mix(in srgb, var(--info) 45%, var(--surface));
    --info-soft-fg:   var(--info);
    --info-hover:     color-mix(in srgb, var(--info) 85%, black);
    --info-fg:        #fff;
```

Replace `--info` with a 10%-darker variant of `--brand`, and bump `--info-soft` to a stronger mix:

```css
    --info:           color-mix(in srgb, var(--brand) 90%, black);
    --info-soft:      color-mix(in srgb, var(--info) 22%, var(--surface));
    --info-border:    color-mix(in srgb, var(--info) 50%, var(--surface));
    --info-soft-fg:   var(--info);
    --info-hover:     color-mix(in srgb, var(--info) 85%, black);
    --info-fg:        #fff;
```

This darkens the primary `--info` (used for the Fitness button background and the new Spinning chip from Task 4), and strengthens `--info-soft` (used for the secondary Spinning class button) so white-on-soft-blue contrast improves.

If a dark-mode override exists at line 123 (`@media (prefers-color-scheme: dark)`), apply the same pattern there. Verify with:

```bash
grep -n "\\-\\-info\\b" spinbike-ui/style.css
```

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/pages/dashboard/action_form.rs spinbike-ui/style.css
git commit -m "feat(ui): bigger + bolder + darker log-visit buttons (#32)" \
  -m "Drops btn--compact, applies .chip-row--readable for 18px/700 weight," \
  -m "darkens --info / --info-soft 10% for stronger white-on-blue contrast." \
  -m "Quick-charge chip from #30 also benefits via shared .btn--info."
```

---

## Task 10: Playwright E2E — `desk-ux.spec.ts`

**Files:**
- Create: `e2e/tests/desk-ux.spec.ts`

Use existing helpers `setupConsoleCheck`, `assertCleanConsole`, `loginViaAPI` from `e2e/tests/helpers.ts`. The existing `txn-note.spec.ts` is a good template for the activate-card + open-by-name workflow.

- [ ] **Step 1: Create the test file with all 8 cases**

Create `e2e/tests/desk-ux.spec.ts` with this exact content:

```typescript
import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `UX-${suffix}`;
    const lastName = `Ux${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'UX', last_name: lastName }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    return { barcode, lastName };
}

async function sellPassToCard(
    token: string,
    cardId: number,
    daysFromToday: number,
): Promise<void> {
    const validUntil = new Date(Date.now() + daysFromToday * 86400e3)
        .toISOString()
        .slice(0, 10);
    const resp = await fetch(`${BASE_URL}/api/payments/sell-pass`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ card_id: cardId, price: 35.0, valid_until: validUntil }),
    });
    if (!resp.ok) throw new Error(`sell-pass failed: ${resp.status} ${await resp.text()}`);
}

async function lookupCardId(token: string, barcode: string): Promise<number> {
    const resp = await fetch(
        `${BASE_URL}/api/cards/lookup/${encodeURIComponent(barcode)}`,
        { headers: { Authorization: `Bearer ${token}` } },
    );
    if (!resp.ok) throw new Error(`lookup failed: ${resp.status}`);
    const body = await resp.json();
    return body.id as number;
}

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Staff desk UX cluster — issues #29 #30 #31 #32', () => {
    test('fitness preselected on form open', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const select = page.locator('[data-testid="charge-service"]');
        const value = await select.inputValue();
        expect(value).not.toBe('');

        const fitnessOption = select.locator('option', { hasText: /Fitness/ });
        const fitnessValue = await fitnessOption.first().getAttribute('value');
        expect(value).toBe(fitnessValue);

        await assertCleanConsole(msgs);
    });

    test('charge form has no empty service option', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        // Empty-value option should be gone — placeholder removed in #29.
        const emptyOption = page.locator('[data-testid="charge-service"] option[value=""]');
        await expect(emptyOption).toHaveCount(0);

        await assertCleanConsole(msgs);
    });

    test('quick spinning charge button charges card', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const quick = page.locator('[data-testid="quick-charge-spinning"]');
        await expect(quick).toBeVisible();
        // Label format: "Spinning {price} €"
        await expect(quick).toHaveText(/^Spinning \d+\.\d{2} €$/);

        const chargeResp = page.waitForResponse(
            (r) => r.url().includes('/api/payments/charge') && r.request().method() === 'POST',
        );
        await quick.click();
        const resp = await chargeResp;
        expect(resp.ok()).toBe(true);

        // Verify the new transaction appears in the card history.
        await expect(
            page.locator('[data-testid="txn-row"]').first(),
        ).toBeVisible();

        await assertCleanConsole(msgs);
    });

    test('card header shows name and barcode on one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const title = page.locator('[data-testid="action-panel"] .card-title');
        await expect(title).toBeVisible();
        await expect(title).toContainText(lastName);
        await expect(title).toContainText(barcode);

        // .card-header__meta div is gone after #32b — barcode lives inside .card-title.
        const meta = page.locator('[data-testid="action-panel"] .card-header__meta');
        await expect(meta).toHaveCount(0);

        // Name font-size visibly larger (≥ 24px).
        const nameFontSize = await page
            .locator('.card-title__name')
            .first()
            .evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
        expect(nameFontSize).toBeGreaterThanOrEqual(24);

        await assertCleanConsole(msgs);
    });

    test('pass banner active is one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0);
        const cardId = await lookupCardId(token, barcode);
        await sellPassToCard(token, cardId, 14);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const banner = page.locator('[data-testid="pass-banner-active"]');
        await expect(banner).toBeVisible();
        await expect(banner).toHaveText(/^✓ Mesačný lístok do \d{1,2}\.\d{1,2}\.\d{4} \(\d+ dní\)/u);

        // Pencil edit button is present and inside the same single-line container.
        const editBtn = banner.locator('[data-testid="pass-date-edit"]');
        await expect(editBtn).toBeVisible();
        // No legacy `.pass-banner-sub` div.
        await expect(banner.locator('.pass-banner-sub')).toHaveCount(0);

        await assertCleanConsole(msgs);
    });

    test('pass banner expired is one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0);
        const cardId = await lookupCardId(token, barcode);
        await sellPassToCard(token, cardId, -5); // expired 5 days ago
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const banner = page.locator('[data-testid="pass-banner-expired"]');
        await expect(banner).toBeVisible();
        // Symmetric guard: expired must also be a single line, no .pass-banner-sub.
        await expect(banner.locator('.pass-banner-sub')).toHaveCount(0);

        await assertCleanConsole(msgs);
    });

    test('Cards — Quick Dashboard h1 is gone', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await expect(page.locator('h1.page-title')).toHaveCount(0);
        const body = (await page.locator('body').textContent()) ?? '';
        expect(body.toLowerCase()).not.toContain('cards — quick dashboard');
        expect(body.toLowerCase()).not.toContain('karty — rychly prehlad');

        await assertCleanConsole(msgs);
    });

    test('log-visit class buttons are bigger and bolder', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0);
        const cardId = await lookupCardId(token, barcode);
        await sellPassToCard(token, cardId, 14);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const visitBtn = page.locator('[data-testid="log-visit-btn"]').first();
        await expect(visitBtn).toBeVisible();
        const { fontSize, fontWeight } = await visitBtn.evaluate((el) => {
            const cs = getComputedStyle(el);
            return { fontSize: parseFloat(cs.fontSize), fontWeight: parseInt(cs.fontWeight, 10) };
        });
        expect(fontSize).toBeGreaterThanOrEqual(18);
        expect(fontWeight).toBeGreaterThanOrEqual(700);

        await assertCleanConsole(msgs);
    });
});
```

Notes about helper-API fit:
- `lookupCardId` uses `/api/cards/lookup/{barcode}` (verified to exist at `crates/spinbike-server/src/routes/cards.rs:168`). The handler returns the card object including `id`.
- `sell-pass` requires `valid_until` ≥ today for active and < today for expired; the offsets handle that.

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/desk-ux.spec.ts
git commit -m "test(e2e): desk UX cluster Playwright spec (#29 #30 #31 #32)" \
  -m "8 cases: Fitness preselected, no empty service option, quick-charge" \
  -m "Spinning chip charges card, card header is one line with bigger name," \
  -m "active/expired pass banner is one line, no Cards — Quick Dashboard h1," \
  -m "log-visit class buttons are bigger and bolder. Each test asserts clean" \
  -m "browser console (zero errors/warnings)."
```

---

## Task 11: Push, monitor CI, open PR

**Files:** none. CI is authoritative for build/test/lint; we never run cargo locally per `feedback_subagent_no_local_build.md`.

- [ ] **Step 1: One last `cargo fmt --all --check`**

```bash
cargo fmt --all --check
```

If anything is unformatted, run `cargo fmt --all` and amend with a NEW commit (do NOT `--amend`):

```bash
git add -u
git commit -m "style: cargo fmt"
```

- [ ] **Step 2: Push to dev**

```bash
git push origin dev
```

- [ ] **Step 3: Identify the latest run, monitor to terminal state**

```bash
sleep 30 && gh run list --branch dev --limit 3 --json databaseId,status,conclusion,event,headSha,workflowName
```

Pick the highest `databaseId` whose `headSha` matches the just-pushed commit. Then monitor:

```bash
sleep 600 && gh run view <run-id> --json status,conclusion,jobs
```

Use `run_in_background: true` on the Bash call (per `ci-monitoring.md`). Do NOT use `/loop`, `CronCreate`, or `gh run watch`.

Required green jobs: **Test Integrity, Lint, Build WASM (UI), Test, E2E Tests**, plus Deploy (dev) and Smoke (dev) since this is a push event. **Mutation Testing** runs on the matched PR event (next step) and must be green there.

If E2E fails on the known SQLITE_BUSY flake (#24) — symptom: `database is locked` (code 5) on a single test, ONE rerun acceptable per `ci-monitoring.md`:

```bash
gh run rerun <run-id> --failed
```

If E2E fails on something else, investigate via `gh run view <run-id> --log-failed`, fix root cause in a NEW commit, push, monitor again.

- [ ] **Step 4: Open the PR `dev` → `main`**

```bash
gh pr create --base main --head dev \
  --title "feat: staff desk UX cluster (#29 #30 #31 #32, v0.13.10)" \
  --body "$(cat <<'EOF'
## Summary
- **#29** — Fitness is now preselected as the default service in the charge form. Empty `<option value="">` placeholder removed; the combo can no longer send `service_id=null` from the UI.
- **#30** — New "Spinning {price}€" quick-charge chip above the service combo. Reads `default_price` live from the Spinning service so admin price edits propagate without redeploy.
- **#31** — Server-side defense-in-depth: `POST /api/payments/charge` now returns 400 when `service_id` is null. Top-up unaffected (service-independent).
- **#32** — Dashboard layout cleanup: removed the "Cards — Quick Dashboard" `<h1>`, card name + barcode now share one bigger line, monthly-pass banner collapsed to a single line ending with a pencil-icon edit button, log-visit class buttons are bigger / bolder / darker.

Spec: `docs/superpowers/specs/2026-04-30-staff-desk-ux-cluster-design.md`

## Test plan
- [ ] CI: Test Integrity, Lint, Build WASM, Test, Mutation Testing, E2E Tests all green.
- [ ] After merge: dev frontend at https://spinbike-dev.newlevel.media renders the new layout (no h1, one-line card title, one-line pass banner, quick-charge chip visible).
- [ ] After merge: Štefan updates Spinning's `default_price` to 3.30 in `/admin/services`; the chip then reads `Spinning 3.30 €`.
- [ ] After merge: prod at https://spinbike.newlevel.media verified the same way.

## Out of scope
- Cleaning up other stale `default_price` values (Štefan handles via `/admin`).
- Adding a Fitness quick-button.
- Issue #28 (note CHECK constraint), #24 (E2E flake), #22 (UI mutation testing).
EOF
)"
```

- [ ] **Step 5: Verify PR is mergeable + clean and CI is green on the PR run**

```bash
PR_NUM=$(gh pr list --base main --head dev --json number -q '.[0].number')
gh api repos/zbynekdrlik/spinbike/pulls/$PR_NUM \
  --jq '{mergeable, mergeable_state, head_sha: .head.sha}'
sleep 30 && gh run list --branch dev --event pull_request --limit 3 \
  --json databaseId,status,conclusion,headSha,workflowName
```

Pick the PR-event run, monitor the same way as Step 3. Mutation testing runs only on PR events — must be green.

The PR is ready when `mergeable: true` AND `mergeable_state: "clean"` AND ALL CI jobs are green (per `pr-merge-policy.md`). Do NOT merge — that's the user's action.

---

## Task 12: Post-deploy verification (runs ONLY after user merges)

**Files:** none. Verification only.

- [ ] **Step 1: Wait for main CI + Deploy (prod) + Smoke (prod) to be green**

After the user merges, monitor the `main` branch run:

```bash
sleep 300 && gh run list --branch main --limit 3 --json databaseId,status,conclusion,workflowName
sleep 600 && gh run view <main-run-id> --json status,conclusion,jobs
```

ALL jobs (including Deploy and Smoke for both dev and prod) must reach green terminal state.

- [ ] **Step 2: Štefan updates Spinning's `default_price` to 3.30 in `/admin/services`**

This is an off-band manual step. Either prompt Štefan to do it, or do it via SQL:

```bash
sqlite3 /opt/spinbike/prod/spinbike.db \
  "UPDATE services SET default_price=3.30 WHERE name_en='Spinning';"
```

(prod and dev are on the same machine per `feedback_prod_dev_same_machine.md` — no SSH needed.) The dev DB picks up the new value on the next dev deploy via the prod→dev sync.

- [ ] **Step 3: Verify dev frontend via Playwright MCP**

Open https://spinbike-dev.newlevel.media in Playwright (per `autonomous-verification.md` — YOU verify, not the user):

1. Login as staff. Pick any card. Confirm:
   - No "Cards — Quick Dashboard" h1.
   - Card title shows name and barcode on one line; name visibly larger than barcode.
   - Service combo is preselected to Fitness; no empty option in the dropdown.
   - Quick-charge chip "Spinning 3.30 €" is visible above the service combo.
   - Click the chip → transaction lands in card history with action=charge, service=Spinning, amount=3.30.
2. For a card with active monthly pass:
   - Pass banner is a single line ending in `✏`.
   - Pencil click opens the existing date editor.
   - Log-visit class buttons (Fitness solid, Spinning soft) are visibly bigger and bolder than v0.13.9.
3. Read DOM `[data-testid="version"]` → must show `0.13.10` (matches the deployed version per `version-on-dashboard.md`). Confirm `/api/version` returns `0.13.10`.
4. Browser console must be clean (zero errors/warnings).

- [ ] **Step 4: Verify prod frontend via Playwright MCP**

Repeat Step 3 for https://spinbike.newlevel.media. Same checks.

- [ ] **Step 5: Send the completion report**

Use the EXACT template from `~/devel/airuleset/modules/core/completion-report.md`. Required elements:
- `## ✅ Work Complete`
- Audits & deploy block: `✅ CI: green`, `✅ /plan-check: 12/12 fulfilled`, `✅ /review: clean — 0 🔴 0 🟡 0 🔵`, `✅ Deploy: dev + prod show v0.13.10 with new layout, quick-charge Spinning 3.30 € verified end-to-end`.
- `**Goal:**` and `**What changed:**` in plain language.
- `🌐 Dev: https://spinbike-dev.newlevel.media` and `🌐 Prod: https://spinbike.newlevel.media`.
- `**[spinbike] PR #<N>: <full PR title>**` line + clickable PR URL — merged at `<sha>`.

---

## Coverage Self-Check (planner-side, completed before saving)

| Spec section | Plan task |
|---|---|
| #29 — Fitness as default service | Task 3 |
| #29 — Remove empty `<option>` placeholder | Task 3, Step 2 |
| #30 — Quick-charge chip placement & price source | Task 4 |
| #30 — Off-band data fix for Spinning's default_price | Task 12, Step 2 |
| #31 — UI side (no empty option) | Task 3 (covered) |
| #31 — Server defense-in-depth | Task 2 |
| #31 — Top-up unaffected | Task 2, Step 1 (test) |
| #32a — Remove "Cards — Quick Dashboard" h1 | Task 6 |
| #32b — Card name + barcode on one line, name bigger | Task 7 |
| #32c — Pass banner one line + pencil icon | Task 8 |
| #32c — i18n drops/adds for pass banner | Task 5 |
| #32d — Log-visit buttons bigger/bolder/darker | Task 9 |
| Server tests for #31 | Task 2, Step 1 |
| E2E for all user-visible changes (8 cases) | Task 10 |
| Mutation testing (PR-event run) | Task 11, Step 5 |
| VERSION bump to 0.13.10 | Task 1 (already at 4a99f07) |
| Push + monitor CI + open PR | Task 11 |
| Post-deploy verification on dev + prod | Task 12 |

No spec section is unmapped. No placeholders in steps (every code step has a complete code block). Type names are consistent across tasks (`ServiceInfo`, `selected_service_id`, `quick_charge_spinning_label` referenced once and only as a possible-future i18n key, not as a depended-on type).
