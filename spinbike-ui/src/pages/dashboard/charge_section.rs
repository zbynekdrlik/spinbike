use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;
use crate::i18n::{self, Lang};

use super::{CardInfo, PaymentResp, ServiceInfo};
use crate::util::parse_money;

#[component]
pub fn ChargeSection(
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
        let typed = amount_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let amount = parse_money(&typed).unwrap_or(0.0);
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
                Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
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
                    Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
                }
            });
        }
    };

    view! {
        <div class="stack-12">
            <div class="section-label">
                {move || i18n::t(lang.get(), "quick_charge")}
            </div>

            // Log-visit primary buttons (ONLY when pass is active).
            {if pass_active {
                view! {
                    <div class="chip-row chip-row--spaced">
                        {services.get().into_iter()
                            .filter(|svc| svc.name != "Monthly pass")
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

            // Charge form — always visible, labelled for drinks/food.
            <div class="section-label">
                {move || i18n::t(lang.get(), "charge_for_extras")}
            </div>
            <form class="inline-form" on:submit=on_submit>
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
                    type="text"
                    inputmode="decimal"
                    autocomplete="off"
                    class="form-control input--narrow"
                    node_ref=amount_ref
                    data-testid="charge-amount"
                    placeholder=move || i18n::t(lang.get(), "amount")
                    required
                />
                <button type="submit" class="btn btn--primary" data-testid="charge-submit" disabled=move || loading.get()>
                    {move || i18n::t(lang.get(), "charge")}
                </button>
            </form>
        </div>
    }
}
