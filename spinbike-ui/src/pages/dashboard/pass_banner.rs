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

    let date_for_title = current_date;
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

    let line_view = if is_active {
        view! {
            <>
                {move || i18n::tf(
                    lang.get(),
                    "pass_active_oneline_format",
                    &[
                        &i18n::fmt_date(date_for_title, lang.get()),
                        &days.to_string(),
                    ],
                )}
            </>
        }
        .into_any()
    } else {
        view! {
            <>
                {move || i18n::tf(
                    lang.get(),
                    "pass_expired_oneline_format",
                    &[
                        &days_ago.to_string(),
                        &i18n::fmt_date(date_for_title, lang.get()),
                    ],
                )}
            </>
        }
        .into_any()
    };

    view! {
        <div class="group">
            <div class=format!("{banner_class} pass-banner--in-group") data-testid=banner_testid>
                <div class="pass-banner__line">
                    <span class="pass-banner__line-text">{line_view}</span>
                    <button
                        class="pass-banner__edit-btn"
                        data-testid="pass-date-edit"
                        aria-label=move || i18n::t(lang.get(), "edit_pass_date")
                        title=move || i18n::t(lang.get(), "edit_pass_date")
                        on:click=move |_| show_edit_sheet.set(true)
                    >
                        "✏"
                    </button>
                </div>
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
