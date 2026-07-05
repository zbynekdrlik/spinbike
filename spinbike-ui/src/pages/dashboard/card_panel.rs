use chrono::NaiveDate;
use leptos::prelude::*;

use crate::components::{PersistentToggles, Segmented, UpcomingClasses};
use crate::i18n::{self, Lang};
use crate::relative_date::format_last_visit;

use super::action_form::ActionForm;
use super::block_button::BlockButton;
use super::edit_info_form::EditInfoForm;
use super::helpers::full_name;
use super::overview_tab::OverviewTab;
use super::pass_banner::PassBanner;
use super::sheets::DeleteUserSheet;
use super::transactions_list::TransactionsList;
use super::{CardInfo, ServiceInfo};

/// Parse the SQLite `created_at` shape ("YYYY-MM-DD HH:MM:SS") into a date.
/// Returns None if the input doesn't match the expected leading 10 chars.
fn parse_last_visit(s: &Option<String>) -> Option<NaiveDate> {
    let s = s.as_deref()?;
    if s.len() < 10 {
        return None;
    }
    NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d").ok()
}

#[component]
pub fn CardActionPanel(
    card: CardInfo,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    msg: ReadSignal<String>,
    set_msg: WriteSignal<String>,
    /// Red-alert channel (mod.rs:473-478) — errors from BlockButton,
    /// TransactionsList and EditInfoForm route here instead of the green
    /// `set_msg` success channel (#126). ActionForm/AddPersonForm keep
    /// their own local red error signal and are NOT wired to this.
    set_err: WriteSignal<String>,
    #[prop(into)] on_close: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (show_edit, set_show_edit) = signal(false);
    let (show_contact, set_show_contact) = signal(false);
    let txn_refresh = RwSignal::new(0u32);
    let upc_tick = RwSignal::new(0u32);
    let (tab, set_tab) = signal("history".to_string());

    let card_id = card.id;
    let card_code = card.card_code.clone().unwrap_or_default();
    let name = full_name(&card);
    let credit = card.credit;
    let is_blocked = card.blocked;
    let company = card.company.clone().unwrap_or_default();
    let phone = card.phone.clone().unwrap_or_default();
    let card_pass = card.pass.clone();
    let last_visit_at = card.last_visit_at.clone();
    let card_for_edit = card.clone();
    let card_for_form = card.clone();

    let editing_delete = RwSignal::new(false);
    let delete_user_id = card_id;
    let delete_user_name = card.name.clone();
    let delete_user_balance = credit;
    let delete_user_pass_end = card.pass.as_ref().map(|p| p.valid_until);

    let tab_items = vec![
        (
            "history".to_string(),
            i18n::t(lang.get_untracked(), "tab_history").to_string(),
        ),
        (
            "upcoming".to_string(),
            i18n::t(lang.get_untracked(), "tab_upcoming").to_string(),
        ),
        (
            "persistent".to_string(),
            i18n::t(lang.get_untracked(), "tab_persistent").to_string(),
        ),
        (
            "overview".to_string(),
            i18n::t(lang.get_untracked(), "tab_overview").to_string(),
        ),
    ];

    view! {
        <>
        <div class="card mb-2" data-testid="action-panel">
            <div class="card-header">
                <div class="card-header__main">
                    <div class="card-title">
                        <span class="card-title__name">{name}</span>
                        " "
                        <code class="card-title__barcode">{card_code.clone()}</code>
                    </div>
                    {move || {
                        match parse_last_visit(&last_visit_at) {
                            Some(visited) => {
                                let today = crate::relative_date::today_local();
                                let label = i18n::t(lang.get(), "last_visit_label");
                                let value = format_last_visit(visited, today, lang.get());
                                view! {
                                    <div class="card-title__last-visit" data-testid="card-last-visit">
                                        {label} ": " {value}
                                    </div>
                                }
                                .into_any()
                            }
                            None => ().into_any(),
                        }
                    }}
                </div>
                <button
                    class="btn btn--compact btn--ghost"
                    on:click=move |e| on_close.run(e)
                    title="close"
                >"\u{2715}"</button>
            </div>

            {
                let has_contact = !company.is_empty() || !phone.is_empty();
                let company_for_show = company.clone();
                let phone_for_show = phone.clone();
                view! {
                    {move || if has_contact {
                        view! {
                            <button
                                class="btn btn--compact btn--ghost"
                                data-testid="toggle-contact"
                                on:click=move |_| set_show_contact.update(|v| *v = !*v)
                            >
                                {move || if show_contact.get() {
                                    i18n::t(lang.get(), "card_hide_contact")
                                } else {
                                    i18n::t(lang.get(), "card_show_contact")
                                }}
                            </button>
                        }.into_any()
                    } else { ().into_any() }}
                    {move || if show_contact.get() {
                        view! {
                            <div class="group" data-testid="card-contact">
                                <div class="list-row">
                                    <div class="list-row__main">
                                        <div class="list-row__sub">{company_for_show.clone()}</div>
                                        <div class="list-row__sub">{phone_for_show.clone()}</div>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}
                }
            }

            <PassBanner pass=card_pass card_code=card_code.clone() set_selected=set_selected />

            <div
                class=if credit < 0.0 { "card-balance card-balance--negative" } else { "card-balance" }
                data-testid="card-credit"
            >
                <span class="card-balance__num">{format!("{:.2}", credit)}</span>
                " "
                <span class="card-balance__unit">"€"</span>
                {if is_blocked {
                    view! {
                        <span class="badge badge--full badge--inline">
                            {move || i18n::t(lang.get(), "blocked")}
                        </span>
                    }.into_any()
                } else {
                    ().into_any()
                }}
            </div>

            <ActionForm
                card=card_for_form.clone()
                services=services
                set_selected=set_selected
                msg=msg
                set_msg=set_msg
                set_txn_refresh=txn_refresh.write_only()
            />

            <div class="action-row stack-12">
                <button
                    class="btn btn--ghost"
                    data-testid="edit-info-button"
                    on:click=move |_| set_show_edit.update(|v| *v = !*v)
                >
                    {move || i18n::t(lang.get(), "edit_info")}
                </button>
                <BlockButton card_id=card_id blocked=is_blocked set_selected=set_selected set_msg=set_msg set_err=set_err />
                <button
                    class="btn btn--danger"
                    data-testid="delete-user-button"
                    on:click=move |_| editing_delete.set(true)
                >
                    {move || i18n::t(lang.get(), "delete_user")}
                </button>
            </div>

            <div class="stack-16">
                <Segmented
                    items=tab_items
                    active=Signal::derive(move || tab.get())
                    on_change=Callback::new(move |key: String| set_tab.set(key))
                    testid_prefix="tab"
                />
                <div class="seg-body">
                    {move || {
                        let t = tab.get();
                        match t.as_str() {
                            "history" => view! {
                                <TransactionsList
                                    card_id=card_id
                                    txn_refresh=txn_refresh
                                    set_err=set_err
                                />
                            }.into_any(),
                            "upcoming" => view! {
                                <UpcomingClasses
                                    card_id=card_id
                                    refresh_tick=upc_tick
                                    on_changed=Callback::new(move |()| upc_tick.update(|n| *n += 1))
                                />
                            }.into_any(),
                            "persistent" => view! {
                                <PersistentToggles
                                    card_id=card_id
                                    on_changed=Callback::new(move |()| upc_tick.update(|n| *n += 1))
                                />
                            }.into_any(),
                            "overview" => view! {
                                <OverviewTab card_id=card_id />
                            }.into_any(),
                            _ => view! { <div></div> }.into_any(),
                        }
                    }}
                </div>
            </div>
        </div>

        <EditInfoForm
            card=card_for_edit.clone()
            set_selected=set_selected
            set_msg=set_msg
            set_err=set_err
            show=Signal::derive(move || show_edit.get())
            on_close=Callback::new(move |()| set_show_edit.set(false))
        />
        <DeleteUserSheet
            show=editing_delete
            user_id=delete_user_id
            name=delete_user_name
            balance=delete_user_balance
            active_pass_end=delete_user_pass_end
            on_saved=Callback::new(move |()| {
                set_selected.set(None);
                // Deep-review follow-up on #126: this is a SECOND panel-close
                // path (besides mod.rs's clear_selection, wired to the ×
                // button) that was bypassing the msg/err clear entirely — a
                // stale red error from an earlier failed action (block/edit/
                // void) would survive a successful delete-and-close.
                set_msg.set(String::new());
                set_err.set(String::new());
            })
        />
        </>
    }
}
