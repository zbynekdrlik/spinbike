//! Web-Push notification settings row for `/my/balance` (#264, redesigned
//! #303).
//!
//! **#303 replaced the original permanent `.btn.btn--primary` CTA with a
//! settings-row toggle switch (`.card-push` idiom, matching `.card-credit`/
//! `.card-pass`) that works BOTH ways** — `Off -> On` subscribes, `On -> Off`
//! unsubscribes (browser `PushSubscription.unsubscribe()` +
//! `POST /api/push/unsubscribe`, which already existed server-side, see
//! `routes::push::unsubscribe`).
//!
//! **Owner follow-up decision (issue #303 comment, 2026-08-08): notifications
//! must be ON by default — this OVERRIDES this module's own original
//! "never auto-prompt" doc comment for the specific case where permission is
//! already `granted`.** Two behaviors layer on top of the switch:
//!
//! 1. **Silent auto-subscribe.** If `Notification.permission === "granted"`
//!    (already decided, at some point, by the user) and the server reports
//!    no subscription, the mount effect subscribes with ZERO tap — no
//!    `requestPermission()` call is needed or made (permission is already
//!    granted), so this never risks a rejected-for-no-gesture prompt.
//! 2. **One-time proactive prompt.** If permission is still `default`
//!    (undecided), a prominent banner (NOT the settings switch) offers
//!    "Zapnut upozornenia" / "Teraz nie" once. Either choice persists via
//!    `localStorage` (`PUSH_PROMPT_DISMISSED_KEY`) so it never shows again.
//!    This is the ONE soft-prompt the original design explicitly banned —
//!    #303's owner comment explicitly supersedes that non-goal for this one
//!    case; nothing else about "no second banner" changes.
//!
//! **The auto-subscribe rule (1) would otherwise fight the switch's `On ->
//! Off` direction:** unsubscribing does not revoke browser permission, so a
//! plain re-application of rule 1 would silently re-subscribe the user on
//! every reload after they explicitly turned notifications off — the
//! opposite of what a working toggle promises. `PUSH_USER_DISABLED_KEY` (a
//! second, independent `localStorage` flag, set only on a manual `On -> Off`
//! click and cleared on a manual `Off -> On` click or a successful
//! auto-subscribe) is the fix: the mount effect only auto-subscribes when
//! `permission == granted && !subscribed && !push_user_disabled()`. This is
//! a purely client-side UI memory, not a new server-side preference (the
//! owner's comment only forbids introducing a NEW server preference when
//! none exists — none does, and none is added here).
//!
//! **Reading the subscription result uses `js_sys::Reflect` and `toJSON()`,
//! not a typed `web_sys::PushSubscription` binding.** `PushSubscription`
//! has no live `keys` PROPERTY, only a `toJSON()` METHOD that computes
//! `{endpoint, keys: {p256dh, auth}}` from the raw key material. Calling
//! that method dynamically, rather than `.dyn_into::<PushSubscription>()`
//! and typed getters, works identically whether the resolved value is a
//! real browser `PushSubscription` or a plain object shaped the same way.
//! That is what makes the E2E tests for this component tractable: they stub
//! `PushManager.prototype.subscribe`/`getSubscription` (the hops that would
//! otherwise need a live round-trip to the browser's real push service)
//! with plain JS objects exposing `endpoint`/`toJSON()`/`unsubscribe()`, and
//! this code neither knows nor cares that they aren't native class
//! instances. `ServiceWorkerRegistration`, `PushManager`, and `Notification`
//! stay on typed `web_sys` bindings throughout; only the final
//! subscription-object read is untyped.

