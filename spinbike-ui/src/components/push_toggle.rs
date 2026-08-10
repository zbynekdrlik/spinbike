//! Web-Push notification settings row for `/my/balance` (#264, redesigned
//! #303, per-device state fixed #305).
//!
//! **#305 — the mount effect never trusts `GET /api/push/config`'s
//! `subscribed` flag for on/off state.** That flag is a per-ACCOUNT
//! aggregate (true the moment ANY device the customer ever used
//! subscribed), so a customer who subscribed on their desktop and then
//! opens `/my/balance` on a phone that never subscribed would see the
//! switch lie "On" while the phone silently receives nothing. The mount
//! effect instead calls `local_subscription_fields()`, which asks the
//! browser's OWN `PushManager.getSubscription()` — the only genuine
//! per-device truth — and drives state from that. When the browser reports
//! a live local subscription, the server row is reconciled with a plain
//! idempotent upsert POST (covers a prior subscribe POST that failed
//! silently) before showing On.
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
//! `permission == granted && no local subscription found (see the #305
//! paragraph above — never the server's account-wide flag) &&
//! !push_user_disabled()`. This is a purely client-side UI memory, not a
//! new server-side preference (the
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
use crate::auth;
use crate::i18n::{self, Lang};
use crate::platform::{get_prop, local_storage, window_value};

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

/// Where a `PushToggle` instance is mounted (#316) — decides which
/// [`PushState`]s render at all. See [`push_toggle_visible`] for the exact
/// per-surface rule; the subscribe/unsubscribe/self-heal state machine
/// below is completely unaffected by this — it only gates the final render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushToggleSurface {
    /// `/my/balance` — only the actionable states render (`Off`, so the
    /// customer can turn it on; `Blocked`, which carries the explanation
    /// of why it can't be turned on). `On`/`Busy` render nothing here —
    /// once notifications are already on (or a click is mid-flight) there
    /// is nothing left to do on the customer's main screen.
    MainBalance,
    /// `/my/settings` — the full row, every state, exactly like the
    /// original (pre-#316) `/my/balance` behavior. This is where the
    /// On -> Off switch click actually lives now.
    Settings,
}

