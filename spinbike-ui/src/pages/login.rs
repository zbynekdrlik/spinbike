use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::auth::{self, AuthData, UserInfo};
use crate::i18n::{self, Lang};

#[derive(serde::Serialize)]
struct LoginReq {
    email: String,
    password: String,
}

#[derive(serde::Serialize)]
struct RegisterReq {
    email: String,
    password: String,
    name: String,
    phone: Option<String>,
}

#[derive(serde::Serialize)]
struct RequestLoginLinkReq {
    email: String,
}

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

fn navigate_role_home(role: &str) {
    // Staff/admin spend all their time on the card dashboard — land them there
    // directly. Customers still get the schedule.
    let target = match role {
        "staff" | "admin" => "/staff",
        _ => "/",
    };
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_href(target);
    }
}

fn save_and_redirect(resp: AuthResp) {
    let role = resp.user.role.clone();
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
    // Bump auth version so navbar updates.
    if let Some(set_ver) = use_context::<WriteSignal<u32>>() {
        set_ver.update(|v| *v += 1);
    }
    navigate_role_home(&role);
}

#[component]
pub fn LoginPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let email_ref = NodeRef::<leptos::html::Input>::new();
    let pass_ref = NodeRef::<leptos::html::Input>::new();
    let (error, set_error) = signal(String::new());
    let (loading, set_loading) = signal(false);

    // Customer login-link section (below the password form) — its own
    // signals, independent of the password-form ones above.
    let customer_email_ref = NodeRef::<leptos::html::Input>::new();
    let (customer_sent, set_customer_sent) = signal(false);
    let (customer_error, set_customer_error) = signal(String::new());
    let (customer_loading, set_customer_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let email = email_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let password = pass_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();

        set_loading.set(true);
        set_error.set(String::new());

        spawn_local(async move {
            match api::post::<LoginReq, AuthResp>("/api/auth/login", &LoginReq { email, password })
                .await
            {
                Ok(resp) => save_and_redirect(resp),
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="page-form">
            <h1 class="page-title">{move || i18n::t(lang.get(), "login")}</h1>
            {move || {
                let e = error.get();
                if e.is_empty() {
                    view! {}.into_any()
                } else {
                    view! { <div class="alert alert-error">{e}</div> }.into_any()
                }
            }}
            <form on:submit=on_submit>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "email")}</label>
                    <input type="email" class="form-control" node_ref=email_ref required />
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "password")}</label>
                    <input type="password" class="form-control" node_ref=pass_ref required />
                </div>
                <button type="submit" class="btn btn--primary btn--hero btn--block" disabled=move || loading.get()>
                    {move || if loading.get() { i18n::t(lang.get(), "logging_in") } else { i18n::t(lang.get(), "login") }}
                </button>
            </form>
            <p class="text-center text-muted mt-2">
                {move || i18n::t(lang.get(), "dont_have_account")} <a href="/register">{move || i18n::t(lang.get(), "register")}</a>
            </p>

            <hr class="mt-3" />

            <h2 class="page-title mt-3" data-testid="customer-login-heading">
                {move || i18n::t(lang.get(), "customer_login_heading")}
            </h2>
            {move || {
                if customer_sent.get() {
                    view! {
                        <div class="alert alert-success" data-testid="login-link-sent">
                            {move || i18n::t(lang.get(), "login_link_sent")}
                        </div>
                    }
                    .into_any()
                } else {
                    let on_customer_submit = move |ev: web_sys::SubmitEvent| {
                        ev.prevent_default();
                        let email = customer_email_ref
                            .get()
                            .map(|el| {
                                let el: &HtmlInputElement = &el;
                                el.value()
                            })
                            .unwrap_or_default();
                        set_customer_loading.set(true);
                        set_customer_error.set(String::new());
                        spawn_local(async move {
                            match api::post_public::<RequestLoginLinkReq, serde_json::Value>(
                                "/api/auth/request-login-link",
                                &RequestLoginLinkReq { email },
                            )
                            .await
                            {
                                Ok(_) => set_customer_sent.set(true),
                                Err(e) => set_customer_error.set(e),
                            }
                            set_customer_loading.set(false);
                        });
                    };
                    view! {
                        <form on:submit=on_customer_submit data-testid="login-link-form">
                            {move || {
                                let e = customer_error.get();
                                if e.is_empty() {
                                    view! {}.into_any()
                                } else {
                                    view! { <div class="alert alert-error">{e}</div> }.into_any()
                                }
                            }}
                            <div class="form-group">
                                <label>{move || i18n::t(lang.get(), "email")}</label>
                                <input type="email" class="form-control" node_ref=customer_email_ref required data-testid="login-link-email" />
                            </div>
                            <button type="submit" class="btn btn--ghost btn--block" disabled=move || customer_loading.get() data-testid="login-link-submit">
                                {move || i18n::t(lang.get(), "send_login_link")}
                            </button>
                        </form>
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

#[component]
pub fn RegisterPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let name_ref = NodeRef::<leptos::html::Input>::new();
    let email_ref = NodeRef::<leptos::html::Input>::new();
    let pass_ref = NodeRef::<leptos::html::Input>::new();
    let phone_ref = NodeRef::<leptos::html::Input>::new();
    let (error, set_error) = signal(String::new());
    let (loading, set_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let name = name_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let email = email_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let password = pass_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let phone_val = phone_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let phone = if phone_val.is_empty() {
            None
        } else {
            Some(phone_val)
        };

        set_loading.set(true);
        set_error.set(String::new());

        spawn_local(async move {
            match api::post::<RegisterReq, AuthResp>(
                "/api/auth/register",
                &RegisterReq {
                    email,
                    password,
                    name,
                    phone,
                },
            )
            .await
            {
                Ok(resp) => save_and_redirect(resp),
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="page-form">
            <h1 class="page-title">{move || i18n::t(lang.get(), "register")}</h1>
            {move || {
                let e = error.get();
                if e.is_empty() {
                    view! {}.into_any()
                } else {
                    view! { <div class="alert alert-error">{e}</div> }.into_any()
                }
            }}
            <form on:submit=on_submit>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "name")}</label>
                    <input type="text" class="form-control" node_ref=name_ref required />
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "email")}</label>
                    <input type="email" class="form-control" node_ref=email_ref required />
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "password")}</label>
                    <input type="password" class="form-control" node_ref=pass_ref required minlength="6" />
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "phone_optional")}</label>
                    <input type="tel" class="form-control" node_ref=phone_ref />
                </div>
                <button type="submit" class="btn btn--primary btn--hero btn--block" disabled=move || loading.get()>
                    {move || if loading.get() { i18n::t(lang.get(), "creating_account") } else { i18n::t(lang.get(), "register") }}
                </button>
            </form>
            <p class="text-center text-muted mt-2">
                {move || i18n::t(lang.get(), "already_have_account")} <a href="/login">{move || i18n::t(lang.get(), "login")}</a>
            </p>
        </div>
    }
}
