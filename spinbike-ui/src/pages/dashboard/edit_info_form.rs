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
/// authoritative server state via `GET /api/users/lookup/{card_code}` and
/// writes values directly to the input elements via `NodeRef`. Bypasses
/// `prop:value` so re-rendering doesn't lose the user's typed input mid-edit
/// and a server refetch DOES override stale signal state.
///
/// Field-level guards on the server:
/// - `allow_self_entry`: admin-only.
/// - `password`: admin OR self.
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
    // Initial values are written into the inputs via NodeRef.set_value
    // inside the refresh Effect (see below) — we don't pass `value=` at
    // the macro level because the move closure that wraps the sheet must
    // stay FnMut across re-renders, and `value=expr.clone()` made it
    // FnOnce. The Effect runs on the first show=true with prev=None and
    // populates inputs from the latest server state.
    let initial_allow_se = card.allow_self_entry;
    // Initial values from the card prop, exposed as ReadSignals so the
    // outer `move ||` closure stays Fn (signals are Copy). Inputs use
    // `value=<sig>.get()` which evaluates fresh on every render.
    let (initial_name, _) = signal(card.name.clone());
    let (initial_email, _) = signal(card.email.clone().unwrap_or_default());
    let (initial_company, _) = signal(card.company.clone().unwrap_or_default());
    let (initial_phone, _) = signal(card.phone.clone().unwrap_or_default());

    // NodeRefs declared at the function-body level so the refresh Effect
    // can write to them directly when fetch completes. They're populated
    // when the sheet mounts (inside the show=true branch); the Effect
    // checks `get_untracked()` for None and no-ops in that case.
    let name_ref = NodeRef::<leptos::html::Input>::new();
    let email_ref = NodeRef::<leptos::html::Input>::new();
    let company_ref = NodeRef::<leptos::html::Input>::new();
    let phone_ref = NodeRef::<leptos::html::Input>::new();
    let password_ref = NodeRef::<leptos::html::Input>::new();
    let (allow_self_entry, set_allow_self_entry) = signal(initial_allow_se);

    // Read the caller's role to gate the admin-only fields client-side.
    let is_admin = auth::get_user()
        .map(|u| u.role == "admin")
        .unwrap_or(false);

    // Refresh from server every time show transitions false→true. Sets the
    // input values via NodeRef + HtmlInputElement::set_value, so the latest
    // saved data is what the user sees on reopen.
    // Refresh on REOPEN only. First-open uses the `value=` HTML attribute
    // on each input (set from the card prop at render time). The Effect
    // fetches the latest user state on the false→true transition AFTER
    // the form has previously been shown — this is the case where the
    // parent's `card` prop may be stale (set_selected.set after save
    // doesn't always trigger CardActionPanel to remount, so the EditInfoForm's
    // initial values can be from the pre-save state).
    let lookup_code = initial_code.clone();
    Effect::new(move |prev_shown: Option<bool>| {
        let now_shown = show.get();
        let is_reopen = prev_shown == Some(false) && now_shown;
        if !is_reopen {
            return now_shown;
        }
        let code = lookup_code.clone();
        if let Some(code) = code {
            spawn_local(async move {
                // Yield so the new sheet is mounted and the NodeRefs point at
                // the live inputs before we write to them.
                gloo_timers::future::TimeoutFuture::new(0).await;
                if let Ok(c) =
                    api::get::<CardInfo>(&format!("/api/users/lookup/{code}")).await
                {
                    let set_value = |nr: &NodeRef<leptos::html::Input>, val: &str| {
                        if let Some(el) = nr.get_untracked() {
                            let input: &HtmlInputElement = &el;
                            input.set_value(val);
                        }
                    };
                    set_value(&name_ref, &c.name);
                    set_value(&email_ref, c.email.as_deref().unwrap_or(""));
                    set_value(&company_ref, c.company.as_deref().unwrap_or(""));
                    set_value(&phone_ref, c.phone.as_deref().unwrap_or(""));
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

            let (loading, set_loading) = signal(false);
            let on_close_cancel = on_close.clone();
            let on_close_btn = on_close.clone();
            let on_close_save = on_close.clone();
            let initial_allow_se_at_open = allow_self_entry.get_untracked();

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
                        // Admin-only field, only sent when changed.
                        allow_self_entry: if is_admin && allow_se != initial_allow_se_at_open {
                            Some(allow_se)
                        } else {
                            None
                        },
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
                                value=initial_name.get_untracked()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "email")}</label>
                            <input
                                type="email"
                                class="form-control"
                                node_ref=email_ref
                                value=initial_email.get_untracked()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "company")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=company_ref
                                value=initial_company.get_untracked()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "phone")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=phone_ref
                                value=initial_phone.get_untracked()
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
