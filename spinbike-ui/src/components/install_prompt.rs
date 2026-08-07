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
//! iOS Google app, and a generic Android in-app WebView — expose NO "Add to
//! Home Screen" at all, so showing the normal Share guide there is misleading
//! (there is no Share-sheet A2HS entry to find, and on Android there's no
//! install prompt to replay either). The shared [`InAppBrowserBanner`]
//! component UA-sniffs a set of known webview markers
//! (`platform::is_in_app_browser_ua`, #226/#248) and, when detected, shows an
//! "open in a real browser" instruction plus a copy-current-URL button
//! instead — on EITHER platform, not just iOS (#248 lifted this out of the
//! iOS-only gate: an Android in-app browser used to get no guidance at all).
//! A webview like iOS `SFSafariViewController` is indistinguishable from real
//! Safari via UA and is NOT caught by this — the normal Safari-guide branch
//! carries a small permanent footer fallback hint for exactly that case.
//!
//! `InAppBrowserBanner` is self-contained (detects + renders independently)
//! so it can mount standalone on `/login` (#248 — webview guidance ONLY
//! there, no full A2HS prompt) in addition to being `InstallPrompt`'s own
//! webview branch on `/welcome` and `/my/balance`.
//!
//! Mounted on `/welcome` (primary) and `/my/balance` (until installed).
//!
//! **iOS auto-login ladder, round 5 (#261)** — on iOS while logged in, this
//! component arms TWO independent legs of a belt-and-braces credential
//! (a third, unrelated leg — the in-app email-code login — is the fallback
//! rendered by `CustomerLoginMethods`/`CodeLoginForm` when neither of these
//! fires): the manifest `start_url` swap (leg 1, kept from #258) AND
//! `history.replaceState`-arming the address bar itself (leg 2, the new
//! primary carrier — works regardless of whether iOS re-reads the manifest
//! or just captures the page's live URL at "Add to Home Screen" time). A
//! `sessionStorage` guard (`sb_install_token`) ensures only the FIRST mount
//! per browser session mints a token — round 4 minted unconditionally on
//! every mount, which prod evidence showed firing twice per install session.
//! The credential itself (`purpose='install'`, minted server-side) is now
//! 365-day, multi-use, and capped at 5 live tokens/user — see
//! `db/login_tokens.rs`'s `create_install_token`/`redeem_install`.

use std::sync::atomic::{AtomicBool, Ordering};

use js_sys::{Function, Promise, Reflect};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::i18n::{self, Lang};
use crate::platform::{
    get_prop, is_in_app_browser_ua, is_ios_ua, is_standalone, user_agent, window_value,
};
use crate::{api, auth};

/// `sessionStorage` key the round-5 (#261) mint guard reads/writes. Kept in
/// sessionStorage (not localStorage) deliberately — it only needs to survive
/// within the SAME browser tab's session (across the two `InstallPrompt`
/// mount points, `/welcome` and `/my/balance`, and a same-tab reload), never
/// across tabs or devices; a stray leftover key in localStorage would be a
/// worse footgun to clean up later.
const INSTALL_TOKEN_STORAGE_KEY: &str = "sb_install_token";

/// The `/welcome` URL an install token arms — the SAME formula the server's
/// `manifest.rs::install_start_url` produces for the manifest `start_url`,
/// duplicated client-side (not fetched) so `history.replaceState` can arm
/// the address bar synchronously, in the same effect, with zero extra round
/// trip. Kept as its own pure fn (mirrors the server-side test) so the
/// formula is unit-tested independently of any DOM/JS interop.
fn install_welcome_url(token: &str) -> String {
    format!("/welcome?t={token}&src=install")
}

/// Empty body for the authenticated install-token mint (#258). The server
/// mints for the caller's own session (`claims.sub`); no fields are needed.
#[derive(Serialize)]
struct InstallTokenReq {}

#[derive(Deserialize)]
struct InstallTokenResp {
    token: String,
}

/// Point the document's `<link rel="manifest">` at the dynamic install-token
/// manifest (`/manifest.json?it=<token>`) so iOS reads the token-bearing
/// `start_url` at "Add to Home Screen" time (#258). Typed web-sys throughout
/// (`HtmlLinkElement::set_href`); every step degrades to a silent no-op if a
/// browser API is unavailable — never panics.
fn swap_manifest_link(token: &str) {
    let href = format!("/manifest.json?it={token}");
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Ok(Some(element)) = document.query_selector("link[rel=\"manifest\"]")
        && let Ok(link) = element.dyn_into::<web_sys::HtmlLinkElement>()
    {
        link.set_href(&href);
    }
}

