use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;
use crate::components::DateInput;
use crate::i18n::{self, Lang};
use crate::util::parse_money;

use super::helpers::pass_is_active;
use super::{CardInfo, CardPass, PaymentResp, ServiceInfo};

/// Unified action form for the staff card-detail panel.
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

    let is_monthly_pass = move || match selected_service_id.get() {
        Some(id) => services
            .get()
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.is_monthly_pass())
            .unwrap_or(false),
        None => false,
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
            if amount <= 0.0 {
                set_err.set(i18n::t(lang.get_untracked(), "price_required").to_string());
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
            {if pass_active {
                view! {
                    <div class="chip-row chip-row--spaced">
                        {services.get().into_iter()
                            .filter(|svc| !svc.is_monthly_pass())
                            .map(|svc| {
                                let service_id = svc.id;
                                let svc_name = svc.display_name(lang.get_untracked()).to_string();
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
                        let lang_now = lang.get();
                        services.get().into_iter().map(|s| {
                            let val = s.id.to_string();
                            let kind = s.kind.clone();
                            let label = format!("{} ({:.2} €)", s.display_name(lang_now), s.default_price);
                            view! { <option value=val data-kind=kind>{label}</option> }
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
