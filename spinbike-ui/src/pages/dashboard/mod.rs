//! Staff card dashboard: fast search + inline action panel.
//!
//! Replaces the old /staff/cards (list-dump) and /staff/payments (separate) pages.
//! Flow: type in search → dropdown → pick result → quick top-up / charge / block / edit.

pub mod action_form;
pub mod block_button;
pub mod card_panel;
pub mod edit_info_form;
pub mod helpers;
pub mod pass_banner;
pub mod sheets;
pub mod transactions_list;

pub use card_panel::CardActionPanel;
pub use transactions_list::TransactionsList;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::i18n::{self, Lang};

use crate::util::parse_money;
use helpers::{full_name, urlencoding_light};

fn decode_uri_component(s: &str) -> String {
    // Browser global decodeURIComponent via JsCast.
    if let Ok(v) = js_sys::decode_uri_component(s) {
        v.as_string().unwrap_or_default()
    } else {
        s.replace('+', " ")
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CardPass {
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
    pub transaction_id: i64,
}

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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ServiceInfo {
    pub id: i64,
    /// Stable identifier: "generic" or "monthly_pass". Used to detect the
    /// pass row regardless of its display name.
    pub kind: String,
    pub name_sk: String,
    pub name_en: String,
    pub default_price: f64,
    #[serde(default = "default_active")]
    pub active: i64,
}

fn default_active() -> i64 {
    1
}

impl ServiceInfo {
    pub fn display_name(&self, lang: crate::i18n::Lang) -> &str {
        match lang {
            crate::i18n::Lang::Sk => &self.name_sk,
            crate::i18n::Lang::En => &self.name_en,
        }
    }
    pub fn is_monthly_pass(&self) -> bool {
        self.kind == "monthly_pass"
    }
    /// Class-attendance services — the only ones that make sense as a "Log
    /// Visit" chip on the staff dashboard. V8 introduced sellable items
    /// (Refreshments, Supplements, Card activation fee) that share `kind=generic`
    /// with the class services but must NOT appear as visits.
    ///
    /// NOTE: identification is by `name_en`, so renaming Spinning or Fitness
    /// in the admin UI silently empties the visit row. If renaming is needed,
    /// migrate the data model to a `kind=class` flag instead of patching this
    /// list.
    pub fn is_class_visit(&self) -> bool {
        spinbike_core::services::CLASS_VISIT_NAMES_EN.contains(&self.name_en.as_str())
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PaymentResp {
    #[allow(dead_code)]
    pub transaction_id: i64,
    pub new_credit: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TxnInfo {
    pub id: i64,
    #[allow(dead_code)]
    pub card_id: Option<i64>,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
    #[serde(default)]
    pub service_name_sk: Option<String>,
    #[serde(default)]
    pub service_name_en: Option<String>,
    #[serde(default)]
    pub service_kind: Option<String>,
    #[serde(default)]
    pub valid_until: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

impl TxnInfo {
    pub fn service_label(&self, lang: crate::i18n::Lang) -> Option<&str> {
        match lang {
            crate::i18n::Lang::Sk => self.service_name_sk.as_deref(),
            crate::i18n::Lang::En => self.service_name_en.as_deref(),
        }
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
        <crate::pages::NowPanel />
        <h1 class="page-title">{move || i18n::t(lang.get(), "card_dashboard")}</h1>

        <div class="card mb-2">
            <input
                type="search"
                class="form-control search-input--lg"
                node_ref=search_input_ref
                inputmode="search"
                prop:value=move || query.get()
                placeholder=move || i18n::t(lang.get(), "search_cards_placeholder")
                on:input=on_search_input
                on:keydown=on_search_keydown
            />
            {move || {
                if searching.get() {
                    view! { <div class="search-hint mt-1">{i18n::t(lang.get(), "searching")}</div> }.into_any()
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
                                    "search-result-row search-result-active"
                                } else {
                                    "search-result-row"
                                }
                            }
                            data-testid="search-result"
                            on:click={
                                let card = card_for_pick.clone();
                                move |_| pick_card(card.clone())
                            }
                        >
                            <div>
                                <div class="search-result-name">
                                    {name}
                                    {if is_blocked {
                                        view! { <span class="badge badge--full badge--inline">{i18n::t(lang.get(), "blocked")}</span> }.into_any()
                                    } else { view! {}.into_any() }}
                                </div>
                                <div class="search-result-meta">
                                    <code>{format!("…{tail_str}")}</code>
                                    {if !company.is_empty() { format!(" · {company}") } else { String::new() }}
                                </div>
                            </div>
                            <div class=format!("search-result-credit {credit_class}")>{credit}</div>
                        </div>
                    }
                }).collect();
                view! { <div class="search-results-list">{items}</div> }.into_any()
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
                            <button class="btn btn--primary btn--compact" on:click=on_activate>
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
                <CardActionPanel
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
                class="btn btn--ghost btn--compact"
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
        let credit: f64 = parse_money(&read(&credit_ref)).unwrap_or(0.0);

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
                Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
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
                    <input type="text" inputmode="decimal" autocomplete="off" class="form-control" node_ref=credit_ref value="0" /></div>
                <button type="submit" class="btn btn--primary btn--compact" disabled=move || loading.get()>
                    {move || i18n::t(lang.get(), "activate")}
                </button>
            </form>
        </div>
    }
}
