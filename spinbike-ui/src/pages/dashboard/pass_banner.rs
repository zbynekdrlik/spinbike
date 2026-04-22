use leptos::prelude::*;

use crate::i18n::{self, Lang};

use super::sheets::EditPassDateSheet;
use super::{CardInfo, CardPass};

#[component]
pub fn PassBanner(
    pass: Option<CardPass>,
    barcode: String,
    set_selected: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let Some(p) = pass else {
        return view! { <div></div> }.into_any();
    };

    let tx_id = p.transaction_id;
    let current_date = p.valid_until;
    let show_edit_sheet = RwSignal::new(false);

    let date_str = current_date.format("%d.%m.%Y").to_string();
    let is_active = p.days_remaining >= 0;
    let days = p.days_remaining;
    let days_ago = -p.days_remaining;

    let banner_class = if is_active {
        "pass-banner pass-banner-ok"
    } else {
        "pass-banner pass-banner-expired"
    };
    let banner_testid = if is_active {
        "pass-banner-active"
    } else {
        "pass-banner-expired"
    };

    let title_view = if is_active {
        let date_str = date_str.clone();
        view! {
            <>
                {move || i18n::t(lang.get(), "pass_valid_until")}" "{date_str.clone()}
            </>
        }
        .into_any()
    } else {
        view! {
            <>
                {move || i18n::t(lang.get(), "pass_expired")}" "{days_ago}" "
                {move || i18n::t(lang.get(), "pass_days_ago")}
            </>
        }
        .into_any()
    };

    let sub_view = if is_active {
        view! {
            <>
                {days}" "{move || i18n::t(lang.get(), "pass_days_remaining")}
            </>
        }
        .into_any()
    } else {
        let date_str = date_str.clone();
        view! {
            <>
                {move || i18n::t(lang.get(), "pass_last_valid_until")}" "{date_str.clone()}
            </>
        }
        .into_any()
    };

    view! {
        <div class="group" style="margin-bottom: var(--s-3)">
            <div class=banner_class data-testid=banner_testid style="margin-bottom:0;border-radius:0;border:none">
                <div class="pass-banner-title" style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
                    <span style="flex:1">{title_view}</span>
                    <button
                        class="btn btn--compact btn--ghost"
                        data-testid="pass-date-edit"
                        title=move || i18n::t(lang.get(), "edit_pass_date")
                        on:click=move |_| show_edit_sheet.set(true)
                    >
                        {move || i18n::t(lang.get(), "edit_pass_date")}
                    </button>
                </div>
                <div class="pass-banner-sub">{sub_view}</div>
            </div>
        </div>
        <EditPassDateSheet
            show=show_edit_sheet
            tx_id=tx_id
            current_date=current_date
            barcode=barcode.clone()
            set_selected=set_selected
        />
    }
    .into_any()
}
