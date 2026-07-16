//! Install-to-home-screen prompt (#110, reworked in #226). `index.html`
//! captures the Chromium/Android `beforeinstallprompt` event into
//! `window.__deferredInstallPrompt` (the event has no typed web-sys
//! binding); this component reads that global and shows a big "Pridat na
//! plochu" button that re-fires the captured `.prompt()`. iOS Safari never
//! fires that event, so on iOS we render a visual numbered Share -> "Pridat
//! na plochu" guide instead (SVG glyphs, not emoji — #226). Renders nothing
//! once the app is already running standalone (installed), and nothing on a
//! browser that offers neither path (e.g. desktop, or Android before the
//! event has fired).
//!
//! **In-app browsers (webviews)** — Facebook/Messenger, Instagram, LINE, the
//! iOS Google app — expose NO "Add to Home Screen" at all, so showing the
//! normal Share guide there is misleading (there is no Share-sheet A2HS
//! entry to find). #226 UA-sniffs a set of known webview markers and, when
//! detected on iOS, replaces the A2HS steps with an "open in Safari"
//! instruction plus a copy-current-URL button. A webview like
//! `SFSafariViewController` is indistinguishable from real Safari via UA and
//! is NOT caught by this — the normal Safari-guide branch carries a small
//! permanent footer fallback hint for exactly that case.
//!
//! Mounted on `/welcome` (primary) and `/my/balance` (until installed).

use js_sys::{Function, Promise, Reflect};
use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::i18n::{self, Lang};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    /// Already installed, or neither install path is available here.
    Hidden,
    /// Chromium/Android captured a `beforeinstallprompt` event we can replay.
    AndroidChromium,
    /// iOS Safari (or an undetectable webview) — no native event; show the
    /// visual Share -> Add-to-Home-Screen guide.
    Ios,
    /// iOS, but a KNOWN in-app-browser (webview) with no A2HS surface at
    /// all — show "open in Safari" + a copy-URL button instead.
    IosWebview,
}

fn window_value() -> Option<JsValue> {
    web_sys::window().map(JsValue::from)
}

/// `Reflect::get` with a string key, defaulting to `undefined` on any error
/// (missing property, non-object target) rather than propagating — every
/// caller here treats "absent" and "errored" the same way.
fn get_prop(target: &JsValue, key: &str) -> JsValue {
    Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// True once the app is already running installed (standalone). Checked two
/// ways: the iOS Safari-only legacy `navigator.standalone` flag (no typed
/// web-sys binding — non-standard), and the standard `display-mode:
/// standalone` media query (Chromium + modern Safari), via the typed
/// `Window::match_media` binding (`MediaQueryList` web-sys feature).
fn is_standalone() -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    let window = JsValue::from(win.clone());
    let navigator = get_prop(&window, "navigator");
    if get_prop(&navigator, "standalone").as_bool() == Some(true) {
        return true;
    }
    win.match_media("(display-mode: standalone)")
        .ok()
        .flatten()
        .is_some_and(|mql| mql.matches())
}

/// True when `index.html`'s `beforeinstallprompt` listener has captured a
/// deferred install event we can still replay (Chromium/Android eligibility).
fn has_deferred_prompt() -> bool {
    let Some(window) = window_value() else {
        return false;
    };
    let v = get_prop(&window, "__deferredInstallPrompt");
    !v.is_undefined() && !v.is_null()
}

/// `navigator.userAgent`, fetched once and shared by `is_ios_ua` and
/// `is_ios_webview_ua` — both used to independently re-fetch it via their own
/// `window` -> `navigator` -> `userAgent` `Reflect` round-trip, which is both
/// duplicated logic and a wasted extra JS/WASM FFI call per mount on every
/// iOS visitor.
fn user_agent() -> String {
    let Some(window) = window_value() else {
        return String::new();
    };
    let navigator = get_prop(&window, "navigator");
    get_prop(&navigator, "userAgent")
        .as_string()
        .unwrap_or_default()
}

/// iOS Safari has no `beforeinstallprompt` event at all, so eligibility is
/// UA-sniffed: `navigator.userAgent` containing `iPhone`/`iPad`. This alone
/// misses real iPads: since iPadOS 13, Safari defaults to "Request Desktop
/// Website", so `navigator.userAgent` reports as a plain Mac
/// (`Macintosh; Intel Mac OS X ...`) with no `iPad` substring at all. The
/// standard disambiguator: a genuine Mac reports zero touch points, while an
/// iPad — even UA-spoofed as a Mac — reports `navigator.maxTouchPoints > 1`.
fn is_ios_ua(ua: &str) -> bool {
    if ua.contains("iPhone") || ua.contains("iPad") {
        return true;
    }
    let Some(window) = window_value() else {
        return false;
    };
    let navigator = get_prop(&window, "navigator");
    let platform = get_prop(&navigator, "platform")
        .as_string()
        .unwrap_or_default();
    let max_touch_points = get_prop(&navigator, "maxTouchPoints")
        .as_f64()
        .unwrap_or(0.0);
    platform == "MacIntel" && max_touch_points > 1.0
}

