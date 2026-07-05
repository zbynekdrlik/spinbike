use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::auth::{self, AuthData, UserInfo};
use crate::components::LoginLinkForm;
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
            // post_public, not post: a wrong-password 401 must not clear a
            // DIFFERENT, still-valid session this browser happens to hold
            // (e.g. a shared kiosk already logged in as someone else) — see
            // api::post_public's doc comment and #109.
            match api::post_public::<LoginReq, AuthResp>(
                "/api/auth/login",
                &LoginReq { email, password },
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
            <LoginLinkForm />
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
