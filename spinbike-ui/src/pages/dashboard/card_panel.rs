use leptos::prelude::*;

use crate::components::{PersistentToggles, Segmented, UpcomingClasses};
use crate::i18n::{self, Lang};

use super::block_button::BlockButton;
use super::charge_section::ChargeSection;
use super::edit_info_form::EditInfoForm;
use super::helpers::{full_name, pass_is_active};
use super::pass_banner::PassBanner;
use super::sell_pass_modal::SellPassModal;
use super::topup_section::TopupSection;
use super::transactions_list::TransactionsList;
use super::{CardInfo, ServiceInfo};

#[component]
pub fn CardActionPanel(
    card: CardInfo,
    services: ReadSignal<Vec<ServiceInfo>>,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    #[prop(into)] on_close: Callback<web_sys::MouseEvent>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (show_edit, set_show_edit) = signal(false);
    let (show_sell_pass, set_show_sell_pass) = signal(false);
    // Counter incremented after a log-visit to re-trigger the history fetch.
    let txn_refresh = RwSignal::new(0u32);
    // Counter driving UpcomingClasses + PersistentToggles refetches after a
    // book/cancel/toggle action updates underlying booking state.
    let upc_tick = RwSignal::new(0u32);
    // Active tab for the card detail (history default, then upcoming, persistent).
    let (tab, set_tab) = signal("history".to_string());

    let card_id = card.id;
    let barcode = card.barcode.clone();
    let name = full_name(&card);
    let credit = card.credit;
    let is_blocked = card.blocked;
    let company = card.company.clone().unwrap_or_default();
    let phone = card.phone.clone().unwrap_or_default();
    let card_pass = card.pass.clone();
    let card_for_edit = card.clone();
    let card_for_modal = card.clone();
    let pass_active = pass_is_active(&card);

    // Segmented tab items
    let tab_items = vec![
        ("history".to_string(), i18n::t(lang.get_untracked(), "tab_history").to_string()),
        ("upcoming".to_string(), i18n::t(lang.get_untracked(), "tab_upcoming").to_string()),
        ("persistent".to_string(), i18n::t(lang.get_untracked(), "tab_persistent").to_string()),
    ];

    view! {
        <>
        <div class="card mb-2" data-testid="action-panel">
            // ── Header bar ───────────────────────────────────────────────
            <div class="card-header">
                <div class="card-header__main">
                    <div class="card-title">{name}</div>
                    <div class="card-header__meta">
                        <code>{barcode.clone()}</code>
                        {if !company.is_empty() { format!(" · {company}") } else { String::new() }}
                        {if !phone.is_empty() { format!(" · {phone}") } else { String::new() }}
                    </div>
                </div>
                <button
                    class="btn btn--compact btn--ghost"
                    on:click=move |e| on_close.run(e)
                    title="close"
                >"\u{2715}"</button>
            </div>

            // ── Pass banner ───────────────────────────────────────────────
            <PassBanner pass=card_pass barcode=barcode.clone() set_selected=set_selected />

            // ── Balance block ─────────────────────────────────────────────
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
                    view! {}.into_any()
                }}
            </div>

            // ── Primary actions row (Charge + Topup side-by-side) ─────────
            <div class="action-row">
                <ChargeSection
                    card_id=card_id
                    services=services
                    set_selected=set_selected
                    set_msg=set_msg
                    pass_active=pass_active
                    set_txn_refresh=txn_refresh.write_only()
                />
                <TopupSection card_id=card_id set_selected=set_selected set_msg=set_msg />
            </div>

            // ── Sell pass button ──────────────────────────────────────────
            <div class="stack-12">
                <button
                    class="btn btn--hero btn--pass btn--block"
                    data-testid="sell-pass-btn"
                    on:click=move |_| set_show_sell_pass.set(true)
                >
                    {move || {
                        let price = services.get().iter()
                            .find(|s| s.name == "Monthly pass")
                            .map(|s| s.default_price)
                            .unwrap_or(35.0);
                        format!("{} {:.2}", i18n::t(lang.get(), "sell_pass_label"), price)
                    }}
                </button>
            </div>

            // ── Secondary actions (Edit info + Block) ─────────────────────
            <div class="action-row stack-12">
                <button
                    class="btn btn--ghost"
                    on:click=move |_| set_show_edit.update(|v| *v = !*v)
                >
                    {move || i18n::t(lang.get(), "edit_info")}
                </button>
                <BlockButton card_id=card_id blocked=is_blocked set_selected=set_selected set_msg=set_msg />
            </div>

            // ── Segmented tab control ─────────────────────────────────────
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
                            _ => view! { <div></div> }.into_any(),
                        }
                    }}
                </div>
            </div>
        </div>

        // ── Sheets (rendered outside the card, overlaid via CSS) ───────────
        <EditInfoForm
            card=card_for_edit.clone()
            set_selected=set_selected
            set_msg=set_msg
            show=Signal::derive(move || show_edit.get())
            on_close=Callback::new(move |()| set_show_edit.set(false))
        />

        <SellPassModal
            card=card_for_modal.clone()
            set_selected=set_selected
            show=show_sell_pass
            set_show=set_show_sell_pass
            monthly_pass_price=services.get_untracked().iter()
                .find(|s| s.name == "Monthly pass")
                .map(|s| s.default_price)
                .unwrap_or(35.0)
        />
        </>
    }
}
