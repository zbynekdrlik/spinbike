use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth;
use crate::i18n::{self, Lang};
use crate::pages::schedule::ClassSlot;

#[component]
pub fn ClassCard(slot: ClassSlot, #[prop(into)] on_change: Callback<()>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let is_logged_in = auth::get_token().is_some();

    let state = if slot.cancelled {
        "cancelled"
    } else if slot.user_booked {
        "booked"
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
    let on_change_book = on_change.clone();
    let date_book = date.clone();

    let time_str = slot.start_time.clone();
    let instructor_id = slot.instructor_id.unwrap_or(0);
    let booked = slot.booked;
    let capacity = slot.capacity;

    let on_book = move |_| {
        let date = date_book.clone();
        let on_change = on_change_book.clone();
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
                id: i64,
            }
            match api::post::<Req, Resp>("/api/bookings", &Req { template_id, date }).await {
                Ok(_) => on_change.run(()),
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    let on_cancel = move |_| {
        let on_change = on_change.clone();
        if let Some(bid) = booking_id {
            set_loading.set(true);
            set_error.set(String::new());
            spawn_local(async move {
                match api::delete(&format!("/api/bookings/{bid}")).await {
                    Ok(_) => on_change.run(()),
                    Err(e) => set_error.set(e),
                }
                set_loading.set(false);
            });
        }
    };

    let slot_cancelled = slot.cancelled;
    let slot_user_booked = slot.user_booked;
    let slot_full = slot.booked >= slot.capacity;
    let slot_booking_source = slot.user_booking_source.clone();

    let cancel_testid = format!("cancel-{template_id}-{}", slot.date);
    let book_testid = format!("book-{template_id}-{}", slot.date);

    let action_view = if slot_cancelled {
        view! { <span class="badge badge--cancelled">{move || i18n::t(lang.get(), "cancelled")}</span> }.into_any()
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
                <div class="list-row__sub">{move || i18n::tf(lang.get(), "instructor_format", &[&instructor_id.to_string()])}</div>
                <div class="list-row__sub">{move || i18n::tf(lang.get(), "spots_format", &[&booked.to_string(), &capacity.to_string()])}</div>
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
