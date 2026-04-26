# Card Management Rework + Blue Theme — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the cluttered staff card-detail (two amount inputs + giant Sell-Pass button) with one unified Charge/Top-up form whose service dropdown also handles Monthly Pass; repaint the brand color from green to vibrant blue; fix the SpinBike-logo-vs-Desk active-state nav bug.

**Architecture:** Three coordinated UI changes inside `spinbike-ui` (Leptos 0.7 CSR + WASM): (a) swap CSS color tokens, (b) introduce a single `ActionForm` component that subsumes `ChargeSection`, `TopupSection`, and `SellPassModal`, (c) make the top-bar SpinBike link role-aware and add a `/`→`/staff` redirect for staff/admin so the active nav state matches the rendered page. No backend changes. Tests are Playwright E2E only — Rust units are unchanged.

**Tech Stack:** Leptos 0.7 (`leptos`, `leptos_router`, `wasm_bindgen_futures`), `chrono`, project-internal `crate::api`, `crate::components::DateInput`, `crate::i18n`. CI runs Playwright against the deployed dev environment.

**Local commands available:** Only `cargo fmt --all --check` (and `cargo fmt --all`). All compile, clippy, test, mutation, build, and Playwright runs happen on CI — do NOT run them locally (per project CLAUDE.md and airuleset).

---

## File map

| File | Action |
|---|---|
| `spinbike-ui/style.css` | Modify — color tokens block (`--brand`, `--brand-tint`, `--info`, `--pass`, `--pass-fg`, hardcoded greens in `.badge--pass`) |
| `spinbike-ui/src/pages/dashboard/action_form.rs` | **Create** — unified Charge/Top-up/Sell-pass form |
| `spinbike-ui/src/pages/dashboard/mod.rs` | Modify — register `action_form`; drop `charge_section`, `topup_section`, `sell_pass_modal` |
| `spinbike-ui/src/pages/dashboard/card_panel.rs` | Modify — render `ActionForm` in place of three components; drop hero Sell-pass button + SellPassModal mount |
| `spinbike-ui/src/pages/dashboard/charge_section.rs` | **Delete** |
| `spinbike-ui/src/pages/dashboard/topup_section.rs` | **Delete** |
| `spinbike-ui/src/pages/dashboard/sell_pass_modal.rs` | **Delete** |
| `spinbike-ui/src/components/nav.rs` | Modify — role-aware brand href |
| `spinbike-ui/src/components/adaptive_nav.rs` | Modify — drop `path == "/"` from `desk_active` |
| `spinbike-ui/src/router.rs` | Modify — root `/` is role-aware, redirects staff/admin to `/staff` |
| `e2e/tests/card-action-form.spec.ts` | **Create** |
| `e2e/tests/nav-brand-link.spec.ts` | **Create** |
| `e2e/tests/theme-blue.spec.ts` | **Create** |
| `e2e/tests/redesign-sheets.spec.ts` | Modify — drop sell-pass sheet tests |
| `e2e/tests/monthly-pass.spec.ts` | Rewrite — use the new dropdown flow |
| `e2e/tests/monthly-pass-expired.spec.ts` | Modify — replace `[data-testid=sell-pass-btn]` assertion |
| `e2e/tests/sell-pass-price-input.spec.ts` | Rewrite — drive the unified form |
| `e2e/tests/redesign-theme.spec.ts` | Modify — sample new blue token color |

---

## Task 1: Swap CSS color tokens (green → blue, retone --pass to amber)

**Files:**
- Modify: `spinbike-ui/style.css` (token block lines 49–96 and light override lines 113–128, plus `.badge--pass` line 606)

- [ ] **Step 1: Replace dark-theme token values**

In `spinbike-ui/style.css`, find the dark token block starting at `:root {` (line 9) and locate the lines below. Replace them in place:

```css
/* Dark palette  (was green)  →  vibrant blue */
--brand:         #60a5fa;
--brand-tint:    rgba(96, 165, 250, 0.14);
/* --pass was lime — retone to amber so it stays distinct from primary blue */
--pass:          #fbbf24;
```

And `--pass-fg` (line 90) — change it to dark text on amber:

```css
--pass-fg:        #1a1306;
```

`--info` was already `#60a5fa`; with the swap it now equals `--brand`. Redefine it to point at `--brand` so there is one source of truth:

```css
--info:           var(--brand);
```

(Place this AFTER the `--brand` line so the variable resolves correctly.)

- [ ] **Step 2: Replace light-theme token values**

In the `@media (prefers-color-scheme: light)` block (line 112), update:

```css
--brand:         #2563eb;
--brand-tint:    rgba(37, 99, 235, 0.10);
--pass:          #d97706;
--info:          var(--brand);
```

(Light theme inherits `--pass-fg` from dark; no need to repeat it. If `--pass-fg` is not in the light block, leave the dark default.)

- [ ] **Step 3: Replace hard-coded green in `.badge--pass`**

Line 606 currently is:

```css
.badge--pass      { background: rgba(132, 204, 22, 0.14); color: var(--pass); }
```

Replace with a token-driven amber tint:

```css
.badge--pass      { background: color-mix(in srgb, var(--pass) 16%, var(--surface)); color: var(--pass); }
```

- [ ] **Step 4: Audit for any other hardcoded greens**

Run:

```bash
grep -nE "#22c55e|#16a34a|#84cc16|#65a30d|rgba\(34, *197, *94|rgba\(22, *163, *74|rgba\(132, *204, *22" spinbike-ui/style.css
```

Expected: no matches after Steps 1–3. If any line matches, it is a leftover — replace with the matching token.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt --all --check
git add spinbike-ui/style.css
git commit -m "feat(ui): brand color → vibrant blue; --pass retoned amber"
```

---

## Task 2: Create the unified `action_form.rs`

**Files:**
- Create: `spinbike-ui/src/pages/dashboard/action_form.rs`

- [ ] **Step 1: Write the file with the full component**

Create `spinbike-ui/src/pages/dashboard/action_form.rs` with this content:

```rust
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;
use crate::components::DateInput;
use crate::i18n::{self, Lang};
use crate::util::parse_money;

use super::helpers::pass_is_active;
use super::{CardInfo, CardPass, PaymentResp, ServiceInfo};

const MONTHLY_PASS_NAME: &str = "Monthly pass";

