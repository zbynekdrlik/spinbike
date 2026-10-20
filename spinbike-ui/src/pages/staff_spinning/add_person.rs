//! "Pridat" under the class detail table (#385): find a customer and enrol
//! them ONCE (this occurrence) or PERMANENTLY (every week, same slot).
//!
//! Both actions reuse existing endpoints — `POST /api/bookings` for the one-off
//! booking and `POST /api/users/{uid}/persistent-bookings` for the weekly slot
//! (which runs a materialiser sweep, so the person shows up across the whole
//! 8-week grid at once). A customer already in this class is shown as
//! "uz prihlaseny" instead of offering a booking the server would refuse with
//! 409 `already_booked` (#378).

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::dashboard::helpers::{event_target_value, urlencoding_light};

#[derive(Debug, Clone, serde::Deserialize)]
struct UserHit {
    id: i64,
    name: String,
    card_code: Option<String>,
}

fn hit_label(user: &UserHit) -> String {
    match user.card_code.as_deref().filter(|c| !c.trim().is_empty()) {
        Some(code) => format!("{} \u{00b7} {}", user.name, code),
        None => user.name.clone(),
    }
}

#[component]
pub fn AddPerson(
    template_id: i64,
    /// The occurrence date (`YYYY-MM-DD`).
    date: String,
    /// "Po 18:00" — the weekly slot the permanent enrolment repeats.
    weekly_label: String,
    /// The one-off booking is offered only while the occurrence is bookable
    /// (not cancelled, not started — the server's own flags).
    once_allowed: bool,
    /// Customers already holding an active booking for this occurrence.
    #[prop(into)]
    booked_user_ids: Signal<Vec<i64>>,
    /// Runs after a successful enrolment — the page reloads grid + table.
    #[prop(into)]
    on_done: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (query, set_query) = signal(String::new());
    let (results, set_results) = signal(Vec::<UserHit>::new());
    let (err, set_err) = signal(String::new());
    let (busy, set_busy) = signal(false);

    // Debounced customer search — same pattern (and endpoint) as the desk.
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
            // `try_` reads: the picker remounts on every cell tap / reload,
            // so the signal may already be disposed when this resolves — a
            // plain `get_untracked` on a disposed signal panics the WASM app.
            if query.try_get_untracked().as_ref() != Some(&q_at_start) {
                return;
            }
            let encoded = urlencoding_light(&q_at_start);
            match api::get_coded::<Vec<UserHit>>(&format!("/api/users/search?q={encoded}&limit=10"))
                .await
            {
                Ok(list) => {
                    if query.try_get_untracked().as_ref() == Some(&q_at_start) {
                        set_results.set(list);
                    }
                }
                Err(e) => set_err.set(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                )),
            }
        });
    });

    // One-off booking for this occurrence only.
    let book_once = move |user_id: i64| {
        let date = date.clone();
        set_busy.set(true);
        set_err.set(String::new());
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                template_id: i64,
                date: String,
                user_id: i64,
            }
            #[derive(serde::Deserialize)]
            struct Resp {
                #[allow(dead_code)]
                id: i64,
            }
            // post_json: the coded error renders 409 already_booked /
            // class_full / class_already_started in the staff's language.
            match api::post_json::<Req, Resp>(
                "/api/bookings",
                &Req {
                    template_id,
                    date,
                    user_id,
                },
            )
            .await
            {
                Ok(_) => {
                    set_query.set(String::new());
                    on_done.run(());
                }
                Err(e) => set_err.set(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                )),
            }
            set_busy.set(false);
        });
    };

    // Permanent weekly slot — the server materialises all 8 weeks at once.
    let book_persistent = move |user_id: i64| {
        set_busy.set(true);
        set_err.set(String::new());
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                template_id: i64,
            }
            #[derive(serde::Deserialize)]
            struct Resp {
                #[allow(dead_code)]
                id: i64,
            }
            match api::post_json::<Req, Resp>(
                &format!("/api/users/{user_id}/persistent-bookings"),
                &Req { template_id },
            )
            .await
            {
                Ok(_) => {
                    set_query.set(String::new());
                    on_done.run(());
                }
                Err(e) => set_err.set(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                )),
            }
            set_busy.set(false);
        });
    };

    view! {
        <div class="card spin-add" data-testid="spin-add">
            <h3>{move || i18n::t(lang.get(), "spin_add_title")}</h3>
            <div class="form-group">
                <label>{move || i18n::t(lang.get(), "search_card")}</label>
                <input
                    type="search"
                    class="form-control"
                    data-testid="spin-add-search"
                    placeholder=move || i18n::t(lang.get(), "search_card_placeholder")
                    prop:value=move || query.get()
                    on:input=move |ev| set_query.set(event_target_value(&ev))
                    autocomplete="off"
                />
            </div>
            <ul class="walkin-results">
                {move || {
                    let l = lang.get();
                    let booked = booked_user_ids.get();
                    let is_busy = busy.get();
                    results.get().into_iter().map(|user| {
                        let uid = user.id;
                        let label = hit_label(&user);
                        let already = booked.contains(&uid);
                        let actions = if already {
                            view! {
                                <span class="badge" data-testid=format!("spin-add-already-{uid}")>
                                    {i18n::t(l, "walkin_already_in_class")}
                                </span>
                            }.into_any()
                        } else {
                            let book_once = book_once.clone();
                            let once_btn = once_allowed.then(|| view! {
                                <button
                                    type="button"
                                    class="btn btn--primary btn--compact"
                                    data-testid=format!("spin-add-once-{uid}")
                                    disabled=is_busy
                                    on:click=move |_| book_once(uid)
                                >
                                    {i18n::t(l, "spin_add_once")}
                                </button>
                            });
                            view! {
                                <div class="spin-add__actions">
                                    {once_btn}
                                    <button
                                        type="button"
                                        class="btn btn--ghost btn--compact"
                                        data-testid=format!("spin-add-persistent-{uid}")
                                        disabled=is_busy
                                        on:click=move |_| book_persistent(uid)
                                    >
                                        {i18n::tf(l, "spin_add_persistent_format", &[&weekly_label])}
                                    </button>
                                </div>
                            }.into_any()
                        };
                        view! {
                            <li class="spin-add__row" data-testid=format!("spin-add-hit-{uid}")>
                                <span class="spin-add__name">{label}</span>
                                {actions}
                            </li>
                        }
                    }).collect::<Vec<_>>()
                }}
            </ul>
            {move || {
                let e = err.get();
                (!e.is_empty()).then(|| view! {
                    <div class="alert alert-error mt-1" data-testid="spin-add-error">{e}</div>
                })
            }}
        </div>
    }
}
