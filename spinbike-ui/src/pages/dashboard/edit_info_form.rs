use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::auth;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

use super::CardInfo;

/// Admin user-edit form. Refreshes its inputs on EVERY reopen from the
/// authoritative server state via `GET /api/users/lookup/{card_code}` so
/// the user always sees the actual saved values, not a stale capture of
/// the previously-loaded card prop.
///
/// Field-level guards on the server:
/// - `allow_self_entry`: admin-only.
/// - `password`: admin OR self.
///
/// Client-side: this form is only opened from the staff dashboard, so the
/// caller is always staff or admin. The `allow_self_entry` checkbox and
/// the password field render only for admin (read role from auth context).
#[component]
pub fn EditInfoForm(
    card: CardInfo,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    /// Signal controlling visibility — the parent sets it to false to hide.
    show: Signal<bool>,
    /// Called when the sheet should close (cancel or save success).
    #[prop(into)]
    on_close: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let card_id = card.id;
    let initial_code = card.card_code.clone();

    // Reactive form-state signals. Initialised from the props once; refreshed
    // from the server on every show=true transition.
    let (name_val, set_name_val) = signal(card.name.clone());
    let (email_val, set_email_val) = signal(card.email.clone().unwrap_or_default());
    let (company_val, set_company_val) = signal(card.company.clone().unwrap_or_default());
    let (phone_val, set_phone_val) = signal(card.phone.clone().unwrap_or_default());
    let (allow_self_entry, set_allow_self_entry) = signal(card.allow_self_entry);
    // Password input is intentionally not pre-populated and not refreshed —
    // it represents "set new password" and must always start empty.

    // Read the caller's role to gate the admin-only fields client-side.
    // The server-side guard is the authoritative gate; this just avoids
    // showing controls the staff role can't actually use.
    let is_admin = auth::get_user()
        .map(|u| u.role == "admin")
        .unwrap_or(false);

    // Refresh from server on every show=true transition. Uses the card_code
    // lookup endpoint to fetch the authoritative current state — covers the
    // case where the user was edited, saved, closed, reopened: previous
    // implementation captured initial values into StoredValue and never
    // refreshed, so reopening showed stale data.
    let lookup_code = initial_code.clone();
    Effect::new(move |prev_shown: Option<bool>| {
        let now_shown = show.get();
        // Edge: only fire on the false→true transition (first mount counts).
        let should_fetch = match prev_shown {
            None => now_shown,
            Some(was) => !was && now_shown,
        };
        if !should_fetch {
            return now_shown;
        }
        let code = lookup_code.clone();
        if let Some(code) = code {
            spawn_local(async move {
                if let Ok(c) =
                    api::get::<CardInfo>(&format!("/api/users/lookup/{code}")).await
                {
                    set_name_val.set(c.name);
                    set_email_val.set(c.email.unwrap_or_default());
                    set_company_val.set(c.company.unwrap_or_default());
                    set_phone_val.set(c.phone.unwrap_or_default());
                    set_allow_self_entry.set(c.allow_self_entry);
                }
            });
        }
        now_shown
    });

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            let name_ref = NodeRef::<leptos::html::Input>::new();
            let email_ref = NodeRef::<leptos::html::Input>::new();
            let company_ref = NodeRef::<leptos::html::Input>::new();
            let phone_ref = NodeRef::<leptos::html::Input>::new();
            let password_ref = NodeRef::<leptos::html::Input>::new();
            let (loading, set_loading) = signal(false);

            let on_close_cancel = on_close.clone();
            let on_close_btn = on_close.clone();
            let on_close_save = on_close.clone();

            let initial_allow_se = allow_self_entry.get_untracked();

            let on_submit = move |ev: web_sys::SubmitEvent| {
                ev.prevent_default();
                let read = |n: &NodeRef<leptos::html::Input>| {
                    n.get()
                        .map(|el| {
                            let el: &HtmlInputElement = &el;
                            el.value()
                        })
                        .unwrap_or_default()
                };
                let name = read(&name_ref);
                let email = read(&email_ref);
                let company = read(&company_ref);
                let phone = read(&phone_ref);
                let password = read(&password_ref);
                let allow_se = allow_self_entry.get_untracked();

                set_loading.set(true);
                let on_close_inner = on_close_save.clone();
                spawn_local(async move {
                    #[derive(serde::Serialize)]
                    struct Req {
                        #[serde(skip_serializing_if = "Option::is_none")]
                        name: Option<String>,
                        #[serde(skip_serializing_if = "Option::is_none")]
                        email: Option<String>,
                        #[serde(skip_serializing_if = "Option::is_none")]
                        company: Option<String>,
                        #[serde(skip_serializing_if = "Option::is_none")]
                        phone: Option<String>,
                        #[serde(skip_serializing_if = "Option::is_none")]
                        allow_self_entry: Option<bool>,
                        #[serde(skip_serializing_if = "Option::is_none")]
                        password: Option<String>,
                    }
                    let req = Req {
                        name: if name.trim().is_empty() { None } else { Some(name) },
                        email: if email.trim().is_empty() { None } else { Some(email) },
                        company: if company.is_empty() { None } else { Some(company) },
                        phone: if phone.is_empty() { None } else { Some(phone) },
                        // Only admin sends allow_self_entry, and only when the
                        // current checkbox state DIFFERS from the value the
                        // form opened with. Staff role sending Some(_) at all
                        // would get 403 from the server; admin sending an
                        // unchanged value is a no-op but pollutes the diff.
                        allow_self_entry: if is_admin && allow_se != initial_allow_se {
                            Some(allow_se)
                        } else {
                            None
                        },
                        // Password only when the user typed something. Server
                        // checks admin OR self.
                        password: if password.is_empty() { None } else { Some(password) },
                    };
                    match api::put_json::<Req, CardInfo>(&format!("/api/users/{card_id}"), &req).await {
                        Ok(c) => {
                            set_selected.set(Some(c));
                            set_msg.set(i18n::t(lang.get_untracked(), "saved").to_string());
                            on_close_inner.run(());
                        }
                        Err(e) => set_msg.set(i18n::tf(
                            lang.get_untracked(),
                            "error_format",
                            &[&e],
                        )),
                    }
                    set_loading.set(false);
                });
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| on_close_cancel.run(()))
                    title=i18n::t(lang.get(), "edit_info").to_string()
                    testid="sheet-edit-info"
                >
                    <form on:submit=on_submit>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "name")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=name_ref
                                prop:value=move || name_val.get()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "email")}</label>
                            <input
                                type="email"
                                class="form-control"
                                node_ref=email_ref
                                prop:value=move || email_val.get()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "company")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=company_ref
                                prop:value=move || company_val.get()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "phone")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=phone_ref
                                prop:value=move || phone_val.get()
                            />
                        </div>
                        {if is_admin {
                            view! {
                                <div class="form-group">
                                    <label>{move || i18n::t(lang.get(), "user_edit_new_password")}</label>
                                    <input
                                        type="password"
                                        class="form-control"
                                        data-testid="user-edit-password"
                                        node_ref=password_ref
                                        placeholder=move || i18n::t(lang.get(), "user_edit_new_password_placeholder")
                                        autocomplete="new-password"
                                    />
                                    <small class="form-help">
                                        {move || i18n::t(lang.get(), "user_edit_new_password_help")}
                                    </small>
                                </div>
                                <label class="form-row" data-testid="user-edit-allow-self-entry-row">
                                    <input
                                        type="checkbox"
                                        data-testid="user-edit-allow-self-entry"
                                        prop:checked=move || allow_self_entry.get()
                                        on:change=move |ev| {
                                            let el: HtmlInputElement =
                                                ev.target().unwrap().unchecked_into();
                                            set_allow_self_entry.set(el.checked());
                                        }
                                    />
                                    <span>{move || i18n::t(lang.get(), "admin_allow_self_entry")}</span>
                                    <small class="form-help">
                                        {move || i18n::t(lang.get(), "admin_allow_self_entry_help")}
                                    </small>
                                </label>
                            }.into_any()
                        } else {
                            ().into_any()
                        }}
                        <div class="sheet__actions">
                            <button
                                type="button"
                                class="btn btn--ghost"
                                disabled=move || loading.get()
                                on:click=move |_| {
                                    let cb = on_close_btn.clone();
                                    spawn_local(async move {
                                        gloo_timers::future::TimeoutFuture::new(0).await;
                                        cb.run(());
                                    });
                                }
                            >
                                {i18n::t(lang.get(), "cancel")}
                            </button>
                            <button
                                type="submit"
                                class="btn btn--primary"
                                disabled=move || loading.get()
                            >
                                {i18n::t(lang.get(), "save")}
                            </button>
                        </div>
                    </form>
                </Sheet>
            }.into_any()
        }}
    }
}
