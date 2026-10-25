//! Class detail of the staff spinning screen (#385): header with the class
//! facts + "Zrusit hodinu", the participants TABLE (no chips — owner
//! decision), per-row "Odhlasit" / "Ukoncit natrvalo", and "Pridat" below.
//!
//! Every cancel is confirmed inline first (a confirm panel in the detail, no
//! `window.confirm` — it behaves inconsistently in an installed iPhone PWA).
//! The "Hostia" +/- stepper (#389) writes directly, one guest per tap — it is
//! a count the desk adjusts as people arrive, and the server answers every
//! tap with the exact charge/refund it made. No write is ever swallowed: a
//! failure shows the server's (localized) error in the panel, success
//! reloads grid + table (`admin-ui-error-handling.md`).

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::guest_stepper::{GuestStepper, put_guests};
use crate::i18n::{self, DAY_KEYS, Lang};
use crate::pages::schedule::ClassSlot;
use spinbike_core::booking::MAX_GUESTS;

use super::add_person::AddPerson;
use super::model::{self, Participant};

/// A write waiting for the staff member's confirmation.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    CancelClass,
    /// Staff cancel of one booking (refunds a charged one).
    Unbook {
        booking_id: i64,
        name: String,
        refund: Option<f64>,
    },
    /// End the weekly slot — also cancels the future bookings it created.
    EndPersistent {
        user_id: i64,
        name: String,
    },
}

impl Pending {
    /// The confirm panel's question for this write.
    fn question(&self, l: Lang) -> String {
        match self {
            Pending::CancelClass => i18n::t(l, "spin_confirm_cancel_class").to_string(),
            Pending::Unbook {
                name,
                refund: Some(amount),
                ..
            } => i18n::tf(
                l,
                "spin_confirm_unbook_refund",
                &[name, &format!("{amount:.2}")],
            ),
            Pending::Unbook {
                name, refund: None, ..
            } => i18n::tf(l, "spin_confirm_unbook", &[name]),
            Pending::EndPersistent { name, .. } => {
                i18n::tf(l, "spin_confirm_end_persistent", &[name])
            }
        }
    }
}

/// Load one occurrence's participants (sorted by surname) into the table,
/// or its localized error; `loaded` flips either way.
fn load_participants(
    template_id: i64,
    date: String,
    lang: ReadSignal<Lang>,
    set_participants: WriteSignal<Vec<Participant>>,
    set_err: WriteSignal<String>,
    set_loaded: WriteSignal<bool>,
) {
    spawn_local(async move {
        match api::get_coded::<Vec<Participant>>(&format!(
            "/api/classes/{template_id}/{date}/participants"
        ))
        .await
        {
            Ok(list) => set_participants.set(model::sorted_by_surname(list)),
            Err(e) => set_err.set(i18n::localize_api_error(
                lang.get_untracked(),
                e.code,
                &e.message,
            )),
        }
        set_loaded.set(true);
    });
}

/// "Po 18:00" for the slot of `slot`.
fn slot_label(slot: &ClassSlot, lang: Lang) -> String {
    let day = i18n::t(lang, DAY_KEYS[slot.weekday.clamp(0, 6) as usize]);
    format!("{day} {}", slot.start_time)
}

/// "Po 20.10.2026 18:00 · Stopa"
fn title(slot: &ClassSlot, lang: Lang) -> String {
    let date = crate::dates::parse_server_date(&slot.date)
        .map(|d| i18n::fmt_date(d, lang))
        .unwrap_or_else(|| slot.date.clone());
    let base = format!(
        "{} {date} {}",
        i18n::t(lang, DAY_KEYS[slot.weekday.clamp(0, 6) as usize]),
        slot.start_time
    );
    match slot.instructor_name.as_deref() {
        Some(i) if !i.trim().is_empty() => format!("{base} \u{00b7} {i}"),
        _ => base,
    }
}

