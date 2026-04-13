use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::components::nav::Navbar;
use crate::i18n;
use crate::pages::admin::AdminPage;
use crate::pages::dashboard::DashboardPage;
use crate::pages::link_card::LinkCardPage;
use crate::pages::login::{LoginPage, RegisterPage};
use crate::pages::my_balance::MyBalancePage;
use crate::pages::my_bookings::MyBookingsPage;
use crate::pages::schedule::SchedulePage;
use crate::pages::staff_dashboard::StaffDashboardPage;

#[component]
pub fn App() -> impl IntoView {
    let ws_msg = crate::ws::connect_ws();
    provide_context(ws_msg);

    // Reactive auth state: triggers re-render when auth changes.
    let (auth_ver, set_auth_ver) = signal(0u32);
    provide_context(set_auth_ver);
    provide_context(auth_ver);

    // i18n language signal
    let (lang, set_lang) = signal(i18n::get_saved_lang());
    provide_context(lang);
    provide_context(set_lang);

    let lang_signal = lang;
    view! {
        <Router>
            <div class="app-shell">
                <Navbar auth_ver=auth_ver />
                <div class="page">
                    <Routes fallback=move || view! { <p class="text-center text-muted mt-3">{move || i18n::t(lang_signal.get(), "page_not_found")}</p> }>
                        <Route path=path!("/") view=SchedulePage />
                        <Route path=path!("/login") view=LoginPage />
                        <Route path=path!("/register") view=RegisterPage />
                        <Route path=path!("/my/bookings") view=MyBookingsPage />
                        <Route path=path!("/my/balance") view=MyBalancePage />
                        <Route path=path!("/link-card") view=LinkCardPage />
                        <Route path=path!("/staff") view=DashboardPage />
                        <Route path=path!("/staff/classes") view=StaffDashboardPage />
                        <Route path=path!("/admin") view=AdminPage />
                    </Routes>
                </div>
            </div>
        </Router>
    }
}
