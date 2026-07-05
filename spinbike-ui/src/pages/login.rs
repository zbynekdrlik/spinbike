use leptos::prelude::*;
use spinbike_core::auth::Role;
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
    role: Role,
}

fn navigate_role_home(role: &Role) {
    // Staff/admin spend all their time on the card dashboard — land them there
    // directly. Customers still get the schedule.
    let target = if role.is_staff_or_admin() {
        "/staff"
    } else {
        "/"
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

            <hr class="mt-3" />

            <h2 class="page-title mt-3" data-testid="customer-login-heading">
                {move || i18n::t(lang.get(), "customer_login_heading")}
            </h2>
            <LoginLinkForm />
        </div>
    }
}
