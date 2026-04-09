use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth;
use crate::pages::schedule::ClassSlot;

#[component]
pub fn ClassCard(
    slot: ClassSlot,
    #[prop(into)] on_change: Callback<()>,
) -> impl IntoView {
    let is_logged_in = auth::get_token().is_some();

    let status_class = if slot.cancelled {
        "class-card cancelled"
    } else if slot.user_booked {
        "class-card booked"
    } else if slot.booked >= slot.capacity {
        "class-card full"
    } else {
        "class-card available"
    };

    let (loading, set_loading) = signal(false);
    let (error, set_error) = signal(String::new());

    let template_id = slot.template_id;
    let date = slot.date.clone();
    let booking_id = slot.user_booking_id;
    let on_change_book = on_change.clone();
    let date_book = date.clone();

    let time_str = slot.start_time.clone();
    let instructor_str = format!("Instructor #{}", slot.instructor_id.unwrap_or(0));
    let spots_str = format!("{}/{} spots", slot.booked, slot.capacity);

    let on_book = move |_| {
        let date = date_book.clone();
        let on_change = on_change_book.clone();
        set_loading.set(true);
        set_error.set(String::new());
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { template_id: i64, date: String }
            #[derive(serde::Deserialize)]
            struct Resp { id: i64 }
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

    let action_view = if slot.cancelled {
        view! { <span class="badge badge-cancelled">"Cancelled"</span> }.into_any()
    } else if !is_logged_in {
        view! { <a href="/login" class="btn btn-sm btn-outline">"Login to book"</a> }.into_any()
    } else if slot.user_booked {
        view! {
            <div>
                <span class="badge badge-booked mb-1">"BOOKED"</span>
                <br/>
                <button class="btn btn-sm btn-danger" on:click=on_cancel disabled=move || loading.get()>
                    {move || if loading.get() { "..." } else { "Cancel" }}
                </button>
            </div>
        }.into_any()
    } else if slot.booked >= slot.capacity {
        view! { <span class="badge badge-full">"FULL"</span> }.into_any()
    } else {
        view! {
            <button class="btn btn-sm btn-primary" on:click=on_book disabled=move || loading.get()>
                {move || if loading.get() { "..." } else { "BOOK" }}
            </button>
        }.into_any()
    };

    view! {
        <div class=status_class>
            <div class="class-info">
                <div class="class-time">{time_str}</div>
                <div class="class-instructor">{instructor_str}</div>
                <div class="class-spots">{spots_str}</div>
                {move || {
                    let e = error.get();
                    if e.is_empty() {
                        view! { <span></span> }.into_any()
                    } else {
                        view! { <div class="alert alert-error" style="margin-top:4px;padding:4px 8px;font-size:0.75rem">{e}</div> }.into_any()
                    }
                }}
            </div>
            <div class="class-action">
                {action_view}
            </div>
        </div>
    }
}