/// Unified action form for the staff card-detail panel.
///
/// One service dropdown (lists every service, including `Monthly pass`),
/// one amount input, and two equal-weight buttons: Top up and Charge.
/// When `Monthly pass` is selected, a `valid until` date row appears and
/// the Charge button label flips to `Sell pass`. Top up always calls
/// `/api/cards/topup` and ignores the service. Charge calls
/// `/api/payments/charge` for normal services or `/api/payments/sell-pass`
/// for Monthly pass.
#[component]
pub fn ActionForm(
    card: CardInfo,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    set_txn_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let card_id = card.id;
    let pass_active = pass_is_active(&card);

    let service_ref = NodeRef::<leptos::html::Select>::new();
    let amount_ref = NodeRef::<leptos::html::Input>::new();

    // Default valid_until: max(current pass valid_until, today) + 30 days.
    let today = chrono::Local::now().date_naive();
    let default_valid_until = card
        .pass
        .as_ref()
        .map(|p| {
            if p.valid_until > today {
                p.valid_until
            } else {
                today
            }
        })
        .unwrap_or(today)
        + chrono::Duration::days(30);
    let (valid_until, set_valid_until) = signal(default_valid_until);

    let (selected_service_id, set_selected_service_id) = signal::<Option<i64>>(None);
    let (loading, set_loading) = signal(false);
    let (err, set_err) = signal(String::new());

    // Selected service is Monthly pass?
    let is_monthly_pass = move || {
        match selected_service_id.get() {
            Some(id) => services
                .get()
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.name == MONTHLY_PASS_NAME)
                .unwrap_or(false),
            None => false,
        }
    };

    let on_service_change = move |_| {
        let raw = service_ref
            .get()
            .map(|el| {
                let el: &HtmlSelectElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let id: Option<i64> = raw.parse().ok();
        set_selected_service_id.set(id);
        if let Some(id) = id {
            if let Some(svc) = services.get().iter().find(|s| s.id == id) {
                if let Some(el) = amount_ref.get() {
                    let el: &HtmlInputElement = &el;
                    el.set_value(&format!("{:.2}", svc.default_price));
                }
            }
        }
    };

    // Top up: ignores service, posts to /api/cards/topup.
    let do_topup = move |_ev: web_sys::MouseEvent| {
        set_err.set(String::new());
        let typed = amount_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let amount = match parse_money(&typed) {
            Some(v) if v > 0.0 => v,
            _ => return,
        };
        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                card_id: i64,
                amount: f64,
            }
            match api::post::<Req, CardInfo>("/api/cards/topup", &Req { card_id, amount }).await {
                Ok(c) => {
                    let credit = c.credit;
                    set_selected.set(Some(c));
                    set_msg.set(i18n::tf(
                        lang.get_untracked(),
                        "topup_ok_format",
                        &[&format!("{credit:.2}")],
                    ));
                }
                Err(e) => set_err.set(e),
            }
            set_loading.set(false);
        });
    };

    // Charge / Sell-pass: routes by service selection.
    let do_charge = move |_ev: web_sys::MouseEvent| {
        set_err.set(String::new());
        let typed = amount_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let amount = match parse_money(&typed) {
            Some(v) => v,
            None => {
                set_err.set(i18n::t(lang.get_untracked(), "price_required").to_string());
                return;
            }
        };
        let service_id = selected_service_id.get_untracked();

        if is_monthly_pass() {
            // Sell-pass path. Backend allows promotional 0 €; negatives are rejected server-side.
            let vu = valid_until.get_untracked();
            set_loading.set(true);
            spawn_local(async move {
                #[derive(serde::Serialize)]
                struct Req {
                    card_id: i64,
                    price: f64,
                    valid_until: chrono::NaiveDate,
                }
                #[derive(serde::Deserialize)]
                struct Resp {
                    transaction_id: i64,
                    new_credit: f64,
                    valid_until: chrono::NaiveDate,
                    days_remaining: i32,
                }
                match api::post::<Req, Resp>(
                    "/api/payments/sell-pass",
                    &Req {
                        card_id,
                        price: amount,
                        valid_until: vu,
                    },
                )
                .await
                {
                    Ok(r) => {
                        set_selected.update(|opt| {
                            if let Some(c) = opt.as_mut() {
                                c.credit = r.new_credit;
                                c.pass = Some(CardPass {
                                    valid_until: r.valid_until,
                                    days_remaining: r.days_remaining,
                                    transaction_id: r.transaction_id,
                                });
                            }
                        });
                        set_txn_refresh.update(|n| *n += 1);
                    }
                    Err(e) => set_err.set(e),
                }
                set_loading.set(false);
            });
        } else {
            // Charge path. Server requires amount > 0; we mirror that client-side.
            if amount <= 0.0 {
                return;
            }
            set_loading.set(true);
            spawn_local(async move {
                #[derive(serde::Serialize)]
                struct Req {
                    card_id: i64,
                    amount: f64,
                    service_id: Option<i64>,
                }
                match api::post::<Req, PaymentResp>(
                    "/api/payments/charge",
                    &Req {
                        card_id,
                        amount,
                        service_id,
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
        }
    };

    // Quick log-visit chip click factory (only shown when pass is active).
    let visit_click_for = move |service_id: i64| {
        move |_: web_sys::MouseEvent| {
            spawn_local(async move {
                #[derive(serde::Serialize)]
                struct Req {
                    card_id: i64,
                    service_id: i64,
                }
                #[derive(serde::Deserialize)]
                struct Resp {
                    #[allow(dead_code)]
                    transaction_id: i64,
                }
                match api::post::<Req, Resp>(
                    "/api/payments/log-visit",
                    &Req {
                        card_id,
                        service_id,
                    },
                )
                .await
                {
                    Ok(_) => {
                        set_txn_refresh.update(|n| *n += 1);
                    }
                    Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
                }
            });
        }
    };

    view! {
        <div class="stack-12" data-testid="action-form">
            // Quick log-visit chips (only when an active pass exists).
            {if pass_active {
                view! {
                    <div class="chip-row chip-row--spaced">
                        {services.get().into_iter()
                            .filter(|svc| svc.name != MONTHLY_PASS_NAME)
                            .map(|svc| {
                                let service_id = svc.id;
                                let svc_name = svc.name.clone();
                                view! {
                                    <button
                                        class="btn btn--compact btn--primary"
                                        data-testid="log-visit-btn"
                                        on:click=visit_click_for(service_id)
                                    >
                                        {move || i18n::t(lang.get(), "log_visit")}" "{svc_name.clone()}
                                    </button>
                                }
                            }).collect::<Vec<_>>()}
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }}

            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "select_service")}</label>
                <select
                    class="form-control"
                    node_ref=service_ref
                    on:change=on_service_change
                    data-testid="charge-service"
                >
                    <option value="">{move || i18n::t(lang.get(), "select_service")}</option>
                    {move || {
                        services.get().into_iter().map(|s| {
                            let val = s.id.to_string();
                            let label = format!("{} ({:.2} €)", s.name, s.default_price);
                            view! { <option value=val>{label}</option> }
                        }).collect::<Vec<_>>()
                    }}
                </select>
            </div>

            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "amount")}</label>
                <input
                    type="text"
                    inputmode="decimal"
                    autocomplete="off"
                    class="form-control"
                    node_ref=amount_ref
                    data-testid="charge-amount"
                    placeholder=move || i18n::t(lang.get(), "amount")
                />
            </div>

            // Valid-until row appears only when Monthly pass is the selected service.
            {move || if is_monthly_pass() {
                view! {
                    <div class="form-group" data-testid="valid-until-row">
                        <label>{move || i18n::t(lang.get(), "modal_valid_until")}</label>
                        <DateInput
                            value=valid_until
                            set_value=set_valid_until
                            testid="sell-pass-date"
                        />
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }}

            {move || if !err.get().is_empty() {
                view! { <div class="alert alert-error">{move || err.get()}</div> }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }}

            <div class="action-row">
                <button
                    type="button"
                    class="btn btn--primary"
                    data-testid="topup-submit"
                    on:click=do_topup
                    disabled=move || loading.get()
                >
                    "+ "{move || i18n::t(lang.get(), "topup")}
                </button>
                <button
                    type="button"
                    class="btn btn--primary"
                    data-testid="charge-submit"
                    on:click=do_charge
                    disabled=move || loading.get()
                >
                    {move || if is_monthly_pass() {
                        i18n::t(lang.get(), "sell_pass_action").to_string()
                    } else {
                        i18n::t(lang.get(), "charge").to_string()
                    }}
                </button>
            </div>
        </div>
    }
}
```

Notes on this code:
- `selected_service_id` mirrors the dropdown's selected `<option>` `value`. The conditional valid-until row and the Charge button label both derive from this signal so they stay in sync.
- `valid_until` defaults to `max(card.pass.valid_until, today) + 30 days` — same rule as the deleted `SellPassModal`.
- The Charge button accepts an explicit `0` price ONLY for Monthly pass (matches existing sell-pass behavior; backend treats `0 €` as a valid promotional pass). For non-pass charge, `amount <= 0.0` short-circuits.
- The chip filter excludes `Monthly pass` so it doesn't appear as a quick log-visit (passes are sold, not logged-as-visit).
- `data-testid` names mirror the deleted components so most existing E2E selectors (`charge-service`, `charge-amount`, `charge-submit`, `sell-pass-date`, `topup-submit`, `log-visit-btn`) continue to match without rework.
- `set_msg` is still set on success so the parent panel's success toast keeps working; errors render inline and ALSO inside `set_msg` for chip-based log-visit failures (preserves current toast behavior).

- [ ] **Step 2: Format and commit**

```bash
cargo fmt --all --check
git add spinbike-ui/src/pages/dashboard/action_form.rs
git commit -m "feat(ui): unified ActionForm (Charge + Top-up + Sell pass)"
```

---

## Task 3: Wire `ActionForm` into the dashboard module + card panel; drop the old components

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/mod.rs:6-15`
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs`
- Delete: `spinbike-ui/src/pages/dashboard/charge_section.rs`
- Delete: `spinbike-ui/src/pages/dashboard/topup_section.rs`
- Delete: `spinbike-ui/src/pages/dashboard/sell_pass_modal.rs`

- [ ] **Step 1: Update `mod.rs` module declarations**

In `spinbike-ui/src/pages/dashboard/mod.rs` lines 6–15, replace the module list:

```rust
pub mod block_button;
pub mod card_panel;
pub mod charge_section;          // ← remove
pub mod edit_info_form;
pub mod helpers;
pub mod pass_banner;
pub mod sell_pass_modal;         // ← remove
pub mod sheets;
pub mod topup_section;           // ← remove
pub mod transactions_list;
pub mod action_form;             // ← add
```

After the edit, the lines must read exactly:

```rust
pub mod action_form;
pub mod block_button;
pub mod card_panel;
pub mod edit_info_form;
pub mod helpers;
pub mod pass_banner;
pub mod sheets;
pub mod transactions_list;
```

- [ ] **Step 2: Replace the body of `card_panel.rs` to use `ActionForm`**

Open `spinbike-ui/src/pages/dashboard/card_panel.rs` and replace the entire file with:

```rust
use leptos::prelude::*;

use crate::components::{PersistentToggles, Segmented, UpcomingClasses};
use crate::i18n::{self, Lang};

use super::action_form::ActionForm;
use super::block_button::BlockButton;
use super::edit_info_form::EditInfoForm;
use super::helpers::full_name;
use super::pass_banner::PassBanner;
use super::transactions_list::TransactionsList;
use super::{CardInfo, ServiceInfo};

#[component]
pub fn CardActionPanel(
    card: CardInfo,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    #[prop(into)] on_close: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (show_edit, set_show_edit) = signal(false);
    let (show_contact, set_show_contact) = signal(false);
    let txn_refresh = RwSignal::new(0u32);
    let upc_tick = RwSignal::new(0u32);
    let (tab, set_tab) = signal("history".to_string());

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

    let tab_items = vec![
        ("history".to_string(), i18n::t(lang.get_untracked(), "tab_history").to_string()),
        ("upcoming".to_string(), i18n::t(lang.get_untracked(), "tab_upcoming").to_string()),
        ("persistent".to_string(), i18n::t(lang.get_untracked(), "tab_persistent").to_string()),
    ];

    view! {
        <>
        <div class="card mb-2" data-testid="action-panel">
            <div class="card-header">
                <div class="card-header__main">
                    <div class="card-title">{name}</div>
                    <div class="card-header__meta">
                        <code>{barcode.clone()}</code>
                    </div>
                </div>
                <button
                    class="btn btn--compact btn--ghost"
                    on:click=move |e| on_close.run(e)
                    title="close"
                >"\u{2715}"</button>
            </div>

            // Contact toggle (unchanged)
            {
                let has_contact = !company.is_empty() || !phone.is_empty();
                let company_for_show = company.clone();
                let phone_for_show = phone.clone();
                view! {
                    {move || if has_contact {
                        view! {
                            <button
                                class="btn btn--compact btn--ghost"
                                data-testid="toggle-contact"
                                on:click=move |_| set_show_contact.update(|v| *v = !*v)
                            >
                                {move || if show_contact.get() {
                                    i18n::t(lang.get(), "card_hide_contact")
                                } else {
                                    i18n::t(lang.get(), "card_show_contact")
                                }}
                            </button>
                        }.into_any()
                    } else { ().into_any() }}
                    {move || if show_contact.get() {
                        view! {
                            <div class="group" data-testid="card-contact">
                                <div class="list-row">
                                    <div class="list-row__main">
                                        <div class="list-row__sub">{company_for_show.clone()}</div>
                                        <div class="list-row__sub">{phone_for_show.clone()}</div>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}
                }
            }

            <PassBanner pass=card_pass barcode=barcode.clone() set_selected=set_selected />

            <div
                class=if credit < 0.0 { "card-balance card-balance--negative" } else { "card-balance" }
                data-testid="card-credit"
            >
                <span class="card-balance__num">{format!("{:.2}", credit)}</span>
                " "
                <span class="card-balance__unit">"€"</span>
                {if is_blocked {
                    view! {
                        <span class="badge badge--full badge--inline">
                            {move || i18n::t(lang.get(), "blocked")}
                        </span>
                    }.into_any()
                } else {
                    view! {}.into_any()
                }}
            </div>

            // Unified action form replaces ChargeSection + TopupSection + SellPassModal.
            <ActionForm
                card=card_for_form.clone()
                services=services
                set_selected=set_selected
                set_msg=set_msg
                set_txn_refresh=txn_refresh.write_only()
            />

            <div class="action-row stack-12">
                <button
                    class="btn btn--ghost"
                    on:click=move |_| set_show_edit.update(|v| *v = !*v)
                >
                    {move || i18n::t(lang.get(), "edit_info")}
                </button>
                <BlockButton card_id=card_id blocked=is_blocked set_selected=set_selected set_msg=set_msg />
            </div>

            <div class="stack-16">
                <Segmented
                    items=tab_items
                    active=Signal::derive(move || tab.get())
                    on_change=Callback::new(move |key: String| set_tab.set(key))
                    testid_prefix="tab"
                />
                <div class="seg-body">
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
                            _ => view! { <div></div> }.into_any(),
                        }
                    }}
                </div>
            </div>
        </div>

        <EditInfoForm
            card=card_for_edit.clone()
            set_selected=set_selected
            set_msg=set_msg
            show=Signal::derive(move || show_edit.get())
            on_close=Callback::new(move |()| set_show_edit.set(false))
        />
        </>
    }
}
```

What changed vs. the prior `card_panel.rs`:
- Removed `super::charge_section::ChargeSection`, `super::topup_section::TopupSection`, `super::sell_pass_modal::SellPassModal`, and `super::helpers::pass_is_active` (no longer used here — `ActionForm` does the chip rendering).
- Removed the hero Sell-pass button + the `SellPassModal` mount + the `show_sell_pass` signal.
- Removed the `<div class="action-row">` wrapping ChargeSection + TopupSection.
- Inserted a single `<ActionForm ... />` invocation.

- [ ] **Step 3: Delete the obsolete files**

```bash
git rm spinbike-ui/src/pages/dashboard/charge_section.rs
git rm spinbike-ui/src/pages/dashboard/topup_section.rs
git rm spinbike-ui/src/pages/dashboard/sell_pass_modal.rs
```

- [ ] **Step 4: Format and commit**

```bash
cargo fmt --all --check
git add spinbike-ui/src/pages/dashboard/mod.rs spinbike-ui/src/pages/dashboard/card_panel.rs
git commit -m "refactor(ui): replace Charge/Topup/SellPass with unified ActionForm"
```

---

## Task 4: Make the SpinBike brand link role-aware

**Files:**
- Modify: `spinbike-ui/src/components/nav.rs:40`

- [ ] **Step 1: Replace the brand link with a role-aware closure**

In `spinbike-ui/src/components/nav.rs`, replace line 40 (`<a href="/" class="navbar-brand">"SpinBike"</a>`) with:

```rust
            <a
                href=move || {
                    let _ = auth_ver.get();
                    match auth::get_user() {
                        Some(u) if u.role == "admin" || u.role == "staff" => "/staff",
                        _ => "/",
                    }
                }
                class="navbar-brand"
                data-testid="brand-link"
            >"SpinBike"</a>