#[component]
pub fn ClassDetail(occurrence: ClassSlot, #[prop(into)] on_changed: Callback<()>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let template_id = occurrence.template_id;
    let date = occurrence.date.clone();
    let (participants, set_participants) = signal(Vec::<Participant>::new());
    let (loaded, set_loaded) = signal(false);
    let (err, set_err) = signal(String::new());
    let (pending, set_pending) = signal(None::<Pending>);
    let (busy, set_busy) = signal(false);

    load_participants(
        template_id,
        date.clone(),
        lang,
        set_participants,
        set_err,
        set_loaded,
    );

    // Run the confirmed write; success reloads the page data, failure stays
    // visible in the panel.
    let run_pending = {
        let date = date.clone();
        move |_ev: leptos::ev::MouseEvent| {
            let Some(action) = pending.get_untracked() else {
                return;
            };
            let date = date.clone();
            set_busy.set(true);
            set_err.set(String::new());
            spawn_local(async move {
                let res: Result<(), String> = match action {
                    Pending::CancelClass => {
                        #[derive(serde::Serialize)]
                        struct Req {
                            template_id: i64,
                            date: String,
                            reason: Option<String>,
                        }
                        // 204 No Content -> post_no_content (never `post`).
                        api::post_no_content(
                            "/api/admin/cancel-class",
                            &Req {
                                template_id,
                                date,
                                reason: None,
                            },
                        )
                        .await
                        .map_err(|e| i18n::tf(lang.get_untracked(), "error_format", &[&e]))
                    }
                    Pending::Unbook { booking_id, .. } => {
                        api::delete_coded(&format!("/api/bookings/{booking_id}"))
                            .await
                            .map_err(|e| {
                                i18n::localize_api_error(lang.get_untracked(), e.code, &e.message)
                            })
                    }
                    Pending::EndPersistent { user_id, .. } => api::delete_coded(&format!(
                        "/api/users/{user_id}/persistent-bookings/{template_id}"
                    ))
                    .await
                    .map_err(|e| {
                        i18n::localize_api_error(lang.get_untracked(), e.code, &e.message)
                    }),
                };
                set_busy.set(false);
                match res {
                    Ok(()) => {
                        set_pending.set(None);
                        on_changed.run(());
                    }
                    Err(msg) => {
                        set_pending.set(None);
                        set_err.set(msg);
                    }
                }
            });
        }
    };

    // #389: set a booking's guest count (staff: any time; after the 09:00
    // charge the server charges an added guest / refunds a removed one).
    let change_guests = move |booking_id: i64, guests: i64| {
        set_busy.set(true);
        set_err.set(String::new());
        spawn_local(async move {
            let res = put_guests(booking_id, guests).await;
            set_busy.set(false);
            match res {
                Ok(_) => on_changed.run(()),
                Err(e) => set_err.set(i18n::localize_api_error(
                    lang.get_untracked(),
                    e.code,
                    &e.message,
                )),
            }
        });
    };

    let header_title = {
        let occurrence = occurrence.clone();
        move || title(&occurrence, lang.get())
    };
    let booked = occurrence.booked;
    let capacity = occurrence.capacity;
    let cancelled = occurrence.cancelled;
    let once_allowed = !occurrence.cancelled && !occurrence.started;
    let seats_full = booked >= capacity;
    // Not reactive on purpose: the detail remounts on every reload anyway.
    let add_label = slot_label(&occurrence, lang.get_untracked());

    view! {
        <section class="card spin-detail" data-testid="spin-detail" data-template-id=template_id.to_string() data-date=date.clone()>
            <div class="spin-detail__head">
                <div>
                    <h2 class="spin-detail__title" data-testid="spin-detail-title">{header_title}</h2>
                    <div class="spin-detail__count" data-testid="spin-detail-count">
                        {format!("{booked}/{capacity}")}
                        {cancelled.then(|| view! {
                            <span class="badge badge--cancelled" data-testid="spin-detail-cancelled">
                                {move || i18n::t(lang.get(), "cancelled")}
                            </span>
                        })}
                    </div>
                </div>
                {(!cancelled).then(|| view! {
                    <button
                        type="button"
                        class="btn btn--danger btn--compact"
                        data-testid="spin-cancel-class"
                        on:click=move |_| set_pending.set(Some(Pending::CancelClass))
                    >
                        {move || i18n::t(lang.get(), "cancel_class")}
                    </button>
                })}
            </div>

            {move || pending.get().map(|p| {
                let l = lang.get();
                let question = p.question(l);
                let run_pending = run_pending.clone();
                view! {
                    <div class="alert spin-confirm" role="alertdialog" data-testid="spin-confirm">
                        <p data-testid="spin-confirm-text">{question}</p>
                        <div class="spin-confirm__actions">
                            <button
                                type="button"
                                class="btn btn--danger btn--compact"
                                data-testid="spin-confirm-yes"
                                disabled=move || busy.get()
                                on:click=run_pending
                            >
                                {i18n::t(l, "spin_confirm_yes")}
                            </button>
                            <button
                                type="button"
                                class="btn btn--outline btn--compact"
                                data-testid="spin-confirm-no"
                                disabled=move || busy.get()
                                on:click=move |_| set_pending.set(None)
                            >
                                {i18n::t(l, "spin_confirm_no")}
                            </button>
                        </div>
                    </div>
                }
            })}

            {move || {
                let e = err.get();
                (!e.is_empty()).then(|| view! {
                    <div class="alert alert-error" data-testid="spin-detail-error">{e}</div>
                })
            }}

            <div class="table-scroll">
                <table class="spin-table" data-testid="spin-participants">
                    <thead>
                        <tr>
                            <th>"#"</th>
                            <th>{move || i18n::t(lang.get(), "name")}</th>
                            <th>{move || i18n::t(lang.get(), "phone")}</th>
                            <th>{move || i18n::t(lang.get(), "spin_col_credit")}</th>
                            <th>{move || i18n::t(lang.get(), "spin_col_guests")}</th>
                            <th>{move || i18n::t(lang.get(), "spin_col_kind")}</th>
                            <th>{move || i18n::t(lang.get(), "spin_col_state")}</th>
                            <th>{move || i18n::t(lang.get(), "spin_col_actions")}</th>
                        </tr>
                    </thead>
                    <tbody>
                        {move || {
                            let l = lang.get();
                            let list = participants.get();
                            if list.is_empty() {
                                let key = if loaded.get() { "spin_no_participants" } else { "spin_loading" };
                                return view! {
                                    <tr data-testid="spin-participants-empty">
                                        <td colspan="8" class="text-muted">{i18n::t(l, key)}</td>
                                    </tr>
                                }.into_any();
                            }
                            list.into_iter().enumerate().map(|(i, p)| {
                                let bid = p.booking_id;
                                let uid = p.user_id;
                                let href = model::customer_href(&p);
                                let persistent = p.is_persistent();
                                let state = model::charge_state(p.charged, p.charged_amount);
                                let credit_class = if p.credit < 0.0 { "credit-negative" } else { "" };
                                let refund = p.refund_on_cancel();
                                let guests = p.guests;
                                let plus_blocked = cancelled || seats_full || guests >= MAX_GUESTS;
                                let name_unbook = p.user_name.clone();
                                let name_end = p.user_name.clone();
                                let phone_cell = match p.phone.clone().filter(|ph| !ph.trim().is_empty()) {
                                    Some(ph) => {
                                        let tel = format!("tel:{}", ph.replace(' ', ""));
                                        view! {
                                            <a href=tel data-testid=format!("spin-phone-{bid}")>{ph}</a>
                                        }.into_any()
                                    }
                                    None => view! { <span class="text-muted">"\u{2014}"</span> }.into_any(),
                                };
                                let unbook_key = if refund.is_some() { "cancel_booking_refund" } else { "spin_unbook" };
                                view! {
                                    <tr data-testid=format!("spin-row-{bid}") data-user-id=uid.to_string() data-source=p.source.clone()>
                                        <td>{i + 1}</td>
                                        <td>
                                            <a href=href rel="external" data-testid=format!("spin-name-{bid}")>{p.user_name.clone()}</a>
                                        </td>
                                        <td>{phone_cell}</td>
                                        <td class=credit_class data-testid=format!("spin-credit-{bid}")>
                                            {format!("{:.2} \u{20ac}", p.credit)}
                                        </td>
                                        <td>
                                            <GuestStepper
                                                prefix="spin-guests"
                                                key=bid.to_string()
                                                guests=guests
                                                count_text=Signal::derive(move || guests.to_string())
                                                plus_text=Signal::derive(|| "+".to_string())
                                                busy=busy
                                                minus_blocked=cancelled
                                                plus_blocked=plus_blocked
                                                on_set=move |n: i64| change_guests(bid, n)
                                            />
                                        </td>
                                        <td data-testid=format!("spin-kind-{bid}")>
                                            {i18n::t(l, if persistent { "spin_kind_persistent" } else { "spin_kind_once" })}
                                        </td>
                                        <td data-testid=format!("spin-state-{bid}")>{i18n::t(l, state.i18n_key())}</td>
                                        <td class="spin-table__actions">
                                            <button
                                                type="button"
                                                class="btn btn--outline btn--compact"
                                                data-testid=format!("spin-unbook-{bid}")
                                                on:click=move |_| set_pending.set(Some(Pending::Unbook {
                                                    booking_id: bid,
                                                    name: name_unbook.clone(),
                                                    refund,
                                                }))
                                            >
                                                {i18n::t(l, unbook_key)}
                                            </button>
                                            {persistent.then(|| view! {
                                                <button
                                                    type="button"
                                                    class="btn btn--outline btn--compact"
                                                    data-testid=format!("spin-end-persistent-{bid}")
                                                    on:click=move |_| set_pending.set(Some(Pending::EndPersistent {
                                                        user_id: uid,
                                                        name: name_end.clone(),
                                                    }))
                                                >
                                                    {i18n::t(l, "spin_end_persistent")}
                                                </button>
                                            })}
                                        </td>
                                    </tr>
                                }.into_any()
                            }).collect::<Vec<_>>().into_any()
                        }}
                    </tbody>
                </table>
            </div>

            <AddPerson
                template_id=template_id
                date=date.clone()
                weekly_label=add_label
                once_allowed=once_allowed
                booked_user_ids=Signal::derive(move || {
                    participants.get().iter().map(|p| p.user_id).collect::<Vec<_>>()
                })
                on_done=on_changed
            />
        </section>
    }
}
