use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth;
use crate::components::guest_stepper::{GuestStepper, put_guests};
use crate::i18n::{self, Lang};
use crate::pages::schedule::ClassSlot;
use spinbike_core::booking::MAX_GUESTS;

#[component]
pub fn ClassCard(slot: ClassSlot, #[prop(into)] on_change: Callback<()>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let is_logged_in = auth::get_token().is_some();

    let state = if slot.cancelled {
        "cancelled"
    } else if slot.user_booked {
        "booked"
    } else if slot.started {
        "past"
    } else if slot.booked >= slot.capacity {
        "full"
    } else {
        "available"
    };

    let (loading, set_loading) = signal(false);
    let (error, set_error) = signal(String::new());

    let template_id = slot.template_id;
    let date = slot.date.clone();
    let booking_id = slot.user_booking_id;
    let on_change_book = on_change;
    let date_book = date.clone();

    let time_str = slot.start_time.clone();
    // #381: show the instructor's NAME (was the raw id: "Instruktor #2").
    let instructor_name = slot.instructor_name.clone();
    let booked = slot.booked;
    let capacity = slot.capacity;

    let on_book = move |_| {
        let date = date_book.clone();
        let on_change = on_change_book;
        set_loading.set(true);
        set_error.set(String::new());
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                template_id: i64,
                date: String,
            }
            #[derive(serde::Deserialize)]
            struct Resp {
                #[allow(dead_code)]
                id: i64,
            }
            // post_json: the coded error localizes a 409 `already_booked`
            // (double-tap race on a self-booking, #378).
            match api::post_json::<Req, Resp>("/api/bookings", &Req { template_id, date }).await {
                Ok(_) => on_change.run(()),
                Err(e) => set_error.set(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                )),
            }
            set_loading.set(false);
        });
    };

    let on_cancel = move |_| {
        if let Some(bid) = booking_id {
            set_loading.set(true);
            set_error.set(String::new());
            spawn_local(async move {
                // delete_coded: a refused cancel (#382 R2 — the 09:00
                // deadline passed while the page was open) shows the
                // localized reason instead of raw server text.
                match api::delete_coded(&format!("/api/bookings/{bid}")).await {
                    Ok(_) => on_change.run(()),
                    Err(e) => set_error.set(i18n::localize_api_error(
                        lang.get_untracked(),
                        e.code,
                        &e.message,
                    )),
                }
                set_loading.set(false);
            });
        }
    };

    // #389: the host's guest count, changed with the "+ host" stepper while
    // the booking is still cancellable (before 09:00 on the class day). Each
    // guest takes a seat and is charged from the host's credit at 09:00.
    let guests = slot.user_booking_guests;
    let set_guests_to = move |n: i64| {
        let Some(bid) = booking_id else {
            return;
        };
        set_loading.set(true);
        set_error.set(String::new());
        spawn_local(async move {
            // A refused change (deadline passed, class full) shows its
            // localized reason — never swallowed.
            match put_guests(bid, n).await {
                Ok(_) => on_change.run(()),
                Err(e) => set_error.set(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                )),
            }
            set_loading.set(false);
        });
    };

    let slot_cancelled = slot.cancelled;
    let slot_user_booked = slot.user_booked;
    let slot_started = slot.started;
    let slot_cancel_open = slot.cancel_open;
    let slot_full = slot.booked >= slot.capacity;
    let slot_booking_source = slot.user_booking_source.clone();

    let cancel_testid = format!("cancel-{template_id}-{}", slot.date);
    let book_testid = format!("book-{template_id}-{}", slot.date);
    let binding_testid = format!("binding-{template_id}-{}", slot.date);
    let started_testid = format!("started-{template_id}-{}", slot.date);
    let guests_key = format!("{template_id}-{}", slot.date);
    let guests_count_testid = format!("guests-count-{guests_key}");

    // Stepper while the customer may still change it; afterwards (binding)
    // just the count, when there are guests.
    let guests_view = if !is_logged_in || !slot_user_booked || slot_cancelled {
        None
    } else if slot_cancel_open {
        let plus_blocked = guests >= MAX_GUESTS || slot_full;
        Some(
            view! {
                <GuestStepper
                    prefix="guests"
                    item_key=guests_key
                    guests=guests
                    count_text=Signal::derive(move || {
                        i18n::tf(lang.get(), "guests_count_format", &[&guests.to_string()])
                    })
                    plus_text=Signal::derive(move || i18n::t(lang.get(), "guest_add").to_string())
                    busy=loading
                    minus_blocked=false
                    plus_blocked=plus_blocked
                    on_set=move |n: i64| set_guests_to(n)
                />
            }
            .into_any(),
        )
    } else if guests > 0 {
        Some(
            view! {
                <div class="list-row__sub" data-testid=guests_count_testid>
                    {move || i18n::tf(lang.get(), "guests_count_format", &[&guests.to_string()])}
                </div>
            }
            .into_any(),
        )
    } else {
        None
    };

    let action_view = if slot_cancelled {
        view! { <span class="badge badge--cancelled">{move || i18n::t(lang.get(), "cancelled")}</span> }.into_any()
    } else if slot_user_booked && !slot_cancel_open {
        // #382 R2: past 09:00 on the class day the booking is binding — the
        // customer can no longer cancel it (staff still can, with a refund).
        view! {
            <span class="badge" data-testid=binding_testid>{move || i18n::t(lang.get(), "spinning_binding")}</span>
        }.into_any()
    } else if slot_started && !slot_user_booked {
        // #382 R4: an occurrence that already started cannot be booked.
        view! {
            <span class="badge badge--cancelled" data-testid=started_testid>{move || i18n::t(lang.get(), "class_started")}</span>
        }.into_any()
    } else if !is_logged_in {
        view! { <a href="/login" class="btn btn--ghost">{move || i18n::t(lang.get(), "login_to_book")}</a> }.into_any()
    } else if slot_user_booked && slot_booking_source.as_deref() == Some("persistent") {
        view! {
            <button class="btn btn--danger btn--compact" data-testid=cancel_testid on:click=on_cancel disabled=move || loading.get()>
                {move || if loading.get() {
                    "...".to_string()
                } else {
                    let auto = i18n::t(lang.get(), "auto");
                    let skip = i18n::t(lang.get(), "skip_this_week");
                    format!("{auto} — {skip}")
                }}
            </button>
        }.into_any()
    } else if slot_user_booked {
        view! {
            <button class="btn btn--danger btn--compact" data-testid=cancel_testid on:click=on_cancel disabled=move || loading.get()>
                {move || if loading.get() { "..." } else { i18n::t(lang.get(), "cancel") }}
            </button>
        }.into_any()
    } else if slot_full {
        view! { <button class="btn btn--ghost" disabled=true>{move || i18n::t(lang.get(), "full")}</button> }
            .into_any()
    } else {
        view! {
            <button class="btn btn--primary" data-testid=book_testid on:click=on_book disabled=move || loading.get()>
                {move || if loading.get() { "..." } else { i18n::t(lang.get(), "book") }}
            </button>
        }
        .into_any()
    };

    view! {
        <div class=format!("list-row list-row--{state}")>
            <span class=format!("list-row__accent list-row__accent--{state}")></span>
            <div class="list-row__main">
                <div class="list-row__title">{time_str}</div>
                {instructor_name.map(|name| view! {
                    <div class="list-row__sub" data-testid="class-instructor">
                        {move || i18n::tf(lang.get(), "instructor_format", &[&name])}
                    </div>
                })}
                <div class="list-row__sub">{move || i18n::tf(lang.get(), "spots_format", &[&booked.to_string(), &capacity.to_string()])}</div>
                {guests_view}
                {move || {
                    let e = error.get();
                    if e.is_empty() {
                        view! { <span></span> }.into_any()
                    } else {
                        view! { <div class="alert alert--error" style="margin-top:4px;padding:4px 8px;font-size:0.75rem">{e}</div> }.into_any()
                    }
                }}
            </div>
            <div class="list-row__end">
                {action_view}
            </div>
        </div>
    }
}
