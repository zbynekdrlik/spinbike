//! Staff card dashboard: fast search + inline action panel.
//!
//! Replaces the old /staff/cards (list-dump) and /staff/payments (separate) pages.
//! Flow: type in search → dropdown → pick result → quick top-up / charge / block / edit.

pub mod action_form;
pub mod add_person_form;
pub mod block_button;
pub mod card_panel;
pub mod edit_info_form;
pub mod helpers;
pub mod negative_balance_list;
pub mod overview_tab;
pub mod pass_banner;
pub mod sheets;
pub mod transactions_list;

pub use card_panel::CardActionPanel;
pub use overview_tab::OverviewTab;
pub use transactions_list::TransactionsList;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::i18n::{self, Lang};

use helpers::urlencoding_light;

use add_person_form::AddPersonForm;

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

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct CardInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub card_code: Option<String>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
    #[serde(default)]
    pub allow_self_entry: bool,
    /// Target user's role. Used by the edit form to hide controls that
    /// have no effect for admin/staff (e.g. `allow_self_entry`, which
    /// admin/staff bypass per 0dfe85b). `None` from an older server →
    /// treated as customer-mode, preserving the show-checkbox default.
    #[serde(default)]
    pub role: Option<spinbike_core::auth::Role>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub pass: Option<CardPass>,
    /// MAX(transactions.created_at) for non-soft-deleted Spinning/Fitness rows
    /// from /api/users/search. `None` when the user has never been used for a
    /// class. The shape is the SQLite literal "YYYY-MM-DD HH:MM:SS"; the
    /// helper in `card_panel::parse_last_visit` extracts the date for display.
    #[serde(default)]
    pub last_visit_at: Option<String>,
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
    #[serde(default)]
    pub note: Option<String>,
}

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

