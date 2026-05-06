use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
struct BalanceResp {
    user_id: i64,
    credit: f64,
    card_code: Option<String>,
}

#[component]
pub fn MyBalancePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<BalanceResp>);
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());

    Effect::new(move || {
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<BalanceResp>("/api/my/balance").await {
                Ok(d) => {
                    set_data.set(Some(d));
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    });

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "my_balance")}</h1>

        {move || {
            let e = error.get();
            if !e.is_empty() {
                return view! { <div class="alert alert-error">{e}</div> }.into_any();
            }
            if loading.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }

            match data.get() {
                None => view! { <div class="empty-state">{i18n::t(lang.get(), "unable_to_load")}</div> }.into_any(),
                Some(balance) => {
                    let credit_val = format!("{:.2}", balance.credit);
                    let card_label = balance.card_code.clone().unwrap_or_else(|| "—".to_string());
                    view! {
                        <div class="group mb-2">
                            <div class="list-row">
                                <div class="list-row__main">
                                    <div class="list-row__title">
                                        {move || i18n::t(lang.get(), "card_code")}
                                        {": "}
                                        {card_label.clone()}
                                    </div>
                                    <div class="list-row__sub">
                                        {move || i18n::t(lang.get(), "balance")}
                                    </div>
                                </div>
                                <div class="card-balance">
                                    <span class="card-balance__num">{credit_val}</span>
                                    <span class="card-balance__unit">"\u{20ac}"</span>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                }
            }
        }}
    }
}