/// Read the sessionStorage mint guard (#261). `Some(token)` means a token was
/// already minted THIS session — reuse it, skip the mint POST entirely. This
/// is the fix for the prod double-mint: `InstallPrompt` mounts on BOTH
/// `/welcome` and `/my/balance`, each mount previously fired its own
/// unconditional mint. Degrades to `None` (treated exactly like "not minted
/// yet") on any unavailable step — sessionStorage can legitimately be absent
/// (private browsing, a security exception) and must never panic.
///
/// #287: thin wrapper over the shared `storage::cache_get`/`cache_set`
/// primitive — see `storage.rs` for why it's shared with `auth.rs`'s
/// (different-shaped) one-shot session-expired flag.
fn stored_install_token() -> Option<String> {
    crate::storage::cache_get(INSTALL_TOKEN_STORAGE_KEY)
}

/// Persist a freshly-minted install token into the sessionStorage guard.
/// Silent no-op on any failure (same interop discipline as every other JS
/// call in this module) — losing the guard only means a future mount in
/// this tab might mint again, not a correctness break.
fn store_install_token(token: &str) {
    crate::storage::cache_set(INSTALL_TOKEN_STORAGE_KEY, token);
}

/// Module-level (NOT component-local) single-flight guard for the install-
/// token mint (#282 fix). A Leptos signal declared inside `InstallPrompt`'s
/// component body would be destroyed on unmount and re-created fresh on
/// the NEXT mount — exactly the same gap `stored_install_token()` already
/// has (see its doc). This `static` instead survives for the lifetime of
/// the WASM instance regardless of how many times `InstallPrompt`
/// mounts/unmounts within one browser session (e.g. `/welcome` -> the CTA
/// link -> `/my/balance`), so it can actually guard ACROSS mounts.
static MINT_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Whether THIS `Effect` firing may proceed to mint a fresh install token:
/// `false` when a token is already stored, OR when another firing already
/// claimed the slot and hasn't finished (or failed) yet.
///
/// #282 fix: a session-storage-only check is a check-then-act race — the
/// mint POST is async and `store_install_token()` only runs after it
/// resolves, so a SECOND `Effect` firing (e.g. `InstallPrompt`'s second
/// mount on `/my/balance` after the `/welcome` CTA link, before the first
/// mint's response has landed) used to also observe no stored token and
/// also mint. The fix is `AtomicBool::swap` — a single SYNCHRONOUS
/// check-and-set with no `.await` in between — so only the first firing in
/// this WASM instance's lifetime can ever pass both checks. See the
/// `only_one_effect_firing_may_claim_the_mint_slot_before_a_token_is_stored`
/// unit test below.
fn try_claim_mint_slot() -> bool {
    if stored_install_token().is_some() {
        return false;
    }
    !MINT_IN_FLIGHT.swap(true, Ordering::SeqCst)
}

/// Release a previously-claimed mint slot after a FAILED mint (network
/// error, non-2xx, etc.) so a later remount in this session can retry —
/// matching this module's existing "every step degrades to a silent no-op
/// on failure" discipline.
fn release_mint_slot() {
    MINT_IN_FLIGHT.store(false, Ordering::SeqCst);
}

/// Persist + arm the credential for a freshly, successfully minted token,
/// then verify the persist actually landed — releasing the slot if it
/// didn't (code-review finding on #282, [green]).
///
/// A mint that succeeds at the HTTP layer can still fail to PERSIST
/// client-side — `store_install_token()` (`storage::cache_set`) silently
/// no-ops on ANY sessionStorage failure (private browsing, quota, a
/// security exception — same degrade-gracefully discipline as every other
/// storage call in this crate, see `storage.rs`). Without this check,
/// `MINT_IN_FLIGHT` would stay permanently claimed even though
/// `stored_install_token()` keeps returning `None` on every later mount in
/// this WASM session — `try_claim_mint_slot()` would then refuse forever,
/// silently disabling the belt-and-braces auto-login credential for the
/// rest of the session with no way to recover short of a full page reload.
/// This mount's OWN credential is still armed regardless (the token is
/// already in hand, storage is only a cache for LATER mounts) — only the
/// slot's retry-eligibility depends on the persist having actually landed.
/// See `a_successful_mint_that_fails_to_persist_releases_the_slot_for_a_later_retry`.
fn confirm_mint_or_release(token: &str) {
    store_install_token(token);
    arm_install_credential(token);
    if stored_install_token().is_none() {
        release_mint_slot();
    }
}

