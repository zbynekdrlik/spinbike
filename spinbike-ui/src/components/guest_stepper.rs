//! The "- / count / +" guest stepper (#389) — a host brings guests to a
//! spinning class and pays for them from their own credit. Shared by the
//! customer's class card on `/schedule` and the staff table on
//! `/staff/spinning`; the server decides every rule (deadline, capacity,
//! charge/refund) — the stepper only asks for `guests ± 1`.

use leptos::prelude::*;

use crate::api;
use crate::i18n::{self, Lang};

/// `PUT /api/bookings/{booking_id}/guests` — set the guest count. The error
/// carries the server's `error_code` so the caller can localize it.
pub async fn put_guests(booking_id: i64, guests: i64) -> Result<(), api::ApiError> {
    #[derive(serde::Serialize)]
    struct Req {
        guests: i64,
    }
    #[derive(serde::Deserialize)]
    struct Resp {
        #[allow(dead_code)]
        guests: i64,
    }
    api::put_json::<Req, Resp>(
        &format!("/api/bookings/{booking_id}/guests"),
        &Req { guests },
    )
    .await
    .map(|_| ())
}

/// Test ids: `{prefix}-{item_key}` (wrapper), `{prefix}-minus-{item_key}`,
/// `{prefix}-count-{item_key}`, `{prefix}-plus-{item_key}`.
#[component]
pub fn GuestStepper(
    /// `"guests"` (customer card) | `"spin-guests"` (staff table).
    prefix: &'static str,
    /// `"{template_id}-{date}"` (customer) | `"{booking_id}"` (staff).
    item_key: String,
    /// The current guest count.
    guests: i64,
    /// What the count shows ("Guests: 2" / "2").
    #[prop(into)]
    count_text: Signal<String>,
    /// The "+" button's visible label ("+ guest" / "+").
    #[prop(into)]
    plus_text: Signal<String>,
    /// A write is in flight — both buttons wait.
    #[prop(into)]
    busy: Signal<bool>,
    /// "-" is not allowed besides `guests == 0` (e.g. a cancelled class).
    minus_blocked: bool,
    /// "+" is not allowed (max guests, class full, cancelled class).
    plus_blocked: bool,
    /// Asked with the NEW count (`guests - 1` / `guests + 1`).
    #[prop(into)]
    on_set: Callback<i64>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    view! {
        <div class="guest-stepper" data-testid=format!("{prefix}-{item_key}")>
            <button
                type="button"
                class="btn btn--outline btn--compact"
                data-testid=format!("{prefix}-minus-{item_key}")
                aria-label=move || i18n::t(lang.get(), "guest_remove")
                disabled=move || busy.get() || minus_blocked || guests == 0
                on:click=move |_| on_set.run(guests - 1)
            >
                "−"
            </button>
            <span class="guest-stepper__count" data-testid=format!("{prefix}-count-{item_key}")>
                {move || count_text.get()}
            </span>
            <button
                type="button"
                class="btn btn--outline btn--compact"
                data-testid=format!("{prefix}-plus-{item_key}")
                aria-label=move || i18n::t(lang.get(), "guest_add")
                disabled=move || busy.get() || plus_blocked
                on:click=move |_| on_set.run(guests + 1)
            >
                {move || plus_text.get()}
            </button>
        </div>
    }
}
