use leptos::prelude::*;
use leptos_router::hooks::use_location;

use crate::auth;
use crate::components::Sheet;
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
            let is_staff_or_admin = u.role == "admin" || u.role == "staff";
            if !is_staff_or_admin {
                return ().into_any();
            }
            let is_admin = u.role == "admin";
            let path = current_path();
            let desk_active = path.starts_with("/staff");
            let schedule_active = path.starts_with("/schedule");
            let reports_active = path.starts_with("/reports");
            let settings_active = path.starts_with("/settings") || path.starts_with("/admin");

            // 'More' sheet state — opens on tap, contains username + lang
            // toggle + logout. Mirrors the controls that live in the top
            // navbar (nav.rs) for desktop users; the top navbar is hidden
            // on phone for staff/admin via the body:has(.adaptive-nav) rule.
            let (more_open, set_more_open) = signal(false);
            let user_name = u.name.clone();
            let set_lang = use_context::<WriteSignal<Lang>>().expect("SetLang context");
            // Increment desk_reset on Desk-link click. The DashboardPage
            // subscribes and clears any open card / search query — this is
            // the only way to "go home" when already on /staff (same URL,
            // no router event).
            let desk_reset = use_context::<crate::router::DeskReset>()
                .expect("DeskReset context")
                .0;

            view! {
                <nav class="adaptive-nav" data-testid="adaptive-nav">
                    <a href="/staff" class="adaptive-nav__item"
                       data-testid="nav-desk"
                       on:click=move |_| desk_reset.update(|n| *n += 1)
                       aria-current=if desk_active { "page" } else { "false" }>
                        <span class="adaptive-nav__icon" inner_html=ICON_DESK></span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_desk")}</span>
                    </a>
                    <a href="/schedule" class="adaptive-nav__item"
                       data-testid="nav-schedule"
                       aria-current=if schedule_active { "page" } else { "false" }>
                        <span class="adaptive-nav__icon" inner_html=ICON_SCHEDULE></span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_schedule")}</span>
                    </a>
                    {if is_admin {
                        view! {
                            <a href="/reports" class="adaptive-nav__item"
                               data-testid="nav-reports"
                               aria-current=if reports_active { "page" } else { "false" }>
                                <span class="adaptive-nav__icon" inner_html=ICON_REPORTS></span>
                                <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_reports")}</span>
                            </a>
                            <a href="/settings" class="adaptive-nav__item"
                               data-testid="nav-settings"
                               aria-current=if settings_active { "page" } else { "false" }>
                                <span class="adaptive-nav__icon" inner_html=ICON_SETTINGS></span>
                                <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_settings")}</span>
                            </a>
                        }.into_any()
                    } else { ().into_any() }}
                    <button
                        class="adaptive-nav__item"
                        data-testid="nav-more"
                        type="button"
                        on:click=move |_| set_more_open.update(|v| *v = !*v)
                    >
                        <span class="adaptive-nav__icon" inner_html=ICON_MORE></span>
                        <span class="adaptive-nav__label">{move || i18n::t(lang.get(), "nav_more")}</span>
                    </button>
                </nav>
                {move || if more_open.get() {
                    let user_name = user_name.clone();
                    view! {
                        <Sheet
                            testid="more-sheet".to_string()
                            title=i18n::t(lang.get(), "nav_more").to_string()
                            on_close=Callback::new(move |_| set_more_open.set(false))
                        >
                            <div class="more-sheet__user">{user_name}</div>
                            <button
                                class="btn btn--block btn--ghost"
                                data-testid="more-lang-toggle"
                                on:click=move |_| {
                                    let new_lang = match lang.get() {
                                        Lang::Sk => Lang::En,
                                        Lang::En => Lang::Sk,
                                    };
                                    i18n::save_lang(new_lang);
                                    set_lang.set(new_lang);
                                }
                            >
                                {move || match lang.get() {
                                    Lang::Sk => "EN",
                                    Lang::En => "SK",
                                }}
                            </button>
                            <button
                                class="btn btn--block btn--danger"
                                data-testid="more-logout"
                                on:click=move |_| {
                                    // Deliberately diverges from nav.rs's
                                    // logout (which does set_auth_ver.update
                                    // before set_href). nav.rs lives at the
                                    // top of the component tree; this button
                                    // lives inside the role-gated reactive
                                    // closure that unmounts itself when
                                    // auth_ver bumps. Earlier attempts to
                                    // include the bump here (with both named-
                                    // closure-capture AND spawn_local defer
                                    // patterns) timed out in E2E because the
                                    // click handler's surrounding DOM was
                                    // unmounted before the navigation could
                                    // commit. set_href("/") is a full-page
                                    // reload that re-bootstraps WASM with
                                    // cleared auth — no reactive update
                                    // needed.
                                    auth::clear_auth();
                                    if let Some(w) = web_sys::window() {
                                        let _ = w.location().set_href("/");
                                    }
                                }
                            >
                                {move || i18n::t(lang.get(), "logout")}
                            </button>
                        </Sheet>
                    }.into_any()
                } else { ().into_any() }}
            }.into_any()
        }}
    }
}

// Inline SVG icons (Heroicons outline 24×24, currentColor stroke). Lightweight,
// scale crisply on retina, follow the active-state colour via `currentColor`.
const ICON_DESK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M2.25 12 12 3l9.75 9M4.5 9.75v10.5h15V9.75"/></svg>"##;
const ICON_SCHEDULE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M6.75 3v2.25M17.25 3v2.25M3 8.25h18M4.5 5.25h15a1.5 1.5 0 0 1 1.5 1.5v12a1.5 1.5 0 0 1-1.5 1.5h-15a1.5 1.5 0 0 1-1.5-1.5v-12a1.5 1.5 0 0 1 1.5-1.5z"/></svg>"##;
const ICON_REPORTS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M3 13.5l4.5-4.5 3.75 3.75L21 6.75M21 6.75H15M21 6.75v6"/></svg>"##;
const ICON_SETTINGS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 0 1 0 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.137-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.28z M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0z"/></svg>"##;
const ICON_MORE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6.75a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5zm0 6a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5zm0 6a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5z"/></svg>"##;
