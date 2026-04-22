use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

use super::helpers::event_target_value;
use super::{CardInfo, CardPass};

#[component]
pub fn SellPassModal(
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
        <Sheet
            show=Signal::derive(move || show.get())
            on_close=Callback::new(move |()| set_show.set(false))
            title=move || i18n::t(lang.get(), "sell_pass_label")
            testid="sheet-sell-pass"
        >
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "modal_price")}</label>
                <input
                    type="number"
                    class="form-control"
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
            </div>
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "modal_valid_until")}</label>
                <input
                    type="date"
                    class="form-control"
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
            </div>
            {move || {
                if err.get().is_empty() {
                    view! { <div></div> }.into_any()
                } else {
                    view! { <div class="alert alert-error">{move || err.get()}</div> }.into_any()
                }
            }}
            <div class="sheet__actions">
                <button
                    class="btn btn--ghost"
                    on:click=move |_| set_show.set(false)
                >
                    {move || i18n::t(lang.get(), "modal_cancel")}
                </button>
                <button
                    class="btn btn--primary"
                    data-testid="sell-pass-confirm"
                    on:click=on_confirm
                >
                    {move || i18n::t(lang.get(), "modal_confirm")}
                </button>
            </div>
        </Sheet>
    }
}
