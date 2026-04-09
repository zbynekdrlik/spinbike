use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::components::nav::Navbar;
use crate::pages::admin::AdminPage;
use crate::pages::card_ops::CardOpsPage;
use crate::pages::link_card::LinkCardPage;
use crate::pages::login::{LoginPage, RegisterPage};
use crate::pages::my_balance::MyBalancePage;
use crate::pages::my_bookings::MyBookingsPage;
use crate::pages::payments::PaymentsPage;
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

    view! {
        <Router>
            <div class="app-shell">
                <Navbar auth_ver=auth_ver />
                <div class="page">
                    <Routes fallback=|| view! { <p class="text-center text-muted mt-3">"Page not found"</p> }>
                        <Route path=path!("/") view=SchedulePage />
                        <Route path=path!("/login") view=LoginPage />
                        <Route path=path!("/register") view=RegisterPage />
                        <Route path=path!("/my/bookings") view=MyBookingsPage />
                        <Route path=path!("/my/balance") view=MyBalancePage />
                        <Route path=path!("/link-card") view=LinkCardPage />
                        <Route path=path!("/staff") view=StaffDashboardPage />
                        <Route path=path!("/staff/cards") view=CardOpsPage />
                        <Route path=path!("/staff/payments") view=PaymentsPage />
                        <Route path=path!("/admin") view=AdminPage />
                    </Routes>
                </div>
            </div>
        </Router>
    }
}
