use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

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
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
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
        <h1 class="page-title">{move || i18n::t(lang.get(), "my_bookings")}</h1>

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
                return view! { <div class="empty-state">{move || i18n::t(lang.get(), "no_bookings")}</div> }.into_any();
            }

            let rows: Vec<_> = list.iter().map(|b| {
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
                    <div class="list-row">
                        <div class="list-row__main">
                            <div class="list-row__title">{title}</div>
                            {move || {
                                let ce = cancel_err.get();
                                if ce.is_empty() {
                                    view! { <span></span> }.into_any()
                                } else {
                                    view! { <div class="alert alert-error">{ce}</div> }.into_any()
                                }
                            }}
                        </div>
                        <div class="list-row__end">
                            <button class="btn btn--danger btn--compact" on:click=on_cancel disabled=move || cancel_loading.get()>
                                {move || if cancel_loading.get() { "..." } else { i18n::t(lang.get(), "cancel") }}
                            </button>
                        </div>
                    </div>
                }.into_any()
            }).collect();
            view! {
                <div class="group">
                    {rows}
                </div>
            }.into_any()
        }}
    }
}
