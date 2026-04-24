use leptos::prelude::*;
use leptos_router::hooks::use_location;

use crate::auth;
use crate::i18n::{self, Lang};

/// Adaptive navigation for admin/staff: bottom tab bar on phone,
/// left sidebar on desktop (CSS media-query driven).
/// Does NOT render for customers or logged-out users.
#[component]
pub fn AdaptiveNav(auth_ver: ReadSignal<u32>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    let user = move || {
        let _ = auth_ver.get();
        auth::get_user()
    };

    let loc = use_location();
    let current_path = move || loc.pathname.get();

    view! {
        {move || {
            let Some(u) = user() else { return ().into_any(); };
            if u.role != "admin" && u.role != "staff" {
                return ().into_any();
            }
            let path = current_path();
            let desk_active = path.starts_with("/staff") || path == "/";
            let schedule_active = path.starts_with("/schedule");
            let reports_active = path.starts_with("/reports");
            let settings_active = path.starts_with("/settings") || path.starts_with("/admin");

            view! {
                <nav class="adaptive-nav" data-testid="adaptive-nav">
                    <a href="/staff" class="adaptive-nav__item"
                       data-testid="nav-desk"
                       aria-current=if desk_active { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"🏠"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_desk")}</span>
                    </a>
                    <a href="/schedule" class="adaptive-nav__item"
                       data-testid="nav-schedule"
                       aria-current=if schedule_active { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"📅"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_schedule")}</span>
                    </a>
                    <a href="/reports" class="adaptive-nav__item"
                       data-testid="nav-reports"
                       aria-current=if reports_active { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"📊"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_reports")}</span>
                    </a>
                    <a href="/settings" class="adaptive-nav__item"
                       data-testid="nav-settings"
                       aria-current=if settings_active { "page" } else { "false" }>
                        <span class="adaptive-nav__icon">"⚙"</span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_settings")}</span>
                    </a>
                </nav>
            }.into_any()
        }}
    }
}
