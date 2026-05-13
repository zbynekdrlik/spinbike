//! Staff card dashboard — restructured panel (productivity-tuned).
//!
//! Layout change vs original:
//!   • Identity strip (one horizontal row): name + barcode + active-pass pill
//!     + last-visit/contact-toggle + BALANCE + close.
//!   • Body split into two columns:
//!       LEFT  — expired-pass warning (if any) + <ActionForm> + demoted
//!               edit/block/delete row.
//!       RIGHT — tabs (history/upcoming/persistent/overview) + content,
//!               ALWAYS VISIBLE without scrolling past the form.
//!
//! All signals, callbacks, child-component imports preserved 1:1 — only
//! JSX shape and class names changed. Visual styling lives in style.css
//! (.desk-panel / .desk-identity / .desk-pass-pill / .desk-body /
//! .desk-danger-row / .desk-link). See out/style-additions.css.

use chrono::NaiveDate;
use leptos::prelude::*;

use crate::components::{PersistentToggles, Segmented, UpcomingClasses};
use crate::i18n::{self, Lang};
use crate::relative_date::format_last_visit;

use super::action_form::ActionForm;
use super::block_button::BlockButton;
use super::edit_info_form::EditInfoForm;
use super::helpers::{full_name, pass_is_active};
use super::overview_tab::OverviewTab;
use super::pass_banner::PassBanner;
use super::sheets::{DeleteUserSheet, EditPassDateSheet};
use super::transactions_list::TransactionsList;
use super::{CardInfo, ServiceInfo};

