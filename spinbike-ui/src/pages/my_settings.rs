//! Customer-facing settings screen at `/my/settings` (#316) — introduced so
//! the push-notification toggle has somewhere to live once it's hidden from
//! `/my/balance` for the `On`/`Busy` states (nothing actionable there once
//! notifications are already on). Not `staff_gated` — a plain customer
//! route, same as `/my/bookings` and `/my/balance`.
//!
//! Deliberately minimal for now: just the full push toggle (every state).
//! The ticket that introduced this screen explicitly does NOT move any
//! other content here — see #316.

use leptos::prelude::*;

use crate::components::{PushToggle, PushToggleSurface};
use crate::i18n::{self, Lang};

#[component]
pub fn MySettingsPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "my_settings")}</h1>

        <PushToggle surface=PushToggleSurface::Settings />
    }
}