#[derive(Deserialize)]
struct PushConfigResp {
    enabled: bool,
    public_key: Option<String>,
    // NOTE (#305): the server's `subscribed` field is deliberately NOT
    // deserialized here — it's a per-ACCOUNT aggregate (true if ANY device
    // the customer ever used subscribed), so it can never answer "is THIS
    // device subscribed" and must never drive on/off state. See
    // `local_subscription_fields` — the mount effect asks the browser's
    // own `PushManager.getSubscription()` instead. `serde` silently
    // ignores the extra JSON field (no `deny_unknown_fields`).
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

/// `localStorage` key BASE for the one-time proactive-prompt dismissal
/// (#303 point 2) — see [`storage_key`] for the per-account suffix. Set on
/// EITHER prompt choice ("Zapnut" or "Teraz nie") so the prompt never
/// reappears once the user has made any decision.
const PUSH_PROMPT_DISMISSED_KEY: &str = "sb_push_prompt_dismissed";

/// `localStorage` key BASE marking "the user explicitly turned
/// notifications off via the switch" (#303 point 1 vs. the switch's
/// `On -> Off` direction — see module doc) — see [`storage_key`] for the
/// per-account suffix. Cleared on a manual re-enable or a successful
/// auto-subscribe.
const PUSH_USER_DISABLED_KEY: &str = "sb_push_user_disabled";

/// Suffix a `localStorage` key BASE with the CURRENTLY LOGGED-IN user's own
/// id (#303 review finding). Both flags above are otherwise plain
/// origin-scoped keys with no account identity — on a device shared by
/// multiple customers (front-desk kiosk, a shared family device, two
/// accounts in the same browser), an unscoped flag written by one customer
/// would silently apply to the next customer who logs in on the same
/// browser: a dismissed prompt they never saw, or an auto-subscribe that
/// never fires because a PREVIOUS customer once turned it off. Falls back
/// to the bare base (old, unscoped behavior) only when no user is known —
/// this component never renders anything before the caller is
/// authenticated, so that fallback is defensive, not a real code path.
fn storage_key(base: &str) -> String {
    match auth::get_user() {
        Some(u) => format!("{base}_{}", u.id),
        None => base.to_string(),
    }
}

fn push_prompt_dismissed() -> bool {
    local_storage()
        .and_then(|s| s.get_item(&storage_key(PUSH_PROMPT_DISMISSED_KEY)).ok())
        .flatten()
        .is_some()
}

fn mark_push_prompt_dismissed() {
    if let Some(s) = local_storage() {
        let _ = s.set_item(&storage_key(PUSH_PROMPT_DISMISSED_KEY), "1");
    }
}

fn push_user_disabled() -> bool {
    local_storage()
        .and_then(|s| s.get_item(&storage_key(PUSH_USER_DISABLED_KEY)).ok())
        .flatten()
        .is_some()
}

fn mark_push_user_disabled() {
    if let Some(s) = local_storage() {
        let _ = s.set_item(&storage_key(PUSH_USER_DISABLED_KEY), "1");
    }
}

fn clear_push_user_disabled() {
    if let Some(s) = local_storage() {
        let _ = s.remove_item(&storage_key(PUSH_USER_DISABLED_KEY));
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

/// Await the SW registration and return its `PushManager` — the exact
/// acquisition sequence `subscribe_flow` and [`get_local_subscription`]
/// both need. Deep-review finding (this PR): the two used to duplicate
/// this verbatim; extracted here so a future fix to the sequence (e.g.
/// handling a rejected `ready()` promise differently) can't silently
/// drift between the two call sites. `Err(())` on any browser-API
/// failure — callers map to their own error type as needed.
async fn ready_push_manager() -> Result<web_sys::PushManager, ()> {
    let window = web_sys::window().ok_or(())?;
    let sw_container = window.navigator().service_worker();
    let ready_promise = sw_container.ready().map_err(|_| ())?;
    let registration_val = JsFuture::from(ready_promise).await.map_err(|_| ())?;
    let registration: web_sys::ServiceWorkerRegistration =
        registration_val.dyn_into().map_err(|_| ())?;
    registration.push_manager().map_err(|_| ())
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

    let push_manager = ready_push_manager()
        .await
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

    extract_subscription_fields(&sub_val).ok_or(SubscribeError::Other)
}

/// Read `{endpoint, keys: {p256dh, auth}}` off a raw `PushSubscription`-like
/// `JsValue` via its `toJSON()` method (Reflect-called, see module doc)
/// rather than a typed `web_sys::PushSubscription` binding — shared by
/// `subscribe_flow`'s own freshly-created subscription and
/// [`local_subscription_fields`]'s read of an EXISTING one.
fn extract_subscription_fields(sub_val: &JsValue) -> Option<(String, String, String)> {
    let to_json = get_prop(sub_val, "toJSON");
    let json_val: JsValue = match to_json.dyn_ref::<Function>() {
        Some(f) => f.call0(sub_val).unwrap_or_else(|_| sub_val.clone()),
        None => sub_val.clone(),
    };
    let endpoint = get_prop(&json_val, "endpoint").as_string()?;
    let keys = get_prop(&json_val, "keys");
    let p256dh = get_prop(&keys, "p256dh").as_string()?;
    let auth = get_prop(&keys, "auth").as_string()?;
    Some((endpoint, p256dh, auth))
}

/// Await the SW registration and read the browser's OWN existing
/// subscription via `PushManager.getSubscription()` — returns `Ok(None)`
/// when this device genuinely has none (not an error, the common case),
/// `Err(())` on a genuine browser-API failure. Shared by
/// [`unsubscribe_flow`] and [`local_subscription_fields`] (#305) — both
/// need the browser's own local truth, never the server's per-account
/// aggregate flag.
async fn get_local_subscription() -> Result<Option<JsValue>, ()> {
    let push_manager = ready_push_manager().await?;
    let get_sub_promise = push_manager.get_subscription().map_err(|_| ())?;
    let sub_val = JsFuture::from(get_sub_promise).await.map_err(|_| ())?;
    if sub_val.is_null() || sub_val.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(sub_val))
    }
}

/// Per spec, `navigator.serviceWorker.ready` never resolves at all if no
/// service worker ever successfully registers/activates for this origin
/// (a private-browsing storage restriction, or a failed `register()` call
/// that `index.html`'s own bootstrap silently swallows). Deep-review
/// finding (this PR): [`local_subscription_fields`] now runs
/// UNCONDITIONALLY on every `/my/balance` mount (#305) — before, this
/// exact await only ever ran on an explicit subscribe/unsubscribe click.
/// Without a bound, a device that never gets a working SW would hang
/// [`get_local_subscription`] forever on EVERY future visit, permanently
/// stranding the mount effect at `PushState::Loading` (the push card
/// silently renders nothing, forever, for that device). `LOCAL_SUBSCRIPTION_TIMEOUT_MS`
/// is generous (this is a background local-truth check, not a
/// user-blocking action) but finite — both callers below use this bounded
/// wrapper, never the raw unbounded fn directly.
const LOCAL_SUBSCRIPTION_TIMEOUT_MS: u32 = 5000;

async fn get_local_subscription_bounded() -> Result<Option<JsValue>, ()> {
    let work = Box::pin(get_local_subscription());
    let timeout = Box::pin(gloo_timers::future::TimeoutFuture::new(
        LOCAL_SUBSCRIPTION_TIMEOUT_MS,
    ));
    match futures::future::select(work, timeout).await {
        futures::future::Either::Left((result, _)) => result,
        futures::future::Either::Right(_) => {
            web_sys::console::warn_1(
                &"push: get_local_subscription timed out — service worker never became ready"
                    .into(),
            );
            Err(())
        }
    }
}

/// #305: read THIS device's own live push subscription directly from the
/// browser and extract its endpoint/keys — the only reliable per-DEVICE
/// truth. `GET /api/push/config`'s `subscribed` flag is a per-ACCOUNT
/// aggregate (true if ANY device the customer ever used subscribed), so it
/// can NEVER answer "is THIS device subscribed" — a customer who subscribed
/// on their desktop and then opens `/my/balance` on their phone (permission
/// already `granted` there too, e.g. from an earlier unrelated grant) would
/// see the switch lie "On" while the phone's own `PushManager` was never
/// actually subscribed and silently receives nothing. The mount effect
/// calls this INSTEAD of trusting `cfg.subscribed` for on/off state.
async fn local_subscription_fields() -> Result<Option<(String, String, String)>, ()> {
    match get_local_subscription_bounded().await? {
        Some(sub_val) => Ok(extract_subscription_fields(&sub_val)),
        None => Ok(None),
    }
}

/// Await the SW registration, read the browser's OWN existing subscription
/// (`PushManager.getSubscription()` — the only place the endpoint is known;
/// the server never sends key material back to the client), call
/// `subscription.unsubscribe()`, and return the endpoint so the caller can
/// tell the server to drop its matching row.
///
/// `Ok(None)` means the browser already had no subscription (nothing to
/// unsubscribe from, nothing to tell the server) — a benign no-op, not an
/// error. `Err(())` means a subscription EXISTED but its unsubscribe
/// could NOT be confirmed — the browser's own `.unsubscribe()` call
/// genuinely rejected, OR `unsubscribe` was missing/malformed (no
/// function, or didn't return a Promise) — in every such case the
/// subscription may still be live, so the caller must NOT treat the
/// device as off (#303 review finding, and this PR's own deep-review
/// finding for the two malformed-object cases: silently swallowing any
/// of these and still returning `Ok` would let the UI claim "off" for a
/// device that can still receive pushes).
async fn unsubscribe_flow() -> Result<Option<String>, ()> {
    let sub_val = match get_local_subscription_bounded().await? {
        Some(v) => v,
        None => return Ok(None),
    };

    let endpoint = get_prop(&sub_val, "endpoint").as_string();

    // Deep-review finding (this PR): both fallback branches below used to
    // just warn and FALL THROUGH to the `Ok(endpoint)` at the end — which
    // directly contradicts this function's own doc comment above ("must
    // NOT treat the device as off" when we can't confirm the browser
    // genuinely unsubscribed). A malformed `unsubscribe` (missing, or not
    // returning a Promise) means we truly cannot confirm success, so both
    // now return `Err(())` — same as the confirmed-rejected case above —
    // instead of silently claiming the device is off.
    let unsubscribe_fn = get_prop(&sub_val, "unsubscribe");
    match unsubscribe_fn.dyn_ref::<Function>() {
        Some(f) => {
            let promise = f
                .call0(&sub_val)
                .ok()
                .and_then(|r| r.dyn_into::<js_sys::Promise>().ok());
            match promise {
                Some(p) => {
                    if let Err(e) = JsFuture::from(p).await {
                        web_sys::console::warn_1(
                            &format!("push: browser-side unsubscribe() rejected: {e:?}").into(),
                        );
                        return Err(());
                    }
                }
                None => {
                    // unsubscribe() didn't return a promise at all (a
                    // malformed subscription object) — can't confirm
                    // success either way, so this is NOT a confirmed
                    // unsubscribe: fail closed rather than claim "off".
                    web_sys::console::warn_1(
                        &"push: unsubscribe() did not return a promise".into(),
                    );
                    return Err(());
                }
            }
        }
        None => {
            // Subscription object has no unsubscribe() method at all — a
            // malformed object; same "can't confirm, fail closed" logic.
            web_sys::console::warn_1(&"push: subscription has no unsubscribe() method".into());
            return Err(());
        }
    }

    Ok(endpoint)
}

/// Whether the toggle row should render at all, given WHERE it is mounted
/// and the current push state (#316). Pure decision function, extracted so
/// it's unit-testable without mounting the component into a DOM — see the
/// `visibility_tests` module below.
///
/// RED (this commit): still the OLD, surface-blind rule everywhere — `_surface`
/// is accepted but ignored. The `MainBalance`-only restriction lands in the
/// #316 GREEN commit; until then this matches today's shipped behavior
/// exactly (no user-visible regression from introducing the type/wiring
/// alone).
fn push_toggle_visible(_surface: PushToggleSurface, state: PushState) -> bool {
    !matches!(
        state,
        PushState::Loading | PushState::Unsupported | PushState::Disabled
    )
}

#[component]
pub fn PushToggle(surface: PushToggleSurface) -> impl IntoView {
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
        spawn_local(async move {
            match subscribe_flow(&key).await {
                Ok((endpoint, p256dh, auth)) => {
                    let body = SubscribeReq {
                        endpoint,
                        keys: SubscribeKeysReq { p256dh, auth },
                    };
                    match api::post::<_, serde_json::Value>("/api/push/subscribe", &body).await {
                        Ok(_) => {
                            mark_push_prompt_dismissed();
                            clear_push_user_disabled();
                            set_state.set(PushState::On);
                        }
                        Err(_) => {
                            // Transient failure (network/session hiccup) —
                            // do NOT mark the prompt dismissed: no real
                            // decision was reached, so it must still be
                            // available to retry on the next visit (#303
                            // review finding).
                            set_error.set(true);
                            set_state.set(PushState::Off);
                        }
                    }
                }
                Err(SubscribeError::PermissionDenied) => {
                    // A genuine decision WAS reached (the user just denied
                    // it) — the prompt must never come back for this.
                    mark_push_prompt_dismissed();
                    set_state.set(PushState::Blocked);
                }
                Err(SubscribeError::Other) => {
                    set_error.set(true);
                    set_state.set(PushState::Off);
                }
            }
        });
    };

