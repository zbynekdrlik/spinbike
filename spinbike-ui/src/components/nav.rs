use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::auth;
use crate::components::Sheet;
use crate::components::sheet::FOCUSABLE_SELECTOR;
use crate::i18n::{self, Lang};

/// DOM id for the customer burger's disclosure panel (`Sheet`) — the
/// trigger button's `aria-controls` references it, and it is also the
/// focus-management query target below. Keep the bare-id and `#`-selector
/// forms in sync (Rust attribute values want the bare id; `query_selector`
/// wants the CSS selector form).
const MENU_SHEET_ID: &str = "navbar-menu-sheet";
const MENU_SHEET_SELECTOR: &str = "#navbar-menu-sheet";
/// DOM id for the burger trigger button — focus is restored here when the
/// sheet closes (Escape, backdrop click, or a menu item navigating away).
const BURGER_ID: &str = "navbar-burger-toggle";
const BURGER_SELECTOR: &str = "#navbar-burger-toggle";

/// Moves keyboard focus to the FIRST focusable descendant of the element
/// matching `container_selector` (falling back to the container itself if
/// it has none). Used when the customer burger menu opens, so Escape
/// (which `Sheet` only handles once focus is genuinely *inside* it — see
/// `components::sheet::Sheet`'s doc comment) works without requiring a
/// prior mouse click inside the panel.
///
/// **Polls rather than waiting a single fixed delay** — a just-toggled
/// `menu_open` signal doesn't guarantee the `Sheet` has *already* landed
/// in the DOM by the next microtask/macrotask; a single-shot
/// `TimeoutFuture::new(0)` genuinely raced ahead of Leptos's own DOM
/// patch in CI (confirmed live: the container query found nothing, so no
/// focus ever moved, and Escape's keydown — bound on `.sheet` itself —
/// never had anything inside it to bubble from). Retrying a few times
/// over ~0.5s comfortably covers that without depending on Leptos's
/// internal scheduling, and resolves near-instantly in the success case
/// since the container is almost always already there.
fn focus_first_in(container_selector: &'static str) {
    spawn_local(async move {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        for _ in 0..20 {
            if let Ok(Some(container)) = doc.query_selector(container_selector) {
                let target = container
                    .query_selector(FOCUSABLE_SELECTOR)
                    .ok()
                    .flatten()
                    .unwrap_or(container);
                if let Ok(html_el) = target.dyn_into::<web_sys::HtmlElement>() {
                    let _ = html_el.focus();
                }
                return;
            }
            gloo_timers::future::TimeoutFuture::new(25).await;
        }
    });
}

/// Moves keyboard focus directly to the element matching `selector` —
/// deferred one macrotask, matching `focus_first_in`'s timing so a close
/// that unmounts the sheet has settled first. Used to restore focus to the
/// burger trigger once its menu closes (Escape / outside click) so focus
/// is never left dangling on a removed node.
fn focus_element(selector: &'static str) {
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Ok(Some(el)) = doc.query_selector(selector) else {
            return;
        };
        if let Ok(html_el) = el.dyn_into::<web_sys::HtmlElement>() {
            let _ = html_el.focus();
        }
    });
}

