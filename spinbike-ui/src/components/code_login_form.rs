//! 6-digit email login code (#227) — the in-PWA login path that closes the iOS
//! "installed-app logged-out loop". On iOS a home-screen web app has storage
//! partitioned from Safari and a magic link always re-opens in Safari, so a link
//! can never complete login INSIDE the installed app; a short code the user types
//! can. Two steps: enter email → "Poslat kod" (`POST /api/auth/request-login-code`,
//! always 200 / no enumeration) → enter the 6-digit code → submit
//! (`POST /api/auth/code-login`) → store the session + role-aware redirect.
//!
//! Both endpoints use `post_public(_coded)` (NOT `post`): they can legitimately
//! 401/429 for reasons unrelated to any session this browser already holds, and
//! must not trigger the 401-clears-session redirect (see api.rs / #109).
//!
//! `CustomerLoginMethods` is the shared toggle (email-link vs code) used by BOTH
//! the login page's customer section and `/welcome`'s invalid-token fallback. It
//! leads with the email-link method everywhere EXCEPT when running installed
//! standalone on iOS (#228) — there a magic link is the dead end this whole
//! feature exists to route around, so the code method leads instead; both
//! methods stay reachable via the toggle regardless. Android/Chromium standalone
//! is unaffected (shares storage with the browser, so link stays primary), and
//! the existing login-link/welcome E2E selectors are unchanged for every
//! non-iOS-standalone case.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::auth::{self, AuthData};
use crate::components::{LoginLinkForm, Segmented};
use crate::i18n::{self, Lang};
use crate::platform;

#[derive(serde::Serialize)]
struct RequestLoginCodeReq {
    email: String,
}

#[derive(serde::Serialize)]
struct CodeLoginReq {
    email: String,
    code: String,
}

/// Store the session and navigate to the role's home. Code-login is
/// customers-only server-side, but this stays role-aware (like `login.rs` and
/// `welcome.rs`) so the redirect is correct if that ever broadens.
fn save_and_redirect(data: AuthData) {
    let target = if data.user.role.is_staff_or_admin() {
        "/staff"
    } else {
        "/my/balance"
    };
    auth::set_auth(&data);
    // Bump auth version so the navbar updates.
    if let Some(set_ver) = use_context::<WriteSignal<u32>>() {
        set_ver.update(|v| *v += 1);
    }
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_href(target);
    }
}

