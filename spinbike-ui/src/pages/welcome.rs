//! Magic-link landing page (`/welcome?t=<token>`). Redeems the token from the
//! query string via `POST /api/auth/token-login`, stores the session exactly
//! like the login page does, and shows a welcome message + CTA. An
//! invalid/expired/already-used token (or a missing `t` param) falls back to
//! a friendly message plus the shared [`LoginLinkForm`] — the client can
//! always ask for a fresh link.
//!
//! Public route — the token itself is the authentication, so there is no
//! auth gate here (see router.rs).

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth::{self, AuthData};
use crate::components::LoginLinkForm;
use crate::i18n::{self, Lang};

#[derive(serde::Serialize)]
struct TokenLoginReq {
    token: String,
}

#[derive(Clone, Copy, PartialEq)]
enum WelcomeState {
    /// Verifying the token (or no token present — resolves synchronously).
    Loading,
    /// Token redeemed, session stored. Carries the CTA target so
    /// staff/admin (redeemable in principle — the server places no role
    /// restriction on invite/login tokens) land on their own dashboard
    /// instead of the customer-only balance page, matching login.rs's
    /// `navigate_role_home`.
    Success { cta_href: &'static str },
    /// Missing / invalid / expired / already-used token.
    Invalid,
}

#[component]
pub fn WelcomePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let query = use_query_map();

    let (state, set_state) = signal(WelcomeState::Loading);

    // Runs exactly once on mount: `get_untracked()` means this effect has NO
    // tracked reactive dependency, so it establishes and never re-fires
    // (Leptos effects always run once immediately regardless of tracked
    // reads). Using the TRACKED `query.get()` here would re-subscribe to the
    // query-map memo and risk re-redeeming an already-used, now-invalid
    // token if the memo ever re-notifies while mounted — flipping a user who
    // just logged in successfully back to the invalid-link screen.
    Effect::new(move |_| {
        let token = query.get_untracked().get("t").filter(|t| !t.is_empty());
        match token {
            Some(t) => {
                spawn_local(async move {
                    match api::post_public::<TokenLoginReq, AuthData>(
                        "/api/auth/token-login",
                        &TokenLoginReq { token: t },
                    )
                    .await
                    {
                        Ok(data) => {
                            let cta_href = match data.user.role.as_str() {
                                "staff" | "admin" => "/staff",
                                _ => "/my/balance",
                            };
                            auth::set_auth(&data);
                            if let Some(set_ver) = use_context::<WriteSignal<u32>>() {
                                set_ver.update(|v| *v += 1);
                            }
                            set_state.set(WelcomeState::Success { cta_href });
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
                WelcomeState::Success { cta_href } => view! {
                    <div data-testid="welcome-success">
                        <h1 class="page-title">{move || i18n::t(lang.get(), "welcome_title")}</h1>
                        <p class="text-center">{move || i18n::t(lang.get(), "welcome_message")}</p>
                        <a href=cta_href class="btn btn--primary btn--hero btn--block" data-testid="welcome-cta">
                            {move || i18n::t(lang.get(), "welcome_cta")}
                        </a>
                    </div>
                }
                .into_any(),
                WelcomeState::Invalid => view! {
                    <div data-testid="welcome-invalid">
                        <h1 class="page-title">{move || i18n::t(lang.get(), "welcome_invalid_title")}</h1>
                        <p class="text-center text-muted">{move || i18n::t(lang.get(), "welcome_invalid_message")}</p>
                        <LoginLinkForm />
                    </div>
                }
                .into_any(),
            }}
        </div>
    }
}
