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
    let (error, set_error) = signal(None::<api::CodedError>);
    let (ver, set_ver) = signal(0u32);

    Effect::new(move || {
        let _ = ver.get();
        set_loading.set(true);
        spawn_local(async move {
            // get_coded (#145): carries the server's `error_code` so the
            // banner below can localize it instead of showing raw English.
            match api::get_coded::<Vec<BookingRow>>("/api/my/bookings").await {
                Ok(data) => {
                    set_bookings.set(data);
                    set_error.set(None);
                }
                Err(e) => set_error.set(Some(e)),
            }
            set_loading.set(false);
        });
    });

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "my_bookings")}</h1>

        {move || {
            match error.get() {
                Some(e) => {
                    let msg = i18n::localize_api_error(lang.get(), e.code, &e.message);
                    view! { <div class="alert alert-error">{msg}</div> }.into_any()
                }
                None => view! { <span></span> }.into_any(),
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
                let (cancel_err, set_cancel_err) = signal(None::<api::CodedError>);

                let on_cancel = move |_| {
                    set_cancel_loading.set(true);
                    set_cancel_err.set(None);
                    spawn_local(async move {
                        // delete_coded (#145): carries the server's
                        // `error_code` (e.g. booking_not_owned) so the
                        // banner below can localize it.
                        match api::delete_coded(&format!("/api/bookings/{bid}")).await {
                            Ok(_) => set_v.update(|v| *v += 1),
                            Err(e) => set_cancel_err.set(Some(e)),
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
                                match cancel_err.get() {
                                    Some(ce) => {
                                        let msg = i18n::localize_api_error(lang.get(), ce.code, &ce.message);
                                        view! { <div class="alert alert-error">{msg}</div> }.into_any()
                                    }
                                    None => view! { <span></span> }.into_any(),
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
