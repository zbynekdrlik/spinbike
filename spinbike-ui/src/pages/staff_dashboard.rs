use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::pages::schedule::ClassSlot;

fn current_week_range() -> (String, String) {
    let now = js_sys::Date::new_0();
    let year = now.get_full_year() as u32;
    let month = now.get_month() as i32;
    let day = now.get_date() as i32;
    let dow = now.get_day();
    let days_since_monday: i32 = if dow == 0 { 6 } else { dow as i32 - 1 };

    let monday = js_sys::Date::new_with_year_month_day(year, month, day - days_since_monday);
    let sunday = js_sys::Date::new_with_year_month_day(
        monday.get_full_year() as u32,
        monday.get_month() as i32,
        monday.get_date() as i32 + 6,
    );

    let from = format!(
        "{:04}-{:02}-{:02}",
        monday.get_full_year(),
        monday.get_month() + 1,
        monday.get_date()
    );
    let to = format!(
        "{:04}-{:02}-{:02}",
        sunday.get_full_year(),
        sunday.get_month() + 1,
        sunday.get_date()
    );
    (from, to)
}

#[component]
pub fn StaffDashboardPage() -> impl IntoView {
    let (classes, set_classes) = signal(Vec::<ClassSlot>::new());
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());
    let (ver, set_ver) = signal(0u32);

    Effect::new(move || {
        let _ = ver.get();
        let (from, to) = current_week_range();
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<Vec<ClassSlot>>(&format!("/api/classes?from={from}&to={to}")).await {
                Ok(data) => {
                    set_classes.set(data);
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    });

    view! {
        <h1 class="page-title">"Staff Dashboard"</h1>

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

            let list = classes.get();
            if list.is_empty() {
                return view! { <div class="empty-state">"No classes this week"</div> }.into_any();
            }

            let cards: Vec<_> = list.iter().map(|slot| {
                let template_id = slot.template_id;
                let date = slot.date.clone();
                let cancelled = slot.cancelled;
                let set_v = set_ver;

                let card_class = if slot.cancelled {
                    "class-card cancelled"
                } else if slot.booked >= slot.capacity {
                    "class-card full"
                } else {
                    "class-card available"
                };

                let time_label = format!("{} {}", slot.date, slot.start_time);
                let spots_label = format!("{}/{} booked", slot.booked, slot.capacity);

                let (cancel_loading, set_cancel_loading) = signal(false);
                let (walkin_open, set_walkin_open) = signal(false);

                let date_c = date.clone();
                let on_cancel_class = move |_| {
                    let date = date_c.clone();
                    set_cancel_loading.set(true);
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { template_id: i64, date: String, reason: Option<String> }
                        #[derive(serde::Deserialize)]
                        struct Resp {}
                        let _ = api::post::<Req, Resp>("/api/admin/cancel-class", &Req {
                            template_id,
                            date,
                            reason: None,
                        }).await;
                        set_cancel_loading.set(false);
                        set_v.update(|v| *v += 1);
                    });
                };

                let actions = if !cancelled {
                    view! {
                        <div class="flex gap-1">
                            <button class="btn btn-sm btn-outline" on:click=move |_| set_walkin_open.update(|v| *v = !*v)>
                                "+ Walk-in"
                            </button>
                            <button class="btn btn-sm btn-danger" on:click=on_cancel_class disabled=move || cancel_loading.get()>
                                "Cancel Class"
                            </button>
                        </div>
                    }.into_any()
                } else {
                    view! { <span class="badge badge-cancelled">"Cancelled"</span> }.into_any()
                };

                view! {
                    <div>
                        <div class=card_class>
                            <div class="class-info">
                                <div class="class-time">{time_label}</div>
                                <div class="class-spots">{spots_label}</div>
                            </div>
                            <div class="class-action">
                                {actions}
                            </div>
                        </div>
                        {move || {
                            if walkin_open.get() && !cancelled {
                                let date = date.clone();
                                WalkinForm(WalkinFormProps {
                                    template_id,
                                    date,
                                    on_done: Callback::new(move |_| {
                                        set_walkin_open.set(false);
                                        set_v.update(|v| *v += 1);
                                    }),
                                }).into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }
                        }}
                    </div>
                }.into_any()
            }).collect();

            view! { <div>{cards}</div> }.into_any()
        }}
    }
}

#[component]
fn WalkinForm(
    template_id: i64,
    date: String,
    #[prop(into)] on_done: Callback<()>,
) -> impl IntoView {
    let uid_ref = NodeRef::<leptos::html::Input>::new();
    let (err, set_err) = signal(String::new());
    let (loading, set_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let user_id_str = uid_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();
        let user_id: i64 = user_id_str.parse().unwrap_or(0);
        if user_id == 0 {
            set_err.set("Enter a valid user ID".into());
            return;
        }

        let date = date.clone();
        let on_done = on_done.clone();
        set_loading.set(true);
        set_err.set(String::new());

        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                template_id: i64,
                date: String,
                user_id: Option<i64>,
            }
            #[derive(serde::Deserialize)]
            struct Resp {
                id: i64,
            }
            match api::post::<Req, Resp>(
                "/api/bookings",
                &Req {
                    template_id,
                    date,
                    user_id: Some(user_id),
                },
            )
            .await
            {
                Ok(_) => on_done.run(()),
                Err(e) => set_err.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="card" style="margin-left:20px;margin-top:-8px">
            <form class="inline-form" on:submit=on_submit>
                <div class="form-group">
                    <label>"User ID"</label>
                    <input type="number" class="form-control" node_ref=uid_ref placeholder="User ID" required />
                </div>
                <button type="submit" class="btn btn-sm btn-primary" disabled=move || loading.get()>
                    {move || if loading.get() { "..." } else { "Book" }}
                </button>
            </form>
            {move || {
                let e = err.get();
                if !e.is_empty() {
                    view! { <div class="alert alert-error mt-1">{e}</div> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
        </div>
    }
}
