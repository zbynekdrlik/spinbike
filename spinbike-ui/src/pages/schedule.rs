use leptos::prelude::*;
use spinbike_core::ws::ServerMsg;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::MySlots;
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
    /// #381: the instructor's name, resolved server-side.
    #[serde(default)]
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub booked: i64,
    pub cancelled: bool,
    pub user_booked: bool,
    pub user_booking_id: Option<i64>,
    pub user_booking_source: Option<String>,
    /// #382 R4: the occurrence already started (server, gym-local time) —
    /// no Book button any more.
    #[serde(default)]
    pub started: bool,
    /// #382 R2: a customer may still cancel a booking on it (before 09:00
    /// on the class day) — otherwise the booking is binding.
    #[serde(default)]
    pub cancel_open: bool,
}

/// Compute the Mon-Sun week `week_offset` weeks after the current one as
/// Vec<(year, month, day)> and date strings (#382: 0 = this week, 1 = next
/// week, so a customer can book ahead on Friday-Sunday too).
fn week_dates(week_offset: i32) -> (Vec<(i32, u32, u32)>, Vec<String>) {
    let now = js_sys::Date::new_0();
    let year = now.get_full_year();
    let month = now.get_month() as i32; // 0-based
    let day = now.get_date() as i32;
    let dow = now.get_day(); // 0=Sun

    let days_since_monday: i32 = if dow == 0 { 6 } else { dow as i32 - 1 };

    let monday = js_sys::Date::new_with_year_month_day(
        year,
        month,
        day - days_since_monday + 7 * week_offset,
    );

    let mut dates = Vec::with_capacity(7);
    let mut date_strs = Vec::with_capacity(7);
    for i in 0..7i32 {
        let d = js_sys::Date::new_with_year_month_day(
            monday.get_full_year(),
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
    // #382: this week (0) or next week (1).
    let (week, set_week) = signal(0i32);

    let now = js_sys::Date::new_0();
    let dow = now.get_day();
    let today_idx = if dow == 0 { 6 } else { (dow - 1) as usize };
    let (selected_idx, set_selected_idx) = signal(today_idx);

    let (classes, set_classes) = signal(Vec::<ClassSlot>::new());
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());
    let (fetch_ver, set_fetch_ver) = signal(0u32);

    Effect::new(move || {
        let _ = fetch_ver.get();
        let (_, date_strs) = week_dates(week.get());
        let from = date_strs.first().cloned().unwrap_or_default();
        let to = date_strs.last().cloned().unwrap_or_default();
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

    // #382 R5: a logged-in customer manages their weekly slots right here
    // (the "Spinning" menu entry and the home card link both land on this
    // page). Logged-out visitors only see the schedule.
    let logged_in = crate::auth::get_token().is_some();
    let on_slots_changed = Callback::new(move |_: ()| set_fetch_ver.update(|v| *v += 1));

    // This week returns to today; next week opens on its Monday.
    let on_this_week = move |_| {
        set_week.set(0);
        set_selected_idx.set(today_idx);
    };
    let on_next_week = move |_| {
        set_week.set(1);
        set_selected_idx.set(0);
    };

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "schedule")}</h1>

        {logged_in.then(|| view! { <MySlots on_changed=on_slots_changed /> })}

        <div class="week-switch">
            <button
                class=move || if week.get() == 0 { "btn btn--compact btn--primary" } else { "btn btn--compact btn--ghost" }
                data-testid="week-this"
                on:click=on_this_week
            >
                {move || i18n::t(lang.get(), "week_this")}
            </button>
            <button
                class=move || if week.get() == 1 { "btn btn--compact btn--primary" } else { "btn btn--compact btn--ghost" }
                data-testid="week-next"
                on:click=on_next_week
            >
                {move || i18n::t(lang.get(), "week_next")}
            </button>
        </div>

        {move || {
            let (dates, _) = week_dates(week.get());
            DayPicker(DayPickerProps { days: dates, selected_idx, set_selected_idx })
        }}

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

            let (_, date_strs) = week_dates(week.get());
            let idx = selected_idx.get();
            let selected_date = date_strs.get(idx).cloned().unwrap_or_default();
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
                <div class="group">{cards}</div>
            }.into_any()
        }}
    }
}
