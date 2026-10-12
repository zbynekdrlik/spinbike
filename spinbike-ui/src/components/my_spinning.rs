//! Customer spinning self-service (#382).
//!
//! - [`MySpinningCard`] — the "Moje spinningy" card at the TOP of the
//!   customer home (`/my/balance`), rendered only for a spinner (R7): the next
//!   bookings with a cancel button while the 09:00 deadline is open (R2) and a
//!   "binding" mark afterwards, plus the link to manage the weekly slots.
//! - [`MySlots`] — the "Pravidelne terminy" list on the schedule page (R5):
//!   one toggle per weekly class; saving PUTs the complete set and the server
//!   books the next two weeks at once.
//!
//! Every deadline decision (`cancel_open`) is computed by the server in
//! Europe/Bratislava time — the client never does its own time-zone math.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::dates;
use crate::i18n::{self, DAY_KEYS, Lang};

/// How many upcoming bookings the home card lists.
const CARD_ROWS: usize = 4;

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct MyBooking {
    id: i64,
    date: String,
    start_time: String,
    instructor_name: Option<String>,
    #[serde(default)]
    source: String,
    #[serde(default)]
    cancel_open: bool,
}

/// "Po 13.10. 18:30" — weekday short name + short date + start time.
fn booking_title(date: &str, start_time: &str, lang: Lang) -> String {
    match dates::parse_server_date(date) {
        Some(d) => {
            use chrono::Datelike;
            let day = i18n::t(lang, DAY_KEYS[d.weekday().num_days_from_monday() as usize]);
            format!("{day} {} {start_time}", i18n::fmt_date_short(d, lang))
        }
        None => format!("{date} {start_time}"),
    }
}

