//! Dedicated door-open page at `/door`. Shows ONLY the hold-2s button —
//! no balance / pass / recent-visits clutter. Used by admin and staff who
//! reach it via the AdaptiveNav "More" sheet, and by anyone who navigates
//! to /door directly.
//!
//! Reads /api/my/balance to determine the caller's `allow_self_entry`
//! flag; everything else from that response is ignored. The actual
//! press logic lives in `components::DoorButton`.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::DoorButton;
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
struct AllowResp {
    allow_self_entry: bool,
}

#[component]
pub fn DoorPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (allowed, set_allowed) = signal(false);
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());

    let load = move || {
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<AllowResp>("/api/my/balance").await {
                Ok(d) => {
                    set_allowed.set(d.allow_self_entry);
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };
    Effect::new(move |_| load());

    let allowed_signal: Signal<bool> = allowed.into();

    view! {
        <div class="door-page" data-testid="door-page">
            <h1 class="page-title">{move || i18n::t(lang.get(), "door_button_idle")}</h1>

            {move || {
                let e = error.get();
                if !e.is_empty() {
                    return view! { <div class="alert alert-error">{e}</div> }.into_any();
                }
                if loading.get() {
                    return view! {
                        <div class="text-center mt-3"><span class="spinner"></span></div>
                    }.into_any();
                }
                view! {
                    <DoorButton
                        allowed=allowed_signal
                        on_success=|| {}
                    />
                }.into_any()
            }}
        </div>
    }
}
