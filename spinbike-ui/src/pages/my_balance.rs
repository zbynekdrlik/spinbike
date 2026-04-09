use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;

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
        <h1 class="page-title">"My Balance"</h1>

        {move || {
            let e = error.get();
            if !e.is_empty() {
                return view! { <div class="alert alert-error">{e}</div> }.into_any();
            }
            if loading.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }

            match data.get() {
                None => view! { <div class="empty-state">"Unable to load balance"</div> }.into_any(),
                Some(balance) => {
                    if balance.cards.is_empty() {
                        view! {
                            <div class="card">
                                <p class="text-muted">"No card linked to your account."</p>
                                <a href="/link-card" class="btn btn-primary mt-2">"Link a Card"</a>
                            </div>
                        }.into_any()
                    } else {
                        let card_views: Vec<_> = balance.cards.iter().map(|c| {
                            let barcode = c.barcode.clone();
                            let credit_str = format!("{:.0} CZK", c.credit);
                            let status = if c.blocked { "BLOCKED" } else { "Active" };
                            view! {
                                <div class="card mb-2">
                                    <div class="flex justify-between items-center">
                                        <div>
                                            <div class="card-title">{format!("Card: {barcode}")}</div>
                                            <div class="text-muted">{status}</div>
                                        </div>
                                        <div style="font-size:1.5rem;font-weight:700;color:var(--primary)">
                                            {credit_str}
                                        </div>
                                    </div>
                                </div>
                            }
                        }).collect();

                        let tx_view = if balance.transactions.is_empty() {
                            view! { <p class="text-muted mt-2">"No transactions yet."</p> }.into_any()
                        } else {
                            let tx_rows: Vec<_> = balance.transactions.iter().map(|tx| {
                                let date = tx.created_at.clone();
                                let action = tx.action.clone();
                                let amount = format!("{:.0} CZK", tx.amount);
                                view! { <tr><td>{date}</td><td>{action}</td><td>{amount}</td></tr> }
                            }).collect();
                            view! {
                                <div>
                                    <h2 style="font-size:1rem;font-weight:700;margin:16px 0 8px">"Transactions"</h2>
                                    <table>
                                        <tbody>{tx_rows}</tbody>
                                    </table>
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
