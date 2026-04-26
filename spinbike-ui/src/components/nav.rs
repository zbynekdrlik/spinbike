use leptos::prelude::*;

use crate::auth;
use crate::i18n::{self, Lang};

/// Top header bar — logo, user name, logout, language toggle.
/// Destination links live in `AdaptiveNav` (bottom tabs / sidebar) for
/// admin/staff. Customer-facing links (login, register, my/bookings,
/// my/balance) are still rendered here.
#[component]
pub fn Navbar(auth_ver: ReadSignal<u32>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let set_lang = use_context::<WriteSignal<Lang>>().expect("SetLang context");

    let user = move || {
        let _ = auth_ver.get();
        auth::get_user()
    };

    let on_logout = move |_| {
        auth::clear_auth();
        let set_auth_ver = expect_context::<WriteSignal<u32>>();
        set_auth_ver.update(|v| *v += 1);
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
            <a
                href=move || {
                    let _ = auth_ver.get();
                    match auth::get_user() {
                        Some(u) if u.role == "admin" || u.role == "staff" => "/staff",
                        _ => "/",
                    }
                }
                class="navbar-brand"
                data-testid="brand-link"
            >"SpinBike"</a>
            <div class="navbar-links">
                {move || {
                    if let Some(u) = user() {
                        let is_staff = u.role == "staff" || u.role == "admin";
                        view! {
                            // Customer-only links: only show for true customers
                            // (admin/staff get these views via different paths).
                            {if !is_staff {
                                view! {
                                    <a href="/my/bookings">{move || i18n::t(lang.get(), "my_bookings")}</a>
                                    <a href="/my/balance">{move || i18n::t(lang.get(), "balance")}</a>
                                }.into_any()
                            } else {
                                ().into_any()
                            }}
                            <span class="navbar-user">{u.name.clone()}</span>
                            <button class="btn btn--compact btn--ghost" on:click=on_logout>{move || i18n::t(lang.get(), "logout")}</button>
                        }.into_any()
                    } else {
                        view! {
                            <a href="/login">{move || i18n::t(lang.get(), "login")}</a>
                            <a href="/register">{move || i18n::t(lang.get(), "register")}</a>
                        }.into_any()
                    }
                }}
                <button
                    class="btn btn--compact btn--ghost lang-toggle"
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