#[component]
pub fn CodeLoginForm() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let email_ref = NodeRef::<leptos::html::Input>::new();
    let code_ref = NodeRef::<leptos::html::Input>::new();
    let (email, set_email) = signal(String::new());
    let (code_sent, set_code_sent) = signal(false);
    let (error, set_error) = signal(None::<api::CodedError>);
    let (loading, set_loading) = signal(false);

    // The error banner is inlined per branch as a nested reactive block (`error`
    // and `lang` are Copy signals) so an error re-render only re-renders the
    // banner — never the whole form, which would drop the code the user typed.
    // Defining it as one outer closure would be moved out on the first render of
    // the re-running outer `move ||`.

    view! {
        {move || {
            if code_sent.get() {
                // Step 2 — verify the entered code.
                let on_verify = move |ev: web_sys::SubmitEvent| {
                    ev.prevent_default();
                    let code = code_ref
                        .get()
                        .map(|el| {
                            let el: &HtmlInputElement = &el;
                            el.value()
                        })
                        .unwrap_or_default();
                    let em = email.get_untracked();
                    set_loading.set(true);
                    set_error.set(None);
                    spawn_local(async move {
                        match api::post_public_coded::<CodeLoginReq, AuthData>(
                            "/api/auth/code-login",
                            &CodeLoginReq { email: em, code },
                        )
                        .await
                        {
                            Ok(data) => save_and_redirect(data),
                            Err(e) => set_error.set(Some(e)),
                        }
                        set_loading.set(false);
                    });
                };
                let on_change_email = move |_| {
                    set_code_sent.set(false);
                    set_error.set(None);
                };
                view! {
                    <form on:submit=on_verify data-testid="code-login-code-form">
                        {move || match error.get() {
                            None => ().into_any(),
                            Some(e) => {
                                let msg = i18n::localize_api_error(lang.get(), e.code, &e.message);
                                view! {
                                    <div class="alert alert-error" data-testid="code-login-error">{msg}</div>
                                }
                                .into_any()
                            }
                        }}
                        <p class="form-help" data-testid="code-login-sent">
                            {move || i18n::t(lang.get(), "login_code_sent_hint")}
                        </p>
                        <div class="form-group">
                            <label>{move || i18n::t(lang.get(), "login_code_label")}</label>
                            <input
                                type="text"
                                inputmode="numeric"
                                autocomplete="one-time-code"
                                class="form-control"
                                node_ref=code_ref
                                required
                                data-testid="code-login-code"
                            />
                        </div>
                        <button
                            type="submit"
                            class="btn btn--primary btn--block"
                            disabled=move || loading.get()
                            data-testid="code-login-submit"
                        >
                            {move || {
                                if loading.get() {
                                    i18n::t(lang.get(), "logging_in_code")
                                } else {
                                    i18n::t(lang.get(), "login_code_submit")
                                }
                            }}
                        </button>
                        <button
                            type="button"
                            class="btn btn--ghost btn--block"
                            on:click=on_change_email
                            data-testid="code-login-change-email"
                        >
                            {move || i18n::t(lang.get(), "login_code_change_email")}
                        </button>
                    </form>
                }
                .into_any()
            } else {
                // Step 1 — request a code for the entered email.
                let on_request = move |ev: web_sys::SubmitEvent| {
                    ev.prevent_default();
                    let entered = email_ref
                        .get()
                        .map(|el| {
                            let el: &HtmlInputElement = &el;
                            el.value()
                        })
                        .unwrap_or_default();
                    set_loading.set(true);
                    set_error.set(None);
                    spawn_local(async move {
                        // Always 200 (no enumeration); a transport failure still
                        // surfaces via the coded error.
                        match api::post_public_coded::<RequestLoginCodeReq, serde_json::Value>(
                            "/api/auth/request-login-code",
                            &RequestLoginCodeReq { email: entered.clone() },
                        )
                        .await
                        {
                            Ok(_) => {
                                set_email.set(entered);
                                set_code_sent.set(true);
                            }
                            Err(e) => set_error.set(Some(e)),
                        }
                        set_loading.set(false);
                    });
                };
                view! {
                    <form on:submit=on_request data-testid="code-login-email-form">
                        {move || match error.get() {
                            None => ().into_any(),
                            Some(e) => {
                                let msg = i18n::localize_api_error(lang.get(), e.code, &e.message);
                                view! {
                                    <div class="alert alert-error" data-testid="code-login-error">{msg}</div>
                                }
                                .into_any()
                            }
                        }}
                        <div class="form-group">
                            <label>{move || i18n::t(lang.get(), "email")}</label>
                            <input
                                type="email"
                                class="form-control"
                                node_ref=email_ref
                                required
                                data-testid="code-login-email"
                            />
                        </div>
                        <button
                            type="submit"
                            class="btn btn--ghost btn--block"
                            disabled=move || loading.get()
                            data-testid="code-login-send"
                        >
                            {move || {
                                if loading.get() {
                                    i18n::t(lang.get(), "sending_login_code")
                                } else {
                                    i18n::t(lang.get(), "send_login_code")
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

/// Toggle between the two customer login methods (email link vs 6-digit code),
/// rendering the selected form. Used by the login page's customer section and
/// `/welcome`'s invalid-token fallback.
///
/// The initial method is platform-aware (#228): a magic link is a dead end
/// when running installed standalone on iOS (storage is partitioned from
/// Safari, so the link always reopens there instead of completing login
/// inside the installed app) — there, the code method leads. Everywhere
/// else (a plain browser tab, or an installed Android/Chromium PWA, which
/// shares storage with the browser and has no such dead end) the email-link
/// method still leads, unchanged. Detected once at mount, same pattern as
/// `InstallPrompt`'s own `detect_kind()` — the platform doesn't change while
/// this component stays mounted.
#[component]
pub fn CustomerLoginMethods() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let initial_method = if platform::is_ios_standalone() {
        "code"
    } else {
        "link"
    };
    let (method, set_method) = signal(initial_method.to_string());
    // Labels read once at mount (same pattern as the admin tab bar) — the toggle
    // is keyed by data-testid/value, not by localized text.
    let seg_items: Vec<(String, String)> = vec![
        (
            "link".to_string(),
            i18n::t(lang.get_untracked(), "login_method_link").to_string(),
        ),
        (
            "code".to_string(),
            i18n::t(lang.get_untracked(), "login_method_code").to_string(),
        ),
    ];

    view! {
        <Segmented
            items=seg_items
            active=Signal::derive(move || method.get())
            on_change=Callback::new(move |key: String| set_method.set(key))
            testid_prefix="login-method"
        />
        {move || {
            if method.get() == "code" {
                view! { <CodeLoginForm /> }.into_any()
            } else {
                view! { <LoginLinkForm /> }.into_any()
            }
        }}
    }
}