#[component]
pub fn MySpinningCard() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (bookings, set_bookings) = signal(Vec::<MyBooking>::new());
    let (loaded, set_loaded) = signal(false);
    let (error, set_error) = signal(None::<String>);
    let (ver, set_ver) = signal(0u32);

    Effect::new(move |_| {
        let _ = ver.get();
        spawn_local(async move {
            match api::get_coded::<Vec<MyBooking>>("/api/my/bookings").await {
                Ok(list) => {
                    set_bookings.set(list);
                    set_error.set(None);
                }
                Err(e) => set_error.set(Some(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                ))),
            }
            set_loaded.set(true);
        });
    });

    view! {
        <section class="card-spinning" data-testid="my-spinning-card">
            <h2 class="card-spinning__title">{move || i18n::t(lang.get(), "my_spinnings")}</h2>
            {move || error.get().map(|msg| view! {
                <div class="alert alert-error" data-testid="my-spinning-error">{msg}</div>
            })}
            {move || {
                let list = bookings.get();
                if loaded.get() && list.is_empty() {
                    return view! {
                        <div class="card-spinning__empty" data-testid="my-spinning-empty">
                            {i18n::t(lang.get(), "my_spinnings_empty")}
                        </div>
                    }.into_any();
                }
                let lang_now = lang.get();
                list.into_iter().take(CARD_ROWS).map(|b| {
                    let bid = b.id;
                    let title = booking_title(&b.date, &b.start_time, lang_now);
                    let recurring = b.source == "persistent";
                    let instructor = b.instructor_name.clone();
                    let end = if b.cancel_open {
                        let (busy, set_busy) = signal(false);
                        let on_cancel = move |_| {
                            set_busy.set(true);
                            spawn_local(async move {
                                // Never swallow a failed write
                                // (admin-ui-error-handling.md): a refused
                                // cancel (e.g. the 09:00 deadline passed
                                // while the page was open) must surface.
                                match api::delete_coded(&format!("/api/bookings/{bid}")).await {
                                    Ok(()) => set_ver.update(|v| *v += 1),
                                    Err(e) => {
                                        set_error.set(Some(i18n::localize_api_error(
                                            lang.get_untracked(),
                                            e.code,
                                            &e.message,
                                        )));
                                        set_ver.update(|v| *v += 1);
                                    }
                                }
                                set_busy.set(false);
                            });
                        };
                        view! {
                            <button
                                class="btn btn--danger btn--compact"
                                data-testid=format!("my-spinning-cancel-{bid}")
                                on:click=on_cancel
                                disabled=move || busy.get()
                            >
                                {move || if busy.get() { "..." } else { i18n::t(lang.get(), "cancel") }}
                            </button>
                        }.into_any()
                    } else {
                        view! {
                            <span class="badge" data-testid=format!("my-spinning-binding-{bid}")>
                                {i18n::t(lang_now, "spinning_binding")}
                            </span>
                        }.into_any()
                    };
                    view! {
                        <div class="list-row" data-testid=format!("my-spinning-row-{bid}")>
                            <div class="list-row__main">
                                <div class="list-row__title">{title}</div>
                                <div class="list-row__sub">
                                    {instructor}
                                    {recurring.then(|| view! {
                                        <span class="badge badge--inline">{i18n::t(lang_now, "recurring")}</span>
                                    })}
                                </div>
                            </div>
                            <div class="list-row__end">{end}</div>
                        </div>
                    }.into_any()
                }).collect_view().into_any()
            }}
            <a href="/schedule" class="btn btn--ghost btn--compact" data-testid="my-spinning-manage">
                {move || i18n::t(lang.get(), "spinning_manage_slots")}
            </a>
        </section>
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct Slot {
    template_id: i64,
    weekday: i64,
    start_time: String,
    instructor_name: Option<String>,
    subscribed: bool,
}

#[derive(Serialize)]
struct SetSlotsReq {
    template_ids: Vec<i64>,
}

/// "Po 18:00 · Stopa".
fn slot_label(slot: &Slot, lang: Lang) -> String {
    let day = usize::try_from(slot.weekday)
        .ok()
        .and_then(|w| DAY_KEYS.get(w))
        .map(|k| i18n::t(lang, k))
        .unwrap_or("?");
    match &slot.instructor_name {
        Some(name) => format!("{day} {} \u{b7} {name}", slot.start_time),
        None => format!("{day} {}", slot.start_time),
    }
}

/// The new subscribed set after toggling `template_id` (pure; unit-tested).
fn toggled_set(slots: &[Slot], template_id: i64) -> Vec<i64> {
    slots
        .iter()
        .filter(|s| {
            if s.template_id == template_id {
                !s.subscribed
            } else {
                s.subscribed
            }
        })
        .map(|s| s.template_id)
        .collect()
}

#[component]
pub fn MySlots(#[prop(into)] on_changed: Callback<()>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (slots, set_slots) = signal(Vec::<Slot>::new());
    let (error, set_error) = signal(None::<String>);
    let (busy, set_busy) = signal(false);

    Effect::new(move |_| {
        spawn_local(async move {
            match api::get_coded::<Vec<Slot>>("/api/my/persistent-bookings").await {
                Ok(list) => set_slots.set(list),
                Err(e) => set_error.set(Some(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                ))),
            }
        });
    });

    view! {
        <section class="card-spinning" data-testid="my-slots">
            <h2 class="card-spinning__title">{move || i18n::t(lang.get(), "recurring_slots")}</h2>
            <div class="card-spinning__empty">{move || i18n::t(lang.get(), "recurring_slots_hint")}</div>
            {move || error.get().map(|msg| view! {
                <div class="alert alert-error" data-testid="my-slots-error">{msg}</div>
            })}
            {move || {
                let lang_now = lang.get();
                slots.get().into_iter().map(|slot| {
                    let tid = slot.template_id;
                    let on = slot.subscribed;
                    let label = slot_label(&slot, lang_now);
                    let btn_class = if on {
                        "btn btn--compact btn--ghost"
                    } else {
                        "btn btn--compact btn--primary"
                    };
                    let subscribed_attr = if on { "true" } else { "false" };
                    let btn_text = if on {
                        i18n::t(lang_now, "recurring_off")
                    } else {
                        i18n::t(lang_now, "recurring_on")
                    };
                    let on_toggle = move |_| {
                        let wanted = toggled_set(&slots.get_untracked(), tid);
                        set_busy.set(true);
                        set_error.set(None);
                        spawn_local(async move {
                            match api::put_json::<SetSlotsReq, Vec<Slot>>(
                                "/api/my/persistent-bookings",
                                &SetSlotsReq { template_ids: wanted },
                            )
                            .await
                            {
                                Ok(list) => {
                                    set_slots.set(list);
                                    on_changed.run(());
                                }
                                Err(e) => set_error.set(Some(i18n::localize_api_error(
                                    lang.get_untracked(),
                                    e.code,
                                    &e.message,
                                ))),
                            }
                            set_busy.set(false);
                        });
                    };
                    view! {
                        <div class="list-row">
                            <div class="list-row__main">
                                <div class="list-row__title">{label}</div>
                            </div>
                            <div class="list-row__end">
                                <button
                                    class=btn_class
                                    data-testid=format!("slot-toggle-{tid}")
                                    data-subscribed=subscribed_attr
                                    on:click=on_toggle
                                    disabled=move || busy.get()
                                >
                                    {btn_text}
                                </button>
                            </div>
                        </div>
                    }
                }).collect_view()
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    fn slot(id: i64, weekday: i64, subscribed: bool) -> Slot {
        Slot {
            template_id: id,
            weekday,
            start_time: "18:30".into(),
            instructor_name: Some("Vlada".into()),
            subscribed,
        }
    }

    #[wasm_bindgen_test]
    fn toggled_set_adds_and_removes_exactly_one_slot() {
        let slots = vec![slot(1, 0, true), slot(2, 1, false), slot(3, 2, true)];
        assert_eq!(toggled_set(&slots, 2), vec![1, 2, 3]);
        assert_eq!(toggled_set(&slots, 1), vec![3]);
        assert_eq!(toggled_set(&slots, 99), vec![1, 3], "unknown id: no change");
    }

    #[wasm_bindgen_test]
    fn slot_label_names_day_time_and_instructor() {
        assert_eq!(
            slot_label(&slot(2, 1, false), Lang::Sk),
            "Ut 18:30 \u{b7} Vlada"
        );
        assert_eq!(
            slot_label(&slot(2, 1, false), Lang::En),
            "Tue 18:30 \u{b7} Vlada"
        );
    }

    #[wasm_bindgen_test]
    fn booking_title_has_weekday_date_and_time() {
        assert_eq!(
            booking_title("2026-10-13", "18:30", Lang::Sk),
            "Ut 13.10. 18:30"
        );
        assert_eq!(booking_title("bogus", "18:30", Lang::Sk), "bogus 18:30");
    }
}
