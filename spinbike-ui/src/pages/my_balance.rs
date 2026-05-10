//! Customer self-service dashboard at `/my/balance` — credit, monthly-pass
//! status, hold-to-open door button (#92), recent visits.
//!
//! State machine for the door button (see Task 12 spec):
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
//! Progress is driven by a 16 ms `gloo_timers::future::TimeoutFuture` poll
//! loop — equivalent in cadence to `requestAnimationFrame` and avoids the
//! callback gymnastics of binding `Window::request_animation_frame` from
//! Leptos closures.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth::{clear_auth, get_token};
use crate::i18n::{self, Lang, fmt_date_short, tf};

#[derive(Debug, Clone, serde::Deserialize)]
struct BalanceResp {
    #[allow(dead_code)]
    user_id: i64,
    name: String,
    credit: f64,
    #[allow(dead_code)]
    card_code: Option<String>,
    allow_self_entry: bool,
    monthly_pass_active_until: Option<String>,
    recent: Vec<RecentTx>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RecentTx {
    #[allow(dead_code)]
    id: i64,
    created_at: String,
    action: String,
    amount: f64,
    #[allow(dead_code)]
    valid_until: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoorState {
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
            DoorState::ErrorRateLimited => "erroratelimited",
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

/// Hold duration before firing the press, in milliseconds.
const HOLD_DURATION_MS: f64 = 2000.0;
/// Animation tick (~60 Hz). RAF-equivalent without the JS callback dance.
const TICK_MS: u32 = 16;

#[component]
pub fn MyBalancePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<BalanceResp>);
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());

    // Door state machine.
    let (door_state, set_door_state) = signal(DoorState::Idle);
    // 0.0..=1.0 progress while Holding.
    let (hold_progress, set_hold_progress) = signal(0.0_f64);
    // Press generation counter — every pointerdown bumps it; the in-flight
    // hold loop checks the value to detect cancellation.
    let (press_gen, set_press_gen) = signal(0_u64);

