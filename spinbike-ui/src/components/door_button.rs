//! Reusable door-open button — extracted from `/my/balance` so the
//! admin/staff `/door` page can render only the button without the
//! customer credit/pass/visits context.
//!
//! State machine (see #92 spec):
//!
//! | State              | Triggered by                           |
//! |--------------------|----------------------------------------|
//! | `Idle`             | default                                |
//! | `Holding`          | pointerdown                            |
//! | `Firing`           | hold reached 2 s, request in flight    |
//! | `Success`          | 200 response                           |
//! | `ErrorUnavailable` | 503 (or any unexpected error)          |
//! | `ErrorRateLimited` | 429                                    |
//! | `NotAllowed`       | `allow_self_entry == false`            |
//!
//! Pointer events ONLY (NOT mouse/touch). Cancellation via pointerup /
//! pointerleave / pointercancel resets to `Idle` without firing the press.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::auth::{clear_auth, get_token};
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorState {
    Idle,
    Holding,
    Firing,
    Success,
    ErrorUnavailable,
    ErrorRateLimited,
    NotAllowed,
}

impl DoorState {
    fn css_suffix(self) -> &'static str {
        match self {
            DoorState::Idle => "idle",
            DoorState::Holding => "holding",
            DoorState::Firing => "firing",
            DoorState::Success => "success",
            DoorState::ErrorUnavailable => "errorunavailable",
            DoorState::ErrorRateLimited => "errorratelimited",
            DoorState::NotAllowed => "notallowed",
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            DoorState::Idle => "door_button_idle",
            DoorState::Holding => "door_button_holding",
            DoorState::Firing => "door_button_firing",
            DoorState::Success => "door_success",
            DoorState::ErrorUnavailable => "door_unavailable",
            DoorState::ErrorRateLimited => "door_rate_limited",
            DoorState::NotAllowed => "door_not_allowed",
        }
    }
}

const HOLD_DURATION_MS: f64 = 2000.0;
const TICK_MS: u32 = 16;

