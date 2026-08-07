//! "Enable notifications" affordance for `/my/balance` (#264).
//!
//! Deliberately NEVER auto-prompts on load — browsers penalise that and
//! users reflexively deny (per the issue). One explicit button; permission
//! is requested only on click.
//!
//! **Reading the subscription result uses `js_sys::Reflect` and `toJSON()`,
//! not a typed `web_sys::PushSubscription` binding.** `PushSubscription`
//! has no live `keys` PROPERTY, only a `toJSON()` METHOD that computes
//! `{endpoint, keys: {p256dh, auth}}` from the raw key material. Calling
//! that method dynamically, rather than `.dyn_into::<PushSubscription>()`
//! and typed getters, works identically whether the resolved value is a
//! real browser `PushSubscription` or a plain object shaped the same way.
//! That is what makes the E2E test for this button tractable: it stubs
//! `PushManager.prototype.subscribe` (the ONE hop that would otherwise
//! need a live round-trip to the browser's real push service) with a
//! plain JS object exposing `endpoint` and `toJSON()`, and this code
//! neither knows nor cares that it isn't the native class instance.
//! `ServiceWorkerRegistration`, `PushManager`, and `Notification` stay on
//! typed `web_sys` bindings throughout; only the final subscription-object
//! read is untyped.

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
/// panicking — mirrors `platform.rs`'s established convention.
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

#[component]
pub fn PushToggle() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (state, set_state) = signal(PushState::Loading);
    let (public_key, set_public_key) = signal(None::<String>);
    let (error, set_error) = signal(false);

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
                    } else if cfg.subscribed {
                        set_state.set(PushState::On);
                    } else {
                        set_state.set(PushState::Off);
                    }
                }
                // Routine load failure (offline, session hiccup) — fail
                // closed to nothing rendered rather than a scary banner for
                // a non-essential affordance.
                Err(_) => set_state.set(PushState::Unsupported),
            }
        });
    });

    let on_enable_click = move |_| {
        let Some(key) = public_key.get_untracked() else {
            return;
        };
        set_error.set(false);
        set_state.set(PushState::Busy);
        spawn_local(async move {
            match subscribe_flow(&key).await {
                Ok((endpoint, p256dh, auth)) => {
                    let body = SubscribeReq {
                        endpoint,
                        keys: SubscribeKeysReq { p256dh, auth },
                    };
                    match api::post::<_, serde_json::Value>("/api/push/subscribe", &body).await {
                        Ok(_) => set_state.set(PushState::On),
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

    view! {
        {move || match state.get() {
            PushState::Loading | PushState::Unsupported | PushState::Disabled => ().into_any(),
            PushState::Blocked => view! {
                <div class="push-toggle push-toggle--blocked" data-testid="push-toggle-blocked">
                    {move || i18n::t(lang.get(), "push_blocked")}
                </div>
            }.into_any(),
            PushState::On => view! {
                <div class="push-toggle push-toggle--on" data-testid="push-toggle-on">
                    {move || i18n::t(lang.get(), "push_on")}
                </div>
            }.into_any(),
            PushState::Off | PushState::Busy => view! {
                <div class="push-toggle" data-testid="push-toggle-off">
                    <button
                        class="btn btn--primary"
                        data-testid="push-enable-button"
                        disabled=move || state.get() == PushState::Busy
                        on:click=on_enable_click
                    >
                        {move || i18n::t(lang.get(), "push_enable_button")}
                    </button>
                    {move || error.get().then(|| view! {
                        <div class="alert alert-error">{i18n::t(lang.get(), "push_error_generic")}</div>
                    })}
                </div>
            }.into_any(),
        }}
    }
}
