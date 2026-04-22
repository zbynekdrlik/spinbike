//! Staff-only upcoming-classes panel for a single card.
//!
//! Shows the next ~14 days of class instances relevant to this card: free slots
//! the staff can book, already-booked rows (with cancel), AUTO rows from a
//! persistent subscription (with skip-this-week), and read-only past / full /
//! cancelled rows. Refetches on `refresh_tick` changes.

use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct UpcomingRow {
    pub template_id: i64,
    pub date: String,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub booked: i64,
    pub state: String,
    pub booking_id: Option<i64>,
}

#[component]
pub fn UpcomingClasses(
    card_id: i64,
    #[prop(into)] refresh_tick: Signal<u32>,
    #[prop(into)] on_changed: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (rows, set_rows) = signal(Vec::<UpcomingRow>::new());
    let (msg, set_msg) = signal(String::new());

    Effect::new(move |_| {
        let _ = refresh_tick.get();
        spawn_local(async move {
            match api::get::<Vec<UpcomingRow>>(&format!(
                "/api/cards/{card_id}/upcoming-classes?days=14"
            ))
            .await
            {
                Ok(v) => set_rows.set(v),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
        });
    });

    view! {
        <div data-testid="upcoming-classes">
            <h3>{move || i18n::t(lang.get(), "upcoming_classes")}</h3>
            <div class="group">
                {move || {
                    let list = rows.get();
                    let items: Vec<_> = list.into_iter().map(|row| {
                        let tid = row.template_id;
                        let date = row.date.clone();
                        let bid = row.booking_id;
                        let state = row.state.clone();
                        let testid = format!("upcoming-{tid}-{date}");

                        let action = match state.as_str() {
                            "free" => {
                                let book_date = date.clone();
                                view! {
                                    <button
                                        class="btn btn--primary btn--compact"
                                        data-testid=format!("book-{tid}-{date}")
                                        on:click=move |_| {
                                            let d = book_date.clone();
                                            spawn_local(async move {
                                                #[derive(serde::Serialize)]
                                                struct Req {
                                                    template_id: i64,
                                                    date: String,
                                                    card_id: i64,
                                                }
                                                #[derive(serde::Deserialize)]
                                                struct Resp { #[allow(dead_code)] id: i64 }
                                                match api::post::<Req, Resp>(
                                                    "/api/bookings",
                                                    &Req { template_id: tid, date: d, card_id },
                                                ).await {
                                                    Ok(_) => on_changed.run(()),
                                                    Err(e) => set_msg.set(format!("Error: {e}")),
                                                }
                                            });
                                        }
                                    >
                                        {move || i18n::t(lang.get(), "book")}
                                    </button>
                                }.into_any()
                            }
                            "booked" => view! {
                                <button
                                    class="btn btn--danger btn--compact"
                                    on:click=move |_| {
                                        if let Some(b) = bid {
                                            spawn_local(async move {
                                                match api::delete(&format!("/api/bookings/{b}"))
                                                    .await
                                                {
                                                    Ok(_) => on_changed.run(()),
                                                    Err(e) => set_msg.set(format!("Error: {e}")),
                                                }
                                            });
                                        }
                                    }
                                >
                                    {move || i18n::t(lang.get(), "cancel_booking")}
                                </button>
                            }.into_any(),
                            "auto" => {
                                let testid_a = format!("auto-cancel-{tid}-{date}");
                                view! {
                                    <button
                                        class="btn btn--ghost btn--compact"
                                        data-testid=testid_a
                                        on:click=move |_| {
                                            if let Some(b) = bid {
                                                spawn_local(async move {
                                                    match api::delete(&format!(
                                                        "/api/bookings/{b}"
                                                    ))
                                                    .await
                                                    {
                                                        Ok(_) => on_changed.run(()),
                                                        Err(e) => set_msg.set(format!("Error: {e}")),
                                                    }
                                                });
                                            }
                                        }
                                    >
                                        {move || format!(
                                            "{} — {}",
                                            i18n::t(lang.get(), "auto"),
                                            i18n::t(lang.get(), "skip_this_week"),
                                        )}
                                    </button>
                                }.into_any()
                            }
                            "full" => view! {
                                <button class="btn btn--ghost btn--compact" disabled=true>
                                    {move || i18n::t(lang.get(), "full")}
                                </button>
                            }.into_any(),
                            "cancelled" => view! {
                                <span class="badge badge--cancelled">
                                    {move || i18n::t(lang.get(), "cancelled")}
                                </span>
                            }.into_any(),
                            _ => view! {
                                <span class="badge">
                                    {move || i18n::t(lang.get(), "past")}
                                </span>
                            }.into_any(),
                        };

                        let instr = row.instructor_name.clone().unwrap_or_default();
                        let count = format!("{}/{}", row.booked, row.capacity);
                        let row_class = match state.as_str() {
                            "past" => "list-row list-row--past".to_string(),
                            "cancelled" => "list-row list-row--cancelled".to_string(),
                            _ => "list-row list-row--interactive".to_string(),
                        };
                        let date_cell = row.date.clone();
                        let time_cell = row.start_time.clone();

                        view! {
                            <div class=row_class data-testid=testid>
                                <div class="list-row__main">
                                    <div class="list-row__title">{date_cell} {" "} {time_cell}</div>
                                    <div class="list-row__sub">{instr} {" "} {count}</div>
                                </div>
                                <div class="list-row__end">
                                    {action}
                                </div>
                            </div>
                        }
                    }).collect();
                    items
                }}
            </div>
            <div class="msg">{move || msg.get()}</div>
        </div>
    }
}