/// Parse the SQLite `created_at` shape ("YYYY-MM-DD HH:MM:SS") into a date.
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
    #[prop(into)] on_close: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (show_edit, set_show_edit) = signal(false);
    let (show_contact, set_show_contact) = signal(false);
    // NEW — controls the inline pass-pill click → opens edit sheet.
    let show_pass_edit = RwSignal::new(false);
    let txn_refresh = RwSignal::new(0u32);
    let upc_tick = RwSignal::new(0u32);
    let (tab, set_tab) = signal("history".to_string());

    let card_id = card.id;
    let card_code = card.card_code.clone().unwrap_or_default();
    let card_code_for_sheet = card_code.clone();
    let name = full_name(&card);
    let credit = card.credit;
    let is_blocked = card.blocked;
    let has_pass = card.pass.is_some();
    let pass_active = pass_is_active(&card);
    let company = card.company.clone().unwrap_or_default();
    let phone = card.phone.clone().unwrap_or_default();
    let card_pass_for_banner = card.pass.clone();
    // Snapshot pass metadata as Copy primitives — usable in both the
    // pill (built once) and the edit sheet at the bottom of the view.
    let pass_meta: Option<(i64, NaiveDate, i32)> = card
        .pass
        .as_ref()
        .map(|p| (p.transaction_id, p.valid_until, p.days_remaining));
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

    // ── Active-pass pill — only when days_remaining >= 0.
    let active_pass_pill = match pass_meta {
        Some((_, valid_until, days)) if days >= 0 => view! {
            <button
                class="desk-pass-pill"
                data-testid="pass-banner-active"
                aria-label=move || i18n::t(lang.get(), "edit_pass_date")
                title=move || i18n::t(lang.get(), "edit_pass_date")
                on:click=move |_| show_pass_edit.set(true)
            >
                <span class="desk-pass-pill__dot"></span>
                <span class="desk-pass-pill__text">
                    {move || i18n::tf(
                        lang.get(),
                        "pass_active_oneline_format",
                        &[
                            &i18n::fmt_date(valid_until, lang.get()),
                            &days.to_string(),
                        ],
                    )}
                </span>
                <span class="desk-pass-pill__edit-hit" data-testid="pass-date-edit" aria-hidden="true"></span>
            </button>
        }
        .into_any(),
        _ => ().into_any(),
    };

    // ── Edit-pass sheet — rendered once at the bottom, only if a pass
    //    exists. Pre-existing PassBanner had its own sheet; we render
    //    ours directly here so the inline pill controls it.
    let pass_edit_sheet = match pass_meta {
        Some((tx_id, valid_until, _)) => view! {
            <EditPassDateSheet
                show=show_pass_edit
                tx_id=tx_id
                current_date=valid_until
                card_code=card_code_for_sheet
                set_selected=set_selected
            />
        }
        .into_any(),
        None => ().into_any(),
    };

    let has_contact = !company.is_empty() || !phone.is_empty();
    let company_for_show = company.clone();
    let phone_for_show = phone.clone();

    let credit_class = if credit < 0.0 {
        "desk-identity__balance desk-identity__balance--neg"
    } else {
        "desk-identity__balance"
    };

    view! {
        <>
        <div class="desk-panel" data-testid="action-panel">
            // ──────────── IDENTITY STRIP ────────────
            <div class="desk-identity">
                <div class="desk-identity__main">
                    <div class="desk-identity__heading">
                        <h2 class="desk-identity__name">{name}</h2>
                        <code class="desk-identity__barcode">{card_code.clone()}</code>
                        {if is_blocked {
                            view! {
                                <span class="badge badge--full">
                                    {move || i18n::t(lang.get(), "blocked")}
                                </span>
                            }.into_any()
                        } else { ().into_any() }}
                        {active_pass_pill}
                    </div>

                    <div class="desk-identity__meta">
                        {move || match parse_last_visit(&last_visit_at) {
                            Some(visited) => {
                                let today = crate::relative_date::today_local();
                                let label = i18n::t(lang.get(), "last_visit_label");
                                let value = format_last_visit(visited, today, lang.get());
                                view! {
                                    <span data-testid="card-last-visit">
                                        {label}": "{value}
                                    </span>
                                }.into_any()
                            }
                            None => ().into_any(),
                        }}
                        {if has_contact {
                            view! {
                                <>
                                    " · "
                                    <button
                                        class="desk-identity__contact-toggle"
                                        data-testid="toggle-contact"
                                        on:click=move |_| set_show_contact.update(|v| *v = !*v)
                                    >
                                        {move || if show_contact.get() {
                                            i18n::t(lang.get(), "card_hide_contact")
                                        } else {
                                            i18n::t(lang.get(), "card_show_contact")
                                        }}
                                    </button>
                                </>
                            }.into_any()
                        } else { ().into_any() }}
                    </div>

                    {move || if show_contact.get() && has_contact {
                        view! {
                            <div class="desk-identity__contact" data-testid="card-contact">
                                {if !company_for_show.is_empty() {
                                    view! { <span>{company_for_show.clone()}</span> }.into_any()
                                } else { ().into_any() }}
                                {if !phone_for_show.is_empty() {
                                    view! { <span>" · "{phone_for_show.clone()}</span> }.into_any()
                                } else { ().into_any() }}
                            </div>
                        }.into_any()
                    } else { ().into_any() }}
                </div>

                <div class=credit_class data-testid="card-credit">
                    <div class="desk-identity__balance-label">
                        {move || i18n::t(lang.get(), "my_balance_credit")}
                    </div>
                    <div class="desk-identity__balance-num">
                        {format!("{:.2}", credit)}
                        <span class="desk-identity__balance-eur">" €"</span>
                    </div>
                </div>

                <button
                    class="desk-identity__close"
                    on:click=move |e| on_close.run(e)
                    aria-label="close"
                    title="close"
                >"\u{2715}"</button>
            </div>

            // ──────────── BODY — 2 columns ────────────
            <div class="desk-body">
                <div class="desk-body__left">
                    // Expired-pass warning kept as the loud red banner.
                    // We only render PassBanner here when a pass exists
                    // AND it's expired — the inline pill above covers
                    // the active case.
                    {if has_pass && !pass_active {
                        view! {
                            <PassBanner
                                pass=card_pass_for_banner
                                card_code=card_code.clone()
                                set_selected=set_selected
                            />
                        }.into_any()
                    } else { ().into_any() }}

                    <ActionForm
                        card=card_for_form.clone()
                        services=services
                        set_selected=set_selected
                        msg=msg
                        set_msg=set_msg
                        set_txn_refresh=txn_refresh.write_only()
                    />

                    <div class="desk-danger-row">
                        <button
                            class="desk-link"
                            on:click=move |_| set_show_edit.update(|v| *v = !*v)
                        >
                            {move || i18n::t(lang.get(), "edit_info")}
                        </button>
                        <BlockButton
                            card_id=card_id
                            blocked=is_blocked
                            set_selected=set_selected
                            set_msg=set_msg
                        />
                        <button
                            class="desk-link desk-link--danger"
                            data-testid="delete-user-button"
                            on:click=move |_| editing_delete.set(true)
                        >
                            {move || i18n::t(lang.get(), "delete_user")}
                        </button>
                    </div>
                </div>

                <div class="desk-body__right">
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
                                        set_msg=set_msg
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
        </div>

        <EditInfoForm
            card=card_for_edit.clone()
            set_selected=set_selected
            set_msg=set_msg
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
            })
        />
        {pass_edit_sheet}
        </>
    }
}
