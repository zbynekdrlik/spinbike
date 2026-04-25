use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::schedule::ClassSlot;

#[derive(Debug, Clone, serde::Deserialize)]
struct WalkinCardHit {
    id: i64,
    barcode: String,
    user_id: Option<i64>,
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    last_name: Option<String>,
}

fn walkin_display(card: &WalkinCardHit) -> String {
    let name = match (&card.first_name, &card.last_name) {
        (Some(f), Some(l)) => format!("{f} {l}"),
        (Some(f), None) => f.clone(),
        (None, Some(l)) => l.clone(),
        (None, None) => String::new(),
    };
    if name.is_empty() {
        card.barcode.clone()
    } else {
        format!("{name} · {}", card.barcode)
    }
}

fn walkin_urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct Participant {
    booking_id: i64,
    user_name: String,
    user_email: String,
}

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
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
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
        <h1 class="page-title">{move || i18n::t(lang.get(), "staff_dashboard")}</h1>

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
                return view! { <div class="empty-state">{i18n::t(lang.get(), "no_classes_week")}</div> }.into_any();
            }

            let cards: Vec<_> = list.iter().map(|slot| {
                let template_id = slot.template_id;
                let date = slot.date.clone();
                let cancelled = slot.cancelled;
                let set_v = set_ver;

                let card_class = if slot.cancelled {
                    "list-row list-row--cancelled"
                } else if slot.booked >= slot.capacity {
                    "list-row list-row--full"
                } else {
                    "list-row list-row--available"
                };

                // Reactive on `lang` — language toggle re-renders the label live.
                let slot_date = slot.date.clone();
                let start_time = slot.start_time.clone();
                let time_label = move || {
                    let l = lang.get();
                    let date_pretty = chrono::NaiveDate::parse_from_str(&slot_date, "%Y-%m-%d")
                        .map(|d| {
                            format!(
                                "{} {}",
                                i18n::fmt_weekday_short(d, l),
                                i18n::fmt_date(d, l)
                            )
                        })
                        .unwrap_or_else(|_| slot_date.clone());
                    format!("{} {}", date_pretty, start_time)
                };
                let booked = slot.booked;
                let capacity = slot.capacity;

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
                            <button class="btn btn--ghost btn--compact" on:click=move |_| set_walkin_open.update(|v| *v = !*v)>
                                {move || i18n::t(lang.get(), "add_walk_in")}
                            </button>
                            <button class="btn btn--danger btn--compact" on:click=on_cancel_class disabled=move || cancel_loading.get()>
                                {move || i18n::t(lang.get(), "cancel_class")}
                            </button>
                        </div>
                    }.into_any()
                } else {
                    view! { <span class="badge badge--cancelled">{move || i18n::t(lang.get(), "cancelled")}</span> }.into_any()
                };

                // Fetch participants for this class
                let (participants, set_participants) = signal(Vec::<Participant>::new());
                let date_p = date.clone();
                {
                    let date_p = date_p.clone();
                    spawn_local(async move {
                        if let Ok(data) = api::get::<Vec<Participant>>(
                            &format!("/api/classes/{template_id}/{date_p}/participants"),
                        )
                        .await
                        {
                            set_participants.set(data);
                        }
                    });
                }

                view! {
                    <div>
                        <div class=card_class>
                            <div class="list-row__main">
                                <div class="list-row__title">{time_label}</div>
                                <div class="list-row__sub">{move || i18n::tf(lang.get(), "booked_format", &[&booked.to_string(), &capacity.to_string()])}</div>
                            </div>
                            <div class="list-row__end">
                                {actions}
                            </div>
                        </div>
                        <div class="participants-list" style="margin-left:8px;margin-bottom:8px">
                            {move || {
                                let list = participants.get();
                                if list.is_empty() {
                                    return view! { <span></span> }.into_any();
                                }
                                let tags: Vec<_> = list.iter().map(|p| {
                                    let bid = p.booking_id;
                                    let name = p.user_name.clone();
                                    let set_p = set_participants;
                                    let set_v = set_ver;
                                    let on_cancel = move |_| {
                                        spawn_local(async move {
                                            if api::delete(&format!("/api/bookings/{bid}")).await.is_ok() {
                                                set_p.update(|list| list.retain(|pp| pp.booking_id != bid));
                                                set_v.update(|v| *v += 1);
                                            }
                                        });
                                    };
                                    view! {
                                        <span class="badge" style="display:inline-flex;align-items:center;gap:4px;margin:2px 4px;padding:2px 8px;background:#e0e7ff;border-radius:12px;font-size:0.8rem">
                                            {name}
                                            <button
                                                class="btn btn--compact btn--ghost"
                                                style="background:none;border:none;cursor:pointer;font-size:0.8rem;color:#dc2626;padding:0 2px"
                                                on:click=on_cancel
                                                title=move || i18n::t(lang.get(), "cancel_booking")
                                            >
                                                "\u{2715}"
                                            </button>
                                        </span>
                                    }.into_any()
                                }).collect();
                                view! { <div>{tags}</div> }.into_any()
                            }}
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
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let search_ref = NodeRef::<leptos::html::Input>::new();
    let (query, set_query) = signal(String::new());
    let (results, set_results) = signal(Vec::<WalkinCardHit>::new());
    let (err, set_err) = signal(String::new());
    let (loading, set_loading) = signal(false);

    // Debounced card search, same pattern as the staff /staff dashboard.
    Effect::new(move |_| {
        let q = query.get();
        set_err.set(String::new());
        if q.trim().is_empty() {
            set_results.set(Vec::new());
            return;
        }
        let q_at_start = q.clone();
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(250).await;
            if query.get_untracked() != q_at_start {
                return;
            }
            let encoded = walkin_urlencode(&q_at_start);
            match api::get::<Vec<WalkinCardHit>>(&format!("/api/cards/search?q={encoded}&limit=10"))
                .await
            {
                Ok(list) => {
                    if query.get_untracked() == q_at_start {
                        set_results.set(list);
                    }
                }
                Err(e) => set_err.set(e),
            }
        });
    });

    let on_input = move |ev: web_sys::Event| {
        let value = ev
            .target()
            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
            .map(|el| el.value())
            .unwrap_or_default();
        set_query.set(value);
    };

    let date_for_pick = date.clone();
    let pick = move |card: WalkinCardHit| {
        let Some(user_id) = card.user_id else {
            set_err.set(i18n::t(lang.get_untracked(), "card_has_no_user").to_string());
            return;
        };
        let date = date_for_pick.clone();
        set_loading.set(true);
        set_err.set(String::new());
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                template_id: i64,
                date: String,
                user_id: Option<i64>,
                card_id: Option<i64>,
            }
            #[derive(serde::Deserialize)]
            struct Resp {
                #[allow(dead_code)]
                id: i64,
            }
            match api::post::<Req, Resp>(
                "/api/bookings",
                &Req {
                    template_id,
                    date,
                    user_id: Some(user_id),
                    card_id: Some(card.id),
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
        <div class="card" style="margin-left:20px;margin-top:-8px" data-testid="walkin-form">
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "search_card")}</label>
                <input
                    type="search"
                    class="form-control"
                    node_ref=search_ref
                    data-testid="walkin-search"
                    placeholder=move || i18n::t(lang.get(), "search_card_placeholder")
                    prop:value=move || query.get()
                    on:input=on_input
                    autocomplete="off"
                />
            </div>
            <ul class="walkin-results">
                {move || {
                    let list = results.get();
                    let is_loading = loading.get();
                    list.into_iter().map(|card| {
                        let label = walkin_display(&card);
                        let pick = pick.clone();
                        let card_id = card.id;
                        view! {
                            <li class="walkin-row" data-testid=format!("walkin-pick-{card_id}")>
                                <button
                                    type="button"
                                    class="btn btn--primary btn--compact"
                                    disabled=is_loading
                                    on:click=move |_| pick(card.clone())
                                >
                                    {label}
                                </button>
                            </li>
                        }
                    }).collect::<Vec<_>>()
                }}
            </ul>
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
