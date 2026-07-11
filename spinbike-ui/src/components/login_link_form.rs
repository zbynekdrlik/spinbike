//! Shared "request a login link" email form (#109) — the SAME widget used
//! by the login page's customer section AND `/welcome`'s invalid-token
//! fallback. Was copy-pasted between the two call sites; extracted here so a
//! future change (copy, validation, a new field) happens once.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::i18n::{self, Lang};

#[derive(serde::Serialize)]
struct RequestLoginLinkReq {
    email: String,
}

#[component]
pub fn LoginLinkForm() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let email_ref = NodeRef::<leptos::html::Input>::new();
    let (sent, set_sent) = signal(false);
    let (error, set_error) = signal(None::<api::CodedError>);
    let (loading, set_loading) = signal(false);

    view! {
        {move || {
            if sent.get() {
                view! {
                    <div class="alert alert-success" data-testid="login-link-sent">
                        {move || i18n::t(lang.get(), "login_link_sent")}
                    </div>
                }
                .into_any()
            } else {
                let on_submit = move |ev: web_sys::SubmitEvent| {
                    ev.prevent_default();
                    let email = email_ref
                        .get()
                        .map(|el| {
                            let el: &HtmlInputElement = &el;
                            el.value()
                        })
                        .unwrap_or_default();
                    set_loading.set(true);
                    set_error.set(None);
                    spawn_local(async move {
                        // post_public_coded, not post: an unknown-email or
                        // throttled request still returns 200 (no enumeration)
                        // so this never actually 401s here, but it's still the
                        // right call for a public, unauthenticated auth
                        // endpoint. The `_coded` variant (#145) carries the
                        // server's `error_code` for the banner below.
                        match api::post_public_coded::<RequestLoginLinkReq, serde_json::Value>(
                            "/api/auth/request-login-link",
                            &RequestLoginLinkReq { email },
                        )
                        .await
                        {
                            Ok(_) => set_sent.set(true),
                            Err(e) => set_error.set(Some(e)),
                        }
                        set_loading.set(false);
                    });
                };
                view! {
                    <form on:submit=on_submit data-testid="login-link-form">
                        {move || {
                            match error.get() {
                                None => ().into_any(),
                                Some(e) => {
                                    let msg = i18n::localize_api_error(lang.get(), e.code, &e.message);
                                    view! { <div class="alert alert-error">{msg}</div> }.into_any()
                                }
                            }
                        }}
                        <div class="form-group">
                            <label>{move || i18n::t(lang.get(), "email")}</label>
                            <input type="email" class="form-control" node_ref=email_ref required data-testid="login-link-email" />
                        </div>
                        <button type="submit" class="btn btn--ghost btn--block" disabled=move || loading.get() data-testid="login-link-submit">
                            {move || {
                                if loading.get() {
                                    i18n::t(lang.get(), "sending_login_link")
                                } else {
                                    i18n::t(lang.get(), "send_login_link")
                                }
                            }}
                        </button>
                    </form>
                }
                .into_any()
            }
        }}
    }
}
