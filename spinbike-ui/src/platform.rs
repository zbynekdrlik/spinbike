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

/// Known in-app browsers (webviews) that expose NO "Add to Home Screen" /
/// "open in system browser" surface at all — Facebook/Messenger, Instagram,
/// LINE, the iOS Google app (GSA), and a generic Android in-app WebView
/// (`"; wv)"` — the standard Android `WebView`-UA marker every Chromium-based
/// in-app browser adds, e.g. Facebook/Instagram's Android webview, distinct
/// from the iOS-only markers above it). This is a best-effort UA substring
/// match; some webviews (notably iOS `SFSafariViewController`-based ones) are
/// indistinguishable from real Safari and are NOT caught here.
///
/// **Single shared marker list (#248):** originally lived private in
/// `components::install_prompt` as `is_ios_webview_ua`, checked only inside
/// an iOS-only gate — an Android in-app browser (same apps, same webview
/// problem) got NO banner at all, since a non-iOS UA short-circuited before
/// the check ever ran. Promoted here (same `platform.rs` pattern as
/// `is_ios_standalone` above) so it can be checked UNCONDITIONALLY —
/// `install_prompt.rs`'s `InAppBrowserBanner` and any future call site share
/// this ONE list; a marker added for one platform is automatically covered
/// for the other.
pub(crate) fn is_in_app_browser_ua(ua: &str) -> bool {
    [
        "FBAN",
        "FBAV",
        "FB_IAB",
        "Instagram",
        "Line/",
        "GSA/",
        "Messenger",
        "; wv)",
    ]
    .into_iter()
    .any(|marker| ua.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::is_in_app_browser_ua;
    use wasm_bindgen_test::*;

    const SAFARI_IOS: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1";
    const CHROME_ANDROID: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
    const DESKTOP_CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
    const ANDROID_WEBVIEW: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 7 Build/UQ1A.240205.004; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/126.0.0.0 Mobile Safari/537.36";

    #[wasm_bindgen_test]
    fn detects_every_known_in_app_browser_marker() {
        let cases: [(&str, String); 8] = [
            (
                "FBAN (iOS Facebook)",
                format!("{SAFARI_IOS} [FBAN/FBIOS;FBAV/300.0]"),
            ),
            (
                "FBAV (Android Facebook)",
                format!("{CHROME_ANDROID} FBAV/300.0.0.0.0"),
            ),
            ("FB_IAB", format!("{SAFARI_IOS} FB_IAB/FB4A")),
            ("Instagram", format!("{SAFARI_IOS} Instagram 300.0.0.0.0")),
            ("Line/", format!("{SAFARI_IOS} Line/13.0.0")),
            (
                "GSA/ (iOS Google app)",
                format!("{SAFARI_IOS} GSA/300.0.123456"),
            ),
            ("Messenger", format!("{CHROME_ANDROID} Messenger")),
            ("Android WebView (; wv))", ANDROID_WEBVIEW.to_string()),
        ];
        for (label, ua) in cases {
            assert!(
                is_in_app_browser_ua(&ua),
                "expected {label} UA to match: {ua}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn plain_browsers_do_not_match() {
        for (label, ua) in [
            ("plain Safari iOS", SAFARI_IOS),
            ("plain Chrome Android", CHROME_ANDROID),
            ("plain desktop Chrome", DESKTOP_CHROME),
        ] {
            assert!(
                !is_in_app_browser_ua(ua),
                "expected {label} UA NOT to match: {ua}"
            );
        }
    }
}
