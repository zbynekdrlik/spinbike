//! Staff card dashboard: fast search + inline action panel.
//!
//! Replaces the old /staff/cards (list-dump) and /staff/payments (separate) pages.
//! Flow: type in search → dropdown → pick result → quick top-up / charge / block / edit.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;
use crate::components::{PersistentToggles, UpcomingClasses};
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
struct CardPass {
    valid_until: chrono::NaiveDate,
    days_remaining: i32,
    transaction_id: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct CardInfo {
    id: i64,
    barcode: String,
    #[allow(dead_code)]
    user_id: Option<i64>,
    blocked: bool,
    credit: f64,
    #[allow(dead_code)]
    allow_debit: bool,
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    last_name: Option<String>,
    #[serde(default)]
    company: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    pass: Option<CardPass>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ServiceInfo {
    id: i64,
    name: String,
    default_price: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PaymentResp {
    #[allow(dead_code)]
    transaction_id: i64,
    new_credit: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct TxnInfo {
    id: i64,
    #[allow(dead_code)]
    card_id: Option<i64>,
    amount: f64,
    action: String,
    created_at: String,
    #[serde(default)]
    service_name: Option<String>,
    #[serde(default)]
    valid_until: Option<chrono::NaiveDate>,
    #[serde(default)]
    deleted_at: Option<String>,
}

const QUICK_TOPUP: [f64; 4] = [5.0, 10.0, 20.0, 50.0];

fn pass_is_active(card: &CardInfo) -> bool {
    card.pass
        .as_ref()
        .map(|p| p.days_remaining >= 0)
        .unwrap_or(false)
}

fn full_name(c: &CardInfo) -> String {
    let f = c.first_name.clone().unwrap_or_default();
    let l = c.last_name.clone().unwrap_or_default();
    let combined = format!("{f} {l}").trim().to_string();
    if combined.is_empty() {
        "—".into()
    } else {
        combined
    }
}

#[component]
pub fn DashboardPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (query, set_query) = signal(String::new());
    let (results, set_results) = signal(Vec::<CardInfo>::new());
    let (searching, set_searching) = signal(false);
    let (selected, set_selected) = signal(None::<CardInfo>);
    let (services, set_services) = signal(Vec::<ServiceInfo>::new());
    let (show_activate, set_show_activate) = signal(false);
    let (msg, set_msg) = signal(String::new());
    let (err, set_err) = signal(String::new());
    // Keyboard-driven highlight within the search dropdown. 0 means "first
    // suggestion" — so typing + Enter picks the top match without a click.
    let (highlighted_idx, set_highlighted_idx) = signal(0usize);

    // Explicit ref so we can restore focus after pick_card and after the
    // action panel closes. HTML `autofocus` only runs once on mount.
    let search_input_ref = NodeRef::<leptos::html::Input>::new();

    Effect::new(move |_| {
        if let Some(el) = search_input_ref.get() {
            let _ = el.focus();
        }
    });

    // Load services once (for charge dropdown).
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(svc) = api::get::<Vec<ServiceInfo>>("/api/admin/services").await {
                set_services.set(svc);
            }
        });
    });

    // Debounced search. We track the query signal and re-issue on each change
    // after a short delay; in-flight requests become stale when the signal
    // changes again, so we drop their result if the query moved on.
    Effect::new(move |_| {
        let q = query.get();
        set_msg.set(String::new());
        // Every new query resets the keyboard highlight to row 0. Without
        // this, a prior mouseenter or a stale `highlighted_idx` from the
        // last search can survive into the new dropdown.
        set_highlighted_idx.set(0);
        if q.trim().is_empty() {
            set_results.set(Vec::new());
            set_searching.set(false);
            return;
        }
        set_searching.set(true);
        let q_at_start = q.clone();
        spawn_local(async move {
            // 250ms debounce via a gloo timer.
            gloo_timers::future::TimeoutFuture::new(250).await;
            // If the query changed while we were waiting, skip this fetch.
            if query.get_untracked() != q_at_start {
                return;
            }
            let encoded = urlencoding_light(&q_at_start);
            match api::get::<Vec<CardInfo>>(&format!("/api/cards/search?q={encoded}&limit=10"))
                .await
            {
                Ok(list) => {
                    if query.get_untracked() == q_at_start {
                        set_results.set(list);
                    }
                }
                Err(e) => set_err.set(e),
            }
            if query.get_untracked() == q_at_start {
                set_searching.set(false);
            }
        });
    });

    let on_search_input = move |ev: web_sys::Event| {
        let value = ev
            .target()
            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
            .map(|el| el.value())
            .unwrap_or_default();
        set_query.set(value);
    };

    let clear_selection = move |_| {
        set_selected.set(None);
        set_msg.set(String::new());
        if let Some(el) = search_input_ref.get() {
            let _ = el.focus();
        }
    };

    // Shared "pick this card" behaviour — used both by click on a dropdown
    // row and by pressing Enter while a row is highlighted. Signals are Copy,
    // so this closure is Copy + Fn.
    let pick_card = move |card: CardInfo| {
        set_selected.set(Some(card));
        set_query.set(String::new());
        set_results.set(Vec::new());
        set_err.set(String::new());
        // Keep the keyboard-first workflow alive: the user should be able to
        // start typing the next card's name immediately without reaching for
        // the mouse.
        if let Some(el) = search_input_ref.get() {
            let _ = el.focus();
        }
    };

    let on_search_keydown = move |ev: web_sys::KeyboardEvent| {
        let list = results.get_untracked();
        let len = list.len();
        match ev.key().as_str() {
            "ArrowDown" if len > 0 => {
                ev.prevent_default();
                set_highlighted_idx.update(|i| *i = (*i + 1) % len);
            }
            "ArrowUp" if len > 0 => {
                ev.prevent_default();
                set_highlighted_idx.update(|i| *i = (*i + len - 1) % len);
            }
            "Enter" if len > 0 => {
                ev.prevent_default();
                let idx = highlighted_idx.get_untracked().min(len - 1);
                if let Some(card) = list.get(idx).cloned() {
                    pick_card(card);
                }
            }
            "Escape" => {
                set_query.set(String::new());
                set_results.set(Vec::new());
            }
            _ => {}
        }
    };

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "card_dashboard")}</h1>

        <div class="card mb-2">
            <input
                type="search"
                class="form-control"
                node_ref=search_input_ref
                inputmode="search"
                prop:value=move || query.get()
                placeholder=move || i18n::t(lang.get(), "search_cards_placeholder")
                on:input=on_search_input
                on:keydown=on_search_keydown
                style="font-size:1.1rem;padding:12px"
            />
            {move || {
                if searching.get() {
                    view! { <div class="text-muted mt-1" style="font-size:0.8rem">{i18n::t(lang.get(), "searching")}</div> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
            {move || {
                let list = results.get();
                if list.is_empty() {
                    return view! { <span></span> }.into_any();
                }
                let items: Vec<_> = list.into_iter().enumerate().map(|(idx, c)| {
                    let card_for_pick = c.clone();
                    let name = full_name(&c);
                    let barcode = c.barcode.clone();
                    let tail_len = barcode.len().min(4);
                    let tail = &barcode[barcode.len() - tail_len..];
                    let tail_str = tail.to_string();
                    let company = c.company.clone().unwrap_or_default();
                    let credit_val = c.credit;
                    let credit = format!("{:.2} €", credit_val);
                    let credit_class = if credit_val < 0.0 { "credit-negative" } else { "" };
                    let is_blocked = c.blocked;
                    view! {
                        <div
                            class=move || {
                                if highlighted_idx.get() == idx {
                                    "search-result search-result-active"
                                } else {
                                    "search-result"
                                }
                            }
                            data-testid="search-result"
                            style="display:flex;justify-content:space-between;align-items:center;padding:10px;border-bottom:1px solid var(--border);cursor:pointer;gap:8px"
                            on:click={
                                let card = card_for_pick.clone();
                                move |_| pick_card(card.clone())
                            }
                        >
                            <div>
                                <div style="font-weight:600">
                                    {name}
                                    {if is_blocked {
                                        view! { <span class="badge badge-full" style="margin-left:8px;font-size:0.7rem">{i18n::t(lang.get(), "blocked")}</span> }.into_any()
                                    } else { view! {}.into_any() }}
                                </div>
                                <div class="text-muted" style="font-size:0.8rem">
                                    <code>{format!("…{tail_str}")}</code>
                                    {if !company.is_empty() { format!(" · {company}") } else { String::new() }}
                                </div>
                            </div>
                            <div class=credit_class style="font-weight:600;white-space:nowrap">{credit}</div>
                        </div>
                    }
                }).collect();
                view! { <div class="mt-1" style="border-top:1px solid var(--border)">{items}</div> }.into_any()
            }}
            {move || {
                let q = query.get();
                if !q.trim().is_empty() && !searching.get() && results.get().is_empty() {
                    let on_activate = move |_| {
                        set_show_activate.set(true);
                        set_selected.set(None);
                    };
                    view! {
                        <div class="text-center mt-2">
                            <p class="text-muted">{i18n::t(lang.get(), "no_matches")}</p>
                            <button class="btn btn-sm btn-primary" on:click=on_activate>
                                {i18n::t(lang.get(), "activate_new_card")}
                            </button>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
        </div>

        {move || {
            let e = err.get();
            if !e.is_empty() {
                view! { <div class="alert alert-error">{e}</div> }.into_any()
            } else { view! { <span></span> }.into_any() }
        }}

        {move || {
            let m = msg.get();
            if !m.is_empty() {
                view! { <div class="alert alert-success">{m}</div> }.into_any()
            } else { view! { <span></span> }.into_any() }
        }}

        {move || match selected.get() {
            None => view! { <span></span> }.into_any(),
            Some(c) => view! {
                <ActionPanel
                    card=c
                    services=services
                    set_selected=set_selected
                    set_msg=set_msg
                    on_close=Callback::new(clear_selection)
                />
            }.into_any()
        }}

        <div class="mt-2">
            <button
                class="btn btn-sm btn-outline"
                on:click=move |_| set_show_activate.update(|v| *v = !*v)
            >
                {move || if show_activate.get() {
                    i18n::t(lang.get(), "hide_activate")
                } else {
                    i18n::t(lang.get(), "activate_new_card")
                }}
            </button>
        </div>

        {move || {
            if show_activate.get() {
                view! { <ActivateCardForm set_selected=set_selected set_msg=set_msg set_show_activate=set_show_activate /> }.into_any()
            } else { view! { <span></span> }.into_any() }
        }}
    }
}

// tiny percent-encoder for the search query (avoids pulling urlencoding crate
// just for this — we only need to escape a handful of chars).
fn urlencoding_light(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            ' ' => out.push_str("%20"),
            _ => {
                let mut buf = [0u8; 4];
                for b in ch.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

#[component]
fn PassBanner(
    pass: Option<CardPass>,
    barcode: String,
    set_selected: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let Some(p) = pass else {
        return view! { <div></div> }.into_any();
    };

    let tx_id = p.transaction_id;
    let current_date = p.valid_until;
    let (editing, set_editing) = signal(false);
    let (draft, set_draft) = signal(current_date);
    let (edit_err, set_edit_err) = signal(String::new());
    let (saving, set_saving) = signal(false);

    let barcode_for_refresh = barcode.clone();
    let on_save = move |_| {
        let vu = draft.get();
        let barcode = barcode_for_refresh.clone();
        set_edit_err.set(String::new());
        set_saving.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                valid_until: chrono::NaiveDate,
            }
            match api::patch::<Req, serde_json::Value>(
                &format!("/api/transactions/{tx_id}/valid-until"),
                &Req { valid_until: vu },
            )
            .await
            {
                Ok(_) => {
                    // Refresh the card so the banner picks up the new date and days_remaining.
                    match api::get::<CardInfo>(&format!("/api/cards/lookup/{barcode}")).await {
                        Ok(c) => {
                            set_selected.set(Some(c));
                            set_editing.set(false);
                        }
                        Err(e) => set_edit_err.set(e),
                    }
                }
                Err(e) => set_edit_err.set(e),
            }
            set_saving.set(false);
        });
    };

    let on_cancel = move |_| {
        set_edit_err.set(String::new());
        set_draft.set(current_date);
        set_editing.set(false);
    };

    let on_edit_click = move |_| {
        set_draft.set(current_date);
        set_edit_err.set(String::new());
        set_editing.set(true);
    };

    let on_date_input = move |ev: web_sys::Event| {
        let s = event_target_value(&ev);
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            set_draft.set(d);
        }
    };

    let date_str = current_date.format("%d.%m.%Y").to_string();
    let is_active = p.days_remaining >= 0;
    let days = p.days_remaining;
    let days_ago = -p.days_remaining;

    let banner_class = if is_active {
        "pass-banner pass-banner-ok"
    } else {
        "pass-banner pass-banner-expired"
    };
    let banner_testid = if is_active {
        "pass-banner-active"
    } else {
        "pass-banner-expired"
    };

    let title_view = if is_active {
        let date_str = date_str.clone();
        view! {
            <>
                {move || i18n::t(lang.get(), "pass_valid_until")}" "{date_str.clone()}
            </>
        }
        .into_any()
    } else {
        view! {
            <>
                {move || i18n::t(lang.get(), "pass_expired")}" "{days_ago}" "
                {move || i18n::t(lang.get(), "pass_days_ago")}
            </>
        }
        .into_any()
    };

    let sub_view = if is_active {
        view! {
            <>
                {days}" "{move || i18n::t(lang.get(), "pass_days_remaining")}
            </>
        }
        .into_any()
    } else {
        let date_str = date_str.clone();
        view! {
            <>
                {move || i18n::t(lang.get(), "pass_last_valid_until")}" "{date_str.clone()}
            </>
        }
        .into_any()
    };

    view! {
        <div class=banner_class data-testid=banner_testid>
            <div class="pass-banner-title" style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
                <span>{title_view}</span>
                <button
                    class="btn btn-sm btn-outline"
                    data-testid="pass-date-edit"
                    title=move || i18n::t(lang.get(), "edit_pass_date")
                    on:click=on_edit_click
                    style:display=move || if editing.get() { "none" } else { "inline-block" }
                    style="padding:2px 8px;font-size:0.85rem"
                >"\u{270E}"</button>
            </div>
            <div class="pass-banner-sub">{sub_view}</div>
            <div
                class="mt-1"
                style:display=move || if editing.get() { "flex" } else { "none" }
                style="gap:6px;flex-wrap:wrap;align-items:center"
            >
                <input
                    type="date"
                    data-testid="pass-date-input"
                    prop:value=move || draft.get().format("%Y-%m-%d").to_string()
                    on:input=on_date_input
                />
                <button
                    class="btn btn-sm btn-primary"
                    data-testid="pass-date-save"
                    disabled=move || saving.get()
                    on:click=on_save
                >
                    {move || i18n::t(lang.get(), "save")}
                </button>
                <button
                    class="btn btn-sm btn-outline"
                    disabled=move || saving.get()
                    on:click=on_cancel
                >
                    {move || i18n::t(lang.get(), "cancel")}
                </button>
            </div>
            {move || {
                let e = edit_err.get();
                if e.is_empty() {
                    view! { <div></div> }.into_any()
                } else {
                    view! { <div class="alert alert-error mt-1" style="font-size:0.85rem">{e}</div> }.into_any()
                }
            }}
        </div>
    }
    .into_any()
}

fn event_target_value(ev: &web_sys::Event) -> String {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default()
}

#[component]
fn SellPassModal(
    card: CardInfo,
    set_selected: WriteSignal<Option<CardInfo>>,
    show: ReadSignal<bool>,
    set_show: WriteSignal<bool>,
    /// Default price pre-fetched from the services list to avoid hardcoding 35.00.
    monthly_pass_price: f64,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let today = chrono::Local::now().date_naive();
    // Default valid_until: max(current valid_until, today) + 30 days.
    let default_date = card
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

    // Use the price looked up from the services table rather than a hardcoded literal.
    let (price, set_price) = signal(monthly_pass_price);
    let (valid_until, set_valid_until) = signal(default_date);
    let (err, set_err) = signal(String::new());

    let card_id = card.id;

    let on_confirm = move |_| {
        let p = price.get();
        let vu = valid_until.get();
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
                    price: p,
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
                    set_show.set(false);
                }
                Err(e) => set_err.set(format!("{e}")),
            }
        });
    };

    view! {
        {move || {
            if !show.get() {
                return view! { <div></div> }.into_any();
            }
            view! {
                <div class="modal-overlay" data-testid="sell-pass-modal">
                    <div class="modal">
                        <h3>{move || i18n::t(lang.get(), "sell_monthly_pass")}</h3>
                        <label>{move || i18n::t(lang.get(), "modal_price")}</label>
                        <input
                            type="number"
                            step="0.01"
                            min="0"
                            data-testid="sell-pass-price"
                            prop:value=move || format!("{:.2}", price.get())
                            on:input=move |ev| {
                                let ev: web_sys::Event = ev.into();
                                if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                                    set_price.set(v);
                                }
                            }
                        />
                        <label>{move || i18n::t(lang.get(), "modal_valid_until")}</label>
                        <input
                            type="date"
                            data-testid="sell-pass-date"
                            prop:value=move || valid_until.get().format("%Y-%m-%d").to_string()
                            on:input=move |ev| {
                                let ev: web_sys::Event = ev.into();
                                let s = event_target_value(&ev);
                                if let Ok(d) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                                    set_valid_until.set(d);
                                }
                            }
                        />
                        {move || {
                            if err.get().is_empty() {
                                view! { <div></div> }.into_any()
                            } else {
                                view! { <div class="alert alert-error">{move || err.get()}</div> }.into_any()
                            }
                        }}
                        <div class="modal-buttons">
                            <button class="btn" on:click=move |_| set_show.set(false)>
                                {move || i18n::t(lang.get(), "modal_cancel")}
                            </button>
                            <button
                                class="btn btn-primary"
                                data-testid="sell-pass-confirm"
                                on:click=on_confirm
                            >
                                {move || i18n::t(lang.get(), "modal_confirm")}
                            </button>
                        </div>
                    </div>
                </div>
            }
            .into_any()
        }}
    }
}

