//! Empty-state list of cards with `credit < 0`, rendered on the Desk under
//! the search box when no card is selected and the search box is empty.
//!
//! Source of truth: `GET /api/cards/negative-balance`. Refetches whenever
//! the parent's `txn_refresh` signal increments.

use chrono::NaiveDate;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::dashboard::{CardInfo, CardPass};
use crate::relative_date::format_last_visit;

#[derive(Clone, Debug, Deserialize)]
pub struct NegativeBalanceCard {
    pub id: i64,
    pub barcode: String,
    pub credit: f64,
    pub blocked: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub last_payment_at: Option<String>,
    #[serde(default)]
    pub pass: Option<CardPass>,
}

#[component]
pub fn NegativeBalanceList(
    txn_refresh: ReadSignal<u32>,
    lang: ReadSignal<Lang>,
    on_pick: Callback<CardInfo>,
) -> impl IntoView {
    // Mirrors the existing pattern in `transactions_list.rs`: an Effect that
    // depends on `txn_refresh` and spawns a local future to fetch the data.
    // We avoid `Resource::new` because the JS fetch future is not `Send`
    // (it holds `Rc<RefCell<...>>` from gloo-net), and Leptos 0.7's
    // `Resource::new` requires `Send`.
    let (rows, set_rows) = signal::<Vec<NegativeBalanceCard>>(Vec::new());

    Effect::new(move |_| {
        let _ = txn_refresh.get(); // reactive dependency
        spawn_local(async move {
            // Errors are swallowed: an alert list that fails to load has no
            // useful UI fallback, and we'd rather hide it than show noise.
            let fetched = api::get::<Vec<NegativeBalanceCard>>("/api/cards/negative-balance")
                .await
                .unwrap_or_default();
            set_rows.set(fetched);
        });
    });

    view! {
        {move || {
            let rows = rows.get();
            if rows.is_empty() {
                return view! { <span></span> }.into_any();
            }
            let lang_now = lang.get();
            let heading = i18n::t(lang_now, "negative_balance_heading").to_string();
            let last_visit_label = i18n::t(lang_now, "last_visit_label").to_string();
            let last_payment_label = i18n::t(lang_now, "last_payment_label").to_string();
            let never_label = i18n::t(lang_now, "never_label").to_string();
            let today = today_local();

            let items = rows.into_iter().map(|r| {
                let name = super::helpers::full_name_or_fallback(
                    r.first_name.as_deref(),
                    r.last_name.as_deref(),
                    r.company.as_deref(),
                    &r.barcode,
                );
                let credit = format!("{:.2} €", r.credit);
                let last_visit = format_optional_date(&r.last_visit_at, today, lang_now, &never_label);
                let last_payment = format_optional_date(&r.last_payment_at, today, lang_now, &never_label);
                let lv = last_visit_label.clone();
                let lp = last_payment_label.clone();
                let card_for_pick = neg_to_card_info(&r);
                view! {
                    <div
                        class="negative-balance-row"
                        data-testid="negative-balance-row"
                        on:click={
                            let card = card_for_pick.clone();
                            move |_| on_pick.run(card.clone())
                        }
                    >
                        <div class="negative-balance-row__main">
                            <div class="negative-balance-row__name">{name}</div>
                            <div class="negative-balance-row__meta">
                                {format!("{lv}: {last_visit}")}
                                {" · "}
                                {format!("{lp}: {last_payment}")}
                            </div>
                        </div>
                        <div class="negative-balance-row__credit credit-negative">{credit}</div>
                    </div>
                }
            }).collect_view();

            view! {
                <div class="card mb-2 negative-balance-list" data-testid="negative-balance-list">
                    <div class="card__body">
                        <h3 class="negative-balance-list__heading">{heading}</h3>
                        {items}
                    </div>
                </div>
            }.into_any()
        }}
    }
}

fn format_optional_date(
    raw: &Option<String>,
    today: NaiveDate,
    lang: Lang,
    never_label: &str,
) -> String {
    match raw {
        None => never_label.to_string(),
        Some(s) => {
            // SQLite literal: "YYYY-MM-DD HH:MM:SS". Slice the leading 10
            // characters defensively via `get(..10)` — the API only returns
            // ASCII timestamps today, but a byte-index slice would panic on
            // a multi-byte char boundary if that ever changes.
            let date_str = s.get(..10).unwrap_or(s.as_str());
            match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                Ok(d) => format_last_visit(d, today, lang),
                Err(_) => never_label.to_string(),
            }
        }
    }
}

fn today_local() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// Promote a `NegativeBalanceCard` into the parent `CardInfo` so clicking a
/// row opens the full action panel with the same fidelity as a search-result
/// click — including the active monthly pass when present.
fn neg_to_card_info(c: &NegativeBalanceCard) -> CardInfo {
    CardInfo {
        id: c.id,
        barcode: c.barcode.clone(),
        blocked: c.blocked,
        credit: c.credit,
        first_name: c.first_name.clone(),
        last_name: c.last_name.clone(),
        company: c.company.clone(),
        pass: c.pass.clone(),
        last_visit_at: c.last_visit_at.clone(),
        // Fields not returned by the negative-balance endpoint — neutral defaults.
        // `user_id`, `allow_debit`, and `phone` aren't read by the action
        // panel's monthly-pass header or the visit-log button, so the
        // defaults are safe.
        user_id: None,
        allow_debit: false,
        phone: None,
    }
}
