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
            // Settings active state still drives more-sheet aria-current
            // even though the link itself moved into the sheet (#82).
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
                            // Open-door link visible to ALL admin/staff (whether
                            // their own allow_self_entry flag is on or off — the
                            // door page itself shows "Ask reception" when the
                            // flag is off, so this stays informative).
                            <a
                                href="/my/balance"
                                class="btn btn--block btn--ghost"
                                data-testid="more-open-door"
                            >
                                {move || i18n::t(lang.get(), "door_button_idle")}
                            </a>
                            {if is_admin {
                                // Settings moved here from the bottom-nav per #82.
                                // Plain anchor — full navigation reload, parent
                                // closure unmounts the sheet on auth_ver bump or
                                // route change.
                                view! {
                                    <a
                                        href="/settings"
                                        class="btn btn--block btn--ghost"
                                        data-testid="more-settings"
                                        aria-current=if settings_active { "page" } else { "false" }
                                    >
                                        {move || i18n::t(lang.get(), "nav_settings")}
                                    </a>
                                }.into_any()
                            } else { ().into_any() }}
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
// ICON_SETTINGS removed in #82 — Settings is now a text link inside the
// More sheet, not a top-level nav item.
const ICON_MORE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6.75a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5zm0 6a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5zm0 6a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5z"/></svg>"##;
