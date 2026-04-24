use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::i18n::{self, Lang};

use super::CardInfo;
use crate::util::parse_money;

const QUICK_TOPUP: [f64; 1] = [30.0];

#[component]
pub fn TopupSection(
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
                Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
            }
            set_loading.set(false);
        });
    };

    let on_custom = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let typed = custom_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let amount = parse_money(&typed).unwrap_or(0.0);
        do_topup(amount);
        if let Some(el) = custom_ref.get() {
            let el: &HtmlInputElement = &el;
            el.set_value("");
        }
    };

    view! {
        <div class="stack-12">
            <div class="section-label">
                {move || i18n::t(lang.get(), "quick_topup")}
            </div>
            <div class="chip-row">
                {QUICK_TOPUP.iter().map(|amt| {
                    let amount = *amt;
                    let label = format!("+{amount:.0} €");
                    view! {
                        <button
                            class="btn btn--compact btn--primary"
                            data-testid=format!("topup-{amount:.0}")
                            disabled=move || loading.get()
                            on:click=move |_| do_topup(amount)
                        >{label}</button>
                    }
                }).collect::<Vec<_>>()}
                <form class="inline-form inline-row" on:submit=on_custom>
                    <input
                        type="text"
                        inputmode="decimal"
                        autocomplete="off"
                        class="form-control input--narrow"
                        node_ref=custom_ref
                        placeholder=move || i18n::t(lang.get(), "custom_amount")
                    />
                    <button type="submit" class="btn btn--compact btn--primary" disabled=move || loading.get()>
                        {move || i18n::t(lang.get(), "topup")}
                    </button>
                </form>
            </div>
        </div>
    }
}