```

Why `auth_ver.get()`: re-evaluate when login state changes so the link target updates without a full reload. `auth::get_user()` reads localStorage, which is non-reactive on its own.

- [ ] **Step 2: Format and commit**

```bash
cargo fmt --all --check
git add spinbike-ui/src/components/nav.rs
git commit -m "fix(ui): SpinBike brand link points to /staff for staff/admin"
```

---

## Task 5: Drop `path == "/"` from `desk_active`

**Files:**
- Modify: `spinbike-ui/src/components/adaptive_nav.rs:31`

- [ ] **Step 1: Edit the active-state rule**

Replace line 31:

```rust
            let desk_active = path.starts_with("/staff") || path == "/";
```

with:

```rust
            let desk_active = path.starts_with("/staff");
```

This is the entire change. The `path` variable above it stays. After this edit, the Desk tab is NOT highlighted at `/`; the redirect added in Task 6 ensures that staff/admin never stay on `/` anyway.

- [ ] **Step 2: Format and commit**

```bash
cargo fmt --all --check
git add spinbike-ui/src/components/adaptive_nav.rs
git commit -m "fix(ui): Desk tab no longer falsely active on / route"
```

---

## Task 6: Add `/` redirect for staff/admin

**Files:**
- Modify: `spinbike-ui/src/router.rs`

- [ ] **Step 1: Introduce a `RootRoute` component**

Open `spinbike-ui/src/router.rs`. Just below the existing `ScheduleRoute` component (the `fn ScheduleRoute()` block at lines 21–39), add a new component:

```rust
/// Role-aware root route. Staff/admin land on the Desk (`/staff`); customers
/// and logged-out visitors see the public schedule. Reactive on `auth_ver`.
#[component]
fn RootRoute() -> impl IntoView {
    let auth_ver = use_context::<ReadSignal<u32>>().expect("auth_ver context");
    view! {
        {move || {
            let _ = auth_ver.get();
            let user = crate::auth::get_user();
            let is_staff = user
                .as_ref()
                .map(|u| u.role == "staff" || u.role == "admin")
                .unwrap_or(false);
            if is_staff {
                view! { <RedirectTo to="/staff".to_string()/> }.into_any()
            } else {
                SchedulePage().into_any()
            }
        }}
    }
}
```

- [ ] **Step 2: Wire `RootRoute` into the router**

Replace line 77 of `spinbike-ui/src/router.rs`:

```rust
                        <Route path=path!("/") view=SchedulePage />
