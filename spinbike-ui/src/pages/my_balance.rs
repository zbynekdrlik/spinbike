//! Customer self-service dashboard at `/my/balance` — credit, monthly-pass
//! status, hold-to-open door button (#92), recent visits.
//!
//! The DoorButton state machine lives in `components::door_button`; this
//! page just renders the button alongside credit / pass / recent-visits.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use spinbike_core::reports::{EventKind, classify};

use crate::api;
use crate::components::{DoorButton, InstallPrompt};
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
    valid_until: Option<String>,
    note: Option<String>,
}

#[component]
pub fn MyBalancePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<BalanceResp>);
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(None::<api::CodedError>);

    let load = move || {
        set_loading.set(true);
        spawn_local(async move {
            // get_coded (#145): carries the server's `error_code` so the
            // banner below can localize it instead of showing raw English.
            match api::get_coded::<BalanceResp>("/api/my/balance").await {
                Ok(d) => {
                    set_data.set(Some(d));
                    set_error.set(None);
                }
                Err(e) => set_error.set(Some(e)),
            }
            set_loading.set(false);
        });
    };
    Effect::new(move |_| load());

    let allowed_signal: Signal<bool> = Signal::derive(move || {
        data.with(|d| d.as_ref().map(|d| d.allow_self_entry).unwrap_or(false))
    });

    let on_door_success = Callback::new(move |()| {
        // Refresh balance — credit / pass / recent rows may have changed.
        spawn_local(async move {
            if let Ok(d) = api::get::<BalanceResp>("/api/my/balance").await {
                set_data.set(Some(d));
            }
        });
    });

    view! {
        <h1 class="page-title">
            {move || data.with(|d| d.as_ref().map(|d| tf(lang.get(), "my_balance_hello", &[&d.name]))
                .unwrap_or_else(|| i18n::t(lang.get(), "my_balance").to_string()))}
        </h1>

        // Credit + pass cards — re-render reactively on data changes (no
        // remount of children; just text updates).
        <div class="card-credit" data-testid="my-balance-credit">
            <div class="card-credit__label">{move || i18n::t(lang.get(), "my_balance_credit")}</div>
            <div class="card-credit__value">
                "\u{20ac} "
                {move || data.with(|d| d.as_ref().map(|d| format!("{:.2}", d.credit)).unwrap_or_else(|| "—".into()))}
            </div>
        </div>

        <div class="card-pass" data-testid="my-balance-pass">
            <div class="card-pass__label">{move || i18n::t(lang.get(), "service_kind_monthly_pass")}</div>
            <div class="card-pass__value">
                {move || data.with(|d| {
                    let Some(b) = d.as_ref() else {
                        return String::new();
                    };
                    match &b.monthly_pass_active_until {
                        Some(ts) => match parse_pass_date(ts) {
                            Some(d) => tf(lang.get(), "monthly_pass_active_until", &[&fmt_date_short(d, lang.get())]),
                            None => tf(lang.get(), "monthly_pass_active_until", &[ts]),
                        },
                        None => i18n::t(lang.get(), "monthly_pass_not_active").to_string(),
                    }
                })}
            </div>
        </div>

        // DoorButton rendered ONCE at the top level. It reads `allowed`
        // reactively but its component instance is stable — `on_door_success`
        // refreshing the parent's `data` signal does NOT remount the button,
        // so the Success banner stays on screen until the auto-reset timer.
        <DoorButton allowed=allowed_signal on_success=on_door_success />

        // Install-to-home-screen nudge (#110) — renders nothing once
        // installed or on a browser offering neither install path.
        <InstallPrompt />

        // Loading spinner / error banner / recent visits — these update
        // reactively on data changes.
        {move || {
            if let Some(e) = error.get() {
                let msg = i18n::localize_api_error(lang.get(), e.code, &e.message);
                return view! { <div class="alert alert-error">{msg}</div> }.into_any();
            }
            if loading.get() && data.with(|d| d.is_none()) {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }
            data.with(|d| match d {
                None => view! { <div class="empty-state">{i18n::t(lang.get(), "unable_to_load")}</div> }.into_any(),
                Some(b) => {
                    let recent_rows = b.recent.clone();
                    let lang_now = lang.get();
                    view! {
                        <h2 class="recent-visits__heading">{i18n::t(lang_now, "my_balance_recent_movements")}</h2>
                        <ul class="recent-visits">
                            {recent_rows.into_iter().map(|t| {
                                let date_label = parse_visit_date(&t.created_at)
                                    .map(|d| fmt_date_short(d, lang_now))
                                    .unwrap_or_else(|| t.created_at.clone());

                                // Derive the movement kind from the SAME shared
                                // classifier the admin uses, so the customer sees
                                // the SAME Slovak labels instead of the raw DB token.
                                let valid_until = t.valid_until.as_deref().and_then(parse_pass_date);
                                let kind = classify(&t.action, t.amount, valid_until);
                                let action_label = i18n::t(lang_now, i18n::tx_label_key(kind)).to_string();

                                // Pass-sale rows show the expiry date, like the admin row.
                                let until_suffix = if matches!(kind, EventKind::PassSale) {
                                    valid_until
                                        .map(|d| format!(" \u{b7} {} {}", i18n::t(lang_now, "tx_until_short"), fmt_date_short(d, lang_now)))
                                        .unwrap_or_default()
                                } else {
                                    String::new()
                                };

                                // Signed + coloured amount (matches admin `{:+.2}`),
                                // so a top-up and a spend are distinguishable. €0 rows
                                // (visits) show no amount — the label carries the meaning.
                                let amount_label = if t.amount.abs() < 0.005 {
                                    String::new()
                                } else {
                                    format!("{:+.2}", t.amount)
                                };
                                let amount_class = if t.amount >= 0.0 {
                                    "list-row__amount list-row__amount--pos"
                                } else {
                                    "list-row__amount list-row__amount--neg"
                                };

                                // Door-entry notes are stored as English "door: Nth"
                                // (door.rs). Localize the DISPLAY only — the stored value
                                // stays intact (door.rs's `note LIKE 'door:%'` same-day
                                // count query AND the admin note view depend on it).
                                let sub_note = match &t.note {
                                    Some(n) if n.starts_with("door: ") => {
                                        let count: String = n["door: ".len()..]
                                            .chars()
                                            .take_while(|c| c.is_ascii_digit())
                                            .collect();
                                        if count.is_empty() {
                                            format!(" \u{b7} {n}")
                                        } else {
                                            format!(" \u{b7} {}", tf(lang_now, "door_note_reentry", &[&count]))
                                        }
                                    }
                                    Some(n) if !n.is_empty() => format!(" \u{b7} {n}"),
                                    _ => String::new(),
                                };

                                view! {
                                    <li data-testid="recent-visit" class="list-row">
                                        <div class="list-row__main">
                                            <div class="list-row__title">{action_label}{until_suffix}</div>
                                            <div class="list-row__sub">{date_label}{sub_note}</div>
                                        </div>
                                        <div class=amount_class>{amount_label}</div>
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
