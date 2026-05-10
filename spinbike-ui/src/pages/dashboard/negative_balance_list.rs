//! Empty-state list of users with `credit < 0`, rendered on the Desk under
//! the search box when no user is selected and the search box is empty.
//!
//! Source of truth: `GET /api/users/negative-balance`. Refetches whenever
//! the parent's `txn_refresh` signal increments.

use chrono::NaiveDate;
use leptos::logging;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::dashboard::{CardInfo, CardPass};
use crate::relative_date::{relative, today_local};
use crate::util::RequestId;

#[derive(Clone, Debug, Deserialize)]
pub struct NegativeBalanceUser {
    pub id: i64,
    pub name: String,
    pub card_code: Option<String>,
    pub credit: f64,
    pub blocked: bool,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
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
    let (rows, set_rows) = signal::<Vec<NegativeBalanceUser>>(Vec::new());

    let req_id = RequestId::new();
    Effect::new(move |_| {
        let _ = txn_refresh.get(); // reactive dependency
        let token = req_id.next();
        spawn_local(async move {
            let result =
                api::get::<Vec<NegativeBalanceUser>>("/api/users/negative-balance").await;
            if !token.is_latest() {
                return; // stale — a newer trigger run superseded this fetch (#66)
            }
            match result {
                Ok(fetched) => set_rows.set(fetched),
                Err(e) => {
                    // Hide the alert list rather than show stale data, but
                    // log the underlying error so a 3am debugging session can
                    // see why no negative-balance customers ever appeared.
                    // See #64 + comprehensive-logging.md.
                    logging::warn!("negative-balance fetch failed: {e}");
                    set_rows.set(Vec::new());
                }
            }
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
            let never_label = i18n::t(lang_now, "never_label").to_string();
            let today = today_local();
            let suffix = summary_suffix(&rows);

            let items = rows.into_iter().map(|r| {
                let name = super::helpers::user_display_name(
                    &r.name,
                    r.company.as_deref(),
                    r.card_code.as_deref(),
                );
                let credit = format!("{:.2} €", r.credit);
                let last_visit = format_optional_date(&r.last_visit_at, today, lang_now, &never_label);
                let meta = meta_inline(&last_visit_label, &last_visit);
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
                        <div class="negative-balance-row__label">
                            {name}
                            <span class="negative-balance-row__meta-inline">{meta}</span>
                        </div>
                        <div class="negative-balance-row__credit credit-negative">{credit}</div>
                    </div>
                }
            }).collect_view();

            view! {
                <div class="card mb-2 negative-balance-list" data-testid="negative-balance-list">
                    <div class="card__body">
                        <h3 class="negative-balance-list__heading">{heading}{suffix}</h3>
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
                Ok(d) => relative(d, today, lang),
                Err(_) => never_label.to_string(),
            }
        }
    }
}

/// Inline meta suffix appended after the user's name in a negative-balance row.
/// Format: " ({label}: {value})" — leading space, parens, colon. Caller passes
/// the localized label (e.g. "Posledna navsteva") and pre-formatted value
/// (e.g. "vcera", "2 dni", "nikdy").
fn meta_inline(label: &str, value: &str) -> String {
    format!(" ({label}: {value})")
}

/// Heading suffix for the negative-balance list: `"  ·  {count}  ·  {sum} €"`.
/// Separator is U+00B7 with two spaces on each side. Sum uses ASCII hyphen
/// (matches the per-row credit formatting). Caller short-circuits the empty
/// case before this is ever invoked.
fn summary_suffix(rows: &[NegativeBalanceUser]) -> String {
    let count = rows.len();
    let sum: f64 = rows.iter().map(|r| r.credit).sum();
    format!("  ·  {count}  ·  {sum:.2} €")
}

/// Promote a `NegativeBalanceUser` into the parent `CardInfo` so clicking a
/// row opens the full action panel with the same fidelity as a search-result
/// click — including the active monthly pass when present.
fn neg_to_card_info(c: &NegativeBalanceUser) -> CardInfo {
    CardInfo {
        id: c.id,
        name: c.name.clone(),
        card_code: c.card_code.clone(),
        blocked: c.blocked,
        credit: c.credit,
        company: c.company.clone(),
        pass: c.pass.clone(),
        last_visit_at: c.last_visit_at.clone(),
        // Fields not returned by the negative-balance endpoint — neutral defaults.
        // `allow_debit` and `phone` aren't read by the action panel's
        // monthly-pass header or the visit-log button, so defaults are safe.
        allow_debit: false,
        phone: None,
        email: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    fn neg_user(credit: f64) -> NegativeBalanceUser {
        NegativeBalanceUser {
            id: 0,
            name: String::new(),
            card_code: None,
            credit,
            blocked: false,
            company: None,
            last_visit_at: None,
            pass: None,
        }
    }

    #[wasm_bindgen_test]
    fn summary_suffix_three_users() {
        let rows = vec![neg_user(-1.50), neg_user(-3.10), neg_user(-7.80)];
        assert_eq!(summary_suffix(&rows), "  ·  3  ·  -12.40 €");
    }

    #[wasm_bindgen_test]
    fn summary_suffix_single_user() {
        let rows = vec![neg_user(-0.50)];
        assert_eq!(summary_suffix(&rows), "  ·  1  ·  -0.50 €");
    }

    #[wasm_bindgen_test]
    fn meta_inline_typical() {
        assert_eq!(meta_inline("Posledna navsteva", "vcera"), " (Posledna navsteva: vcera)");
    }

    #[wasm_bindgen_test]
    fn meta_inline_never_label() {
        assert_eq!(meta_inline("Last visit", "never"), " (Last visit: never)");
    }
}
