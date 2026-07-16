//! Shared browser-platform detection helpers (#228).
//!
//! Extracted out of `components::install_prompt` (where `is_standalone` /
//! `is_ios_ua` originally lived, private) because `CustomerLoginMethods`
//! (`components::code_login_form`) also needs to detect "installed
//! standalone PWA on iOS" — to lead with the code-login method instead of
//! the magic-link method there (a magic link always reopens in Safari, never
//! inside the installed app, so it's a dead end in that one context). Moving
//! these here lets both call sites share the exact same `Reflect`-based JS
//! interop and the iPadOS-desktop-UA disambiguator (#110/#226) rather than
//! re-deriving (or subtly diverging on) the same detection twice.

use js_sys::Reflect;
use wasm_bindgen::JsValue;

pub(crate) fn window_value() -> Option<JsValue> {
    web_sys::window().map(JsValue::from)
}

/// `Reflect::get` with a string key, defaulting to `undefined` on any error
/// (missing property, non-object target) rather than propagating — every
/// caller here treats "absent" and "errored" the same way.
pub(crate) fn get_prop(target: &JsValue, key: &str) -> JsValue {
    Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// True once the app is already running installed (standalone). Checked two
/// ways: the iOS Safari-only legacy `navigator.standalone` flag (no typed
/// web-sys binding — non-standard), and the standard `display-mode:
/// standalone` media query (Chromium + modern Safari), via the typed
/// `Window::match_media` binding (`MediaQueryList` web-sys feature).
pub(crate) fn is_standalone() -> bool {
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

/// `navigator.userAgent`, fetched once and shared by every caller that needs
/// it — avoids each predicate independently re-doing its own `window` ->
/// `navigator` -> `userAgent` `Reflect` round-trip (a wasted extra JS/WASM
/// FFI call per call site).
pub(crate) fn user_agent() -> String {
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
pub(crate) fn is_ios_ua(ua: &str) -> bool {
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

/// True when running as an installed standalone app on iOS specifically —
/// the ONE context where a magic-link login is a dead end (storage is
/// partitioned from Safari, so a link always reopens there instead of
/// completing login inside the installed app). Android/Chromium is
/// deliberately excluded: the browser and the installed PWA share storage
/// there, so no logged-out loop exists and the magic link stays the primary
/// path (#228 — do NOT reorder on Android standalone).
pub(crate) fn is_ios_standalone() -> bool {
    is_standalone() && is_ios_ua(&user_agent())
}
