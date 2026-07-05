//! Install-to-home-screen prompt (#110). `index.html` captures the
//! Chromium/Android `beforeinstallprompt` event into
//! `window.__deferredInstallPrompt` (the event has no typed web-sys
//! binding); this component reads that global and shows a big "Pridat na
//! plochu" button that re-fires the captured `.prompt()`. iOS Safari never
//! fires that event, so on iOS we render a static 2-step Share -> "Pridat na
//! plochu" guide instead. Renders nothing once the app is already running
//! standalone (installed), and nothing on a browser that offers neither path
//! (e.g. desktop, or Android before the event has fired).
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
    /// iOS Safari — no native event; show the manual Share guide.
    Ios,
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
/// standalone` media query (Chromium + modern Safari).
fn is_standalone() -> bool {
    let Some(window) = window_value() else {
        return false;
    };
    let navigator = get_prop(&window, "navigator");
    if get_prop(&navigator, "standalone").as_bool() == Some(true) {
        return true;
    }
    let match_media = get_prop(&window, "matchMedia");
    if let Some(func) = match_media.dyn_ref::<Function>() {
        if let Ok(result) = func.call1(&window, &JsValue::from_str("(display-mode: standalone)")) {
            if get_prop(&result, "matches").as_bool() == Some(true) {
                return true;
            }
        }
    }
    false
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

/// iOS Safari has no `beforeinstallprompt` event at all, so eligibility is
/// UA-sniffed: `navigator.userAgent` containing `iPhone`/`iPad`.
fn is_ios_ua() -> bool {
    let Some(window) = window_value() else {
        return false;
    };
    let navigator = get_prop(&window, "navigator");
    let ua = get_prop(&navigator, "userAgent")
        .as_string()
        .unwrap_or_default();
    ua.contains("iPhone") || ua.contains("iPad")
}

fn detect_kind() -> PromptKind {
    if is_standalone() {
        return PromptKind::Hidden;
    }
    if has_deferred_prompt() {
        return PromptKind::AndroidChromium;
    }
    if is_ios_ua() {
        return PromptKind::Ios;
    }
    PromptKind::Hidden
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
    if let Some(prompt_fn) = prompt_val.dyn_ref::<Function>() {
        if let Ok(result) = prompt_fn.call0(&event) {
            let _ = JsFuture::from(Promise::resolve(&result)).await;
        }
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

#[component]
pub fn InstallPrompt() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    // Detected once at mount — the deferred event (if any) has virtually
    // always already fired by the time our WASM bundle finishes loading and
    // this component mounts, and re-checking reactively would need either
    // polling or a Rust-side event listener the design map didn't ask for.
    let (kind, set_kind) = signal(detect_kind());

    let on_install_click = move |_| {
        // Hide immediately — the captured event can only be used once, so
        // there is nothing useful to show after this click regardless of
        // the user's choice in the native dialog.
        set_kind.set(PromptKind::Hidden);
        spawn_local(async move {
            trigger_install_prompt().await;
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
                            <span class="install-prompt__icon" aria-hidden="true">"\u{1F4E4}"</span>
                            {move || i18n::t(lang.get(), "install_prompt_ios_step1")}
                        </li>
                        <li data-testid="install-prompt-ios-step2">
                            <span class="install-prompt__icon" aria-hidden="true">"\u{2795}"</span>
                            {move || i18n::t(lang.get(), "install_prompt_ios_step2")}
                        </li>
                    </ol>
                </div>
            }
            .into_any(),
        }}
    }
}
