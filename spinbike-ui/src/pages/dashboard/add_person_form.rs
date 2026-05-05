use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};

use crate::api;
use crate::i18n::{self, Lang};

use super::helpers::event_target_value;
use super::CardInfo;

#[derive(Serialize)]
struct CreateUserReq {
    name: String,
    email: Option<String>,
    phone: Option<String>,
    company: Option<String>,
    card_code: Option<String>,
}

#[derive(Deserialize, Clone)]
struct UserResp {
    id: i64,
    email: Option<String>,
    name: String,
    phone: Option<String>,
    company: Option<String>,
    card_code: Option<String>,
    credit: f64,
    blocked: bool,
    allow_debit: bool,
}

#[component]
pub fn AddPersonForm(
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    set_show: WriteSignal<bool>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang");
    let (name, set_name) = signal(String::new());
    let (email, set_email) = signal(String::new());
    let (phone, set_phone) = signal(String::new());
    let (company, set_company) = signal(String::new());
    let (card_code, set_card_code) = signal(String::new());
    let (err, set_err) = signal(String::new());
    let (loading, set_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if loading.get_untracked() {
            return;
        }
        let n = name.get_untracked();
        if n.trim().is_empty() {
            set_err.set(i18n::t(lang.get_untracked(), "name_required").to_string());
            return;
        }
        set_err.set(String::new());
        set_loading.set(true);
        let to_opt = |s: String| {
            if s.trim().is_empty() {
                None
            } else {
                Some(s.trim().to_string())
            }
        };
        let body = CreateUserReq {
            name: n.trim().to_string(),
            email: to_opt(email.get_untracked()),
            phone: to_opt(phone.get_untracked()),
            company: to_opt(company.get_untracked()),
            card_code: to_opt(card_code.get_untracked()),
        };
        spawn_local(async move {
            match api::post::<CreateUserReq, UserResp>("/api/users", &body).await {
                Ok(u) => {
                    set_msg.set(i18n::tf(lang.get_untracked(), "add_person_ok_format", &[&u.name]));
                    set_selected.set(Some(CardInfo {
                        id: u.id,
                        card_code: u.card_code,
                        name: u.name,
                        email: u.email,
                        phone: u.phone,
                        company: u.company,
                        credit: u.credit,
                        blocked: u.blocked,
                        allow_debit: u.allow_debit,
                        ..Default::default()
                    }));
                    set_show.set(false);
                }
                Err(e) => set_err.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <form class="add-person-form" on:submit=on_submit>
            <label>
                {move || i18n::t(lang.get(), "name")}
                <input
                    type="text"
                    required
                    prop:value=move || name.get()
                    on:input=move |ev| set_name.set(event_target_value(&ev))
                />
            </label>
            <label>
                {move || i18n::t(lang.get(), "email")}
                " "
                {move || i18n::t(lang.get(), "optional_paren")}
                <input
                    type="email"
                    prop:value=move || email.get()
                    on:input=move |ev| set_email.set(event_target_value(&ev))
                />
            </label>
            <label>
                {move || i18n::t(lang.get(), "phone")}
                " "
                {move || i18n::t(lang.get(), "optional_paren")}
                <input
                    type="text"
                    prop:value=move || phone.get()
                    on:input=move |ev| set_phone.set(event_target_value(&ev))
                />
            </label>
            <label>
                {move || i18n::t(lang.get(), "company")}
                " "
                {move || i18n::t(lang.get(), "optional_paren")}
                <input
                    type="text"
                    prop:value=move || company.get()
                    on:input=move |ev| set_company.set(event_target_value(&ev))
                />
            </label>
            <label>
                {move || i18n::t(lang.get(), "card_code")}
                " "
                {move || i18n::t(lang.get(), "optional_paren")}
                <input
                    type="text"
                    prop:value=move || card_code.get()
                    on:input=move |ev| set_card_code.set(event_target_value(&ev))
                />
            </label>
            <button
                type="submit"
                data-testid="add-person-submit"
                class="btn btn--primary"
                disabled=move || loading.get()
            >
                {move || i18n::t(lang.get(), "add_person_submit")}
            </button>
            {move || if !err.get().is_empty() {
                view! { <p class="alert-error">{err.get()}</p> }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}
        </form>
    }
}
