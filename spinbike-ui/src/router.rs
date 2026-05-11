use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_navigate;
use leptos_router::path;

/// Newtype for the desk-reset signal so the context key is purpose-typed
/// (rather than a bare `RwSignal<u32>` shape that any future counter would
/// collide with).
#[derive(Copy, Clone)]
pub struct DeskReset(pub RwSignal<u32>);

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

/// Role-aware root route. Staff/admin → `/staff`; customers → `/my/balance`;
/// logged-out visitors see the public schedule. Reactive on `auth_ver`.
#[component]
fn RootRoute() -> impl IntoView {
    let auth_ver = use_context::<ReadSignal<u32>>().expect("auth_ver context");
    view! {
        {move || {
            let _ = auth_ver.get();
            let user = crate::auth::get_user();
            match user.as_ref().map(|u| u.role.as_str()) {
                Some("staff") | Some("admin") => {
                    view! { <RedirectTo to="/staff".to_string()/> }.into_any()
                }
                Some(_) => {
                    view! { <RedirectTo to="/my/balance".to_string()/> }.into_any()
                }
                None => SchedulePage().into_any(),
            }
        }}
    }
}

/// Returns true when the current auth token's role is staff or admin.
/// Reactive on auth_ver so a logout flips it back to false immediately.
fn is_staff_or_admin(auth_ver: ReadSignal<u32>) -> bool {
    let _ = auth_ver.get();
    crate::auth::get_user()
        .map(|u| u.role == "staff" || u.role == "admin")
        .unwrap_or(false)
}

/// Render `inner` if the current auth is staff/admin, otherwise redirect.
/// Customer JWTs bounce to `/my/balance`; logged-out visitors to `/login`.
/// Complements the server-side API 403 with a client-side route gate so
/// customer JWTs never even render the staff dashboard page.
fn staff_gated<F>(inner: F) -> impl IntoView
where
    F: Fn() -> leptos::prelude::AnyView + Send + Sync + 'static,
{
    let auth_ver = use_context::<ReadSignal<u32>>().expect("auth_ver context");
    view! {
        {move || {
            if is_staff_or_admin(auth_ver) {
                inner()
            } else {
                let user = crate::auth::get_user();
                let target = if user.is_some() { "/my/balance" } else { "/login" };
                view! { <RedirectTo to=target.to_string()/> }.into_any()
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
use crate::pages::door::DoorPage;
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

    // Desk-reset signal: AdaptiveNav increments on Desk-link click so the
    // dashboard clears its selected card / search query and returns to the
    // idle list (negative balance), even when already on /staff (same URL,
    // no router event fires). Wrapped in a newtype so the context key isn't
    // a bare RwSignal<u32> (which would collide with any future signal of
    // the same shape).
    let desk_reset = DeskReset(RwSignal::new(0u32));
    provide_context(desk_reset);

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
                        // Door page — minimal UI for admin/staff/customers
                        // with allow_self_entry=1. No role gate; server's
                        // allow_self_entry check is the actual authorization.
                        <Route path=path!("/door") view=DoorPage />
                        <Route path=path!("/staff") view=|| staff_gated(|| DashboardPage().into_any()) />
                        // Admin schedule view (rosters, walk-in, cancel) lives at /schedule.
                        // /staff/classes kept as alias for back-compat.
                        <Route path=path!("/staff/classes") view=|| staff_gated(|| StaffDashboardPage().into_any()) />
                        <Route path=path!("/schedule") view=ScheduleRoute />
                        <Route path=path!("/reports") view=|| staff_gated(|| ReportsPage().into_any()) />
                        <Route path=path!("/settings") view=|| staff_gated(|| AdminPage().into_any()) />
                        <Route path=path!("/admin") view=|| view! { <RedirectTo to="/settings".to_string()/> } />
                    </Routes>
                </div>
                <VersionFooter />
            </div>
        </Router>
    }
}
