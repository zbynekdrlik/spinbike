//! Customer self-service dashboard at `/my/balance` — credit, monthly-pass
//! status, hold-to-open door button (#92), recent visits.
//!
//! The DoorButton state machine lives in `components::door_button`; this
//! page just renders the button alongside credit / pass / recent-visits.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::DoorButton;
use crate::i18n::{self, Lang, fmt_date_short, tf};

#[derive(Debug, Clone, serde::Deserialize)]
struct BalanceResp {
    #[allow(dead_code)]
    user_id: i64,
    name: String,
    credit: f64,
    #[allow(dead_code)]
    card_code: Option<String>,
    allow_self_entry: bool,
    monthly_pass_active_until: Option<String>,
    recent: Vec<RecentTx>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RecentTx {
    #[allow(dead_code)]
    id: i64,
    created_at: String,
    action: String,
    amount: f64,
    #[allow(dead_code)]
    valid_until: Option<String>,
    note: Option<String>,
}

#[component]
pub fn MyBalancePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<BalanceResp>);
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(String::new());

    let load = move || {
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<BalanceResp>("/api/my/balance").await {
                Ok(d) => {
                    set_data.set(Some(d));
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };
    Effect::new(move |_| load());

    let allowed_signal: Signal<bool> = Signal::derive(move || {
        data.with(|d| d.as_ref().map(|d| d.allow_self_entry).unwrap_or(false))
    });

    let on_door_success = move || {
        // Refresh balance — credit / pass / recent rows may have changed.
        spawn_local(async move {
            if let Ok(d) = api::get::<BalanceResp>("/api/my/balance").await {
                set_data.set(Some(d));
            }
        });
    };

    view! {
        <h1 class="page-title">
            {move || data.with(|d| d.as_ref().map(|d| tf(lang.get(), "my_balance_hello", &[&d.name]))
                .unwrap_or_else(|| i18n::t(lang.get(), "my_balance").to_string()))}
        </h1>

        {move || {
            let e = error.get();
            if !e.is_empty() {
                return view! { <div class="alert alert-error">{e}</div> }.into_any();
            }
            if loading.get() {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }

            data.with(|d| match d {
                None => view! { <div class="empty-state">{i18n::t(lang.get(), "unable_to_load")}</div> }.into_any(),
                Some(b) => {
                    let credit_val = format!("{:.2}", b.credit);
                    let pass_label = match &b.monthly_pass_active_until {
                        Some(ts) => match parse_pass_date(ts) {
                            Some(d) => tf(lang.get(), "monthly_pass_active_until", &[&fmt_date_short(d, lang.get())]),
                            None => tf(lang.get(), "monthly_pass_active_until", &[ts]),
                        },
                        None => i18n::t(lang.get(), "monthly_pass_not_active").to_string(),
                    };
                    let recent_rows = b.recent.clone();
                    let lang_now = lang.get();

                    view! {
                        <div class="card-credit" data-testid="my-balance-credit">
                            <div class="card-credit__label">{i18n::t(lang_now, "my_balance_credit")}</div>
                            <div class="card-credit__value">"\u{20ac} "{credit_val}</div>
                        </div>

                        <div class="card-pass" data-testid="my-balance-pass">
                            <div class="card-pass__label">{i18n::t(lang_now, "service_kind_monthly_pass")}</div>
                            <div class="card-pass__value">{pass_label}</div>
                        </div>

                        <DoorButton allowed=allowed_signal on_success=on_door_success />

                        <h2 class="recent-visits__heading">{i18n::t(lang_now, "my_balance_recent_visits")}</h2>
                        <ul class="recent-visits">
                            {recent_rows.into_iter().map(|t| {
                                let date_label = parse_visit_date(&t.created_at)
                                    .map(|d| fmt_date_short(d, lang_now))
                                    .unwrap_or_else(|| t.created_at.clone());
                                let amount_label = if t.amount.abs() < 0.005 {
                                    String::new()
                                } else {
                                    format!("\u{20ac}{:.2}", t.amount)
                                };
                                let note_view = match &t.note {
                                    Some(n) if !n.is_empty() => {
                                        let n = n.clone();
                                        view! { <span class="recent-visits__note">{n}</span> }.into_any()
                                    }
                                    _ => view! {}.into_any(),
                                };
                                view! {
                                    <li data-testid="recent-visit" class="recent-visits__row">
                                        <span class="recent-visits__date">{date_label}</span>
                                        <span class="recent-visits__action">{t.action.clone()}</span>
                                        <span class="recent-visits__amount">{amount_label}</span>
                                        {note_view}
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any()
                }
            })
        }}
    }
}

fn parse_pass_date(s: &str) -> Option<chrono::NaiveDate> {
    let trimmed = s.trim();
    let date_str = trimmed.split_whitespace().next().unwrap_or(trimmed);
    let date_str = date_str.split('T').next().unwrap_or(date_str);
    chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

fn parse_visit_date(s: &str) -> Option<chrono::NaiveDate> {
    parse_pass_date(s)
}