/// Known iOS in-app-browsers (webviews) — Facebook/Messenger, Instagram,
/// LINE, and the iOS Google app's embedded browser (GSA) — none of which
/// expose "Add to Home Screen" at all, unlike real Safari. This is a
/// best-effort UA substring match; some webviews (notably
/// `SFSafariViewController`-based ones) are indistinguishable from real
/// Safari and are NOT caught here — see the footer fallback hint rendered
/// on the normal iOS Safari-guide branch.
fn is_ios_webview_ua(ua: &str) -> bool {
    ["FBAN", "FBAV", "FB_IAB", "Instagram", "Line/", "GSA/"]
        .into_iter()
        .any(|marker| ua.contains(marker))
}

fn detect_kind() -> PromptKind {
    if is_standalone() {
        return PromptKind::Hidden;
    }
    if has_deferred_prompt() {
        return PromptKind::AndroidChromium;
    }
    let ua = user_agent();
    if !is_ios_ua(&ua) {
        return PromptKind::Hidden;
    }
    if is_ios_webview_ua(&ua) {
        return PromptKind::IosWebview;
    }
    PromptKind::Ios
}

/// Replays the captured `beforeinstallprompt` event: calls `.prompt()`,
/// awaits `userChoice` (resolves once the user accepts/dismisses the native
/// dialog), then clears the global — a captured event only honors ONE
/// `.prompt()` call, so it can never be replayed after this.
async fn trigger_install_prompt() {
    let Some(window) = window_value() else {
        return;
    };
    let event = get_prop(&window, "__deferredInstallPrompt");
    if event.is_undefined() || event.is_null() {
        return;
    }
    let prompt_val = get_prop(&event, "prompt");
    if let Some(prompt_fn) = prompt_val.dyn_ref::<Function>()
        && let Ok(result) = prompt_fn.call0(&event)
    {
        let _ = JsFuture::from(Promise::resolve(&result)).await;
    }
    let user_choice = get_prop(&event, "userChoice");
    if !user_choice.is_undefined() {
        let _ = JsFuture::from(Promise::resolve(&user_choice)).await;
    }
    let _ = Reflect::set(
        &window,
        &JsValue::from_str("__deferredInstallPrompt"),
        &JsValue::NULL,
    );
}

/// Copies the current page's URL (origin + pathname, deliberately WITHOUT
/// the query string — see below) via `navigator.clipboard.writeText`, the
/// only way to hand a webview user the real address to paste into Safari
/// since a webview has no address bar to copy from directly (#226).
/// `navigator.clipboard` has no typed web-sys binding used elsewhere in this
/// crate, so it's read via `Reflect` like the rest of this file. Degrades to
/// `None` (silent no-op, never panics) if the property, the method, or the
/// call itself is unavailable — e.g. an older webview with no Clipboard API
/// at all, or one that denies the permission.
///
/// **Deliberately drops any query string** (`href` minus its `?...` suffix):
/// `InstallPrompt` also mounts on `/welcome?t=<token>` right after a
/// magic-link token is redeemed (`pages/welcome.rs`) — that redemption is
/// single-use and the page never strips `?t=` from the address bar
/// afterward, so copying the raw `href` there would hand the user their own
/// already-spent, now-invalid token, sending them straight back to the
/// "invalid link" screen when they paste it into Safari (deep-review finding
/// on #226). `origin + pathname` is always the right thing to paste
/// regardless of which page/query-string state this component is mounted in.
///
/// Split from the `await` on purpose: `clipboard.writeText()` itself is
/// dispatched HERE, synchronously, so it runs inside the same call stack as
/// the triggering click — some stricter WebKit/Safari builds only honor the
/// Clipboard API's required user-activation when the write is fired
/// synchronously from the originating event, not after a `spawn_local`/
/// `.await` microtask hop. The caller only awaits the returned `Promise`.
fn start_copy_current_url() -> Option<Promise> {
    let window = window_value()?;
    let navigator = get_prop(&window, "navigator");
    let clipboard = get_prop(&navigator, "clipboard");
    if clipboard.is_undefined() || clipboard.is_null() {
        return None;
    }
    let location = get_prop(&window, "location");
    let origin = get_prop(&location, "origin")
        .as_string()
        .unwrap_or_default();
    let pathname = get_prop(&location, "pathname")
        .as_string()
        .unwrap_or_default();
    let url = format!("{origin}{pathname}");
    let write_text_val = get_prop(&clipboard, "writeText");
    let write_text_fn = write_text_val.dyn_ref::<Function>()?;
    let result = write_text_fn
        .call1(&clipboard, &JsValue::from_str(&url))
        .ok()?;
    Some(Promise::resolve(&result))
}

