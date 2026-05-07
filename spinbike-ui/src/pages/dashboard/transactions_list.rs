use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::dashboard::sheets::EditTxDateSheet;

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
                "/api/users/{card_id}/transactions?limit={l}"
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
                    <div class="empty-state" data-testid="transactions-list-empty">{move || i18n::t(lang.get(), "no_transactions_card")}</div>
                }.into_any();
            }

            let l = lang.get();
            let rows: Vec<_> = t.iter().map(|tx| {
                let date = i18n::fmt_datetime_str(&tx.created_at, l);
                let kind = tx.kind();
                let action_key = match kind {
                    spinbike_core::reports::EventKind::PassSale => "tx_label_pass",
                    spinbike_core::reports::EventKind::Visit    => "tx_label_visit",
                    spinbike_core::reports::EventKind::Charge   => "tx_label_charge",
                    spinbike_core::reports::EventKind::TopUp    => "tx_label_topup",
                    spinbike_core::reports::EventKind::Other    => "event_other",
                };
                let action = i18n::t(l, action_key).to_string();
                let until_suffix = tx
                    .valid_until
                    .map(|d| format!(" · {} {}", i18n::t(l, "tx_until_short"), i18n::fmt_date_short(d, l)))
                    .unwrap_or_default();
                let service = tx
                    .service_label(l)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "—".into());
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
                } else if matches!(kind, spinbike_core::reports::EventKind::Visit) {
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

                let note_initial = tx.note.clone().unwrap_or_default();
                // Per-row signal so the editor opens independently for each row.
                let (editing, set_editing) = signal(false);
                let (note_value, set_note_value) = signal(note_initial.clone());
                let editing_date = RwSignal::new(false);
                let current_date = tx
                    .created_at
                    .split_once(' ')
                    .map(|(d, _)| d)
                    .unwrap_or(&tx.created_at);
                let current_date = chrono::NaiveDate::parse_from_str(current_date, "%Y-%m-%d")
                    .unwrap_or_else(|_| chrono::Local::now().date_naive());

                let on_edit = move |_| set_editing.set(true);

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

                let on_saved = Callback::new(move |()| txn_refresh.update(|n| *n += 1));

                view! {
                    <div class=row_class data-testid="transaction-row">
                        <div class="list-row__main">
                            <div class="list-row__title">
                                {action}{until_suffix}
                                {voided_tag}
                            </div>
                            <div class="list-row__sub">{date}" · "{service}</div>
                            {move || if editing.get() {
                                // Editor closures defined inside the reactive
                                // block so each tick re-creates them (Leptos
                                // requires FnMut for view children — closures
                                // that consume non-Copy captures otherwise
                                // become FnOnce).
                                let note_initial = note_initial.clone();
                                let on_cancel = move |_| {
                                    set_note_value.set(note_initial.clone());
                                    set_editing.set(false);
                                };
                                let on_save = move |_| {
                                    let new_note = note_value.get_untracked();
                                    spawn_local(async move {
                                        #[derive(serde::Serialize)]
                                        struct Req { note: Option<String> }
                                        #[derive(serde::Deserialize)]
                                        struct Resp { #[allow(dead_code)] id: i64, #[allow(dead_code)] note: Option<String> }
                                        let body = Req {
                                            note: if new_note.trim().is_empty() { None } else { Some(new_note) },
                                        };
                                        match api::patch::<Req, Resp>(
                                            &format!("/api/transactions/{tx_id}/note"), &body
                                        ).await {
                                            Ok(_) => {
                                                set_editing.set(false);
                                                txn_refresh.update(|n| *n += 1);
                                            }
                                            Err(e) => set_msg.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
                                        }
                                    });
                                };
                                let on_input = move |ev: web_sys::Event| {
                                    let v = ev.target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                        .map(|el| el.value())
                                        .unwrap_or_default();
                                    set_note_value.set(v);
                                };
                                view! {
                                    <div class="list-row__note-edit">
                                        <input
                                            type="text"
                                            maxlength="200"
                                            class="form-control form-control--inline"
                                            data-testid="txn-note-edit-input"
                                            prop:value=move || note_value.get()
                                            on:input=on_input
                                        />
                                        <button class="btn btn--compact btn--primary"
                                                data-testid="txn-note-save"
                                                on:click=on_save>
                                            {move || i18n::t(lang.get(), "tx_note_save")}
                                        </button>
                                        <button class="btn btn--compact btn--ghost"
                                                data-testid="txn-note-cancel"
                                                on:click=on_cancel>
                                            {move || i18n::t(lang.get(), "tx_note_cancel")}
                                        </button>
                                    </div>
                                }.into_any()
                            } else if !note_value.get().is_empty() {
                                view! {
                                    <div class="list-row__note" data-testid="txn-note-text">
                                        {move || note_value.get()}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                        </div>
                        <div class=amount_class>{amount_str}</div>
                        {if !is_voided {
                            view! {
                                <div class="list-row__end list-row__end--column">
                                    <button
                                        class="btn btn--compact btn--ghost"
                                        data-testid="txn-note-edit"
                                        title=move || i18n::t(lang.get(), "tx_note_edit")
                                        on:click=on_edit
                                    >"\u{270e}"</button>
                                    <button
                                        class="btn btn--compact btn--ghost"
                                        data-testid="txn-date-edit"
                                        title=move || i18n::t(lang.get(), "tx_date_edit_tooltip")
                                        on:click=move |_| editing_date.set(true)
                                    >"\u{1F4C5}"</button>
                                    <button
                                        class="btn btn--compact btn--ghost"
                                        data-testid="txn-void"
                                        title=move || i18n::t(lang.get(), "void")
                                        on:click=on_void
                                    >"\u{2715}"</button>
                                </div>
                            }.into_any()
                        } else {
                            view! { <div></div> }.into_any()
                        }}
                    </div>
                    <EditTxDateSheet
                        show=editing_date
                        tx_id=tx_id
                        current_date=current_date
                        on_saved=on_saved
                    />
                }
            }).collect();

            view! {
                <div class="group" data-testid="transactions-list">
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