#[component]
fn ActionPanel(
    card: CardInfo,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    #[prop(into)] on_close: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (txns, set_txns) = signal(Vec::<TxnInfo>::new());
    let (show_edit, set_show_edit) = signal(false);
    let (show_sell_pass, set_show_sell_pass) = signal(false);
    // Counter incremented after a log-visit to re-trigger the history fetch.
    let (txn_refresh, set_txn_refresh) = signal(0u32);
    // Counter driving UpcomingClasses + PersistentToggles refetches after a
    // book/cancel/toggle action updates underlying booking state.
    let upc_tick = RwSignal::new(0u32);
    // Active tab for the card detail (history default, then upcoming, persistent).
    let (tab, set_tab) = signal("history".to_string());

    // Transaction history is the most-read piece of card context, so load it
    // as soon as the panel mounts and always render it below the actions.
    let card_id_for_txn = card.id;
    Effect::new(move |_| {
        let _ = txn_refresh.get(); // reactive dependency — re-runs on increment
        spawn_local(async move {
            if let Ok(t) =
                api::get::<Vec<TxnInfo>>(&format!("/api/cards/{card_id_for_txn}/transactions"))
                    .await
            {
                set_txns.set(t);
            }
        });
    });

    let card_id = card.id;
    let barcode = card.barcode.clone();
    let name = full_name(&card);
    let credit = card.credit;
    let is_blocked = card.blocked;
    let company = card.company.clone().unwrap_or_default();
    let phone = card.phone.clone().unwrap_or_default();
    let card_pass = card.pass.clone();
    let card_for_edit = card.clone();
    let card_for_modal = card.clone();

    view! {
        <div class="card mb-2" data-testid="action-panel">
            <div class="card-header" style="display:flex;justify-content:space-between;align-items:flex-start;gap:8px">
                <div>
                    <div class="card-title" style="font-size:1.1rem">{name}</div>
                    <div class="text-muted" style="font-size:0.85rem">
                        <code>{barcode.clone()}</code>
                        {if !company.is_empty() { format!(" · {company}") } else { String::new() }}
                        {if !phone.is_empty() { format!(" · {phone}") } else { String::new() }}
                    </div>
                </div>
                <button class="btn btn-sm btn-outline" on:click=move |e| on_close.run(e) title="close">"\u{2715}"</button>
            </div>

            <PassBanner pass=card_pass barcode=barcode.clone() set_selected=set_selected />

            <div
                class=if credit < 0.0 { "credit-negative" } else { "" }
                style="font-size:1.4rem;font-weight:700;margin:8px 0"
                data-testid="card-credit"
            >
                {format!("{credit:.2} €")}
                {if is_blocked {
                    view! { <span class="badge badge-full" style="margin-left:8px;font-size:0.75rem">{i18n::t(lang.get(), "blocked")}</span> }.into_any()
                } else { view! {}.into_any() }}
            </div>

            // Ordered by actual staff usage frequency: charge (pay-for-service)
            // is the most-common action, then top-up. Edit/block stay secondary.
            <ChargeSection
                card_id=card_id
                services=services
                set_selected=set_selected
                set_msg=set_msg
                pass_active=pass_is_active(&card)
                set_txn_refresh=set_txn_refresh
            />
            <TopupSection card_id=card_id set_selected=set_selected set_msg=set_msg />

            <div class="mt-2">
                <button
                    class="btn btn-pass"
                    data-testid="sell-pass-btn"
                    on:click=move |_| set_show_sell_pass.set(true)
                >
                    {move || {
                        let price = services.get().iter()
                            .find(|s| s.name == "Monthly pass")
                            .map(|s| s.default_price)
                            .unwrap_or(35.0);
                        format!("{} {:.2}", i18n::t(lang.get(), "sell_monthly_pass"), price)
                    }}
                </button>
            </div>

            <div class="flex gap-1 mt-2" style="flex-wrap:wrap">
                <button
                    class="btn btn-sm btn-outline"
                    on:click=move |_| set_show_edit.update(|v| *v = !*v)
                >
                    {move || i18n::t(lang.get(), "edit")}
                </button>
                <BlockButton card_id=card_id blocked=is_blocked set_selected=set_selected set_msg=set_msg />
            </div>

            {move || {
                if show_edit.get() {
                    view! { <EditInfoForm card=card_for_edit.clone() set_selected=set_selected set_msg=set_msg set_show_edit=set_show_edit /> }.into_any()
                } else { view! { <span></span> }.into_any() }
            }}

            <SellPassModal
                card=card_for_modal.clone()
                set_selected=set_selected
                show=show_sell_pass
                set_show=set_show_sell_pass
                monthly_pass_price=services.get_untracked().iter()
                    .find(|s| s.name == "Monthly pass")
                    .map(|s| s.default_price)
                    .unwrap_or(35.0)
            />

            <div class="tabbar">
                <button
                    class=move || if tab.get() == "history" { "tab tab--active" } else { "tab" }
                    on:click=move |_| set_tab.set("history".to_string())
                    data-testid="tab-history"
                >
                    {move || i18n::t(lang.get(), "tab_history")}
                </button>
                <button
                    class=move || if tab.get() == "upcoming" { "tab tab--active" } else { "tab" }
                    on:click=move |_| set_tab.set("upcoming".to_string())
                    data-testid="tab-upcoming"
                >
                    {move || i18n::t(lang.get(), "tab_upcoming")}
                </button>
                <button
                    class=move || if tab.get() == "persistent" { "tab tab--active" } else { "tab" }
                    on:click=move |_| set_tab.set("persistent".to_string())
                    data-testid="tab-persistent"
                >
                    {move || i18n::t(lang.get(), "tab_persistent")}
                </button>
            </div>
            <div class="tab-body">
                {move || {
                    let t = tab.get();
                    match t.as_str() {
                        "history" => view! {
                            <div class="mt-2">
                                <h3 style="font-size:0.95rem;margin-bottom:8px">{move || i18n::t(lang.get(), "transaction_history")}</h3>
                                {move || {
                                    let t = txns.get();
                                    if t.is_empty() {
                                        return view! { <p class="text-muted">{i18n::t(lang.get(), "no_transactions_card")}</p> }.into_any();
                                    }
                                    let rows: Vec<_> = t.iter().map(|tx| {
                                        let date = format_sk_datetime(&tx.created_at);
                                        let action = tx.action.clone();
                                        let until_suffix = tx.valid_until
                                            .map(|d| format!(" · until {}", d.format("%d.%m")))
                                            .unwrap_or_default();
                                        let service = tx.service_name.clone().unwrap_or_else(|| "—".into());
                                        let amount = format!("{:+.2}", tx.amount);
                                        let is_voided = tx.deleted_at.is_some();
                                        let row_class = if is_voided {
                                            "txn-row--voided"
                                        } else if tx.action == "visit" {
                                            "txn-row-visit"
                                        } else {
                                            "txn-row"
                                        };
                                        let tx_id = tx.id;
                                        let voided_tag = if is_voided {
                                            view! {
                                                <span class="txn-voided-tag">{move || i18n::t(lang.get(), "voided")}</span>
                                            }.into_any()
                                        } else {
                                            view! { <span></span> }.into_any()
                                        };
                                        let action_cell = if is_voided {
                                            view! { <td></td> }.into_any()
                                        } else {
                                            let on_void = move |_| {
                                                let confirm_msg = i18n::t(lang.get(), "confirm_void");
                                                let win = leptos::prelude::window();
                                                if !win.confirm_with_message(confirm_msg).unwrap_or(false) {
                                                    return;
                                                }
                                                spawn_local(async move {
                                                    match api::delete_empty(&format!("/api/transactions/{tx_id}")).await {
                                                        Ok(()) => set_txn_refresh.update(|n| *n += 1),
                                                        Err(e) => set_msg.set(format!("Error: {e}")),
                                                    }
                                                });
                                            };
                                            view! {
                                                <td>
                                                    <button
                                                        class="btn btn-sm btn-outline"
                                                        data-testid="txn-void"
                                                        title=move || i18n::t(lang.get(), "void")
                                                        on:click=on_void
                                                        style="padding:2px 8px;font-size:0.85rem"
                                                    >"\u{2715}"</button>
                                                </td>
                                            }.into_any()
                                        };
                                        view! {
                                            <tr class=row_class>
                                                <td>{date}</td>
                                                <td>{action}{until_suffix}</td>
                                                <td>{service}{voided_tag}</td>
                                                <td class="txn-amount">{amount}</td>
                                                {action_cell}
                                            </tr>
                                        }
                                    }).collect();
                                    view! {
                                        <div style="overflow-x:auto">
                                            <table class="data-table">
                                                <thead>
                                                    <tr>
                                                        <th>{i18n::t(lang.get(), "date")}</th>
                                                        <th>{i18n::t(lang.get(), "action")}</th>
                                                        <th>{i18n::t(lang.get(), "service")}</th>
                                                        <th>{i18n::t(lang.get(), "amount")}</th>
                                                        <th></th>
                                                    </tr>
                                                </thead>
                                                <tbody>{rows}</tbody>
                                            </table>
                                        </div>
                                    }.into_any()
                                }}
                            </div>
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
    }
}

/// Format a server-side timestamp into the Slovak convention `dd.MM.yyyy HH:mm`.
/// Handles current SQLite output, ISO 8601, and legacy MS Access dumps
/// (`MM/dd/yy` or `MM/dd/yyyy`) imported via the migrate-legacy tool.
/// Falls back to the raw string so rows never disappear, even on unknown formats.
fn format_sk_datetime(raw: &str) -> String {
    use chrono::NaiveDateTime;
    let trimmed = raw.trim();
    let patterns = [
        "%Y-%m-%d %H:%M:%S",    // SQLite datetime('now')
        "%Y-%m-%dT%H:%M:%S",    // ISO 8601 with T
        "%Y-%m-%d %H:%M:%S%.f", // SQLite with fractional seconds
        "%m/%d/%y %H:%M:%S",    // legacy MS Access, 2-digit year
        "%m/%d/%Y %H:%M:%S",    // legacy MS Access, 4-digit year
    ];
    for pattern in patterns {
        if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, pattern) {
            return dt.format("%d.%m.%Y %H:%M").to_string();
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod date_tests {
    use super::format_sk_datetime;

    #[test]
    fn sqlite_format() {
        assert_eq!(
            format_sk_datetime("2026-04-14 18:13:11"),
            "14.04.2026 18:13"
        );
    }

    #[test]
    fn iso_8601_format() {
        assert_eq!(
            format_sk_datetime("2026-04-14T18:13:11"),
            "14.04.2026 18:13"
        );
    }

    #[test]
    fn legacy_two_digit_year() {
        assert_eq!(format_sk_datetime("03/24/26 18:59:08"), "24.03.2026 18:59");
    }

    #[test]
    fn legacy_four_digit_year() {
        assert_eq!(
            format_sk_datetime("03/24/2026 18:59:08"),
            "24.03.2026 18:59"
        );
    }

    #[test]
    fn unknown_returns_input() {
        assert_eq!(format_sk_datetime("not-a-date"), "not-a-date");
    }
}

#[component]
fn TopupSection(
    card_id: i64,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let custom_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let do_topup = move |amount: f64| {
        if amount <= 0.0 {
            return;
        }
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
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    let on_custom = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let amount: f64 = custom_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default()
            .parse()
            .unwrap_or(0.0);
        do_topup(amount);
        if let Some(el) = custom_ref.get() {
            let el: &HtmlInputElement = &el;
            el.set_value("");
        }
    };

    view! {
        <div class="mt-2">
            <div class="text-muted" style="font-size:0.85rem;margin-bottom:4px">
                {move || i18n::t(lang.get(), "quick_topup")}
            </div>
            <div class="flex gap-1" style="flex-wrap:wrap">
                {QUICK_TOPUP.iter().map(|amt| {
                    let amount = *amt;
                    let label = format!("+{amount:.0} €");
                    view! {
                        <button
                            class="btn btn-sm btn-primary"
                            data-testid=format!("topup-{amount:.0}")
                            disabled=move || loading.get()
                            on:click=move |_| do_topup(amount)
                        >{label}</button>
                    }
                }).collect::<Vec<_>>()}
                <form class="inline-form" on:submit=on_custom style="display:inline-flex;gap:4px">
                    <input
                        type="number"
                        class="form-control"
                        node_ref=custom_ref
                        placeholder=move || i18n::t(lang.get(), "custom_amount")
                        step="0.01"
                        min="0.01"
                        style="width:8em"
                    />
                    <button type="submit" class="btn btn-sm btn-primary" disabled=move || loading.get()>
                        {move || i18n::t(lang.get(), "topup")}
                    </button>
                </form>
            </div>
        </div>
    }
}

#[component]
fn ChargeSection(
    card_id: i64,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    pass_active: bool,
    set_txn_refresh: WriteSignal<u32>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let service_ref = NodeRef::<leptos::html::Select>::new();
    let amount_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let on_service_change = move |_| {
        let id: i64 = service_ref
            .get()
            .map(|el| {
                let el: &HtmlSelectElement = &el;
                el.value()
            })
            .unwrap_or_default()
            .parse()
            .unwrap_or(0);
        if let Some(svc) = services.get().iter().find(|s| s.id == id) {
            if let Some(el) = amount_ref.get() {
                let el: &HtmlInputElement = &el;
                el.set_value(&format!("{:.2}", svc.default_price));
            }
        }
    };

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let amount: f64 = amount_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default()
            .parse()
            .unwrap_or(0.0);
        let service_id: Option<i64> = service_ref.get().and_then(|el| {
            let el: &HtmlSelectElement = &el;
            el.value().parse().ok()
        });

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
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    // Closure factory for per-service log-visit click handlers (pass active path).
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
                    Err(e) => set_msg.set(format!("Error: {e}")),
                }
            });
        }
    };

    view! {
        <div class="mt-2">
            <div class="text-muted" style="font-size:0.85rem;margin-bottom:4px">
                {move || i18n::t(lang.get(), "quick_charge")}
            </div>

            // Log-visit primary buttons (ONLY when pass is active).
            {if pass_active {
                view! {
                    <div class="flex gap-1" style="flex-wrap:wrap">
                        {services.get().into_iter()
                            .filter(|svc| svc.name != "Monthly pass")
                            .map(|svc| {
                                let service_id = svc.id;
                                let svc_name = svc.name.clone();
                                view! {
                                    <button
                                        class="btn btn-sm btn-primary"
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

            // Charge form — always visible, labelled for drinks/food.
            <div class="text-muted" style="font-size:0.8rem;margin:6px 0 2px">
                {move || i18n::t(lang.get(), "charge_for_extras")}
            </div>
            <form class="inline-form" on:submit=on_submit style="flex-wrap:wrap">
                <select class="form-control" node_ref=service_ref on:change=on_service_change data-testid="charge-service">
                    <option value="">{move || i18n::t(lang.get(), "select_service")}</option>
                    {move || {
                        services.get().into_iter()
                            .filter(|s| s.name != "Monthly pass")
                            .map(|s| {
                                let val = s.id.to_string();
                                let label = format!("{} ({:.2} €)", s.name, s.default_price);
                                view! { <option value=val>{label}</option> }
                            }).collect::<Vec<_>>()
                    }}
                </select>
                <input
                    type="number"
                    class="form-control"
                    node_ref=amount_ref
                    placeholder=move || i18n::t(lang.get(), "amount")
                    step="0.01"
                    min="0.01"
                    style="width:8em"
                    required
                />
                <button type="submit" class="btn btn-sm btn-danger" data-testid="charge-submit" disabled=move || loading.get()>
                    {move || i18n::t(lang.get(), "charge")}
                </button>
            </form>
        </div>
    }
}

#[component]
fn BlockButton(
    card_id: i64,
    blocked: bool,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (loading, set_loading) = signal(false);
    let btn_class = if blocked {
        "btn btn-sm btn-primary"
    } else {
        "btn btn-sm btn-outline"
    };

    let on_click = move |_| {
        set_loading.set(true);
        let new_blocked = !blocked;
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                card_id: i64,
                blocked: bool,
            }
            match api::post::<Req, CardInfo>(
                "/api/cards/block",
                &Req {
                    card_id,
                    blocked: new_blocked,
                },
            )
            .await
            {
                Ok(c) => {
                    set_msg.set(if c.blocked {
                        i18n::t(lang.get_untracked(), "block_ok").to_string()
                    } else {
                        i18n::t(lang.get_untracked(), "unblock_ok").to_string()
                    });
                    set_selected.set(Some(c));
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <button class=btn_class disabled=move || loading.get() on:click=on_click>
            {move || if blocked { i18n::t(lang.get(), "unblock") } else { i18n::t(lang.get(), "block") }}
        </button>
    }
}

#[component]
fn EditInfoForm(
    card: CardInfo,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    set_show_edit: WriteSignal<bool>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let first_ref = NodeRef::<leptos::html::Input>::new();
    let last_ref = NodeRef::<leptos::html::Input>::new();
    let company_ref = NodeRef::<leptos::html::Input>::new();
    let phone_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let card_id = card.id;
    let fv = card.first_name.clone().unwrap_or_default();
    let lv = card.last_name.clone().unwrap_or_default();
    let cv = card.company.clone().unwrap_or_default();
    let pv = card.phone.clone().unwrap_or_default();

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let read = |n: &NodeRef<leptos::html::Input>| {
            n.get()
                .map(|el| {
                    let el: &HtmlInputElement = &el;
                    el.value()
                })
                .unwrap_or_default()
        };
        let first = read(&first_ref);
        let last = read(&last_ref);
        let company = read(&company_ref);
        let phone = read(&phone_ref);

        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                first_name: Option<String>,
                last_name: Option<String>,
                company: Option<String>,
                phone: Option<String>,
            }
            let req = Req {
                first_name: if first.is_empty() { None } else { Some(first) },
                last_name: if last.is_empty() { None } else { Some(last) },
                company: if company.is_empty() {
                    None
                } else {
                    Some(company)
                },
                phone: if phone.is_empty() { None } else { Some(phone) },
            };
            match api::put_json::<Req, CardInfo>(&format!("/api/cards/{card_id}"), &req).await {
                Ok(c) => {
                    set_selected.set(Some(c));
                    set_msg.set(i18n::t(lang.get_untracked(), "saved").to_string());
                    set_show_edit.set(false);
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <form class="mt-2" on:submit=on_submit>
            <div class="form-group"><label>{move || i18n::t(lang.get(), "first_name")}</label>
                <input type="text" class="form-control" node_ref=first_ref value=fv /></div>
            <div class="form-group"><label>{move || i18n::t(lang.get(), "last_name")}</label>
                <input type="text" class="form-control" node_ref=last_ref value=lv /></div>
            <div class="form-group"><label>{move || i18n::t(lang.get(), "company")}</label>
                <input type="text" class="form-control" node_ref=company_ref value=cv /></div>
            <div class="form-group"><label>{move || i18n::t(lang.get(), "phone")}</label>
                <input type="text" class="form-control" node_ref=phone_ref value=pv /></div>
            <button type="submit" class="btn btn-sm btn-primary" disabled=move || loading.get()>
                {move || i18n::t(lang.get(), "save")}
            </button>
        </form>
    }
}

#[component]
fn ActivateCardForm(
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    set_show_activate: WriteSignal<bool>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let barcode_ref = NodeRef::<leptos::html::Input>::new();
    let first_ref = NodeRef::<leptos::html::Input>::new();
    let last_ref = NodeRef::<leptos::html::Input>::new();
    let company_ref = NodeRef::<leptos::html::Input>::new();
    let phone_ref = NodeRef::<leptos::html::Input>::new();
    let credit_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let read = |n: &NodeRef<leptos::html::Input>| {
            n.get()
                .map(|el| {
                    let el: &HtmlInputElement = &el;
                    el.value()
                })
                .unwrap_or_default()
        };
        let barcode = read(&barcode_ref);
        let first = read(&first_ref);
        let last = read(&last_ref);
        let company = read(&company_ref);
        let phone = read(&phone_ref);
        let credit: f64 = read(&credit_ref).parse().unwrap_or(0.0);

        if barcode.is_empty() {
            return;
        }
        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                barcode: String,
                initial_credit: f64,
                first_name: Option<String>,
                last_name: Option<String>,
                company: Option<String>,
                phone: Option<String>,
            }
            let req = Req {
                barcode,
                initial_credit: credit,
                first_name: if first.is_empty() { None } else { Some(first) },
                last_name: if last.is_empty() { None } else { Some(last) },
                company: if company.is_empty() {
                    None
                } else {
                    Some(company)
                },
                phone: if phone.is_empty() { None } else { Some(phone) },
            };
            match api::post::<Req, CardInfo>("/api/cards/activate", &req).await {
                Ok(c) => {
                    set_msg.set(i18n::t(lang.get_untracked(), "activate_ok").to_string());
                    set_selected.set(Some(c));
                    set_show_activate.set(false);
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="card mt-2">
            <form on:submit=on_submit>
                <div class="form-group"><label>{move || i18n::t(lang.get(), "barcode")}</label>
                    <input type="text" class="form-control" node_ref=barcode_ref placeholder=move || i18n::t(lang.get(), "new_card_barcode") required /></div>
                <div class="form-group"><label>{move || i18n::t(lang.get(), "first_name")}</label>
                    <input type="text" class="form-control" node_ref=first_ref /></div>
                <div class="form-group"><label>{move || i18n::t(lang.get(), "last_name")}</label>
                    <input type="text" class="form-control" node_ref=last_ref /></div>
                <div class="form-group"><label>{move || i18n::t(lang.get(), "company")}</label>
                    <input type="text" class="form-control" node_ref=company_ref /></div>
                <div class="form-group"><label>{move || i18n::t(lang.get(), "phone")}</label>
                    <input type="text" class="form-control" node_ref=phone_ref /></div>
                <div class="form-group"><label>{move || i18n::t(lang.get(), "initial_credit")}</label>
                    <input type="number" class="form-control" node_ref=credit_ref step="0.01" min="0" value="0" /></div>
                <button type="submit" class="btn btn-sm btn-primary" disabled=move || loading.get()>
                    {move || i18n::t(lang.get(), "activate")}
                </button>
            </form>
        </div>
    }
}