```

with:

```rust
                        <Route path=path!("/") view=RootRoute />
```

- [ ] **Step 3: Format and commit**

```bash
cargo fmt --all --check
git add spinbike-ui/src/router.rs
git commit -m "feat(ui): / redirects staff/admin to Desk"
```

---

## Task 7: New E2E spec — `card-action-form.spec.ts`

**Files:**
- Create: `e2e/tests/card-action-form.spec.ts`

- [ ] **Step 1: Find the existing login + setup helpers**

Read `e2e/tests/_helpers.ts` (or whichever file the existing tests import from) to confirm the helper names. Look at `e2e/tests/monthly-pass.spec.ts` lines 1–25 for the pattern (`loginViaAPI`, `seedCardWithPass`, etc.). Use the same helpers in this new spec.

```bash
ls e2e/tests/_*.ts e2e/tests/helpers*.ts e2e/tests/fixtures*.ts 2>/dev/null
head -40 e2e/tests/monthly-pass.spec.ts
```

- [ ] **Step 2: Write the spec**

Create `e2e/tests/card-action-form.spec.ts` with this content (adjust helper imports if Step 1 reveals different names — keep test bodies identical):

```typescript
import { test, expect } from '@playwright/test';
import { loginViaAPI, seedCard, baseURL } from './_helpers';

