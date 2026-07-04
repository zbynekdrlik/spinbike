//! Magic-link landing page (`/welcome?t=<token>`). Redeems the token from the
//! query string via `POST /api/auth/token-login`, stores the session exactly
//! like the login page does, and shows a welcome message + CTA to
//! `/my/balance`. An invalid/expired/already-used token (or a missing `t`
//! param) falls back to a friendly message plus the same request-login-link
//! email form the login page's customer section uses — the client can always
//! ask for a fresh link.
//!
//! Public route — the token itself is the authentication, so there is no
//! auth gate here (see router.rs).

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::auth::{self, AuthData, UserInfo};
use crate::i18n::{self, Lang};

#[derive(serde::Serialize)]
struct TokenLoginReq {
    token: String,
}

#[derive(serde::Serialize)]
struct RequestLoginLinkReq {
    email: String,
}

// Small duplicate of login.rs's AuthResp/UserInfoResp — deliberately not
// shared (see .claude/skills/auth-onboarding + issue #109 map): these are
// two independent 4-field deserialize targets for the same server response
// shape, not worth a shared module for.
#[derive(serde::Deserialize)]
struct AuthResp {
    token: String,
    user: UserInfoResp,
}

#[derive(serde::Deserialize)]
struct UserInfoResp {
    id: i64,
    email: String,
    name: String,
    role: String,
}

#[derive(Clone, Copy, PartialEq)]
enum WelcomeState {
    /// Verifying the token (or no token present — resolves synchronously).
    Loading,
    /// Token redeemed, session stored.
    Success,
    /// Missing / invalid / expired / already-used token.
    Invalid,
}

#[component]
pub fn WelcomePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let query = use_query_map();

    let (state, set_state) = signal(WelcomeState::Loading);

    // Runs once on mount (the query string is stable for this page's
    // lifetime). Missing `t` resolves straight to Invalid — no API call.
    Effect::new(move |_| {
        let token = query.get().get("t").filter(|t| !t.is_empty());
        match token {
            Some(t) => {
                spawn_local(async move {
                    match api::post_public::<TokenLoginReq, AuthResp>(
                        "/api/auth/token-login",
                        &TokenLoginReq { token: t },
                    )
                    .await
                    {
                        Ok(resp) => {
                            let data = AuthData {
                                token: resp.token,
                                user: UserInfo {
                                    id: resp.user.id,
                                    email: resp.user.email,
                                    name: resp.user.name,
                                    role: resp.user.role,
                                },
                            };
                            auth::set_auth(&data);
                            if let Some(set_ver) = use_context::<WriteSignal<u32>>() {
                                set_ver.update(|v| *v += 1);
                            }
                            set_state.set(WelcomeState::Success);
                        }
                        Err(_) => set_state.set(WelcomeState::Invalid),
                    }
                });
            }
            None => set_state.set(WelcomeState::Invalid),
        }
    });

    view! {
        <div class="page-form" data-testid="welcome-page">
            {move || match state.get() {
                WelcomeState::Loading => view! {
                    <p class="text-center text-muted" data-testid="welcome-loading">
                        {move || i18n::t(lang.get(), "welcome_loading")}
                    </p>
                }
                .into_any(),
                WelcomeState::Success => view! {
                    <div data-testid="welcome-success">
                        <h1 class="page-title">{move || i18n::t(lang.get(), "welcome_title")}</h1>
                        <p class="text-center">{move || i18n::t(lang.get(), "welcome_message")}</p>
                        <a href="/my/balance" class="btn btn--primary btn--hero btn--block" data-testid="welcome-cta">
                            {move || i18n::t(lang.get(), "welcome_cta")}
                        </a>
                    </div>
                }
                .into_any(),
                WelcomeState::Invalid => {
                    let (email_sent, set_email_sent) = signal(false);
                    let (email_error, set_email_error) = signal(String::new());
                    let (email_loading, set_email_loading) = signal(false);
                    let email_ref = NodeRef::<leptos::html::Input>::new();

                    view! {
                        <div data-testid="welcome-invalid">
                            <h1 class="page-title">{move || i18n::t(lang.get(), "welcome_invalid_title")}</h1>
                            <p class="text-center text-muted">{move || i18n::t(lang.get(), "welcome_invalid_message")}</p>
                            {move || {
                                if email_sent.get() {
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
                                        set_email_loading.set(true);
                                        set_email_error.set(String::new());
                                        spawn_local(async move {
                                            match api::post_public::<RequestLoginLinkReq, serde_json::Value>(
                                                "/api/auth/request-login-link",
                                                &RequestLoginLinkReq { email },
                                            )
                                            .await
                                            {
                                                Ok(_) => set_email_sent.set(true),
                                                Err(e) => set_email_error.set(e),
                                            }
                                            set_email_loading.set(false);
                                        });
                                    };
                                    view! {
                                        <form on:submit=on_submit data-testid="login-link-form">
                                            {move || {
                                                let e = email_error.get();
                                                if e.is_empty() {
                                                    view! {}.into_any()
                                                } else {
                                                    view! { <div class="alert alert-error">{e}</div> }.into_any()
                                                }
                                            }}
                                            <div class="form-group">
                                                <label>{move || i18n::t(lang.get(), "email")}</label>
                                                <input type="email" class="form-control" node_ref=email_ref required data-testid="login-link-email" />
                                            </div>
                                            <button type="submit" class="btn btn--ghost btn--block" disabled=move || email_loading.get() data-testid="login-link-submit">
                                                {move || i18n::t(lang.get(), "send_login_link")}
                                            </button>
                                        </form>
                                    }
                                    .into_any()
                                }
                            }}
                        </div>
                    }
                    .into_any()
                }
            }}
        </div>
    }
}