#[component]
pub fn DashboardPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (query, set_query) = signal(String::new());
    let (results, set_results) = signal(Vec::<CardInfo>::new());
    let (searching, set_searching) = signal(false);
    let (selected, set_selected) = signal(None::<CardInfo>);
    let (services, set_services) = signal(Vec::<ServiceInfo>::new());
    let (show_add_person, set_show_add_person) = signal(false);
    let (msg, set_msg) = signal(String::new());
    let (err, set_err) = signal(String::new());
    // Keyboard-driven highlight within the search dropdown. 0 means "first
    // suggestion" — so typing + Enter picks the top match without a click.
    let (highlighted_idx, set_highlighted_idx) = signal(0usize);

    // Incremented whenever a transaction completes (via clear_selection after
    // the card panel closes). The negative-balance list subscribes to this so
    // it refetches after every top-up / charge / visit.
    let txn_refresh = RwSignal::new(0u32);

    // Explicit ref so we can restore focus after pick_card and after the
    // action panel closes. HTML `autofocus` only runs once on mount.
    let search_input_ref = NodeRef::<leptos::html::Input>::new();

    // Desk-reset signal: AdaptiveNav / brand link increment on click. Even
    // when already on /staff (same URL, no router event), this lets the
    // dashboard return to the idle state — clear selected card, search query,
    // and any visible message — so the negative-balance list takes over.
    let desk_reset = use_context::<crate::router::DeskReset>()
        .expect("DeskReset context")
        .0;
    Effect::new(move |prev: Option<u32>| {
        let cur = desk_reset.get();
        // On the first run prev is None — don't clear anything (initial mount).
        if prev.is_some() && prev != Some(cur) {
            set_selected.set(None);
            set_query.set(String::new());
            set_results.set(Vec::new());
            set_show_add_person.set(false);
            set_msg.set(String::new());
            set_err.set(String::new());
            if let Some(el) = search_input_ref.get_untracked() {
                let _ = el.focus();
            }
        }
        cur
    });

    Effect::new(move |_| {
        if let Some(el) = search_input_ref.get() {
            let _ = el.focus();
        }
    });

    // Parse query params used by the Reports → row click jump.
    //
    // * `?card=<card_code>` — exact lookup via /api/users/lookup/{code};
    //   on success, the user panel opens directly (skips dropdown).
    // * `?q=<text>` — search prefill (existing behavior).
    //
    // `?card=` wins when both are present (defensive — Reports only
    // sets `?card=` since v0.13.15).
    Effect::new(move |_| {
        let Some(w) = web_sys::window() else {
            return;
        };
        let search = w.location().search().unwrap_or_default();
        let Some(stripped) = search.strip_prefix('?') else {
            return;
        };

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
            // Direct user lookup by card_code. On 404 (user not found since
            // report rendered), fall back to populating the search box so the
            // user sees the existing search-empty UX.
            spawn_local(async move {
                let encoded = urlencoding_light(&bc);
                match api::get::<CardInfo>(&format!("/api/users/lookup/{encoded}")).await {
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
            match api::get::<Vec<CardInfo>>(&format!("/api/users/search?q={encoded}&limit=10"))
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
        // Refresh the negative-balance list so any credit changes from the
        // just-closed action panel are reflected immediately.
        txn_refresh.update(|n| *n += 1);
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
                    let name = helpers::user_display_name(&c.name, c.company.as_deref(), c.card_code.as_deref());
                    let code = c.card_code.clone().unwrap_or_default();
                    let tail_len = code.len().min(4);
                    let tail = &code[code.len() - tail_len..];
                    let tail_str = tail.to_string();
                    let company = c.company.clone().unwrap_or_default();
                    let credit_val = c.credit;
                    let credit = format!("{:.2} €", credit_val);
                    let credit_class = if credit_val < 0.0 { "credit-negative" } else { "" };
                    let is_blocked = c.blocked;
                    view! {
                        <div
                            class={
                                let credit_val = credit_val;
                                move || {
                                    helpers::result_row_class(
                                        highlighted_idx.get() == idx,
                                        credit_val,
                                    )
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
                    view! {
                        <div class="text-center mt-2">
                            <p class="text-muted">{i18n::t(lang.get(), "no_matches")}</p>
                            <AddPersonForm set_selected=set_selected set_msg=set_msg set_show=set_show_add_person />
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

        // Idle-state proactive list: only when no card selected AND search is empty.
        {move || {
            if selected.get().is_none() && query.get().is_empty() {
                view! {
                    <negative_balance_list::NegativeBalanceList
                        txn_refresh=txn_refresh.read_only()
                        lang=lang
                        on_pick=Callback::new(move |c: CardInfo| pick_card(c))
                    />
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }
        }}

        {move || match selected.get() {
            None => view! { <span></span> }.into_any(),
            Some(c) => view! {
                <CardActionPanel
                    card=c
                    services=services
                    set_selected=set_selected
                    msg=msg
                    set_msg=set_msg
                    set_err=set_err
                    on_close=Callback::new(clear_selection)
                />
            }.into_any()
        }}

        <div class="mt-2">
            <button
                class="btn btn--ghost btn--compact"
                on:click=move |_| set_show_add_person.update(|v| *v = !*v)
            >
                {move || if show_add_person.get() {
                    i18n::t(lang.get(), "hide_add_person")
                } else {
                    i18n::t(lang.get(), "add_person")
                }}
            </button>
        </div>

        {move || {
            if show_add_person.get() {
                view! { <AddPersonForm set_selected=set_selected set_msg=set_msg set_show=set_show_add_person /> }.into_any()
            } else { view! { <span></span> }.into_any() }
        }}
    }
}

#[cfg(test)]
mod is_class_visit_tests {
    // The truthy assertions pin the contents of CLASS_VISIT_NAMES_EN
    // (currently "Spinning" + "Fitness"). If the const grows — e.g. a new
    // class type like "HIIT" — add a positive case here so the gate
    // surfaces the addition. Renaming an existing entry will break this
    // test (intended), matching the doc comment on is_class_visit.
    use super::*;
    use wasm_bindgen_test::*;

    fn make_svc(name_en: &str) -> ServiceInfo {
        ServiceInfo {
            id: 1,
            kind: "generic".to_string(),
            name_sk: "x".to_string(),
            name_en: name_en.to_string(),
            default_price: 0.0,
            active: 1,
        }
    }

    // Strong: covers the two truthy class-visit names AND a sample of names
    // that must return false. Catches mutants that flip the return constant
    // OR replace `contains` with always-true / always-false equivalents.
    #[wasm_bindgen_test]
    fn is_class_visit_true_for_spinning() {
        assert!(make_svc("Spinning").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_true_for_fitness() {
        assert!(make_svc("Fitness").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_false_for_refreshments() {
        assert!(!make_svc("Refreshments").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_false_for_unknown() {
        assert!(!make_svc("Whatever").is_class_visit());
    }

    #[wasm_bindgen_test]
    fn is_class_visit_false_for_empty() {
        assert!(!make_svc("").is_class_visit());
    }
}
