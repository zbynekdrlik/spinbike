//! Staff card dashboard: fast search + inline action panel.
//!
//! Replaces the old /staff/cards (list-dump) and /staff/payments (separate) pages.
//! Flow: type in search → dropdown → pick result → quick top-up / charge / block / edit.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;
use crate::i18n::{self, Lang};

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
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    card_id: Option<i64>,
    amount: f64,
    action: String,
    created_at: String,
    #[serde(default)]
    service_name: Option<String>,
}

const QUICK_TOPUP: [f64; 4] = [5.0, 10.0, 20.0, 50.0];

fn full_name(c: &CardInfo) -> String {
    let f = c.first_name.clone().unwrap_or_default();
    let l = c.last_name.clone().unwrap_or_default();
    let combined = format!("{f} {l}").trim().to_string();
    if combined.is_empty() { "—".into() } else { combined }
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
            match api::get::<Vec<CardInfo>>(&format!("/api/cards/search?q={encoded}&limit=10")).await {
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
    };

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "card_dashboard")}</h1>

        <div class="card mb-2">
            <input
                type="search"
                class="form-control"
                autofocus
                inputmode="search"
                prop:value=move || query.get()
                placeholder=move || i18n::t(lang.get(), "search_cards_placeholder")
                on:input=on_search_input
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
                let items: Vec<_> = list.into_iter().map(|c| {
                    let card_for_pick = c.clone();
                    let name = full_name(&c);
                    let barcode = c.barcode.clone();
                    let tail_len = barcode.len().min(4);
                    let tail = &barcode[barcode.len() - tail_len..];
                    let tail_str = tail.to_string();
                    let company = c.company.clone().unwrap_or_default();
                    let credit = format!("{:.2} €", c.credit);
                    let is_blocked = c.blocked;
                    view! {
                        <div
                            class="search-result"
                            data-testid="search-result"
                            style="display:flex;justify-content:space-between;align-items:center;padding:10px;border-bottom:1px solid var(--border);cursor:pointer;gap:8px"
                            on:click=move |_| {
                                set_selected.set(Some(card_for_pick.clone()));
                                set_query.set(String::new());
                                set_results.set(Vec::new());
                                set_err.set(String::new());
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
                            <div style="font-weight:600;white-space:nowrap">{credit}</div>
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

    // Transaction history is the most-read piece of card context, so load it
    // as soon as the panel mounts and always render it below the actions.
    let card_id_for_txn = card.id;
    Effect::new(move |_| {
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
    let card_for_edit = card.clone();

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

            <div style="font-size:1.4rem;font-weight:700;margin:8px 0">
                {format!("{credit:.2} €")}
                {if is_blocked {
                    view! { <span class="badge badge-full" style="margin-left:8px;font-size:0.75rem">{i18n::t(lang.get(), "blocked")}</span> }.into_any()
                } else { view! {}.into_any() }}
            </div>

            // Ordered by actual staff usage frequency: charge (pay-for-service)
            // is the most-common action, then top-up. Edit/block stay secondary.
            <ChargeSection card_id=card_id services=services set_selected=set_selected set_msg=set_msg />
            <TopupSection card_id=card_id set_selected=set_selected set_msg=set_msg />

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
                        let service = tx.service_name.clone().unwrap_or_else(|| "—".into());
                        let amount = format!("{:+.2}", tx.amount);
                        view! {
                            <tr>
                                <td>{date}</td>
                                <td>{action}</td>
                                <td>{service}</td>
                                <td>{amount}</td>
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
                                    </tr>
                                </thead>
                                <tbody>{rows}</tbody>
                            </table>
                        </div>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

/// Format a server-side timestamp ("YYYY-MM-DD HH:MM:SS" or ISO 8601) into
/// the Slovak convention `dd.MM.yyyy HH:mm`. Returns the input unchanged if
/// the string doesn't parse — better to show something than nothing.
fn format_sk_datetime(raw: &str) -> String {
    use chrono::NaiveDateTime;
    let trimmed = raw.trim();
    // SQLite `datetime('now')` format.
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return dt.format("%d.%m.%Y %H:%M").to_string();
    }
    // ISO 8601 with T separator (rarer but handle it just in case).
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        return dt.format("%d.%m.%Y %H:%M").to_string();
    }
    raw.to_string()
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
            struct Req { card_id: i64, amount: f64 }
            match api::post::<Req, CardInfo>("/api/cards/topup", &Req { card_id, amount }).await {
                Ok(c) => {
                    let credit = c.credit;
                    set_selected.set(Some(c));
                    set_msg.set(i18n::tf(lang.get_untracked(), "topup_ok_format", &[&format!("{credit:.2}")]));
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
            .map(|el| { let el: &HtmlInputElement = &el; el.value() })
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
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let service_ref = NodeRef::<leptos::html::Select>::new();
    let amount_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let on_service_change = move |_| {
        let id: i64 = service_ref
            .get()
            .map(|el| { let el: &HtmlSelectElement = &el; el.value() })
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
            .map(|el| { let el: &HtmlInputElement = &el; el.value() })
            .unwrap_or_default()
            .parse()
            .unwrap_or(0.0);
        let service_id: Option<i64> = service_ref
            .get()
            .and_then(|el| { let el: &HtmlSelectElement = &el; el.value().parse().ok() });

        if amount <= 0.0 { return; }
        set_loading.set(true);

        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { card_id: i64, amount: f64, service_id: Option<i64> }
            match api::post::<Req, PaymentResp>(
                "/api/payments/charge",
                &Req { card_id, amount, service_id },
            ).await {
                Ok(r) => {
                    set_msg.set(i18n::tf(lang.get_untracked(), "charge_ok_format", &[&format!("{:.2}", r.new_credit)]));
                    set_selected.update(|s| { if let Some(c) = s { c.credit = r.new_credit; } });
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="mt-2">
            <div class="text-muted" style="font-size:0.85rem;margin-bottom:4px">
                {move || i18n::t(lang.get(), "quick_charge")}
            </div>
            <form class="inline-form" on:submit=on_submit style="flex-wrap:wrap">
                <select class="form-control" node_ref=service_ref on:change=on_service_change data-testid="charge-service">
                    <option value="">{move || i18n::t(lang.get(), "select_service")}</option>
                    {move || {
                        services.get().iter().map(|s| {
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
    let btn_class = if blocked { "btn btn-sm btn-primary" } else { "btn btn-sm btn-outline" };

    let on_click = move |_| {
        set_loading.set(true);
        let new_blocked = !blocked;
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { card_id: i64, blocked: bool }
            match api::post::<Req, CardInfo>("/api/cards/block", &Req { card_id, blocked: new_blocked }).await {
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
        let read = |n: &NodeRef<leptos::html::Input>| n.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        let first = read(&first_ref);
        let last = read(&last_ref);
        let company = read(&company_ref);
        let phone = read(&phone_ref);

        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { first_name: Option<String>, last_name: Option<String>, company: Option<String>, phone: Option<String> }
            let req = Req {
                first_name: if first.is_empty() { None } else { Some(first) },
                last_name: if last.is_empty() { None } else { Some(last) },
                company: if company.is_empty() { None } else { Some(company) },
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
        let read = |n: &NodeRef<leptos::html::Input>| n.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        let barcode = read(&barcode_ref);
        let first = read(&first_ref);
        let last = read(&last_ref);
        let company = read(&company_ref);
        let phone = read(&phone_ref);
        let credit: f64 = read(&credit_ref).parse().unwrap_or(0.0);

        if barcode.is_empty() { return; }
        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                barcode: String, initial_credit: f64,
                first_name: Option<String>, last_name: Option<String>,
                company: Option<String>, phone: Option<String>,
            }
            let req = Req {
                barcode, initial_credit: credit,
                first_name: if first.is_empty() { None } else { Some(first) },
                last_name: if last.is_empty() { None } else { Some(last) },
                company: if company.is_empty() { None } else { Some(company) },
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
