use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use spinbike_core::auth::Role;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

use super::CardInfo;
use super::deleted_email_conflict::DeletedEmailConflictDialog;
use super::helpers::event_target_value;

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
    #[serde(default)]
    role: Option<Role>,
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
    // #143 — soft-deleted-email conflict: (archived id, name, deleted_at).
    // When Some, the resolution dialog is shown instead of a plain error.
    let (conflict, set_conflict) = signal::<Option<(i64, String, Option<String>)>>(None);

    // The create action, reusable so the #143 free-email path can re-run it
    // (with the same field values, now that the address is free). Reads the
    // form signals fresh on each invocation.
    let run_create = Callback::new(move |()| {
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
            match api::post_json::<CreateUserReq, UserResp>("/api/users", &body).await {
                Ok(u) => {
                    set_msg.set(i18n::tf(
                        lang.get_untracked(),
                        "add_person_ok_format",
                        &[&u.name],
                    ));
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
                        role: u.role,
                        ..Default::default()
                    }));
                    set_show.set(false);
                }
                Err(e) => {
                    // #143: an email held by a soft-deleted account opens the
                    // restore / free-email dialog instead of a dead-end error.
                    if let Some(c) = e.deleted_email_conflict() {
                        set_conflict.set(Some(c));
                    } else {
                        set_err.set(e.message);
                    }
                }
            }
            set_loading.set(false);
        });
    });

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        run_create.run(());
    };

    view! {
        <form class="add-person-form" on:submit=on_submit>
            <label>
                {move || i18n::t(lang.get(), "name")}
                <input
                    type="text"
                    data-testid="add-person-name"
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
                view! { <p class="alert-error" data-testid="add-person-error">{err.get()}</p> }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}
        </form>
        {move || match conflict.get() {
            Some((cid, cname, cdel)) => view! {
                <DeletedEmailConflictDialog
                    conflict_id=cid
                    conflict_name=cname
                    conflict_deleted_at=cdel
                    // Email freed → retry the create; the address is now free.
                    on_email_freed=Callback::new(move |()| {
                        set_conflict.set(None);
                        run_create.run(());
                    })
                    // Old account restored → the new-person action is abandoned;
                    // close the form with a confirmation.
                    on_restored=Callback::new(move |()| {
                        set_conflict.set(None);
                        set_msg.set(
                            i18n::t(lang.get_untracked(), "deleted_email_restored_ok").to_string(),
                        );
                        set_show.set(false);
                    })
                    on_cancel=Callback::new(move |()| set_conflict.set(None))
                />
            }.into_any(),
            None => ().into_any(),
        }}
    }
}