/// Top header bar — logo, user name, logout, language toggle.
/// Destination links live in `AdaptiveNav` (bottom tabs / sidebar) for
/// admin/staff. Customer-facing links (login, my/bookings, my/balance)
/// are still rendered here.
///
/// #319: for a logged-in CUSTOMER only, the destination links + Logout +
/// language toggle collapse into a burger-triggered `Sheet` (the same
/// component `AdaptiveNav` already uses for staff's "More" sheet) so the
/// header stays a single row even on a phone. Staff/admin and the
/// logged-out state are UNCHANGED — staff's `.navbar-links` is already
/// fully hidden by the `body:has(.adaptive-nav) .navbar-links` CSS rule
/// regardless of what renders here, and the logged-out branch never had a
/// burger to begin with.
#[component]
pub fn Navbar(auth_ver: ReadSignal<u32>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let set_lang = use_context::<WriteSignal<Lang>>().expect("SetLang context");

    let user = move || {
        let _ = auth_ver.get();
        auth::get_user()
    };

    let set_auth_ver = expect_context::<WriteSignal<u32>>();
    let on_logout = move |_| {
        auth::clear_auth();
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

    let desk_reset = use_context::<crate::router::DeskReset>()
        .expect("DeskReset context")
        .0;

    // Derived once, reused by the inline-lang-toggle visibility gate and
    // the Sheet mount gate below — both only need the boolean, unlike the
    // main links block above which needs the full `AuthUser` (name, role
    // branching).
    let is_customer = move || matches!(user(), Some(u) if !u.role.is_staff_or_admin());

    // #319: the customer burger menu's open/close state. The Sheet below
    // is only ever mounted from this signal, and this signal is only ever
    // flipped from the customer branch's own toggle button — safe to gate
    // the Sheet's render on `menu_open` alone.
    let (menu_open, set_menu_open) = signal(false);

    let on_toggle_menu = move |_| {
        let now_open = !menu_open.get();
        set_menu_open.set(now_open);
        if now_open {
            focus_first_in(MENU_SHEET_SELECTOR);
        }
    };
    let on_close_menu = Callback::new(move |()| {
        set_menu_open.set(false);
        focus_element(BURGER_SELECTOR);
    });

    view! {
        <nav class="navbar">
            <a
                href=move || {
                    let _ = auth_ver.get();
                    match auth::get_user() {
                        Some(u) if u.role.is_staff_or_admin() => "/staff",
                        _ => "/",
                    }
                }
                class="navbar-brand"
                data-testid="brand-link"
                on:click=move |_| desk_reset.update(|n| *n += 1)
            >"SpinBike"</a>
            <div class="navbar-links">
                {move || {
                    if let Some(u) = user() {
                        let is_staff = u.role.is_staff_or_admin();
                        if is_staff {
                            // Staff/admin: unchanged. Hidden entirely on
                            // every breakpoint by the
                            // `body:has(.adaptive-nav) .navbar-links`
                            // CSS rule — AdaptiveNav's own "More" sheet
                            // carries these same controls instead.
                            view! {
                                <span class="navbar-user">{u.name.clone()}</span>
                                <button class="btn btn--compact btn--ghost" on:click=on_logout>{move || i18n::t(lang.get(), "logout")}</button>
                            }.into_any()
                        } else {
                            // Customer: compact header — shortened name +
                            // burger toggle only. The destination links,
                            // Logout, and language toggle live inside the
                            // Sheet below.
                            let short_name = i18n::short_display_name(&u.name);
                            view! {
                                <span class="navbar-user" data-testid="navbar-user-name">{short_name}</span>
                                <button
                                    type="button"
                                    id=BURGER_ID
                                    class="navbar-burger"
                                    data-testid="navbar-burger"
                                    aria-expanded=move || menu_open.get().to_string()
                                    aria-controls=MENU_SHEET_ID
                                    aria-label=move || i18n::t(lang.get(), "nav_more")
                                    on:click=on_toggle_menu
                                >
                                    <span class="navbar-burger__icon" inner_html=ICON_BURGER></span>
                                </button>
                            }.into_any()
                        }
                    } else {
                        view! {
                            <a href="/login">{move || i18n::t(lang.get(), "login")}</a>
                        }.into_any()
                    }
                }}
                {move || {
                    // The language toggle stays inline for staff and the
                    // logged-out state; for a customer it moves inside the
                    // burger Sheet below instead (see #319).
                    if !is_customer() {
                        view! {
                            <button
                                class="btn btn--compact btn--ghost lang-toggle"
                                on:click=on_toggle_lang
                            >
                                {move || match lang.get() {
                                    Lang::Sk => "EN",
                                    Lang::En => "SK",
                                }}
                            </button>
                        }.into_any()
                    } else {
                        ().into_any()
                    }
                }}
            </div>
        </nav>
        // #319: the burger's Sheet is a SIBLING of `<nav>`, not nested
        // inside it — matches `AdaptiveNav`'s own "More" sheet, which uses
        // the exact same shape for the exact same reason (a fixed-position
        // overlay doesn't need to live inside the sticky header it's
        // triggered from).
        {move || {
            if is_customer() && menu_open.get() {
                view! {
                    <Sheet
                        id=MENU_SHEET_ID.to_string()
                        testid=MENU_SHEET_ID.to_string()
                        title=i18n::t(lang.get(), "nav_more").to_string()
                        on_close=on_close_menu
                    >
                        // These 3 links are PLAIN anchors — matches the rest
                        // of this app's convention (see e.g. adaptive_nav.rs's
                        // own more-sheet Settings link): a full navigation
                        // reload remounts the whole app, so `menu_open`
                        // resets for free. Still close it explicitly on
                        // click, defensively — never rely on a full-reload
                        // side effect alone to close a dialog, in case a
                        // future change ever swaps these for `leptos_router`'s
                        // `<A>` (client-side nav, no remount).
                        <a
                            href="/my/bookings"
                            class="btn btn--block btn--ghost"
                            data-testid="menu-my-bookings"
                            on:click=move |_| set_menu_open.set(false)
                        >
                            {move || i18n::t(lang.get(), "my_bookings")}
                        </a>
                        <a
                            href="/my/balance"
                            class="btn btn--block btn--ghost"
                            data-testid="menu-balance"
                            on:click=move |_| set_menu_open.set(false)
                        >
                            {move || i18n::t(lang.get(), "balance")}
                        </a>
                        <a
                            href="/my/settings"
                            class="btn btn--block btn--ghost"
                            data-testid="menu-settings"
                            on:click=move |_| set_menu_open.set(false)
                        >
                            {move || i18n::t(lang.get(), "my_settings")}
                        </a>
                        <button
                            class="btn btn--block btn--ghost"
                            data-testid="menu-lang-toggle"
                            on:click=on_toggle_lang
                        >
                            {move || match lang.get() {
                                Lang::Sk => "EN",
                                Lang::En => "SK",
                            }}
                        </button>
                        <button
                            class="btn btn--block btn--danger"
                            data-testid="menu-logout"
                            on:click=on_logout
                        >
                            {move || i18n::t(lang.get(), "logout")}
                        </button>
                    </Sheet>
                }.into_any()
            } else {
                ().into_any()
            }
        }}
    }
}

const ICON_BURGER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5M3.75 17.25h16.5"/></svg>"##;
