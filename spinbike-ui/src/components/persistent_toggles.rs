//! Staff-only persistent-booking toggles for a single card.
//!
//! Derives the set of available templates from the upcoming-classes list (first
//! 7 days, one row per weekly template), and lets staff flip each template's
//! subscription on or off. The label flips between "On" and "Off" reactively
//! as the active set updates.

use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::upcoming_classes::UpcomingRow;
use crate::i18n::{self, Lang};

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct PersistentRow {
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    card_id: i64,
    template_id: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct TemplateLite {
    id: i64,
    weekday: i64,
    start_time: String,
    instructor_name: Option<String>,
}

#[component]
pub fn PersistentToggles(card_id: i64, #[prop(into)] on_changed: Callback<()>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (active_ids, set_active_ids) = signal(std::collections::HashSet::<i64>::new());
    let (templates, set_templates) = signal(Vec::<TemplateLite>::new());
    let (msg, set_msg) = signal(String::new());
    let (version, set_version) = signal(0u32);

    Effect::new(move |_| {
        let _ = version.get();
        spawn_local(async move {
            // Derive the list of templates from upcoming-classes (first 7 days),
            // so we show exactly the templates this card could subscribe to.
            match api::get::<Vec<UpcomingRow>>(&format!(
                "/api/cards/{card_id}/upcoming-classes?days=7"
            ))
            .await
            {
                Ok(rs) => {
                    let mut seen = std::collections::HashMap::<i64, TemplateLite>::new();
                    for r in rs {
                        seen.entry(r.template_id).or_insert_with(|| TemplateLite {
                            id: r.template_id,
                            weekday: chrono::NaiveDate::parse_from_str(&r.date, "%Y-%m-%d")
                                .map(|d| {
                                    use chrono::Datelike;
                                    d.weekday().num_days_from_monday() as i64
                                })
                                .unwrap_or(0),
                            start_time: r.start_time.clone(),
                            instructor_name: r.instructor_name.clone(),
                        });
                    }
                    let mut v: Vec<_> = seen.into_values().collect();
                    v.sort_by_key(|t| (t.weekday, t.start_time.clone()));
                    set_templates.set(v);
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }

            match api::get::<Vec<PersistentRow>>(&format!(
                "/api/cards/{card_id}/persistent-bookings"
            ))
            .await
            {
                Ok(rs) => set_active_ids.set(rs.into_iter().map(|r| r.template_id).collect()),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
        });
    });

    view! {
        <div class="card mb-2" data-testid="persistent-toggles">
            <h3>{move || i18n::t(lang.get(), "persistent_booking")}</h3>
            <ul class="persistent-list">
                {move || {
                    let list = templates.get();
                    let items: Vec<_> = list.into_iter().map(|tpl| {
                        let tid = tpl.id;
                        let label = format!(
                            "{} — {} {}",
                            weekday_label(tpl.weekday),
                            tpl.instructor_name.clone().unwrap_or_default(),
                            tpl.start_time,
                        );
                        let row_testid = format!("persistent-row-{tid}");
                        let btn_testid = format!("persistent-toggle-{tid}");

                        view! {
                            <li class="persistent-row" data-testid=row_testid>
                                <span>{label}</span>
                                <button
                                    class="btn btn-sm btn-outline"
                                    data-testid=btn_testid
                                    on:click=move |_| {
                                        let currently_on =
                                            active_ids.get_untracked().contains(&tid);
                                        spawn_local(async move {
                                            let res = if currently_on {
                                                api::delete(&format!(
                                                    "/api/cards/{card_id}/persistent-bookings/{tid}"
                                                )).await
                                            } else {
                                                #[derive(serde::Serialize)]
                                                struct Req { template_id: i64 }
                                                #[derive(serde::Deserialize)]
                                                struct Resp {
                                                    #[allow(dead_code)]
                                                    id: i64,
                                                }
                                                api::post::<Req, Resp>(
                                                    &format!(
                                                        "/api/cards/{card_id}/persistent-bookings"
                                                    ),
                                                    &Req { template_id: tid },
                                                ).await.map(|_| ())
                                            };
                                            match res {
                                                Ok(_) => {
                                                    set_version.update(|n| *n += 1);
                                                    on_changed.run(());
                                                }
                                                Err(e) => {
                                                    set_msg.set(format!("Error: {e}"))
                                                }
                                            }
                                        });
                                    }
                                >
                                    {move || {
                                        if active_ids.get().contains(&tid) {
                                            i18n::t(lang.get(), "turn_off")
                                        } else {
                                            i18n::t(lang.get(), "turn_on")
                                        }
                                    }}
                                </button>
                            </li>
                        }
                    }).collect();
                    items
                }}
            </ul>
            <div class="msg">{move || msg.get()}</div>
        </div>
    }
}

fn weekday_label(w: i64) -> &'static str {
    match w {
        0 => "Mon",
        1 => "Tue",
        2 => "Wed",
        3 => "Thu",
        4 => "Fri",
        5 => "Sat",
        _ => "Sun",
    }
}