const consoleSink = (page) => {
    const errors: string[] = [];
    page.on('console', (msg) => {
        if (msg.type() === 'error' || msg.type() === 'warning') {
            errors.push(`[${msg.type()}] ${msg.text()}`);
        }
    });
    return errors;
};

test.describe('Card action form — unified Charge / Top-up / Sell pass', () => {
    test('default state shows Charge button; service select includes Monthly pass', async ({ page }) => {
        const errors = consoleSink(page);
        await loginViaAPI(page, 'admin');
        const card = await seedCard(page, { initialCredit: 50 });
        await page.goto(`${baseURL}/staff?q=${encodeURIComponent(card.barcode)}`);
        await page.locator('[data-testid="search-result"]').first().click();

        // Service dropdown options include Monthly pass
        const options = await page.locator('[data-testid="charge-service"] option').allTextContents();
        expect(options.some(o => o.includes('Monthly pass'))).toBe(true);

        // Default Charge button label (not "Sell pass")
        const chargeLabel = await page.locator('[data-testid="charge-submit"]').textContent();
        expect(chargeLabel?.toLowerCase()).not.toContain('predat');
        expect(chargeLabel?.toLowerCase()).not.toContain('sell');

        // valid-until row is hidden
        await expect(page.locator('[data-testid="valid-until-row"]')).toHaveCount(0);

        expect(errors).toEqual([]);
    });

    test('selecting Monthly pass shows date row and flips Charge → Sell pass', async ({ page }) => {
        const errors = consoleSink(page);
        await loginViaAPI(page, 'admin');
        const card = await seedCard(page, { initialCredit: 50 });
        await page.goto(`${baseURL}/staff?q=${encodeURIComponent(card.barcode)}`);
        await page.locator('[data-testid="search-result"]').first().click();

        await page.locator('[data-testid="charge-service"]').selectOption({ label: /Monthly pass/ });

        await expect(page.locator('[data-testid="valid-until-row"]')).toBeVisible();
        const chargeLabel = await page.locator('[data-testid="charge-submit"]').textContent();
        // Slovak default = "Predat"; English fallback = "Sell pass"
        expect(/predat|sell/i.test(chargeLabel ?? '')).toBe(true);

        // Switching back to a normal service hides the date row and restores the label
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        await expect(page.locator('[data-testid="valid-until-row"]')).toHaveCount(0);
        const restored = await page.locator('[data-testid="charge-submit"]').textContent();
        expect(/predat|sell/i.test(restored ?? '')).toBe(false);

        expect(errors).toEqual([]);
    });

    test('Sell pass submits to /api/payments/sell-pass and updates pass banner', async ({ page }) => {
        const errors = consoleSink(page);
        await loginViaAPI(page, 'admin');
        const card = await seedCard(page, { initialCredit: 50 });
        await page.goto(`${baseURL}/staff?q=${encodeURIComponent(card.barcode)}`);
        await page.locator('[data-testid="search-result"]').first().click();

        const sellPassReq = page.waitForRequest(req =>
            req.url().endsWith('/api/payments/sell-pass') && req.method() === 'POST'
        );

        await page.locator('[data-testid="charge-service"]').selectOption({ label: /Monthly pass/ });
        // Amount auto-fills with default price (35.00) on selection — accept it.
        await page.locator('[data-testid="charge-submit"]').click();

        const req = await sellPassReq;
        const body = JSON.parse(req.postData() ?? '{}');
        expect(body.card_id).toBe(card.id);
        expect(body.price).toBe(35.0);
        expect(body.valid_until).toMatch(/^\d{4}-\d{2}-\d{2}$/);

        // Pass banner appears
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();

        expect(errors).toEqual([]);
    });

    test('Charge submits to /api/payments/charge for non-pass service', async ({ page }) => {
        const errors = consoleSink(page);
        await loginViaAPI(page, 'admin');
        const card = await seedCard(page, { initialCredit: 50 });
        await page.goto(`${baseURL}/staff?q=${encodeURIComponent(card.barcode)}`);
        await page.locator('[data-testid="search-result"]').first().click();

        const chargeReq = page.waitForRequest(req =>
            req.url().endsWith('/api/payments/charge') && req.method() === 'POST'
        );

        // Pick the first non-empty option (likely Fitness or Spinning).
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        await page.locator('[data-testid="charge-amount"]').fill('3.50');
        await page.locator('[data-testid="charge-submit"]').click();

        const req = await chargeReq;
        const body = JSON.parse(req.postData() ?? '{}');
        expect(body.card_id).toBe(card.id);
        expect(body.amount).toBe(3.5);
        expect(typeof body.service_id).toBe('number');

        expect(errors).toEqual([]);
    });

    test('Top up submits to /api/cards/topup regardless of selected service', async ({ page }) => {
        const errors = consoleSink(page);
        await loginViaAPI(page, 'admin');
        const card = await seedCard(page, { initialCredit: 50 });
        await page.goto(`${baseURL}/staff?q=${encodeURIComponent(card.barcode)}`);
        await page.locator('[data-testid="search-result"]').first().click();

        const topupReq = page.waitForRequest(req =>
            req.url().endsWith('/api/cards/topup') && req.method() === 'POST'
        );

        await page.locator('[data-testid="charge-service"]').selectOption({ label: /Monthly pass/ });
        await page.locator('[data-testid="charge-amount"]').fill('20.00');
        await page.locator('[data-testid="topup-submit"]').click();

        const req = await topupReq;
        const body = JSON.parse(req.postData() ?? '{}');
        expect(body.card_id).toBe(card.id);
        expect(body.amount).toBe(20.0);
        expect(body).not.toHaveProperty('service_id');

        expect(errors).toEqual([]);
    });
});
```

- [ ] **Step 3: Verify the helpers used in this spec exist**

If Step 1 showed different helper names, the engineer MUST adjust the import line and any helper signatures in this spec to match what `_helpers.ts` actually exports. The helpers `loginViaAPI`, `seedCard`, and `baseURL` are the ones used by other current specs — those are the correct names if `_helpers.ts` matches the rest of the suite. Do NOT invent new helpers; reuse existing ones.

- [ ] **Step 4: Commit**

```bash
git add e2e/tests/card-action-form.spec.ts
git commit -m "test(e2e): card action form — service dropdown, sell pass flip, three submit endpoints"
```

---

## Task 8: New E2E spec — `nav-brand-link.spec.ts`

**Files:**
- Create: `e2e/tests/nav-brand-link.spec.ts`

- [ ] **Step 1: Write the spec**

Create `e2e/tests/nav-brand-link.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { loginViaAPI, baseURL } from './_helpers';