const ICON_SHARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M12 16.5V9.75m0 0 3 3m-3-3-3 3M6.75 19.5h10.5a2.25 2.25 0 0 0 2.25-2.25V13.5a.75.75 0 0 0-1.5 0v3.75a.75.75 0 0 1-.75.75H6.75a.75.75 0 0 1-.75-.75V13.5a.75.75 0 0 0-1.5 0v3.75A2.25 2.25 0 0 0 6.75 19.5Z"/></svg>"##;
const ICON_PLUS_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor" aria-hidden="true"><rect x="3.75" y="3.75" width="16.5" height="16.5" rx="3" stroke-linecap="round" stroke-linejoin="round"/><path stroke-linecap="round" stroke-linejoin="round" d="M12 8.25v7.5M8.25 12h7.5"/></svg>"##;

#[component]
pub fn InstallPrompt() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    // Detected once at mount — the deferred event (if any) has virtually
    // always already fired by the time our WASM bundle finishes loading and
    // this component mounts, and re-checking reactively would need either
    // polling or a Rust-side event listener the design map didn't ask for.
    let (kind, set_kind) = signal(detect_kind());
    // Local to the webview branch only — whether the copy-URL click has
    // succeeded, so a small confirmation line can render under the button.
    let (copied, set_copied) = signal(false);

    let on_install_click = move |_| {
        // Hide immediately — the captured event can only be used once, so
        // there is nothing useful to show after this click regardless of
        // the user's choice in the native dialog.
        set_kind.set(PromptKind::Hidden);
        spawn_local(async move {
            trigger_install_prompt().await;
        });
    };

    let on_copy_click = move |_| {
        // `start_copy_current_url` fires the actual `clipboard.writeText()`
        // call SYNCHRONOUSLY, right here in the click handler — only the
        // `Promise` it returns is awaited inside `spawn_local` (see its
        // doc comment for why the split matters).
        let Some(promise) = start_copy_current_url() else {
            return;
        };
        spawn_local(async move {
            if JsFuture::from(promise).await.is_ok() {
                set_copied.set(true);
            }
        });
    };

    view! {
        {move || match kind.get() {
            PromptKind::Hidden => ().into_any(),
            PromptKind::AndroidChromium => view! {
                <div class="install-prompt" data-testid="install-prompt-android">
                    <button
                        class="btn btn--primary btn--hero btn--block"
                        data-testid="install-prompt-button"
                        on:click=on_install_click
                    >
                        {move || i18n::t(lang.get(), "install_prompt_cta")}
                    </button>
                </div>
            }
            .into_any(),
            PromptKind::Ios => view! {
                <div class="install-prompt install-prompt--ios" data-testid="install-prompt-ios">
                    <p class="install-prompt__title">
                        {move || i18n::t(lang.get(), "install_prompt_ios_title")}
                    </p>
                    <ol class="install-prompt__steps">
                        <li data-testid="install-prompt-ios-step1">
                            <span class="install-prompt__step-num" aria-hidden="true">"1"</span>
                            <span class="install-prompt__icon" inner_html=ICON_SHARE></span>
                            <span>{move || i18n::t(lang.get(), "install_prompt_ios_step1")}</span>
                        </li>
                        <li data-testid="install-prompt-ios-step2">
                            <span class="install-prompt__step-num" aria-hidden="true">"2"</span>
                            <span class="install-prompt__icon" inner_html=ICON_PLUS_SQUARE></span>
                            <span>{move || i18n::t(lang.get(), "install_prompt_ios_step2")}</span>
                        </li>
                    </ol>
                    <p class="install-prompt__scroll-hint" data-testid="install-prompt-ios-scroll-hint">
                        {move || i18n::t(lang.get(), "install_prompt_ios_scroll_hint")}
                    </p>
                    <p class="install-prompt__footer-hint" data-testid="install-prompt-ios-footer-hint">
                        {move || i18n::t(lang.get(), "install_prompt_ios_footer_hint")}
                    </p>
                </div>
            }
            .into_any(),
            PromptKind::IosWebview => view! {
                <div class="install-prompt install-prompt--ios install-prompt--webview" data-testid="install-prompt-ios-webview">
                    <p class="install-prompt__title">
                        {move || i18n::t(lang.get(), "install_prompt_webview_title")}
                    </p>
                    <button
                        class="btn btn--ghost btn--block"
                        data-testid="install-prompt-copy-url"
                        on:click=on_copy_click
                    >
                        {move || i18n::t(lang.get(), "install_prompt_copy_button")}
                    </button>
                    {move || {
                        copied.get().then(|| view! {
                            <div class="alert alert-success" data-testid="install-prompt-copy-confirm">
                                {move || i18n::t(lang.get(), "install_prompt_copy_confirm")}
                            </div>
                        })
                    }}
                </div>
            }
            .into_any(),
        }}
    }
}