/// Arm BOTH remaining legs of the round-5 (#261) ladder for an install
/// token, given a token already minted (or reused from the sessionStorage
/// guard):
///
/// - Leg 1 (kept from #258): swap the manifest `<link>` so iOS versions that
///   re-read the manifest at "Add to Home Screen" time pick up the
///   token-bearing `start_url`.
/// - Leg 2 (new, #261, the PRIMARY carrier): `history.replaceState` the
///   current address bar to the exact install URL. Whether iOS captures the
///   page's live URL (H1) or a stale manifest parse (H2) at A2HS time, the
///   captured URL is already the auto-login URL — no manifest-read
///   dependency needed for this leg to work. `replaceState` adds no history
///   entry, so the back button is unaffected. Typed `web_sys::History` API
///   (no `Reflect` needed — `History` is a normal DOM interface).
///
/// Called on EVERY mount that has a token available (fresh mint or reused
/// from storage) — `InstallPrompt` mounts on both `/welcome` and
/// `/my/balance`, so both pages get both legs armed, per the design's
/// requirement.
fn arm_install_credential(token: &str) {
    swap_manifest_link(token);
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(history) = window.history() else {
        return;
    };
    let url = install_welcome_url(token);
    let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    /// Already installed, running inside a known in-app browser (handled
    /// separately by `InAppBrowserBanner`), or neither install path is
    /// available here.
    Hidden,
    /// Chromium/Android captured a `beforeinstallprompt` event we can replay.
    AndroidChromium,
    /// iOS Safari (or an undetectable webview) — no native event; show the
    /// visual Share -> Add-to-Home-Screen guide.
    Ios,
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

fn detect_kind() -> PromptKind {
    if is_standalone() {
        return PromptKind::Hidden;
    }
    let ua = user_agent();
    // A known in-app browser is handled entirely by `InAppBrowserBanner`
    // (mounted alongside this component) — checked BEFORE the deferred-
    // prompt/iOS branches, and unconditionally (not gated behind "is this
    // iOS"), so an Android webview short-circuits here exactly like an iOS
    // one instead of falling through to the Android-Chromium install button.
    if is_in_app_browser_ua(&ua) {
        return PromptKind::Hidden;
    }
    if has_deferred_prompt() {
        return PromptKind::AndroidChromium;
    }
    if !is_ios_ua(&ua) {
        return PromptKind::Hidden;
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

/// "Open in a real browser" banner shown when this page is being viewed
/// inside a known in-app browser (#226/#248 — see the module doc). Detected
/// once at mount (the UA doesn't change while a page stays mounted) and
/// renders nothing when not applicable, so it's safe to mount UNCONDITIONALLY
/// anywhere: standalone on `/login` (#248, webview guidance ONLY — no full
/// A2HS prompt there), and as `InstallPrompt`'s own webview branch below.
///
/// Copy differs by platform (#248): the iOS copy says "open in Safari" (the
/// only browser that can complete an A2HS install there); the non-iOS
/// (Android in-app-browser) copy says "open in Chrome" instead — Safari
/// guidance would be actively wrong on Android.
#[component]
pub fn InAppBrowserBanner() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let ua = user_agent();
    if is_standalone() || !is_in_app_browser_ua(&ua) {
        return ().into_any();
    }
    let ios = is_ios_ua(&ua);
    let title_key = if ios {
        "install_prompt_webview_title"
    } else {
        "install_prompt_webview_title_other"
    };
    let (copied, set_copied) = signal(false);

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
        <div class="install-prompt install-prompt--webview" data-testid="install-prompt-webview">
            <p class="install-prompt__title">
                {move || i18n::t(lang.get(), title_key)}
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
    .into_any()
}

#[component]
pub fn InstallPrompt() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    // Detected once at mount — the deferred event (if any) has virtually
    // always already fired by the time our WASM bundle finishes loading and
    // this component mounts, and re-checking reactively would need either
    // polling or a Rust-side event listener the design map didn't ask for.
    let initial_kind = detect_kind();
    let (kind, set_kind) = signal(initial_kind);

    // #258, round-5 redesign #261: on iOS specifically (detect_kind() already
    // returns Hidden when standalone, so PromptKind::Ios implies "iOS AND not
    // yet installed") AND while logged in, arm both remaining legs of the
    // belt-and-braces ladder — the manifest swap (leg 1) AND
    // history.replaceState (leg 2, the primary carrier since #261; leg 3 is
    // the in-app code-login fallback, unrelated to this component). iOS
    // ONLY: Android shares storage between browser and installed PWA (no
    // logged-out loop) and mutating the manifest/address-bar at runtime
    // could disturb its own beforeinstallprompt eligibility.
    //
    // #261 mint guard: check sessionStorage FIRST. `InstallPrompt` mounts on
    // BOTH `/welcome` and `/my/balance` — without this guard each mount
    // fires its own unconditional mint POST, which is exactly the prod
    // double-mint evidence (#261: two `install-token: minted` log lines 16
    // minutes apart for the same user, no re-entrancy guard). A stored token
    // is reused and re-armed on every mount (including a same-tab reload);
    // only the FIRST mount in a session actually POSTs.
    //
    // #282: `try_claim_mint_slot()` additionally guards against a SECOND
    // mount racing ahead of the FIRST mint's async response — see its doc.
    //
    // Fires once on mount; the mint is an authenticated call (api::post) and
    // every step degrades to a silent no-op on failure.
    if initial_kind == PromptKind::Ios && auth::get_token().is_some() {
        Effect::new(move |_| {
            if let Some(token) = stored_install_token() {
                arm_install_credential(&token);
                return;
            }
            if !try_claim_mint_slot() {
                return;
            }
            spawn_local(async move {
                match api::post::<InstallTokenReq, InstallTokenResp>(
                    "/api/auth/install-token",
                    &InstallTokenReq {},
                )
                .await
                {
                    Ok(resp) => confirm_mint_or_release(&resp.token),
                    Err(_) => release_mint_slot(),
                }
            });
        });
    }

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
        // A known in-app browser is handled entirely by `InAppBrowserBanner`
        // — `detect_kind()` returns `Hidden` for that case (see its doc), so
        // this never renders alongside the Android/iOS branches below, only
        // in place of them.
        <InAppBrowserBanner />
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
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        confirm_mint_or_release, install_welcome_url, release_mint_slot, try_claim_mint_slot,
    };
    use wasm_bindgen_test::*;

    /// Mirrors the server-side `manifest.rs::install_start_url` formula
    /// exactly (#261) — both sides must agree byte-for-byte since `/welcome`
    /// is reached via EITHER the manifest's `start_url` (server-built) or
    /// `history.replaceState` (client-built, this fn).
    #[wasm_bindgen_test]
    fn install_welcome_url_matches_the_server_formula() {
        assert_eq!(
            install_welcome_url("abc-123_XYZ"),
            "/welcome?t=abc-123_XYZ&src=install"
        );
    }

    /// #282 regression: two `Effect` firings can both race ahead of the
    /// first mint POST's response landing — `store_install_token()` never
    /// runs until then, so a session-storage-only check (`stored_install_token()
    /// .is_none()`) sees "no token yet" both times and lets BOTH claim a
    /// mint slot, exactly the CI-observed `mintCount == 2`
    /// (`install-prompt.spec.ts`'s "sessionStorage guard" test). `wasm-pack
    /// test --node` has no real `window`/sessionStorage (see `storage.rs`'s
    /// own tests), so `stored_install_token()` returns `None` on every call
    /// in this env regardless of what would "really" be stored — which is
    /// exactly what makes this a clean, deterministic repro of the race:
    /// the ONLY thing that may distinguish the first call from the second
    /// is an in-memory single-flight guard, not sessionStorage state.
    #[wasm_bindgen_test]
    fn only_one_effect_firing_may_claim_the_mint_slot_before_a_token_is_stored() {
        // Normalize state first: MINT_IN_FLIGHT is a module-level static
        // shared across every test in this file's wasm-pack test binary
        // (same wasm instance, no per-test isolation) — without this reset,
        // a run order where another test claims-and-leaves-claimed first
        // would make `first` false here, an order-dependent flake.
        release_mint_slot();
        let first = try_claim_mint_slot();
        let second = try_claim_mint_slot();
        assert!(first, "the first firing must be allowed to mint");
        assert!(
            !second,
            "#282: a second firing before the first's mint response lands must NOT also claim the slot"
        );
    }

    /// Code-review finding on #282: a mint that succeeds at the HTTP layer
    /// can still fail to PERSIST client-side (sessionStorage throwing in
    /// private browsing, quota, a security exception — `store_install_token`
    /// silently no-ops on any such failure, see `storage.rs`). If the slot
    /// stays claimed regardless, `try_claim_mint_slot()` refuses FOREVER on
    /// every later mount in this WASM session even though
    /// `stored_install_token()` keeps returning `None` — silently disabling
    /// the belt-and-braces auto-login credential with no way to recover
    /// short of a full page reload. `wasm-pack test --node` has no real
    /// `window`/sessionStorage at all (see `storage.rs`'s own tests), so
    /// `store_install_token()` always silently no-ops here — exactly
    /// simulating a persist failure even though this test's fake token
    /// stands in for a real, successful 200 response.
    ///
    /// [red -> green]: failed on the commit that only extracted
    /// `confirm_mint_or_release` without yet verifying the persist landed
    /// (`try_claim_mint_slot()` stayed `false` after a "successful" mint
    /// that never actually persisted); passes now that it releases the
    /// slot when the persist didn't land.
    #[wasm_bindgen_test]
    fn a_successful_mint_that_fails_to_persist_releases_the_slot_for_a_later_retry() {
        release_mint_slot();
        assert!(
            try_claim_mint_slot(),
            "must be claimable at the start of this test"
        );
        confirm_mint_or_release("fake-token-never-persists-in-this-test-env");
        assert!(
            try_claim_mint_slot(),
            "#282: a mint whose persist silently failed must release its slot so a later mount can retry"
        );
    }
}