    // Initial fetch.
    let load = move || {
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<BalanceResp>("/api/my/balance").await {
                Ok(d) => {
                    set_data.set(Some(d));
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };
    Effect::new(move |_| load());

    // ----- Door button event handlers -----

    let on_pointer_down = move |_ev: web_sys::PointerEvent| {
        // Reject if disabled-by-state or not allowed.
        let allowed =
            data.with_untracked(|d| d.as_ref().map(|d| d.allow_self_entry).unwrap_or(false));
        if !allowed {
            return;
        }
        if !matches!(door_state.get_untracked(), DoorState::Idle) {
            return;
        }

        let gen_id = press_gen.get_untracked().wrapping_add(1);
        set_press_gen.set(gen_id);
        set_door_state.set(DoorState::Holding);
        set_hold_progress.set(0.0);

        // Hold loop: tick every 16 ms, advancing progress until 2 s OR until
        // the user cancels (which bumps press_gen). On full hold, fire the
        // press request and transition to Firing → Success/Error.
        spawn_local(async move {
            let start = now_ms();
            loop {
                gloo_timers::future::TimeoutFuture::new(TICK_MS).await;
                if press_gen.get_untracked() != gen_id {
                    // Cancelled by pointerup / leave / cancel.
                    return;
                }
                let elapsed = now_ms() - start;
                let progress = (elapsed / HOLD_DURATION_MS).clamp(0.0, 1.0);
                set_hold_progress.set(progress);
                if elapsed >= HOLD_DURATION_MS {
                    break;
                }
            }

            // Final cancellation window — make sure we still own the press.
            if press_gen.get_untracked() != gen_id {
                return;
            }

            // Fire the press.
            set_door_state.set(DoorState::Firing);
            let next_state = match post_door_open().await {
                Ok(()) => DoorState::Success,
                Err(429) => DoorState::ErrorRateLimited,
                Err(_) => DoorState::ErrorUnavailable,
            };
            set_door_state.set(next_state);
            set_hold_progress.set(0.0);

            if matches!(next_state, DoorState::Success) {
                // Refresh balance — credit / pass / recent rows may have changed.
                spawn_local(async move {
                    if let Ok(d) = api::get::<BalanceResp>("/api/my/balance").await {
                        set_data.set(Some(d));
                    }
                });
            }

            // Auto-reset: success after 3 s, errors after 5 s. The timeout is
            // bound to gen_id so a new press during the banner clears cleanly.
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
    };

    let on_pointer_cancel = move |_ev: web_sys::PointerEvent| {
        if matches!(door_state.get_untracked(), DoorState::Holding) {
            // Bump generation to invalidate the in-flight hold loop, return
            // to Idle. Firing/Success/Error states are NOT cancellable here.
            set_press_gen.update(|g| *g = g.wrapping_add(1));
            set_door_state.set(DoorState::Idle);
            set_hold_progress.set(0.0);
        }
    };

    view! {
        <h1 class="page-title">
            {move || data.with(|d| d.as_ref().map(|d| tf(lang.get(), "my_balance_hello", &[&d.name]))
                .unwrap_or_else(|| i18n::t(lang.get(), "my_balance").to_string()))}
        </h1>

        {move || {
            let e = error.get();
            if !e.is_empty() {
                return view! { <div class="alert alert-error">{e}</div> }.into_any();
            }
            if loading.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }

            data.with(|d| match d {
                None => view! { <div class="empty-state">{i18n::t(lang.get(), "unable_to_load")}</div> }.into_any(),
                Some(b) => {
                    let credit_val = format!("{:.2}", b.credit);
                    let pass_label = match &b.monthly_pass_active_until {
                        Some(ts) => match parse_pass_date(ts) {
                            Some(d) => tf(lang.get(), "monthly_pass_active_until", &[&fmt_date_short(d, lang.get())]),
                            None => tf(lang.get(), "monthly_pass_active_until", &[ts]),
                        },
                        None => i18n::t(lang.get(), "monthly_pass_not_active").to_string(),
                    };
                    let allow = b.allow_self_entry;
                    let recent_rows = b.recent.clone();
                    let lang_now = lang.get();

                    view! {
                        // Credit card
                        <div class="card-credit" data-testid="my-balance-credit">
                            <div class="card-credit__label">{i18n::t(lang_now, "my_balance_credit")}</div>
                            <div class="card-credit__value">"\u{20ac} "{credit_val}</div>
                        </div>

                        // Monthly pass card. The full sentence (e.g. "Monthly
                        // pass active until 2026-05-31" or "Monthly pass not
                        // active") is the value — there is no separate header.
                        <div class="card-pass" data-testid="my-balance-pass">
                            <div class="card-pass__label">{i18n::t(lang_now, "service_kind_monthly_pass")}</div>
                            <div class="card-pass__value">{pass_label}</div>
                        </div>

                        // Door button + banner — only if the customer is allowed.
                        // The closures capture only `Copy` signals so they are
                        // themselves `Copy` and can be reused across the three
                        // cancel-event handlers.
                        {if allow {
                            view! {
                                <button
                                    class=move || format!("door-btn door-btn--{}", door_state.get().css_suffix())
                                    data-testid="door-open-button"
                                    aria-label=move || i18n::t(lang.get(), DoorState::Idle.label_key())
                                    disabled=move || matches!(door_state.get(),
                                        DoorState::Firing | DoorState::Success
                                            | DoorState::ErrorUnavailable | DoorState::ErrorRateLimited)
                                    on:pointerdown=on_pointer_down
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
                        } else {
                            view! {
                                <button
                                    class="door-btn door-btn--notallowed"
                                    data-testid="door-open-button"
                                    disabled=true
                                    title=i18n::t(lang_now, "door_not_allowed")
                                >
                                    <span class="door-btn__label">{i18n::t(lang_now, "door_not_allowed")}</span>
                                </button>
                            }.into_any()
                        }}

                        // Recent visits
                        <h2 class="recent-visits__heading">{i18n::t(lang_now, "my_balance_recent_visits")}</h2>
                        <ul class="recent-visits">
                            {recent_rows.into_iter().map(|t| {
                                let date_label = parse_visit_date(&t.created_at)
                                    .map(|d| fmt_date_short(d, lang_now))
                                    .unwrap_or_else(|| t.created_at.clone());
                                let amount_label = if t.amount.abs() < 0.005 {
                                    String::new()
                                } else {
                                    format!("\u{20ac}{:.2}", t.amount)
                                };
                                let note_view = match &t.note {
                                    Some(n) if !n.is_empty() => {
                                        let n = n.clone();
                                        view! { <span class="recent-visits__note">{n}</span> }.into_any()
                                    }
                                    _ => view! {}.into_any(),
                                };
                                view! {
                                    <li data-testid="recent-visit" class="recent-visits__row">
                                        <span class="recent-visits__date">{date_label}</span>
                                        <span class="recent-visits__action">{t.action.clone()}</span>
                                        <span class="recent-visits__amount">{amount_label}</span>
                                        {note_view}
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any()
                }
            })
        }}
    }
}

/// Wall-clock time in milliseconds. We use `Date.now()` rather than
/// `performance.now()` because the latter requires a `Performance` web-sys
/// feature flag that isn't enabled in this crate's feature set, and the
/// absolute monotonicity tradeoff is irrelevant for a 2-second hold gesture
/// on the user's own device.
fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// Parse the SQLite UTC timestamp `YYYY-MM-DD HH:MM:SS` into a NaiveDate
/// (date portion). For pass display we only show the day, not the time.
fn parse_pass_date(s: &str) -> Option<chrono::NaiveDate> {
    let trimmed = s.trim();
    let date_str = trimmed.split_whitespace().next().unwrap_or(trimmed);
    let date_str = date_str.split('T').next().unwrap_or(date_str);
    chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

/// Parse the SQLite-formatted `created_at` of a transaction. Same format as
/// `parse_pass_date`, kept separate so future divergence is contained.
fn parse_visit_date(s: &str) -> Option<chrono::NaiveDate> {
    parse_pass_date(s)
}

/// Inline POST /api/door/open that surfaces the HTTP status as `Err(status)`.
/// `api::post` collapses status into a string; we need to distinguish 429
/// (rate-limited) from 503 (hardware-unavailable) for the UI banners.
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
            // Network error → treat as hardware unavailable for UX.
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
