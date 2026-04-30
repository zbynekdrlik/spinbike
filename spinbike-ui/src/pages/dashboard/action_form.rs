use leptos::prelude::*;
use spinbike_core::services::FITNESS_NAME_EN;
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
    let note_ref = NodeRef::<leptos::html::Input>::new();

    let read_note = move || -> Option<String> {
        note_ref.get().and_then(|el| {
            let el: &web_sys::HtmlInputElement = &el;
            let v = el.value();
            if v.trim().is_empty() { None } else { Some(v) }
        })
    };
    let clear_note = move || {
        if let Some(el) = note_ref.get() {
            let el: &web_sys::HtmlInputElement = &el;
            el.set_value("");
        }
    };

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
        // Auto-fill from default_price was removed (#17). Staff types the price
        // every time. The is_monthly_pass() helper still reads
        // selected_service_id, so the date-row visibility and Sell-vs-Charge
        // submit-label flip continue to work.
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
        let note = read_note();
        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                card_id: i64,
                amount: f64,
                note: Option<String>,
            }
            match api::post::<Req, CardInfo>("/api/cards/topup", &Req { card_id, amount, note }).await {
                Ok(c) => {
                    let credit = c.credit;
                    set_selected.set(Some(c));
                    set_msg.set(i18n::tf(
                        lang.get_untracked(),
                        "topup_ok_format",
                        &[&format!("{credit:.2}")],
                    ));
                    clear_note();
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
        let note = read_note();

        if is_monthly_pass() {
            let vu = valid_until.get_untracked();
            set_loading.set(true);
            spawn_local(async move {
                #[derive(serde::Serialize)]
                struct Req {
                    card_id: i64,
                    price: f64,
                    valid_until: chrono::NaiveDate,
                    note: Option<String>,
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
                        note,
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
                        clear_note();
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
                    note: Option<String>,
                }
                match api::post::<Req, PaymentResp>(
                    "/api/payments/charge",
                    &Req {
                        card_id,
                        amount,
                        service_id,
                        note,
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
                        clear_note();
                    }
                    Err(e) => set_err.set(e),
                }
                set_loading.set(false);
            });
        }
    };

    let visit_click_for = move |service_id: i64| {
        move |_: web_sys::MouseEvent| {
            let note = read_note();
            spawn_local(async move {
                #[derive(serde::Serialize)]
                struct Req {
                    card_id: i64,
                    service_id: i64,
                    note: Option<String>,
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
                        note,
                    },
                )
                .await
                {
                    Ok(_) => {
                        set_txn_refresh.update(|n| *n += 1);
                        clear_note();
                    }
                    Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
                }
            });
        }
    };

    // Fitness preselect (#33). On first non-empty services load with no
    // current selection, selects Fitness in both the signal and the DOM
    // <select>. set_value() is imperative DOM mutation — NOT a prop:value
    // reactive binding. The previous prop:value attempt re-rendered the
    // <select> and broke set_selected.update in the parent (txn list went
    // empty after a charge). Imperative set_value() doesn't subscribe the
    // <select> to any signal, so the parent's update flow is untouched.
    // Empty <option value=""> stays as the missing-Fitness fallback.
    Effect::new(move |_| {
        let svcs = services.get();
        if svcs.is_empty() {
            return;
        }
        if selected_service_id.get_untracked().is_some() {
            return;
        }
        let Some(fitness) = svcs
            .iter()
            .find(|s| s.name_en == FITNESS_NAME_EN && s.active != 0)
            .cloned()
        else {
            return;
        };
        set_selected_service_id.set(Some(fitness.id));
        if let Some(el) = service_ref.get() {
            let el: &HtmlSelectElement = &el;
            el.set_value(&fitness.id.to_string());
        }
    });

    view! {
        <div class="stack-12" data-testid="action-form">
            {if pass_active {
                view! {
                    <div class="chip-row chip-row--spaced chip-row--readable">
                        {
                            // Sort so Fitness renders left of Spinning. is_class_visit()
                            // restricts name_en to "Fitness" | "Spinning", so a plain
                            // alphabetical sort (Fitness < Spinning) yields the right order.
                            let mut visits: Vec<_> = services.get().into_iter()
                                .filter(|svc| svc.is_class_visit())
                                .collect();
                            visits.sort_by(|a, b| a.name_en.cmp(&b.name_en));
                            visits.into_iter().map(|svc| {
                                let service_id = svc.id;
                                let svc_name = svc.display_name(lang.get_untracked()).to_string();
                                // Fitness is the more-used activity (per CEO feedback on
                                // PR #25 v0.13.5) → solid blue, eye-catching. Spinning gets
                                // the soft-blue sibling so the pair reads as primary /
                                // secondary within one hue family — small visual difference,
                                // not a radical color shift.
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
                            }).collect::<Vec<_>>()
                        }
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
                            // No price annotation (#17) — staff sees just the service name.
                            let label = s.display_name(lang_now).to_string();
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

            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "tx_note_edit")}</label>
                <input
                    type="text"
                    maxlength="200"
                    class="form-control"
                    node_ref=note_ref
                    data-testid="txn-note-input"
                    placeholder=move || i18n::t(lang.get(), "tx_note_placeholder")
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
                <button
                    type="button"
                    class="btn btn--primary-soft"
                    data-testid="topup-submit"
                    on:click=do_topup
                    disabled=move || loading.get()
                >
                    "+ "{move || i18n::t(lang.get(), "topup")}
                </button>
            </div>
        </div>
    }
}
