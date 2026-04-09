use leptos::prelude::*;

use crate::auth;
use crate::i18n::{self, Lang};

#[component]
pub fn Navbar(auth_ver: ReadSignal<u32>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let set_lang = use_context::<WriteSignal<Lang>>().expect("SetLang context");

    let user = move || {
        // Re-read user when auth_ver changes.
        let _ = auth_ver.get();
        auth::get_user()
    };

    let on_logout = move |_| {
        auth::clear_auth();
        let set_auth_ver = expect_context::<WriteSignal<u32>>();
        set_auth_ver.update(|v| *v += 1);
        // Navigate to home.
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href("/");
        }
    };

    let on_toggle_lang = move |_| {
        let new_lang = match lang.get() {
            Lang::Sk => Lang::En,
            Lang::En => Lang::Sk,
        };
        i18n::save_lang(new_lang);
        set_lang.set(new_lang);
    };

    view! {
        <nav class="navbar">
            <a href="/" class="navbar-brand">"SpinBike"</a>
            <div class="navbar-links">
                <a href="/">{move || i18n::t(lang.get(), "schedule")}</a>
                {move || {
                    if let Some(u) = user() {
                        let is_staff = u.role == "staff" || u.role == "admin";
                        let is_admin = u.role == "admin";
                        view! {
                            <a href="/my/bookings">{move || i18n::t(lang.get(), "my_bookings")}</a>
                            <a href="/my/balance">{move || i18n::t(lang.get(), "balance")}</a>
                            {if is_staff {
                                view! {
                                    <a href="/staff">{move || i18n::t(lang.get(), "staff")}</a>
                                    <a href="/staff/cards">{move || i18n::t(lang.get(), "cards")}</a>
                                    <a href="/staff/payments">{move || i18n::t(lang.get(), "payments")}</a>
                                }.into_any()
                            } else {
                                view! {}.into_any()
                            }}
                            {if is_admin {
                                view! {
                                    <a href="/admin">{move || i18n::t(lang.get(), "admin")}</a>
                                }.into_any()
                            } else {
                                view! {}.into_any()
                            }}
                            <span class="navbar-user">{u.name.clone()}</span>
                            <button on:click=on_logout>{move || i18n::t(lang.get(), "logout")}</button>
                        }.into_any()
                    } else {
                        view! {
                            <a href="/login">{move || i18n::t(lang.get(), "login")}</a>
                            <a href="/register">{move || i18n::t(lang.get(), "register")}</a>
                        }.into_any()
                    }
                }}
                <button
                    class="lang-toggle"
                    style="margin-left:8px;padding:2px 8px;font-size:0.75rem;border:1px solid var(--border);border-radius:4px;background:transparent;cursor:pointer"
                    on:click=on_toggle_lang
                >
                    {move || match lang.get() {
                        Lang::Sk => "EN",
                        Lang::En => "SK",
                    }}
                </button>
            </div>
        </nav>
    }
}