const consoleSink = (page) => {
    const errors: string[] = [];
    page.on('console', (msg) => {
        if (msg.type() === 'error' || msg.type() === 'warning') {
            errors.push(`[${msg.type()}] ${msg.text()}`);
        }
    });
    return errors;
};

test.describe('SpinBike brand link — role-aware target + active state', () => {
    test('staff click on SpinBike → /staff with Desk tab marked active', async ({ page }) => {
        const errors = consoleSink(page);
        await loginViaAPI(page, 'admin');
        await page.goto(`${baseURL}/schedule`);
        await page.locator('[data-testid="brand-link"]').click();

        await expect(page).toHaveURL(new RegExp('/staff$'));
        await expect(page.locator('[data-testid="nav-desk"]')).toHaveAttribute('aria-current', 'page');
        // No other nav item is active.
        for (const id of ['nav-schedule', 'nav-reports', 'nav-settings']) {
            const el = page.locator(`[data-testid="${id}"]`);
            if (await el.count() > 0) {
                await expect(el).toHaveAttribute('aria-current', 'false');
            }
        }
        expect(errors).toEqual([]);
    });

    test('staff visiting / is redirected to /staff (no false Desk-active flicker)', async ({ page }) => {
        const errors = consoleSink(page);
        await loginViaAPI(page, 'admin');
        await page.goto(`${baseURL}/`);
        await expect(page).toHaveURL(new RegExp('/staff$'));
        await expect(page.locator('[data-testid="nav-desk"]')).toHaveAttribute('aria-current', 'page');
        expect(errors).toEqual([]);
    });

    test('logged-out visit to / renders the Schedule page', async ({ page }) => {
        const errors = consoleSink(page);
        await page.goto(`${baseURL}/`);
        // Schedule renders some week-day buttons; brand link still present.
        await expect(page.locator('[data-testid="brand-link"]')).toBeVisible();
        // No adaptive nav for anon
        await expect(page.locator('[data-testid="adaptive-nav"]')).toHaveCount(0);
        expect(errors).toEqual([]);
    });
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/nav-brand-link.spec.ts
git commit -m "test(e2e): SpinBike brand link is role-aware; / redirects staff to Desk"
```

---

## Task 9: New E2E spec — `theme-blue.spec.ts`

**Files:**
- Create: `e2e/tests/theme-blue.spec.ts`

- [ ] **Step 1: Write the spec**

Create `e2e/tests/theme-blue.spec.ts`:

```typescript
import { test, expect } from '@playwright/test';
import { baseURL } from './_helpers';

const consoleSink = (page) => {
    const errors: string[] = [];
    page.on('console', (msg) => {
        if (msg.type() === 'error' || msg.type() === 'warning') {
            errors.push(`[${msg.type()}] ${msg.text()}`);
        }
    });
    return errors;
};

// Convert "rgb(R, G, B)" or "rgba(R, G, B, A)" → [R, G, B].
function parseRgb(s: string): [number, number, number] {
    const m = s.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
    if (!m) throw new Error(`unparseable color: ${s}`);
    return [parseInt(m[1], 10), parseInt(m[2], 10), parseInt(m[3], 10)];
}

function near(a: number, b: number, tol = 4): boolean {
    return Math.abs(a - b) <= tol;
}

test.describe('Theme — vibrant blue', () => {
    test('dark mode primary button background ≈ #60a5fa', async ({ page, browser }) => {
        const errors = consoleSink(page);
        const ctx = await browser.newContext({ colorScheme: 'dark' });
        const p = await ctx.newPage();
        await p.goto(`${baseURL}/login`);
        const bg = await p.locator('button.btn--primary').first().evaluate(el =>
            getComputedStyle(el).backgroundColor
        );
        const [r, g, b] = parseRgb(bg);
        // #60a5fa = (96, 165, 250)
        expect(near(r, 96)).toBe(true);
        expect(near(g, 165)).toBe(true);
        expect(near(b, 250)).toBe(true);
        await ctx.close();
        expect(errors).toEqual([]);
    });

    test('light mode primary button background ≈ #2563eb', async ({ browser }) => {
        const ctx = await browser.newContext({ colorScheme: 'light' });
        const p = await ctx.newPage();
        const errors = consoleSink(p);
        await p.goto(`${baseURL}/login`);
        const bg = await p.locator('button.btn--primary').first().evaluate(el =>
            getComputedStyle(el).backgroundColor
        );
        const [r, g, b] = parseRgb(bg);
        // #2563eb = (37, 99, 235)
        expect(near(r, 37)).toBe(true);
        expect(near(g, 99)).toBe(true);
        expect(near(b, 235)).toBe(true);
        await ctx.close();
        expect(errors).toEqual([]);
    });
});
```

- [ ] **Step 2: Commit**

```bash
git add e2e/tests/theme-blue.spec.ts
git commit -m "test(e2e): primary buttons render vibrant blue in dark and light"
```

---

## Task 10: Update `redesign-sheets.spec.ts` — drop sell-pass sheet tests

**Files:**
- Modify: `e2e/tests/redesign-sheets.spec.ts`

- [ ] **Step 1: Inspect the file**

```bash
sed -n '30,50p;95,130p' e2e/tests/redesign-sheets.spec.ts
```

Identify the three test bodies that reference `[data-testid="sell-pass-btn"]` or `[data-testid="sheet-sell-pass"]` (lines 34, 100, 118 according to the spec). Note their `test('...')` names so you can remove the entire test block, not just the lines.

- [ ] **Step 2: Delete the three sell-pass tests**

For each of the three tests:
- Find the line `test('sell pass sheet ...', ...)` (or the title containing "sell pass"),
- Delete from that line through the matching closing `});` of that test only.
- Keep all other tests in the file unchanged.

- [ ] **Step 3: Verify no orphan references remain**

```bash
grep -n "sell-pass\|sell_pass\|sheet-sell-pass" e2e/tests/redesign-sheets.spec.ts
```

Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add e2e/tests/redesign-sheets.spec.ts
git commit -m "test(e2e): drop sell-pass sheet tests (sheet replaced by dropdown flow)"
```