/// Hold-2s door button + status banner. Caller provides:
/// - `allowed` — derived from the user's `allow_self_entry` flag. When false,
///   button renders disabled with the "Ask reception" label.
/// - `on_success` — called after a successful press so the parent can refresh
///   any related data (e.g. /my/balance recent visits).
#[component]
pub fn DoorButton<F>(
    /// Reactive: true when the current user has `allow_self_entry = 1`.
    allowed: Signal<bool>,
    /// Called when the press succeeded. Parent uses this to refresh balance /
    /// recent-visits / wherever the visit row should show up.
    on_success: F,
) -> impl IntoView
where
    F: Fn() + Send + Sync + 'static,
{
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (door_state, set_door_state) = signal(DoorState::Idle);
    let (hold_progress, set_hold_progress) = signal(0.0_f64);
    let (press_gen, set_press_gen) = signal(0_u64);
    let on_success = std::sync::Arc::new(on_success);

    let on_pointer_down = {
        let on_success = on_success.clone();
        move |_ev: web_sys::PointerEvent| {
            if !allowed.get_untracked() {
                return;
            }
            if !matches!(door_state.get_untracked(), DoorState::Idle) {
                return;
            }

            let gen_id = press_gen.get_untracked().wrapping_add(1);
            set_press_gen.set(gen_id);
            set_door_state.set(DoorState::Holding);
            set_hold_progress.set(0.0);

            let on_success = on_success.clone();
            spawn_local(async move {
                let start = now_ms();
                loop {
                    gloo_timers::future::TimeoutFuture::new(TICK_MS).await;
                    if press_gen.get_untracked() != gen_id {
                        return;
                    }
                    let elapsed = now_ms() - start;
                    let progress = (elapsed / HOLD_DURATION_MS).clamp(0.0, 1.0);
                    set_hold_progress.set(progress);
                    if elapsed >= HOLD_DURATION_MS {
                        break;
                    }
                }
                if press_gen.get_untracked() != gen_id {
                    return;
                }

                set_door_state.set(DoorState::Firing);
                let next_state = match post_door_open().await {
                    Ok(()) => DoorState::Success,
                    Err(429) => DoorState::ErrorRateLimited,
                    Err(_) => DoorState::ErrorUnavailable,
                };
                set_door_state.set(next_state);
                set_hold_progress.set(0.0);

                if matches!(next_state, DoorState::Success) {
                    on_success();
                }

                let reset_ms = if matches!(next_state, DoorState::Success) {
                    3000
                } else {
                    5000
                };
                spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(reset_ms).await;
                    if press_gen.get_untracked() == gen_id {
                        set_door_state.set(DoorState::Idle);
                    }
                });
            });
        }
    };

    let on_pointer_cancel = move |_ev: web_sys::PointerEvent| {
        if matches!(door_state.get_untracked(), DoorState::Holding) {
            set_press_gen.update(|g| *g = g.wrapping_add(1));
            set_door_state.set(DoorState::Idle);
            set_hold_progress.set(0.0);
        }
    };

    view! {
        {move || {
            if !allowed.get() {
                let lang_now = lang.get();
                return view! {
                    <button
                        class="door-btn door-btn--notallowed"
                        data-testid="door-open-button"
                        disabled=true
                        title=i18n::t(lang_now, "door_not_allowed")
                    >
                        <span class="door-btn__label">{i18n::t(lang_now, "door_not_allowed")}</span>
                    </button>
                }.into_any();
            }
            view! {
                <button
                    class=move || format!("door-btn door-btn--{}", door_state.get().css_suffix())
                    data-testid="door-open-button"
                    aria-label=move || i18n::t(lang.get(), DoorState::Idle.label_key())
                    disabled=move || matches!(door_state.get(),
                        DoorState::Firing | DoorState::Success
                            | DoorState::ErrorUnavailable | DoorState::ErrorRateLimited)
                    on:pointerdown=on_pointer_down.clone()
                    on:pointerup=on_pointer_cancel
                    on:pointerleave=on_pointer_cancel
                    on:pointercancel=on_pointer_cancel
                >
                    <span class="door-btn__progress" style:width=move || format!("{}%", (hold_progress.get() * 100.0).clamp(0.0, 100.0))></span>
                    <span class="door-btn__icon" role="img" aria-label=move || i18n::t(lang.get(), "door_lock_icon_aria")>"\u{1F513}"</span>
                    <span class="door-btn__label">
                        {move || i18n::t(lang.get(), door_state.get().label_key())}
                    </span>
                </button>

                {move || {
                    let state = door_state.get();
                    let kind = match state {
                        DoorState::Success => "success",
                        DoorState::ErrorUnavailable => "error",
                        DoorState::ErrorRateLimited => "warn",
                        _ => "",
                    };
                    if kind.is_empty() {
                        view! {}.into_any()
                    } else {
                        view! {
                            <div data-testid="door-banner" class=format!("banner banner--{}", kind)>
                                {i18n::t(lang.get(), state.label_key())}
                            </div>
                        }.into_any()
                    }
                }}
            }.into_any()
        }}
    }
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// Inline POST /api/door/open that surfaces the HTTP status as `Err(status)`.
async fn post_door_open() -> Result<(), u16> {
    use gloo_net::http::{Method, RequestBuilder};
    let mut req = RequestBuilder::new("/api/door/open").method(Method::POST);
    if let Some(t) = get_token() {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            web_sys::console::warn_1(&format!("door open: network error: {e}").into());
            return Err(503);
        }
    };
    let status = resp.status();
    if status == 401 && get_token().is_some() {
        clear_auth();
        if let Some(win) = web_sys::window() {
            let _ = win.location().set_href("/login");
        }
        return Err(401);
    }
    if !resp.ok() {
        web_sys::console::warn_1(&format!("door open: HTTP {status}").into());
        return Err(status);
    }
    Ok(())
}
