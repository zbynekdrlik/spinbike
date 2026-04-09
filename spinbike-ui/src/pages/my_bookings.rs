use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct BookingRow {
    id: i64,
    template_id: i64,
    date: String,
    user_id: i64,
}

#[component]
pub fn MyBookingsPage() -> impl IntoView {
    let (bookings, set_bookings) = signal(Vec::<BookingRow>::new());
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());
    let (ver, set_ver) = signal(0u32);

    Effect::new(move || {
        let _ = ver.get();
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<Vec<BookingRow>>("/api/my/bookings").await {
                Ok(data) => {
                    set_bookings.set(data);
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    });

    view! {
        <h1 class="page-title">"My Bookings"</h1>

        {move || {
            let e = error.get();
            if !e.is_empty() {
                view! { <div class="alert alert-error">{e}</div> }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }
        }}

        {move || {
            if loading.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }

            let list = bookings.get();
            if list.is_empty() {
                return view! { <div class="empty-state">"No upcoming bookings"</div> }.into_any();
            }

            let cards: Vec<_> = list.iter().map(|b| {
                let bid = b.id;
                let template_id = b.template_id;
                let date = b.date.clone();
                let set_v = set_ver;
                let (cancel_loading, set_cancel_loading) = signal(false);
                let (cancel_err, set_cancel_err) = signal(String::new());

                let on_cancel = move |_| {
                    set_cancel_loading.set(true);
                    set_cancel_err.set(String::new());
                    spawn_local(async move {
                        match api::delete(&format!("/api/bookings/{bid}")).await {
                            Ok(_) => set_v.update(|v| *v += 1),
                            Err(e) => set_cancel_err.set(e),
                        }
                        set_cancel_loading.set(false);
                    });
                };

                let title = format!("Class #{template_id} — {date}");

                view! {
                    <div class="card">
                        <div class="card-header">
                            <div class="card-title">{title}</div>
                            <button class="btn btn-sm btn-danger" on:click=on_cancel disabled=move || cancel_loading.get()>
                                {move || if cancel_loading.get() { "..." } else { "Cancel" }}
                            </button>
                        </div>
                        {move || {
                            let ce = cancel_err.get();
                            if ce.is_empty() {
                                view! { <span></span> }.into_any()
                            } else {
                                view! { <div class="alert alert-error">{ce}</div> }.into_any()
                            }
                        }}
                    </div>
                }.into_any()
            }).collect();
            view! { <div>{cards}</div> }.into_any()
        }}
    }
}
