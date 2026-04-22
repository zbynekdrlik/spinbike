use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

use super::helpers::format_sk_datetime;
use super::TxnInfo;

#[component]
pub fn TransactionsList(
    card_id: i64,
    txn_refresh: RwSignal<u32>,
    set_msg: WriteSignal<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (txns, set_txns) = signal(Vec::<TxnInfo>::new());

    Effect::new(move |_| {
        let _ = txn_refresh.get(); // reactive dependency — re-runs on increment
        spawn_local(async move {
            if let Ok(t) =
                api::get::<Vec<TxnInfo>>(&format!("/api/cards/{card_id}/transactions")).await
            {
                set_txns.set(t);
            }
        });
    });

    view! {
        <div class="mt-2">
            <h3 style="font-size:0.95rem;margin-bottom:8px">{move || i18n::t(lang.get(), "transaction_history")}</h3>
            {move || {
                let t = txns.get();
                if t.is_empty() {
                    return view! { <p class="text-muted">{i18n::t(lang.get(), "no_transactions_card")}</p> }.into_any();
                }
                let rows: Vec<_> = t.iter().map(|tx| {
                    let date = format_sk_datetime(&tx.created_at);
                    let action = tx.action.clone();
                    let until_suffix = tx.valid_until
                        .map(|d| format!(" · until {}", d.format("%d.%m")))
                        .unwrap_or_default();
                    let service = tx.service_name.clone().unwrap_or_else(|| "—".into());
                    let amount = format!("{:+.2}", tx.amount);
                    let is_voided = tx.deleted_at.is_some();
                    let row_class = if is_voided {
                        "txn-row--voided"
                    } else if tx.action == "visit" {
                        "txn-row-visit"
                    } else {
                        "txn-row"
                    };
                    let tx_id = tx.id;
                    let voided_tag = if is_voided {
                        view! {
                            <span class="txn-voided-tag">{move || i18n::t(lang.get(), "voided")}</span>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    };
                    let action_cell = if is_voided {
                        view! { <td></td> }.into_any()
                    } else {
                        let on_void = move |_| {
                            let confirm_msg = i18n::t(lang.get(), "confirm_void");
                            let win = leptos::prelude::window();
                            if !win.confirm_with_message(confirm_msg).unwrap_or(false) {
                                return;
                            }
                            spawn_local(async move {
                                match api::delete_empty(&format!("/api/transactions/{tx_id}")).await {
                                    Ok(()) => txn_refresh.update(|n| *n += 1),
                                    Err(e) => set_msg.set(format!("Error: {e}")),
                                }
                            });
                        };
                        view! {
                            <td>
                                <button
                                    class="btn btn-sm btn-outline"
                                    data-testid="txn-void"
                                    title=move || i18n::t(lang.get(), "void")
                                    on:click=on_void
                                    style="padding:2px 8px;font-size:0.85rem"
                                >"\u{2715}"</button>
                            </td>
                        }.into_any()
                    };
                    view! {
                        <tr class=row_class>
                            <td>{date}</td>
                            <td>{action}{until_suffix}</td>
                            <td>{service}{voided_tag}</td>
                            <td class="txn-amount">{amount}</td>
                            {action_cell}
                        </tr>
                    }
                }).collect();
                view! {
                    <div style="overflow-x:auto">
                        <table class="data-table">
                            <thead>
                                <tr>
                                    <th>{i18n::t(lang.get(), "date")}</th>
                                    <th>{i18n::t(lang.get(), "action")}</th>
                                    <th>{i18n::t(lang.get(), "service")}</th>
                                    <th>{i18n::t(lang.get(), "amount")}</th>
                                    <th></th>
                                </tr>
                            </thead>
                            <tbody>{rows}</tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </div>
    }
}