---

## Task 11: Rewrite `monthly-pass.spec.ts` to drive the unified form

**Files:**
- Modify: `e2e/tests/monthly-pass.spec.ts`

- [ ] **Step 1: Read the current file**

```bash
cat e2e/tests/monthly-pass.spec.ts
```

Identify: helper imports, login pattern, card-seed pattern, the test that was `'sell pass → banner appears → visit logs 0 EUR row'`. Preserve the helpers and outer `describe`/login setup.

- [ ] **Step 2: Replace the single sell-pass test body**

Find the test body that opens `[data-testid="sell-pass-btn"]` and the `[data-testid="sheet-sell-pass"]` modal (around lines 24–34 in the existing file). Replace ONLY that test's body with the new flow — the unified form is now in the card-action panel, not a sheet:

```typescript
test('sell pass → banner appears → visit logs 0 EUR row', async ({ page }) => {
    // <Existing setup that places the card and lands on the staff dashboard.>
    // <Existing locator that opens the card detail.>

    // Pick Monthly pass from the unified service dropdown
    await page.locator('[data-testid="charge-service"]').selectOption({ label: /Monthly pass/ });
    // Amount auto-fills with default price; we'll accept it.
    // Date defaults to today + 30 days; accept the default.
    await page.locator('[data-testid="charge-submit"]').click();

    // Banner appears
    await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();

    // Quick log-visit chip is visible (only renders when an active pass exists)
    await expect(page.locator('[data-testid="log-visit-btn"]').first()).toBeVisible();
    await page.locator('[data-testid="log-visit-btn"]').first().click();

    // History tab shows a 0 € visit row (existing assertion logic; keep as-is)
    await page.locator('[data-testid="tab-history"]').click();
    await expect(page.locator('text=0,00').first()).toBeVisible();
});
```

Keep the surrounding `describe`, login, and seeding code unchanged. The shape above is a drop-in replacement for the old `sell-pass-btn` flow ONLY; do not touch other tests in the file.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/monthly-pass.spec.ts
git commit -m "test(e2e): monthly-pass — sell via dropdown, not modal"
```

---

## Task 12: Rewrite `sell-pass-price-input.spec.ts`

**Files:**
- Modify: `e2e/tests/sell-pass-price-input.spec.ts`

- [ ] **Step 1: Read the current file**

```bash
cat e2e/tests/sell-pass-price-input.spec.ts
```

It has two tests today (per the earlier grep): a positive flow ("sell pass via modal") and an "empty price" error case. The unified form removes the modal — both tests need rewriting.

- [ ] **Step 2: Replace the entire test bodies**

Keep the `import` lines, the `consoleSink` helper if any, the `describe` block, and any `loginViaAPI`/seeding setup. Replace both test bodies with:

```typescript
test('sell pass via dropdown — typed price flows through to /api/payments/sell-pass', async ({ page }) => {
    // <Existing setup: login as admin, seed a SellPass card, navigate, open detail.>

    const sellPassReq = page.waitForRequest(req =>
        req.url().endsWith('/api/payments/sell-pass') && req.method() === 'POST'
    );

    await page.locator('[data-testid="charge-service"]').selectOption({ label: /Monthly pass/ });
    // Overwrite the auto-filled price.
    const amountInput = page.locator('[data-testid="charge-amount"]');
    await amountInput.fill('27.50');
    await page.locator('[data-testid="charge-submit"]').click();

    const req = await sellPassReq;
    const body = JSON.parse(req.postData() ?? '{}');
    expect(body.price).toBe(27.5);
});

test('empty amount keeps Sell pass disabled and surfaces an error', async ({ page }) => {
    // <Existing setup>

    await page.locator('[data-testid="charge-service"]').selectOption({ label: /Monthly pass/ });
    // Clear the auto-filled price.
    await page.locator('[data-testid="charge-amount"]').fill('');
    await page.locator('[data-testid="charge-submit"]').click();

    // Inline error appears; no request fires.
    await expect(page.locator('.alert-error')).toBeVisible();
});
```

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/sell-pass-price-input.spec.ts
git commit -m "test(e2e): sell-pass price input — drive the unified form"
```

---

## Task 13: Update `monthly-pass-expired.spec.ts`

**Files:**
- Modify: `e2e/tests/monthly-pass-expired.spec.ts:29`

- [ ] **Step 1: Inspect the assertion**

```bash
sed -n '20,40p' e2e/tests/monthly-pass-expired.spec.ts
```

Find the line that asserts `[data-testid="sell-pass-btn"]` is visible (line 29).

- [ ] **Step 2: Replace with a dropdown-option assertion**

Replace:

