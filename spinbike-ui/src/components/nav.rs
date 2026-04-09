use leptos::prelude::*;

use crate::auth;

#[component]
pub fn Navbar(auth_ver: ReadSignal<u32>) -> impl IntoView {
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

    view! {
        <nav class="navbar">
            <a href="/" class="navbar-brand">"SpinBike"</a>
            <div class="navbar-links">
                <a href="/">"Schedule"</a>
                {move || {
                    if let Some(u) = user() {
                        let is_staff = u.role == "staff" || u.role == "admin";
                        let is_admin = u.role == "admin";
                        view! {
                            <a href="/my/bookings">"My Bookings"</a>
                            <a href="/my/balance">"Balance"</a>
                            {if is_staff {
                                view! {
                                    <a href="/staff">"Staff"</a>
                                    <a href="/staff/cards">"Cards"</a>
                                    <a href="/staff/payments">"Payments"</a>
                                }.into_any()
                            } else {
                                view! {}.into_any()
                            }}
                            {if is_admin {
                                view! {
                                    <a href="/admin">"Admin"</a>
                                }.into_any()
                            } else {
                                view! {}.into_any()
                            }}
                            <span class="navbar-user">{u.name.clone()}</span>
                            <button on:click=on_logout>"Logout"</button>
                        }.into_any()
                    } else {
                        view! {
                            <a href="/login">"Login"</a>
                            <a href="/register">"Register"</a>
                        }.into_any()
                    }
                }}
            </div>
        </nav>
    }
}
