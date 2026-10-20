//! `/schedule` (staff/admin) and `/staff/classes` — a read-only summary of
//! the current week's classes plus the way into the real spinning screen.
//!
//! #385: the participant chips, the walk-in picker and the per-class cancel
//! button that used to live here were replaced by `/staff/spinning`
//! (8-week occupancy grid + class detail TABLE, owner decision "tabulky, nie
//! stitky"). This page only keeps the at-a-glance occupancy of this week.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::schedule::ClassSlot;
use crate::pages::staff_spinning::model;

#[component]
pub fn StaffDashboardPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (classes, set_classes) = signal(Vec::<ClassSlot>::new());
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());

    // Gym-local (Europe/Bratislava) current week, Monday..Sunday.
    let monday = model::week_monday(crate::relative_date::today_local());
    let from = monday.to_string();
    let to = (monday + chrono::Duration::days(6)).to_string();
    spawn_local(async move {
        match api::get_coded::<Vec<ClassSlot>>(&format!("/api/classes?from={from}&to={to}")).await {
            Ok(data) => {
                set_classes.set(data);
                set_error.set(String::new());
            }
            Err(e) => set_error.set(i18n::localize_api_error(
                lang.get_untracked(),
                e.code,
                &e.message,
            )),
        }
        set_loading.set(false);
    });

    view! {
        <h1 class="page-title">{move || i18n::t(lang.get(), "staff_dashboard")}</h1>

        <a href="/staff/spinning" class="btn btn--primary mb-2" data-testid="open-staff-spinning">
            {move || i18n::t(lang.get(), "spin_open_screen")}
        </a>

        {move || {
            let e = error.get();
            (!e.is_empty()).then(|| view! { <div class="alert alert-error">{e}</div> })
        }}

        {move || {
            if loading.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }

            let list = classes.get();
            if list.is_empty() {
                return view! { <div class="empty-state">{i18n::t(lang.get(), "no_classes_week")}</div> }.into_any();
            }

            let l = lang.get();
            let rows: Vec<_> = list.iter().map(|slot| {
                let row_class = if slot.cancelled {
                    "list-row list-row--cancelled"
                } else if slot.booked >= slot.capacity {
                    "list-row list-row--full"
                } else {
                    "list-row list-row--available"
                };
                let date_pretty = crate::dates::parse_server_date(&slot.date)
                    .map(|d| format!("{} {}", i18n::fmt_weekday_short(d, l), i18n::fmt_date(d, l)))
                    .unwrap_or_else(|| slot.date.clone());
                let title = format!("{date_pretty} {}", slot.start_time);
                let sub = if slot.cancelled {
                    i18n::t(l, "cancelled").to_string()
                } else {
                    i18n::tf(l, "booked_format", &[&slot.booked.to_string(), &slot.capacity.to_string()])
                };
                view! {
                    <a
                        href=model::occurrence_href(slot.template_id, &slot.date)
                        class=row_class
                        data-testid=format!("roster-class-{}-{}", slot.template_id, slot.date)
                    >
                        <div class="list-row__main">
                            <div class="list-row__title">{title}</div>
                            <div class="list-row__sub">{sub}</div>
                        </div>
                    </a>
                }
            }).collect();
            view! { <div>{rows}</div> }.into_any()
        }}
    }
}
