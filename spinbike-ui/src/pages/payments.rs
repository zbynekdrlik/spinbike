use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
struct CardInfo {
    id: i64,
    barcode: String,
    credit: f64,
    blocked: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ServiceInfo {
    id: i64,
    name: String,
    default_price: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PaymentResp {
    transaction_id: i64,
    new_credit: f64,
}

#[component]
pub fn PaymentsPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let barcode_ref = NodeRef::<leptos::html::Input>::new();
    let (card, set_card) = signal(None::<CardInfo>);
    let (services, set_services) = signal(Vec::<ServiceInfo>::new());
    let (error, set_error) = signal(String::new());
    let (msg, set_msg) = signal(String::new());
    let (loading, set_loading) = signal(false);

    Effect::new(move || {
        spawn_local(async move {
            if let Ok(svc) = api::get::<Vec<ServiceInfo>>("/api/admin/services").await {
                set_services.set(svc);
            }
        });
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
        <h1 class="page-title">{move || i18n::t(lang.get(), "payments")}</h1>

        <form class="inline-form mb-2" on:submit=on_lookup>
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "card_barcode_label")}</label>
                <input type="text" class="form-control" node_ref=barcode_ref placeholder=move || i18n::t(lang.get(), "scan_barcode") required />
            </div>
            <button type="submit" class="btn btn-primary" disabled=move || loading.get()>{move || i18n::t(lang.get(), "lookup")}</button>
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
                    let info_str = format!(
                        "Card: {} | Credit: {:.2} EUR{}",
                        c.barcode, c.credit, if c.blocked { " | BLOCKED" } else { "" }
                    );

                    view! {
                        <div class="card mb-2">
                            <p><strong>{info_str}</strong></p>
                        </div>
                        {ChargeForm(ChargeFormProps { card_id, services, set_msg, set_card })}
                        {StornoForm(StornoFormProps { card_id, set_msg, set_card })}
                    }.into_any()
                }
            }
        }}
    }
}

#[component]
fn ChargeForm(
    card_id: i64,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_msg: WriteSignal<String>,
    set_card: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let service_ref = NodeRef::<leptos::html::Select>::new();
    let amount_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let on_service_change = move |_| {
        let service_id_str = service_ref
            .get()
            .map(|el| {
                let el: &HtmlSelectElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let service_id: i64 = service_id_str.parse().unwrap_or(0);
        let svcs = services.get();
        if let Some(svc) = svcs.iter().find(|s| s.id == service_id) {
            if let Some(el) = amount_ref.get() {
                let el: &HtmlInputElement = &el;
                el.set_value(&format!("{:.0}", svc.default_price));
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
        let service_id: Option<i64> = service_ref
            .get()
            .and_then(|el| {
                let el: &HtmlSelectElement = &el;
                el.value().parse().ok()
            });

        if amount <= 0.0 {
            return;
        }

        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { card_id: i64, amount: f64, service_id: Option<i64> }
            match api::post::<Req, PaymentResp>(
                "/api/payments/charge",
                &Req { card_id, amount, service_id },
            ).await {
                Ok(r) => {
                    set_msg.set(format!("Charged! New credit: {:.2} EUR (tx #{})", r.new_credit, r.transaction_id));
                    set_card.update(|c| { if let Some(c) = c { c.credit = r.new_credit; } });
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="card mb-2">
            <h3 style="font-size:0.95rem;margin-bottom:8px">{move || i18n::t(lang.get(), "charge")}</h3>
            <form class="inline-form" on:submit=on_submit>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "service")}</label>
                    <select class="form-control" node_ref=service_ref on:change=on_service_change>
                        <option value="">{move || i18n::t(lang.get(), "select_service")}</option>
                        {move || {
                            services.get().iter().map(|s| {
                                let val = s.id.to_string();
                                let label = format!("{} ({:.0})", s.name, s.default_price);
                                view! { <option value=val>{label}</option> }
                            }).collect::<Vec<_>>()
                        }}
                    </select>
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "amount_czk")}</label>
                    <input type="number" class="form-control" node_ref=amount_ref step="1" min="1" required />
                </div>
                <button type="submit" class="btn btn-sm btn-primary" disabled=move || loading.get()>{move || i18n::t(lang.get(), "charge")}</button>
            </form>
        </div>
    }
}

#[component]
fn StornoForm(
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
            struct Req { card_id: i64, amount: f64, reason: Option<String> }
            match api::post::<Req, PaymentResp>(
                "/api/payments/storno",
                &Req { card_id, amount, reason: None },
            ).await {
                Ok(r) => {
                    set_msg.set(format!("Storno done! New credit: {:.2} EUR (tx #{})", r.new_credit, r.transaction_id));
                    set_card.update(|c| { if let Some(c) = c { c.credit = r.new_credit; } });
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="card">
            <h3 style="font-size:0.95rem;margin-bottom:8px">{move || i18n::t(lang.get(), "storno_refund")}</h3>
            <form class="inline-form" on:submit=on_submit>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "amount_czk")}</label>
                    <input type="number" class="form-control" node_ref=amount_ref step="1" min="1" required />
                </div>
                <button type="submit" class="btn btn-sm btn-danger" disabled=move || loading.get()>{move || i18n::t(lang.get(), "storno")}</button>
            </form>
        </div>
    }
}
