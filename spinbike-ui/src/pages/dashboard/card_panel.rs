use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::components::{PersistentToggles, UpcomingClasses};
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

    view! {
        <div class="card mb-2" data-testid="action-panel">
            <div class="card-header" style="display:flex;justify-content:space-between;align-items:flex-start;gap:8px">
                <div>
                    <div class="card-title" style="font-size:1.1rem">{name}</div>
                    <div class="text-muted" style="font-size:0.85rem">
                        <code>{barcode.clone()}</code>
                        {if !company.is_empty() { format!(" · {company}") } else { String::new() }}
                        {if !phone.is_empty() { format!(" · {phone}") } else { String::new() }}
                    </div>
                </div>
                <button class="btn btn-sm btn-outline" on:click=move |e| on_close.run(e) title="close">"\u{2715}"</button>
            </div>

            <PassBanner pass=card_pass barcode=barcode.clone() set_selected=set_selected />

            <div
                class=if credit < 0.0 { "credit-negative" } else { "" }
                style="font-size:1.4rem;font-weight:700;margin:8px 0"
                data-testid="card-credit"
            >
                {format!("{credit:.2} €")}
                {if is_blocked {
                    view! { <span class="badge badge-full" style="margin-left:8px;font-size:0.75rem">{i18n::t(lang.get(), "blocked")}</span> }.into_any()
                } else { view! {}.into_any() }}
            </div>

            // Ordered by actual staff usage frequency: charge (pay-for-service)
            // is the most-common action, then top-up. Edit/block stay secondary.
            <ChargeSection
                card_id=card_id
                services=services
                set_selected=set_selected
                set_msg=set_msg
                pass_active=pass_is_active(&card)
                set_txn_refresh=txn_refresh.write_only()
            />
            <TopupSection card_id=card_id set_selected=set_selected set_msg=set_msg />

            <div class="mt-2">
                <button
                    class="btn btn-pass"
                    data-testid="sell-pass-btn"
                    on:click=move |_| set_show_sell_pass.set(true)
                >
                    {move || {
                        let price = services.get().iter()
                            .find(|s| s.name == "Monthly pass")
                            .map(|s| s.default_price)
                            .unwrap_or(35.0);
                        format!("{} {:.2}", i18n::t(lang.get(), "sell_monthly_pass"), price)
                    }}
                </button>
            </div>

            <div class="flex gap-1 mt-2" style="flex-wrap:wrap">
                <button
                    class="btn btn-sm btn-outline"
                    on:click=move |_| set_show_edit.update(|v| *v = !*v)
                >
                    {move || i18n::t(lang.get(), "edit")}
                </button>
                <BlockButton card_id=card_id blocked=is_blocked set_selected=set_selected set_msg=set_msg />
            </div>

            {move || {
                if show_edit.get() {
                    view! { <EditInfoForm card=card_for_edit.clone() set_selected=set_selected set_msg=set_msg set_show_edit=set_show_edit /> }.into_any()
                } else { view! { <span></span> }.into_any() }
            }}

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

            <div class="tabbar">
                <button
                    class=move || if tab.get() == "history" { "tab tab--active" } else { "tab" }
                    on:click=move |_| set_tab.set("history".to_string())
                    data-testid="tab-history"
                >
                    {move || i18n::t(lang.get(), "tab_history")}
                </button>
                <button
                    class=move || if tab.get() == "upcoming" { "tab tab--active" } else { "tab" }
                    on:click=move |_| set_tab.set("upcoming".to_string())
                    data-testid="tab-upcoming"
                >
                    {move || i18n::t(lang.get(), "tab_upcoming")}
                </button>
                <button
                    class=move || if tab.get() == "persistent" { "tab tab--active" } else { "tab" }
                    on:click=move |_| set_tab.set("persistent".to_string())
                    data-testid="tab-persistent"
                >
                    {move || i18n::t(lang.get(), "tab_persistent")}
                </button>
            </div>
            <div class="tab-body">
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
    }
}
