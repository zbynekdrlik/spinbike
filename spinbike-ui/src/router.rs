use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_navigate;
use leptos_router::path;

/// Imperative redirect — runs once on mount, navigates to `to`.
#[component]
fn RedirectTo(#[prop(into)] to: String) -> impl IntoView {
    let nav = use_navigate();
    let to_clone = to.clone();
    Effect::new(move |_| {
        nav(&to_clone, Default::default());
    });
    view! { <span></span> }
}

/// Role-aware /schedule: admin/staff see the rich roster view (StaffDashboardPage);
/// customers (and logged-out visitors) see the public week schedule (SchedulePage).
/// Reactive on `auth_ver` so the view flips immediately on login/logout while
/// the user is parked on this route.
#[component]
fn ScheduleRoute() -> impl IntoView {
    let auth_ver = use_context::<ReadSignal<u32>>().expect("auth_ver context");
    view! {
        {move || {
            let _ = auth_ver.get();
            let user = crate::auth::get_user();
            let is_staff = user
                .as_ref()
                .map(|u| u.role == "staff" || u.role == "admin")
                .unwrap_or(false);
            if is_staff {
                StaffDashboardPage().into_any()
            } else {
                SchedulePage().into_any()
            }
        }}
    }
}

/// Role-aware root route. Staff/admin land on the Desk (`/staff`); customers
/// and logged-out visitors see the public schedule. Reactive on `auth_ver`.
#[component]
fn RootRoute() -> impl IntoView {
    let auth_ver = use_context::<ReadSignal<u32>>().expect("auth_ver context");
    view! {
        {move || {
            let _ = auth_ver.get();
            let user = crate::auth::get_user();
            let is_staff = user
                .as_ref()
                .map(|u| u.role == "staff" || u.role == "admin")
                .unwrap_or(false);
            if is_staff {
                view! { <RedirectTo to="/staff".to_string()/> }.into_any()
            } else {
                SchedulePage().into_any()
            }
        }}
    }
}

use crate::components::AdaptiveNav;
use crate::components::VersionFooter;
use crate::components::nav::Navbar;
use crate::i18n;
use crate::pages::admin::AdminPage;
use crate::pages::dashboard::DashboardPage;
use crate::pages::link_card::LinkCardPage;
use crate::pages::login::{LoginPage, RegisterPage};
use crate::pages::my_balance::MyBalancePage;
use crate::pages::my_bookings::MyBookingsPage;
use crate::pages::reports::ReportsPage;
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
                <AdaptiveNav auth_ver=auth_ver />
                <div class="page">
                    <Routes fallback=move || view! { <p class="text-center text-muted mt-3">{move || i18n::t(lang_signal.get(), "page_not_found")}</p> }>
                        <Route path=path!("/") view=RootRoute />
                        <Route path=path!("/login") view=LoginPage />
                        <Route path=path!("/register") view=RegisterPage />
                        <Route path=path!("/my/bookings") view=MyBookingsPage />
                        <Route path=path!("/my/balance") view=MyBalancePage />
                        <Route path=path!("/link-card") view=LinkCardPage />
                        <Route path=path!("/staff") view=DashboardPage />
                        // Admin schedule view (rosters, walk-in, cancel) lives at /schedule.
                        // /staff/classes kept as alias for back-compat.
                        <Route path=path!("/staff/classes") view=StaffDashboardPage />
                        <Route path=path!("/schedule") view=ScheduleRoute />
                        <Route path=path!("/reports") view=ReportsPage />
                        <Route path=path!("/settings") view=AdminPage />
                        <Route path=path!("/admin") view=|| view! { <RedirectTo to="/settings".to_string()/> } />
                    </Routes>
                </div>
                <VersionFooter />
            </div>
        </Router>
    }
}
