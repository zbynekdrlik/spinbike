use leptos::prelude::*;
use spinbike_core::ws::ServerMsg;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::class_card::{ClassCard, ClassCardProps};
use crate::components::day_picker::{DayPicker, DayPickerProps};
use crate::i18n::{self, Lang};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClassSlot {
    pub template_id: i64,
    pub date: String,
    pub weekday: i64,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub capacity: i64,
    pub booked: i64,
    pub cancelled: bool,
    pub user_booked: bool,
    pub user_booking_id: Option<i64>,
    pub user_booking_source: Option<String>,
}

/// Compute current week (Mon-Sun) as Vec<(year, month, day)> and date strings.
fn current_week_dates() -> (Vec<(i32, u32, u32)>, Vec<String>) {
    let now = js_sys::Date::new_0();
    let year = now.get_full_year() as u32;
    let month = now.get_month() as i32; // 0-based
    let day = now.get_date() as i32;
    let dow = now.get_day(); // 0=Sun

    let days_since_monday: i32 = if dow == 0 { 6 } else { dow as i32 - 1 };

    let monday = js_sys::Date::new_with_year_month_day(year, month, day - days_since_monday);

    let mut dates = Vec::with_capacity(7);
    let mut date_strs = Vec::with_capacity(7);
    for i in 0..7i32 {
        let d = js_sys::Date::new_with_year_month_day(
            monday.get_full_year() as u32,
            monday.get_month() as i32,
            monday.get_date() as i32 + i,
        );
        let y = d.get_full_year() as i32;
        let m = d.get_month() + 1; // 1-based
        let dd = d.get_date();
        dates.push((y, m, dd));
        date_strs.push(format!("{y:04}-{m:02}-{dd:02}"));
    }
    (dates, date_strs)
}

#[component]
pub fn SchedulePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (dates, date_strs) = current_week_dates();
    let date_strs_stored = date_strs.clone();

    let now = js_sys::Date::new_0();
    let dow = now.get_day();
    let today_idx = if dow == 0 { 6 } else { (dow - 1) as usize };
    let (selected_idx, set_selected_idx) = signal(today_idx);

    let (classes, set_classes) = signal(Vec::<ClassSlot>::new());
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());
    let (fetch_ver, set_fetch_ver) = signal(0u32);

    let date_strs_for_fetch = date_strs_stored.clone();
    Effect::new(move || {
        let _ = fetch_ver.get();
        let from = date_strs_for_fetch.first().cloned().unwrap_or_default();
        let to = date_strs_for_fetch.last().cloned().unwrap_or_default();
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

    // Listen for WebSocket updates.
    let ws_msg = use_context::<ReadSignal<Option<ServerMsg>>>();
    if let Some(ws) = ws_msg {
        Effect::new(move || {
            if let Some(msg) = ws.get() {
                match msg {
                    ServerMsg::BookingUpdate {
                        template_id,
                        date,
                        booked,
                        capacity,
                    } => {
                        set_classes.update(|list| {
                            for slot in list.iter_mut() {
                                if slot.template_id == template_id && slot.date == date {
                                    slot.booked = booked as i64;
                                    slot.capacity = capacity as i64;
                                }
                            }
                        });
                    }
                    ServerMsg::ClassCancelled { template_id, date } => {
                        set_classes.update(|list| {
                            for slot in list.iter_mut() {
                                if slot.template_id == template_id && slot.date == date {
                                    slot.cancelled = true;
                                }
                            }
                        });
                    }
                    ServerMsg::Pong => {}
                }
            }
        });
    }

    let date_strs_for_view = date_strs_stored.clone();

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "schedule")}</h1>
        {DayPicker(DayPickerProps { days: dates, selected_idx, set_selected_idx })}

        {move || {
            let e = error.get();
            if e.is_empty() {
                view! { <span></span> }.into_any()
            } else {
                view! { <div class="alert alert-error">{e}</div> }.into_any()
            }
        }}

        {move || {
            if loading.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }

            let idx = selected_idx.get();
            let selected_date = date_strs_for_view.get(idx).cloned().unwrap_or_default();
            let day_classes: Vec<ClassSlot> = classes.get()
                .iter()
                .filter(|c| c.date == selected_date)
                .cloned()
                .collect();

            if day_classes.is_empty() {
                return view! {
                    <div class="empty-state">{move || i18n::t(lang.get(), "no_classes_today")}</div>
                }.into_any();
            }

            let cards: Vec<_> = day_classes.into_iter().map(|slot| {
                let set_fv = set_fetch_ver;
                ClassCard(ClassCardProps {
                    slot,
                    on_change: Callback::new(move |_| set_fv.update(|v| *v += 1)),
                })
            }).collect();

            view! {
                <div>{cards}</div>
            }.into_any()
        }}
    }
}
