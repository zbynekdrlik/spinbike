//! `/staff/spinning` — the CEO's spinning screen (#385, owner-approved spec
//! `docs/superpowers/specs/2026-10-20-staff-spinning-screen-design.md`).
//!
//! An 8-week occupancy grid (rows = weeks, columns = weekly classes, cell =
//! `booked/capacity` tinted by fill) loaded with ONE `/api/classes` call;
//! tapping a cell opens that occurrence's detail table below
//! ([`detail::ClassDetail`]). Every write there reloads the grid and the
//! table. Regulars who never install the app are managed entirely from here.

mod add_person;
mod detail;
pub mod model;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, DAY_KEYS, Lang};
use crate::pages::schedule::ClassSlot;
use crate::util::RequestId;

use detail::ClassDetail;

#[component]
pub fn StaffSpinningPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let today = crate::relative_date::today_local();
    let (from, to) = model::grid_range(today);
    let mondays = model::grid_mondays(today);
    let (slots, set_slots) = signal(Vec::<ClassSlot>::new());
    let (loaded, set_loaded) = signal(false);
    let (error, set_error) = signal(String::new());
    let (reload, set_reload) = signal(0u32);
    let (selected, set_selected) = signal(None::<(i64, String)>);
    let req = RequestId::new();

    Effect::new(move |_| {
        let _ = reload.get();
        let token = req.next();
        let url = format!("/api/classes?from={from}&to={to}");
        spawn_local(async move {
            let res = api::get_coded::<Vec<ClassSlot>>(&url).await;
            if !token.is_latest() {
                return;
            }
            match res {
                Ok(list) => {
                    set_slots.set(list);
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                )),
            }
            set_loaded.set(true);
        });
    });

    let on_changed = Callback::new(move |_: ()| set_reload.update(|v| *v += 1));

    view! {
        <h1 class="page-title" data-testid="spin-page-title">{move || i18n::t(lang.get(), "spin_title")}</h1>

        {move || {
            let e = error.get();
            (!e.is_empty()).then(|| view! {
                <div class="alert alert-error" data-testid="spin-error">{e}</div>
            })
        }}

        {move || {
            if !loaded.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }
            let list = slots.get();
            let cols = model::grid_columns(&list);
            if cols.is_empty() {
                return view! {
                    <div class="empty-state" data-testid="spin-empty">{i18n::t(lang.get(), "spin_no_classes")}</div>
                }.into_any();
            }
            let l = lang.get();
            let sel = selected.get();
            let header: Vec<_> = cols.iter().map(|c| {
                let day = i18n::t(l, DAY_KEYS[c.weekday.clamp(0, 6) as usize]);
                let instr = c.instructor_name.clone().unwrap_or_default();
                view! {
                    <th scope="col" data-testid=format!("occ-col-{}", c.template_id)>
                        <div>{format!("{day} {}", c.start_time)}</div>
                        <div class="occ-grid__instr">{instr}</div>
                    </th>
                }
            }).collect();
            let rows: Vec<_> = mondays.iter().enumerate().map(|(wi, monday)| {
                let cells: Vec<_> = cols.iter().map(|c| {
                    let date = model::cell_date(*monday, c);
                    let tid = c.template_id;
                    let Some(slot) = model::find_slot(&list, tid, &date) else {
                        return view! { <td class="occ-grid__none">"\u{2014}"</td> }.into_any();
                    };
                    let is_selected = sel.as_ref() == Some(&(tid, date.clone()));
                    let mut class = format!(
                        "occ-cell {}",
                        model::fill_level(slot.booked, slot.capacity).css()
                    );
                    if slot.cancelled {
                        class.push_str(" occ-cell--cancelled");
                    }
                    if slot.started {
                        class.push_str(" occ-cell--past");
                    }
                    if is_selected {
                        class.push_str(" occ-cell--selected");
                    }
                    let label = if slot.cancelled {
                        i18n::t(l, "spin_cancelled_short").to_string()
                    } else if slot.booked >= slot.capacity {
                        format!("{}/{} \u{25cf}", slot.booked, slot.capacity)
                    } else {
                        format!("{}/{}", slot.booked, slot.capacity)
                    };
                    let testid = format!("occ-cell-{tid}-{date}");
                    let pressed = if is_selected { "true" } else { "false" };
                    view! {
                        <td>
                            <button
                                type="button"
                                class=class
                                data-testid=testid
                                aria-pressed=pressed
                                on:click=move |_| set_selected.set(Some((tid, date.clone())))
                            >
                                {label}
                            </button>
                        </td>
                    }.into_any()
                }).collect();
                view! {
                    <tr data-testid=format!("occ-week-{wi}")>
                        <th scope="row" class="occ-grid__week">{model::week_label(*monday)}</th>
                        {cells}
                    </tr>
                }
            }).collect();
            view! {
                <div class="table-scroll">
                    <table class="occ-grid" data-testid="occ-grid">
                        <thead>
                            <tr>
                                <th scope="col">{i18n::t(l, "spin_week")}</th>
                                {header}
                            </tr>
                        </thead>
                        <tbody>{rows}</tbody>
                    </table>
                </div>
            }.into_any()
        }}

        {move || {
            let Some((tid, date)) = selected.get() else {
                return loaded.get().then(|| view! {
                    <p class="text-muted mt-2" data-testid="spin-pick-hint">{i18n::t(lang.get(), "spin_pick_cell")}</p>
                }).into_any();
            };
            match model::find_slot(&slots.get(), tid, &date) {
                Some(slot) => view! { <ClassDetail slot=slot.clone() on_changed=on_changed /> }.into_any(),
                None => ().into_any(),
            }
        }}
    }
}