```typescript
        await expect(page.locator('[data-testid="sell-pass-btn"]')).toBeVisible();
```

with:

```typescript
        // After the prior pass expired, the user should still be able to sell a new one
        // via the unified service dropdown.
        const opts = await page.locator('[data-testid="charge-service"] option').allTextContents();
        expect(opts.some(o => /Monthly pass/.test(o))).toBe(true);
```

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/monthly-pass-expired.spec.ts
git commit -m "test(e2e): expired-pass — assert Monthly pass option, not legacy sell-pass-btn"
```

---

## Task 14: Update `redesign-theme.spec.ts` color sample

**Files:**
- Modify: `e2e/tests/redesign-theme.spec.ts`

- [ ] **Step 1: Inspect**

```bash
cat e2e/tests/redesign-theme.spec.ts
```

Look for any RGB / hex assertion that references the old green primary (e.g., `(34, 197, 94)`, `#22c55e`, `(22, 163, 74)`, `#16a34a`).

- [ ] **Step 2: Update the expected colors**

Replace any green expectation:
- Dark theme: `(34, 197, 94)` → `(96, 165, 250)`
- Light theme: `(22, 163, 74)` → `(37, 99, 235)`

If the spec checks `--pass` (lime), update:
- Dark: `(132, 204, 22)` → `(251, 191, 36)` (#fbbf24 amber-400)
- Light: `(101, 163, 13)` → `(217, 119, 6)` (#d97706 amber-600)

If the spec only checks the brand background and uses a `near()` tolerance, the existing tolerance (typically 2–4 channels) is fine.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/redesign-theme.spec.ts
git commit -m "test(e2e): theme spec — expect blue brand and amber pass tints"
```

---

## Task 15: Push and watch CI

**Files:**
- None (push + CI monitoring)

- [ ] **Step 1: Confirm formatting one last time**

```bash
cargo fmt --all --check
```

Expected: no output (clean).

- [ ] **Step 2: Push**

```bash
git push origin dev
```

- [ ] **Step 3: Identify the run**

```bash
gh run list --branch dev --limit 1
```

Note the run ID.

- [ ] **Step 4: Monitor the run to terminal state**

Use the canonical pattern from `ci-monitoring.md`:

```bash
# Replace <RUN_ID> with the value from Step 3.
sleep 300 && gh run view <RUN_ID> --json status,conclusion,jobs
```

Run this in the background. When it returns, inspect the JSON. If `status` is `in_progress`, repeat with another 300s sleep. If `conclusion` is `failure`, get the failed-job logs:

```bash
gh run view <RUN_ID> --log-failed
```

Investigate, fix root cause (no timeout band-aids, no continue-on-error), batch the fix into one commit, push once, and monitor the new run. Repeat until `conclusion: success` for ALL jobs (lint, test, build-wasm, e2e, mutation-test, deploy-dev, smoke-dev, check-version-bump). Deploy-prod and smoke-prod do not run on `dev`.

- [ ] **Step 5: Open the PR**

Once the dev branch is green:

```bash
gh pr create --title "Card management rework + blue theme (v0.11.0)" --body "$(cat <<'EOF'
## Summary

- One amount input + one service dropdown + two equal-weight buttons (Top up / Charge) replace the prior Charge + Top-up + huge Sell-pass triple. Monthly pass is a regular option in the dropdown; picking it reveals a date row and flips the Charge button to Sell pass.
- Brand color repainted from green to vibrant blue (#2563eb light / #60a5fa dark). `--pass` retoned to amber so the active-pass badge stays distinct.
- SpinBike brand link is role-aware (staff → /staff, customers → /). `/` redirects staff/admin to the Desk so the active nav matches the visible page.

Closes the long-running "card detail is a mess" complaint after three prior reworks.

## Test plan

- [x] New: card-action-form.spec.ts — service dropdown contents, sell-pass label flip, three submit endpoints
- [x] New: nav-brand-link.spec.ts — role-aware brand link + / redirect
- [x] New: theme-blue.spec.ts — primary button RGB samples in dark and light
- [x] Rewritten: monthly-pass, sell-pass-price-input
- [x] Updated: monthly-pass-expired, redesign-sheets, redesign-theme
- [x] All Playwright specs assert zero browser console errors
- [x] cargo fmt clean
- [x] CI green (lint, test, build-wasm, e2e, mutation-test, deploy-dev, smoke-dev)

## Version

VERSION 0.10.0 → **0.11.0** — user-visible UI rework, no schema or API changes.
EOF
)"
```

- [ ] **Step 6: Verify PR mergeable=clean**

```bash
gh pr view --json number --jq '.number' | xargs -I{} gh api repos/:owner/:repo/pulls/{} --jq '{mergeable, mergeable_state}'
```

Expected: `{ "mergeable": true, "mergeable_state": "clean" }`. If `behind`, sync from main; if `dirty`, resolve conflicts; if `blocked`, fix the blocking check (do NOT bypass branch protection).

---

## Self-review checklist

(Run mentally before reporting completion.)

- Spec coverage:
  - Brand color swap → Task 1 ✅
  - Unified ActionForm → Task 2 ✅
  - card_panel rewires + delete obsolete files → Task 3 ✅
  - Pass banner unchanged (per spec) — no task ✅
  - Brand-link role-aware → Task 4 ✅
  - desk_active drop `path == "/"` → Task 5 ✅
  - `/` role-aware redirect → Task 6 ✅
  - New E2E for action form → Task 7 ✅
  - New E2E for nav brand link → Task 8 ✅
  - New E2E for theme color → Task 9 ✅
  - Existing E2E updates → Tasks 10, 11, 12, 13, 14 ✅
  - Push + CI green + PR → Task 15 ✅
- Type/property names consistent: `selected_service_id` (sig), `service_ref` (NodeRef), `amount_ref` (NodeRef), `valid_until` / `set_valid_until` (sig pair), `loading` / `set_loading`, `err` / `set_err`. All match between definitions and usages within Task 2.
- Component prop names: `card`, `services`, `set_selected`, `set_msg`, `set_txn_refresh` — Task 2 defines them, Task 3 uses them with the same names.
- Test selectors: `charge-service`, `charge-amount`, `charge-submit`, `topup-submit`, `sell-pass-date`, `valid-until-row`, `log-visit-btn`, `pass-banner-active`, `brand-link`, `nav-desk` — all introduced in component code (Task 2 / Task 4) and consumed consistently in test code (Tasks 7–14).