use js_sys::Function;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::api;
use crate::i18n::{self, Lang};
use crate::platform::{get_prop, window_value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PushState {
    Loading,
    /// No `Notification`/`serviceWorker`/`PushManager` API at all.
    Unsupported,
    /// Server has no `VAPID_PRIVATE_KEY` configured.
    Disabled,
    /// `Notification.permission === "denied"`.
    Blocked,
    Off,
    Busy,
    On,
}

#[derive(Deserialize)]
struct PushConfigResp {
    enabled: bool,
    public_key: Option<String>,
    subscribed: bool,
}

#[derive(Serialize)]
struct SubscribeKeysReq {
    p256dh: String,
    auth: String,
}

#[derive(Serialize)]
struct SubscribeReq {
    endpoint: String,
    keys: SubscribeKeysReq,
}

#[derive(Serialize)]
struct UnsubscribeReq {
    endpoint: String,
}

/// `localStorage` key for the one-time proactive-prompt dismissal (#303
/// point 2). Set on EITHER prompt choice ("Zapnut" or "Teraz nie") so the
/// prompt never reappears once the user has made any decision.
const PUSH_PROMPT_DISMISSED_KEY: &str = "sb_push_prompt_dismissed";

/// `localStorage` key marking "the user explicitly turned notifications off
/// via the switch" (#303 point 1 vs. the switch's `On -> Off` direction —
/// see module doc). Cleared on a manual re-enable or a successful
/// auto-subscribe.
const PUSH_USER_DISABLED_KEY: &str = "sb_push_user_disabled";

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn push_prompt_dismissed() -> bool {
    local_storage()
        .and_then(|s| s.get_item(PUSH_PROMPT_DISMISSED_KEY).ok())
        .flatten()
        .is_some()
}

fn mark_push_prompt_dismissed() {
    if let Some(s) = local_storage() {
        let _ = s.set_item(PUSH_PROMPT_DISMISSED_KEY, "1");
    }
}

fn push_user_disabled() -> bool {
    local_storage()
        .and_then(|s| s.get_item(PUSH_USER_DISABLED_KEY).ok())
        .flatten()
        .is_some()
}

fn mark_push_user_disabled() {
    if let Some(s) = local_storage() {
        let _ = s.set_item(PUSH_USER_DISABLED_KEY, "1");
    }
}

fn clear_push_user_disabled() {
    if let Some(s) = local_storage() {
        let _ = s.remove_item(PUSH_USER_DISABLED_KEY);
    }
}

/// True when the three browser APIs this feature needs are all present.
/// Checked via `Reflect` (not a typed call) so an unsupported browser can
/// never trip a JS exception just from the feature check itself.
fn notifications_supported() -> bool {
    let Some(window) = window_value() else {
        return false;
    };
    let navigator = get_prop(&window, "navigator");
    !get_prop(&window, "Notification").is_undefined()
        && !get_prop(&window, "PushManager").is_undefined()
        && !get_prop(&navigator, "serviceWorker").is_undefined()
}

fn permission_denied() -> bool {
    notifications_supported()
        && web_sys::Notification::permission() == web_sys::NotificationPermission::Denied
}

enum SubscribeError {
    PermissionDenied,
    Other,
}

/// Request permission (if not already decided), await the SW registration,
/// call `PushManager.subscribe`, and read back `{endpoint, p256dh, auth}`.
/// Every JS boundary degrades to `Err(SubscribeError::Other)` rather than
/// panicking — mirrors `platform.rs`'s established convention. When
/// permission is already `granted` (the auto-subscribe path, #303), the
/// `request_permission()` branch below is simply skipped — no prompt is
/// ever shown for an already-decided permission.
async fn subscribe_flow(
    vapid_public_key_b64: &str,
) -> Result<(String, String, String), SubscribeError> {
    let mut permission = web_sys::Notification::permission();
    if permission == web_sys::NotificationPermission::Default {
        let promise =
            web_sys::Notification::request_permission().map_err(|_| SubscribeError::Other)?;
        let result = JsFuture::from(promise)
            .await
            .map_err(|_| SubscribeError::Other)?;
        permission = match result.as_string().as_deref() {
            Some("granted") => web_sys::NotificationPermission::Granted,
            Some("denied") => web_sys::NotificationPermission::Denied,
            _ => web_sys::NotificationPermission::Default,
        };
    }
    if permission == web_sys::NotificationPermission::Denied {
        return Err(SubscribeError::PermissionDenied);
    }
    if permission != web_sys::NotificationPermission::Granted {
        return Err(SubscribeError::Other);
    }

    let window = web_sys::window().ok_or(SubscribeError::Other)?;
    let sw_container = window.navigator().service_worker();
    let ready_promise = sw_container.ready().map_err(|_| SubscribeError::Other)?;
    let registration_val = JsFuture::from(ready_promise)
        .await
        .map_err(|_| SubscribeError::Other)?;
    let registration: web_sys::ServiceWorkerRegistration = registration_val
        .dyn_into()
        .map_err(|_| SubscribeError::Other)?;
    let push_manager = registration
        .push_manager()
        .map_err(|_| SubscribeError::Other)?;

    let key_bytes = {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(vapid_public_key_b64)
            .map_err(|_| SubscribeError::Other)?
    };
    let key_array = js_sys::Uint8Array::from(key_bytes.as_slice());
    let options = web_sys::PushSubscriptionOptionsInit::new();
    options.set_user_visible_only(true);
    options.set_application_server_key_opt_u8_array(Some(&key_array));

    let subscribe_promise = push_manager
        .subscribe_with_options(&options)
        .map_err(|_| SubscribeError::Other)?;
    let sub_val = JsFuture::from(subscribe_promise)
        .await
        .map_err(|_| SubscribeError::Other)?;

    // toJSON() (Reflect-called, see module doc) rather than a typed
    // PushSubscription binding.
    let to_json = get_prop(&sub_val, "toJSON");
    let json_val: JsValue = match to_json.dyn_ref::<Function>() {
        Some(f) => f.call0(&sub_val).unwrap_or_else(|_| sub_val.clone()),
        None => sub_val.clone(),
    };
    let endpoint = get_prop(&json_val, "endpoint")
        .as_string()
        .ok_or(SubscribeError::Other)?;
    let keys = get_prop(&json_val, "keys");
    let p256dh = get_prop(&keys, "p256dh")
        .as_string()
        .ok_or(SubscribeError::Other)?;
    let auth = get_prop(&keys, "auth")
        .as_string()
        .ok_or(SubscribeError::Other)?;

    Ok((endpoint, p256dh, auth))
}

/// Await the SW registration, read the browser's OWN existing subscription
/// (`PushManager.getSubscription()` — the only place the endpoint is known;
/// the server never sends key material back to the client), call
/// `subscription.unsubscribe()`, and return the endpoint so the caller can
/// tell the server to drop its matching row. `Ok(None)` means the browser
/// already had no subscription (nothing to unsubscribe from, nothing to
/// tell the server) — treated as a benign no-op by the caller, not an
/// error.
async fn unsubscribe_flow() -> Result<Option<String>, ()> {
    let window = web_sys::window().ok_or(())?;
    let sw_container = window.navigator().service_worker();
    let ready_promise = sw_container.ready().map_err(|_| ())?;
    let registration_val = JsFuture::from(ready_promise).await.map_err(|_| ())?;
    let registration: web_sys::ServiceWorkerRegistration =
        registration_val.dyn_into().map_err(|_| ())?;
    let push_manager = registration.push_manager().map_err(|_| ())?;

    let get_sub_promise = push_manager.get_subscription().map_err(|_| ())?;
    let sub_val = JsFuture::from(get_sub_promise).await.map_err(|_| ())?;
    if sub_val.is_null() || sub_val.is_undefined() {
        return Ok(None);
    }

    let endpoint = get_prop(&sub_val, "endpoint").as_string();

    let unsubscribe_fn = get_prop(&sub_val, "unsubscribe");
    if let Some(f) = unsubscribe_fn.dyn_ref::<Function>()
        && let Ok(result) = f.call0(&sub_val)
        && let Ok(promise) = result.dyn_into::<js_sys::Promise>()
        && let Err(e) = JsFuture::from(promise).await
    {
        // Non-fatal: the server-side row is still dropped below (the push
        // service will just have no subscription to deliver to). Logged so
        // a real browser-side unsubscribe failure isn't silently invisible.
        web_sys::console::warn_1(
            &format!("push: browser-side unsubscribe() rejected: {e:?}").into(),
        );
    }

    Ok(endpoint)
}

#[component]
pub fn PushToggle() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (state, set_state) = signal(PushState::Loading);
    let (public_key, set_public_key) = signal(None::<String>);
    let (error, set_error) = signal(false);
    let (show_prompt, set_show_prompt) = signal(false);

    // Shared "subscribe now" logic — used by the mount-effect auto-subscribe
    // path (#303 point 1), the settings switch's Off->On click, and the
    // one-time prompt banner's "Zapnut upozornenia" button. `signal()` pairs
    // are `Copy`, so this closure can be invoked from all three call sites.
    let do_subscribe = move || {
        let Some(key) = public_key.get_untracked() else {
            return;
        };
        set_error.set(false);
        set_state.set(PushState::Busy);
        set_show_prompt.set(false);
        mark_push_prompt_dismissed();
        spawn_local(async move {
            match subscribe_flow(&key).await {
                Ok((endpoint, p256dh, auth)) => {
                    let body = SubscribeReq {
                        endpoint,
                        keys: SubscribeKeysReq { p256dh, auth },
                    };
                    match api::post::<_, serde_json::Value>("/api/push/subscribe", &body).await {
                        Ok(_) => {
                            clear_push_user_disabled();
                            set_state.set(PushState::On);
                        }
                        Err(_) => {
                            set_error.set(true);
                            set_state.set(PushState::Off);
                        }
                    }
                }
                Err(SubscribeError::PermissionDenied) => set_state.set(PushState::Blocked),
                Err(SubscribeError::Other) => {
                    set_error.set(true);
                    set_state.set(PushState::Off);
                }
            }
        });
    };

    Effect::new(move |_| {
        spawn_local(async move {
            if !notifications_supported() {
                set_state.set(PushState::Unsupported);
                return;
            }
            match api::get::<PushConfigResp>("/api/push/config").await {
                Ok(cfg) if !cfg.enabled => set_state.set(PushState::Disabled),
                Ok(cfg) => {
                    set_public_key.set(cfg.public_key);
                    if permission_denied() {
                        set_state.set(PushState::Blocked);
                        return;
                    }
                    if cfg.subscribed {
                        set_state.set(PushState::On);
                        return;
                    }
                    let permission = web_sys::Notification::permission();
                    if permission == web_sys::NotificationPermission::Granted {
                        // Permission already decided — auto-subscribe with
                        // zero tap, UNLESS the user explicitly turned
                        // notifications off before (see module doc).
                        if push_user_disabled() {
                            set_state.set(PushState::Off);
                        } else {
                            do_subscribe();
                        }
                    } else {
                        set_state.set(PushState::Off);
                        if permission == web_sys::NotificationPermission::Default
                            && !push_prompt_dismissed()
                        {
                            set_show_prompt.set(true);
                        }
                    }
                }
                // Routine load failure (offline, session hiccup) — fail
                // closed to nothing rendered rather than a scary banner for
                // a non-essential affordance.
                Err(_) => set_state.set(PushState::Unsupported),
            }
        });
    });

    let on_dismiss_prompt = move |_| {
        set_show_prompt.set(false);
        mark_push_prompt_dismissed();
    };

    let on_toggle_click = move |_| match state.get_untracked() {
        PushState::Off => do_subscribe(),
        PushState::On => {
            set_error.set(false);
            set_state.set(PushState::Busy);
            spawn_local(async move {
                match unsubscribe_flow().await {
                    Ok(Some(endpoint)) => {
                        let body = UnsubscribeReq { endpoint };
                        match api::post::<_, serde_json::Value>("/api/push/unsubscribe", &body)
                            .await
                        {
                            Ok(_) => {
                                mark_push_user_disabled();
                                set_state.set(PushState::Off);
                            }
                            Err(_) => {
                                set_error.set(true);
                                set_state.set(PushState::On);
                            }
                        }
                    }
                    Ok(None) => {
                        // Browser already had no subscription — nothing to
                        // tell the server; reflect the honest (off) state.
                        mark_push_user_disabled();
                        set_state.set(PushState::Off);
                    }
                    Err(_) => {
                        set_error.set(true);
                        set_state.set(PushState::On);
                    }
                }
            });
        }
        PushState::Loading
        | PushState::Unsupported
        | PushState::Disabled
        | PushState::Blocked
        | PushState::Busy => {}
    };

    view! {
        {move || match state.get() {
            PushState::Loading | PushState::Unsupported | PushState::Disabled => ().into_any(),
            state => {
                let testid = match state {
                    PushState::On => "push-toggle-on",
                    PushState::Blocked => "push-toggle-blocked",
                    PushState::Busy => "push-toggle-busy",
                    _ => "push-toggle-off",
                };
                let checked = state == PushState::On;
                let disabled = matches!(state, PushState::Busy | PushState::Blocked);
                let busy = state == PushState::Busy;
                let switch_class = if busy {
                    "push-switch push-switch--busy"
                } else {
                    "push-switch"
                };
                view! {
                    <>
                        {move || show_prompt.get().then(|| view! {
                            <div class="push-prompt" data-testid="push-prompt">
                                <div class="push-prompt__text">{move || i18n::t(lang.get(), "push_prompt_body")}</div>
                                <div class="push-prompt__actions">
                                    <button
                                        class="btn btn--primary btn--compact"
                                        data-testid="push-prompt-enable"
                                        on:click=on_toggle_click
                                    >
                                        {move || i18n::t(lang.get(), "push_enable_button")}
                                    </button>
                                    <button
                                        class="btn btn--ghost btn--compact"
                                        data-testid="push-prompt-dismiss"
                                        on:click=on_dismiss_prompt
                                    >
                                        {move || i18n::t(lang.get(), "push_prompt_dismiss")}
                                    </button>
                                </div>
                            </div>
                        })}
                        <div class="card-push" data-testid=testid>
                            <div class="card-push__row">
                                <div class="card-push__text">
                                    <div class="card-push__label">{move || i18n::t(lang.get(), "push_settings_label")}</div>
                                    <div class="card-push__sublabel">
                                        {move || if state == PushState::Blocked {
                                            i18n::t(lang.get(), "push_blocked")
                                        } else {
                                            i18n::t(lang.get(), "push_settings_sublabel")
                                        }}
                                    </div>
                                    {(state == PushState::Blocked).then(|| view! {
                                        <div class="card-push__hint">{move || i18n::t(lang.get(), "push_blocked_hint")}</div>
                                    })}
                                </div>
                                <button
                                    class=switch_class
                                    role="switch"
                                    aria-checked=checked.to_string()
                                    data-testid="push-toggle-switch"
                                    disabled=disabled
                                    on:click=on_toggle_click
                                >
                                    <span class="push-switch__track">
                                        <span class="push-switch__knob"></span>
                                    </span>
                                </button>
                            </div>
                            {move || error.get().then(|| view! {
                                <div class="alert alert-error">{move || i18n::t(lang.get(), "push_error_generic")}</div>
                            })}
                        </div>
                    </>
                }
                .into_any()
            }
        }}
    }
}
