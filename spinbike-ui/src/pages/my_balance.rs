use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
struct BalanceResp {
    cards: Vec<CardInfo>,
    transactions: Vec<TxInfo>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct CardInfo {
    id: i64,
    barcode: String,
    credit: f64,
    blocked: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct TxInfo {
    id: i64,
    amount: f64,
    action: String,
    created_at: String,
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
                    if balance.cards.is_empty() {
                        view! {
                            <div class="group">
                                <div class="list-row">
                                    <div class="list-row__main">
                                        <p class="text-muted">{move || i18n::t(lang.get(), "no_card_linked")}</p>
                                    </div>
                                    <div class="list-row__end">
                                        <a href="/link-card" class="btn btn--primary btn--compact">{move || i18n::t(lang.get(), "link_a_card")}</a>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        let card_views: Vec<_> = balance.cards.iter().map(|c| {
                            let barcode = c.barcode.clone();
                            let credit_val = format!("{:.0}", c.credit);
                            let is_blocked = c.blocked;
                            view! {
                                <div class="group mb-2">
                                    <div class="list-row">
                                        <div class="list-row__main">
                                            <div class="list-row__title">{format!("Card: {barcode}")}</div>
                                            <div class="list-row__sub">{move || if is_blocked { i18n::t(lang.get(), "blocked") } else { i18n::t(lang.get(), "active") }}</div>
                                        </div>
                                        <div class="card-balance">
                                            <span class="card-balance__num">{credit_val}</span>
                                            <span class="card-balance__unit">"CZK"</span>
                                        </div>
                                    </div>
                                </div>
                            }
                        }).collect();

                        let tx_view = if balance.transactions.is_empty() {
                            view! { <p class="text-muted mt-2">{i18n::t(lang.get(), "no_transactions")}</p> }.into_any()
                        } else {
                            let tx_rows: Vec<_> = balance.transactions.iter().map(|tx| {
                                let date = i18n::fmt_datetime_str(&tx.created_at, lang.get());
                                let action = tx.action.clone();
                                let amount = format!("{:.0} CZK", tx.amount);
                                view! {
                                    <div class="list-row">
                                        <div class="list-row__main">
                                            <div class="list-row__title">{action}</div>
                                            <div class="list-row__sub">{date}</div>
                                        </div>
                                        <div class="list-row__amount">{amount}</div>
                                    </div>
                                }
                            }).collect();
                            view! {
                                <div class="group">
                                    <div class="group__head">{i18n::t(lang.get(), "transactions")}</div>
                                    {tx_rows}
                                    <div class="list-row">
                                        <div class="list-row__end">
                                            <button class="btn btn--compact btn--ghost" data-testid="show-older">
                                                {i18n::t(lang.get(), "show_older")}
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            }.into_any()
                        };

                        view! {
                            <div>
                                {card_views}
                                {tx_view}
                            </div>
                        }.into_any()
                    }
                }
            }
        }}
    }
}
