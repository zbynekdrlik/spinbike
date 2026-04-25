use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

use super::helpers::format_datetime;
use super::TxnInfo;

#[component]
pub fn TransactionsList(
    card_id: i64,
    txn_refresh: RwSignal<u32>,
    set_msg: WriteSignal<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (txns, set_txns) = signal(Vec::<TxnInfo>::new());
    let (limit, set_limit) = signal(10usize);
    let (has_more, set_has_more) = signal(false);

    let lang_for_fetch = lang;
    Effect::new(move |_| {
        let _ = txn_refresh.get(); // reactive dependency — re-runs on increment
        let l = limit.get();
        spawn_local(async move {
            match api::get::<Vec<TxnInfo>>(&format!(
                "/api/cards/{card_id}/transactions?limit={l}"
            ))
            .await
            {
                Ok(t) => {
                    set_has_more.set(t.len() >= l);
                    set_txns.set(t);
                }
                Err(e) => set_msg.set(i18n::tf(
                    lang_for_fetch.get_untracked(),
                    "error_format",
                    &[&e],
                )),
            }
        });
    });

    view! {
        {move || {
            let t = txns.get();
            if t.is_empty() {
                return view! {
                    <div class="empty-state">{move || i18n::t(lang.get(), "no_transactions_card")}</div>
                }.into_any();
            }

            let l = lang.get();
            let rows: Vec<_> = t.iter().map(|tx| {
                let date = format_datetime(&tx.created_at, l);
                let action_key = match tx.action.as_str() {
                    "topup"  => "tx_action_topup",
                    "charge" => "tx_action_charge",
                    "visit"  => "tx_action_visit",
                    _ => "",
                };
                let action = if action_key.is_empty() {
                    tx.action.clone()
                } else {
                    i18n::t(l, action_key).to_string()
                };
                let until_suffix = tx
                    .valid_until
                    .map(|d| format!(" · {} {}", i18n::t(l, "tx_until_short"), i18n::fmt_date_short(d, l)))
                    .unwrap_or_default();
                let service = tx.service_name.clone().unwrap_or_else(|| "—".into());
                let amount = tx.amount;
                let amount_class = if amount >= 0.0 {
                    "list-row__amount list-row__amount--pos"
                } else {
                    "list-row__amount list-row__amount--neg"
                };
                let amount_str = format!("{:+.2}", amount);
                let is_voided = tx.deleted_at.is_some();
                let row_class = if is_voided {
                    "list-row txn-row--voided"
                } else if tx.action == "visit" {
                    "list-row txn-row-visit"
                } else {
                    "list-row"
                };
                let tx_id = tx.id;

                let voided_tag = if is_voided {
                    view! {
                        <span class="txn-voided-tag">{move || i18n::t(lang.get(), "voided")}</span>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                };

                let void_btn = if is_voided {
                    view! { <div></div> }.into_any()
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
                                Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
                            }
                        });
                    };
                    view! {
                        <div class="list-row__end">
                            <button
                                class="btn btn--compact btn--ghost"
                                data-testid="txn-void"
                                title=move || i18n::t(lang.get(), "void")
                                on:click=on_void
                            >"\u{2715}"</button>
                        </div>
                    }.into_any()
                };

                view! {
                    <div class=row_class>
                        <div class="list-row__main">
                            <div class="list-row__title">
                                {action}{until_suffix}
                                {voided_tag}
                            </div>
                            <div class="list-row__sub">{date}" · "{service}</div>
                        </div>
                        <div class=amount_class>{amount_str}</div>
                        {void_btn}
                    </div>
                }
            }).collect();

            view! {
                <div class="group">
                    {rows}
                </div>
            }.into_any()
        }}
        {move || {
            if has_more.get() {
                view! {
                    <button
                        class="btn btn--ghost btn--block"
                        data-testid="show-older"
                        on:click=move |_| set_limit.update(|n| *n += 20)
                    >
                        {move || i18n::t(lang.get(), "show_older")}
                    </button>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }
        }}
    }
}