    // Shared "unsubscribe now" logic — used by the settings switch's
    // On->Off click AND the mount-effect self-heal below (#303 review
    // finding: a device the user already marked off locally must reconcile
    // itself with the server, never silently flip back to a lying "on").
    // `report_errors` suppresses the error banner during the silent
    // self-heal path — a background reconciliation the user never
    // triggered shouldn't surface an alert on page load.
    let do_unsubscribe = move |report_errors: bool| {
        set_error.set(false);
        set_state.set(PushState::Busy);
        spawn_local(async move {
            match unsubscribe_flow().await {
                Ok(endpoint_opt) => {
                    // The browser side is now confirmed off (or already
                    // was) — reflect that immediately regardless of the
                    // server POST outcome below. A failed POST just means
                    // the NEXT load's self-heal retries the delete;
                    // reverting to "On" here would show a lying "on" for a
                    // device that can no longer receive anything (#303
                    // review finding).
                    mark_push_user_disabled();
                    set_state.set(PushState::Off);
                    if let Some(endpoint) = endpoint_opt {
                        let body = UnsubscribeReq { endpoint };
                        if api::post::<_, serde_json::Value>("/api/push/unsubscribe", &body)
                            .await
                            .is_err()
                            && report_errors
                        {
                            set_error.set(true);
                        }
                    }
                }
                Err(()) => {
                    // The browser subscription genuinely still exists (its
                    // own unsubscribe() call rejected) — must NOT claim off.
                    if report_errors {
                        set_error.set(true);
                    }
                    set_state.set(PushState::On);
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
                    // #305: never trust the server's per-ACCOUNT aggregate
                    // for on/off state — ask the browser's OWN PushManager
                    // what THIS device actually has. A customer who
                    // subscribed on their desktop and then opens
                    // /my/balance on a phone that never subscribed must
                    // NOT see a lying "On" here.
                    match local_subscription_fields().await {
                        Ok(Some((endpoint, p256dh, auth))) => {
                            if push_user_disabled() {
                                // Self-heal (#303 review finding, extended
                                // by #305): the LOCAL device already
                                // decided "off" but still holds a live
                                // browser subscription (e.g. a prior
                                // unsubscribe() call never completed) —
                                // reconcile silently rather than showing a
                                // lying "on".
                                do_unsubscribe(false);
                                return;
                            }
                            // This device genuinely has a live local
                            // subscription. Reconcile the server row
                            // (plain idempotent upsert) in case an earlier
                            // subscribe POST failed silently while the
                            // browser-side subscribe succeeded, then show
                            // On regardless of this POST's own outcome —
                            // the device demonstrably has a live
                            // subscription either way (review finding: a
                            // FAILING reconcile must not go completely
                            // unlogged just because it isn't shown in the
                            // UI, so it still gets a console warning here,
                            // same pattern as unsubscribe_flow's own
                            // best-effort warnings below).
                            clear_push_user_disabled();
                            let body = SubscribeReq {
                                endpoint,
                                keys: SubscribeKeysReq { p256dh, auth },
                            };
                            if let Err(e) =
                                api::post::<_, serde_json::Value>("/api/push/subscribe", &body)
                                    .await
                            {
                                web_sys::console::warn_1(
                                    &format!(
                                        "push: #305 reconcile of an existing local subscription failed: {e}"
                                    )
                                    .into(),
                                );
                            }
                            set_state.set(PushState::On);
                            return;
                        }
                        Ok(None) => {
                            // No local subscription on THIS device — fall
                            // through to the auto-subscribe / prompt logic
                            // below regardless of what the server's
                            // account-wide flag says (another device may
                            // well be subscribed; irrelevant here).
                        }
                        Err(()) => {
                            // Genuine browser-API failure reading local
                            // push state — fail closed to nothing rendered
                            // rather than a scary banner for a
                            // non-essential affordance (same philosophy as
                            // the outer Err(_) arm below).
                            set_state.set(PushState::Unsupported);
                            return;
                        }
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
        PushState::On => do_unsubscribe(true),
        PushState::Loading
        | PushState::Unsupported
        | PushState::Disabled
        | PushState::Blocked
        | PushState::Busy => {}
    };

    view! {
        {move || {
            let state_now = state.get();
            if !push_toggle_visible(surface, state_now) {
                return ().into_any();
            }
            let state = state_now;
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
                                    aria-label=move || i18n::t(lang.get(), "push_settings_label")
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
        }}
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    // #316: on the customer's main screen, the row must vanish once there's
    // nothing actionable left — RED today (2026-08-10): push_toggle_visible
    // still ignores `surface` entirely, so this asserts the future/correct
    // behavior and fails against the current stub body.
    #[wasm_bindgen_test]
    fn main_balance_hides_on() {
        assert!(
            !push_toggle_visible(PushToggleSurface::MainBalance, PushState::On),
            "On is not actionable on the main screen — the row must not render"
        );
    }

    #[wasm_bindgen_test]
    fn main_balance_hides_busy() {
        assert!(
            !push_toggle_visible(PushToggleSurface::MainBalance, PushState::Busy),
            "a mid-flight click isn't actionable either — the row must not render"
        );
    }

    #[wasm_bindgen_test]
    fn main_balance_shows_off() {
        assert!(
            push_toggle_visible(PushToggleSurface::MainBalance, PushState::Off),
            "Off is actionable (turn it on) — the row must render"
        );
    }

    #[wasm_bindgen_test]
    fn main_balance_shows_blocked() {
        assert!(
            push_toggle_visible(PushToggleSurface::MainBalance, PushState::Blocked),
            "Blocked carries the explanation of why it can't be turned on — must render"
        );
    }

    // Settings surface behavior is UNCHANGED from before #316 — every state
    // except the three that already render nothing everywhere.
    #[wasm_bindgen_test]
    fn settings_shows_every_actionable_state() {
        for state in [
            PushState::Off,
            PushState::Busy,
            PushState::On,
            PushState::Blocked,
        ] {
            assert!(
                push_toggle_visible(PushToggleSurface::Settings, state),
                "{state:?} must render on the settings surface"
            );
        }
    }

    #[wasm_bindgen_test]
    fn settings_hides_loading_unsupported_disabled() {
        for state in [
            PushState::Loading,
            PushState::Unsupported,
            PushState::Disabled,
        ] {
            assert!(
                !push_toggle_visible(PushToggleSurface::Settings, state),
                "{state:?} must render nothing on either surface"
            );
        }
    }
}
