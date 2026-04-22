use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

use super::helpers::event_target_value;
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
    let (editing, set_editing) = signal(false);
    let (draft, set_draft) = signal(current_date);
    let (edit_err, set_edit_err) = signal(String::new());
    let (saving, set_saving) = signal(false);

    let barcode_for_refresh = barcode.clone();
    let on_save = move |_| {
        let vu = draft.get();
        let barcode = barcode_for_refresh.clone();
        set_edit_err.set(String::new());
        set_saving.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                valid_until: chrono::NaiveDate,
            }
            match api::patch::<Req, serde_json::Value>(
                &format!("/api/transactions/{tx_id}/valid-until"),
                &Req { valid_until: vu },
            )
            .await
            {
                Ok(_) => {
                    // Refresh the card so the banner picks up the new date and days_remaining.
                    match api::get::<CardInfo>(&format!("/api/cards/lookup/{barcode}")).await {
                        Ok(c) => {
                            set_selected.set(Some(c));
                            set_editing.set(false);
                        }
                        Err(e) => set_edit_err.set(e),
                    }
                }
                Err(e) => set_edit_err.set(e),
            }
            set_saving.set(false);
        });
    };

    let on_cancel = move |_| {
        set_edit_err.set(String::new());
        set_draft.set(current_date);
        set_editing.set(false);
    };

    let on_edit_click = move |_| {
        set_draft.set(current_date);
        set_edit_err.set(String::new());
        set_editing.set(true);
    };

    let on_date_input = move |ev: web_sys::Event| {
        let s = event_target_value(&ev);
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            set_draft.set(d);
        }
    };

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
        <div class=banner_class data-testid=banner_testid>
            <div class="pass-banner-title" style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
                <span>{title_view}</span>
                <button
                    class="btn btn-sm btn-outline"
                    data-testid="pass-date-edit"
                    title=move || i18n::t(lang.get(), "edit_pass_date")
                    on:click=on_edit_click
                    style:display=move || if editing.get() { "none" } else { "inline-block" }
                    style="padding:2px 8px;font-size:0.85rem"
                >"\u{270E}"</button>
            </div>
            <div class="pass-banner-sub">{sub_view}</div>
            <div
                class="mt-1"
                style:display=move || if editing.get() { "flex" } else { "none" }
                style="gap:6px;flex-wrap:wrap;align-items:center"
            >
                <input
                    type="date"
                    data-testid="pass-date-input"
                    prop:value=move || draft.get().format("%Y-%m-%d").to_string()
                    on:input=on_date_input
                />
                <button
                    class="btn btn-sm btn-primary"
                    data-testid="pass-date-save"
                    disabled=move || saving.get()
                    on:click=on_save
                >
                    {move || i18n::t(lang.get(), "save")}
                </button>
                <button
                    class="btn btn-sm btn-outline"
                    disabled=move || saving.get()
                    on:click=on_cancel
                >
                    {move || i18n::t(lang.get(), "cancel")}
                </button>
            </div>
            {move || {
                let e = edit_err.get();
                if e.is_empty() {
                    view! { <div></div> }.into_any()
                } else {
                    view! { <div class="alert alert-error mt-1" style="font-size:0.85rem">{e}</div> }.into_any()
                }
            }}
        </div>
    }
    .into_any()
}
