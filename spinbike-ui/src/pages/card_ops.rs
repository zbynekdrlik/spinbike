use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct CardInfo {
    id: i64,
    barcode: String,
    user_id: Option<i64>,
    blocked: bool,
    credit: f64,
    allow_debit: bool,
}

#[component]
pub fn CardOpsPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let barcode_ref = NodeRef::<leptos::html::Input>::new();
    let (card, set_card) = signal(None::<CardInfo>);
    let (error, set_error) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (msg, set_msg) = signal(String::new());
    let (all_cards, set_all_cards) = signal(Vec::<CardInfo>::new());
    let (cards_loading, set_cards_loading) = signal(true);

    // Load all cards on mount
    spawn_local({
        let set_all_cards = set_all_cards.clone();
        async move {
            match api::get::<Vec<CardInfo>>("/api/cards").await {
                Ok(cards) => set_all_cards.set(cards),
                Err(_) => {} // silently fail — user may not be staff
            }
            set_cards_loading.set(false);
        }
    });

    let on_lookup = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let barcode = barcode_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        if barcode.is_empty() {
            return;
        }

        set_loading.set(true);
        set_error.set(String::new());
        set_card.set(None);
        set_msg.set(String::new());

        spawn_local(async move {
            match api::get::<CardInfo>(&format!("/api/cards/lookup/{barcode}")).await {
                Ok(c) => set_card.set(Some(c)),
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "card_operations")}</h1>

        <form class="inline-form mb-2" on:submit=on_lookup>
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "barcode_lookup")}</label>
                <input type="text" class="form-control" node_ref=barcode_ref placeholder=move || i18n::t(lang.get(), "enter_barcode") required />
            </div>
            <button type="submit" class="btn btn-primary" disabled=move || loading.get()>
                {move || i18n::t(lang.get(), "lookup")}
            </button>
        </form>

        {move || {
            let e = error.get();
            if !e.is_empty() {
                view! { <div class="alert alert-error">{e}</div> }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }
        }}

        {move || {
            let m = msg.get();
            if !m.is_empty() {
                view! { <div class="alert alert-success">{m}</div> }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }
        }}

        {move || {
            match card.get() {
                None => view! { <span></span> }.into_any(),
                Some(c) => {
                    let card_id = c.id;
                    let is_blocked = c.blocked;
                    let barcode = c.barcode.clone();
                    let credit_str = format!("{:.0} CZK", c.credit);
                    let status_badge = if c.blocked { "badge badge-full" } else { "badge badge-booked" };
                    let user_id_str = c.user_id.map(|id| id.to_string()).unwrap_or_else(|| "None".into());

                    view! {
                        <div class="card">
                            <div class="card-header">
                                <div class="card-title">{format!("Card #{card_id} — {barcode}")}</div>
                                <span class=status_badge>{move || if is_blocked { i18n::t(lang.get(), "blocked") } else { i18n::t(lang.get(), "active") }}</span>
                            </div>
                            <p>{move || format!("{}: {credit_str}", i18n::t(lang.get(), "credit"))}</p>
                            <p class="text-muted">{format!("User ID: {user_id_str}")}</p>
                            <div class="flex gap-1 mt-2">
                                {TopupForm(TopupFormProps { card_id, set_msg, set_card })}
                                {BlockToggle(BlockToggleProps { card_id, blocked: is_blocked, set_msg, set_card })}
                            </div>
                        </div>
                    }.into_any()
                }
            }
        }}

        <div class="mt-3">
            <h2 style="font-size:1rem;font-weight:700;margin-bottom:8px">{move || i18n::t(lang.get(), "activate_new_card")}</h2>
            {ActivateForm(ActivateFormProps { set_msg, set_card })}
        </div>

        <div class="mt-3">
            <h2 style="font-size:1rem;font-weight:700;margin-bottom:8px">{move || i18n::t(lang.get(), "all_member_cards")}</h2>
            {move || {
                if cards_loading.get() {
                    return view! { <p class="text-muted">{i18n::t(lang.get(), "loading_cards")}</p> }.into_any();
                }
                let cards = all_cards.get();
                if cards.is_empty() {
                    return view! { <p class="text-muted">{i18n::t(lang.get(), "no_cards_found")}</p> }.into_any();
                }
                view! {
                    <div style="overflow-x:auto;">
                        <table class="data-table">
                            <thead>
                                <tr>
                                    <th>{i18n::t(lang.get(), "barcode")}</th>
                                    <th>{i18n::t(lang.get(), "credit")}</th>
                                    <th>{i18n::t(lang.get(), "status")}</th>
                                    <th>{i18n::t(lang.get(), "linked")}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {cards.into_iter().map(|c| {
                                    let barcode = c.barcode.clone();
                                    let credit = format!("{:.2} EUR", c.credit);
                                    let is_blocked = c.blocked;
                                    let status_class = if c.blocked { "text-danger" } else { "text-success" };
                                    let has_user = c.user_id.is_some();
                                    let bc = c.barcode.clone();
                                    view! {
                                        <tr style="cursor:pointer" on:click=move |_| {
                                            if let Some(input) = barcode_ref.get() {
                                                let el: &HtmlInputElement = &input;
                                                el.set_value(&bc);
                                            }
                                        }>
                                            <td><code>{barcode}</code></td>
                                            <td>{credit}</td>
                                            <td class=status_class>{move || if is_blocked { i18n::t(lang.get(), "blocked") } else { i18n::t(lang.get(), "active") }}</td>
                                            <td>{move || if has_user { i18n::t(lang.get(), "yes") } else { i18n::t(lang.get(), "no") }}</td>
                                        </tr>
                                    }
                                }).collect::<Vec<_>>()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn TopupForm(
    card_id: i64,
    set_msg: WriteSignal<String>,
    set_card: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let amount_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

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
        if amount <= 0.0 {
            return;
        }

        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { card_id: i64, amount: f64 }
            match api::post::<Req, CardInfo>("/api/cards/topup", &Req { card_id, amount }).await {
                Ok(c) => {
                    set_msg.set(format!("Topped up! New credit: {:.0} CZK", c.credit));
                    set_card.set(Some(c));
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <form class="inline-form" on:submit=on_submit>
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "topup")}</label>
                <input type="number" class="form-control" node_ref=amount_ref placeholder=move || i18n::t(lang.get(), "amount") step="1" min="1" required />
            </div>
            <button type="submit" class="btn btn-sm btn-primary" disabled=move || loading.get()>{move || i18n::t(lang.get(), "topup")}</button>
        </form>
    }
}

#[component]
fn BlockToggle(
    card_id: i64,
    blocked: bool,
    set_msg: WriteSignal<String>,
    set_card: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (loading, set_loading) = signal(false);

    let on_click = move |_| {
        set_loading.set(true);
        let new_blocked = !blocked;
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { card_id: i64, blocked: bool }
            match api::post::<Req, CardInfo>("/api/cards/block", &Req { card_id, blocked: new_blocked }).await {
                Ok(c) => {
                    set_msg.set(if c.blocked { "Card blocked".into() } else { "Card unblocked".into() });
                    set_card.set(Some(c));
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    let btn_class = if blocked { "btn btn-sm btn-primary" } else { "btn btn-sm btn-danger" };

    view! {
        <button class=btn_class on:click=on_click disabled=move || loading.get()>
            {move || if blocked { i18n::t(lang.get(), "unblock") } else { i18n::t(lang.get(), "block") }}
        </button>
    }
}

#[component]
fn ActivateForm(
    set_msg: WriteSignal<String>,
    set_card: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let barcode_ref = NodeRef::<leptos::html::Input>::new();
    let credit_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let barcode = barcode_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        let credit: f64 = credit_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default().parse().unwrap_or(0.0);

        if barcode.is_empty() { return; }

        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { barcode: String, initial_credit: f64 }
            match api::post::<Req, CardInfo>("/api/cards/activate", &Req { barcode, initial_credit: credit }).await {
                Ok(c) => {
                    set_msg.set(format!("Card activated! ID: {}", c.id));
                    set_card.set(Some(c));
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <form class="inline-form" on:submit=on_submit>
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "barcode")}</label>
                <input type="text" class="form-control" node_ref=barcode_ref placeholder=move || i18n::t(lang.get(), "new_card_barcode") required />
            </div>
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "initial_credit")}</label>
                <input type="number" class="form-control" node_ref=credit_ref placeholder="0" step="1" min="0" value="0" />
            </div>
            <button type="submit" class="btn btn-sm btn-primary" disabled=move || loading.get()>{move || i18n::t(lang.get(), "activate")}</button>
        </form>
    }
}
