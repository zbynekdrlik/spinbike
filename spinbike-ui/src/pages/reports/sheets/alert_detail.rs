use leptos::prelude::*;

use crate::components::Sheet;
use crate::i18n::{self, Lang};
use spinbike_core::reports::AlertsResponse;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AlertType {
    Expiring,
    LowCredit,
    Inactive,
}

#[component]
pub fn AlertDetailSheet(
    alert_type: AlertType,
    data: AlertsResponse,
    #[prop(into)] on_close: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let title_key = match alert_type {
        AlertType::Expiring => "alerts_expiring_passes",
        AlertType::LowCredit => "alerts_low_credit",
        AlertType::Inactive => "alerts_inactive",
    };
    let count = match alert_type {
        AlertType::Expiring => data.expiring_passes.len(),
        AlertType::LowCredit => data.low_credit.len(),
        AlertType::Inactive => data.inactive.len(),
    };
    let title = i18n::t(lang.get_untracked(), title_key).replace("{n}", &count.to_string());

    let rows: Vec<(String, String, Option<String>, String)> = match alert_type {
        AlertType::Expiring => data
            .expiring_passes
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.barcode.clone(),
                    Some(format!(
                        "{} · {} dní",
                        p.valid_until.format("%Y-%m-%d"),
                        p.days_left
                    )),
                    p.barcode.clone(),
                )
            })
            .collect(),
        AlertType::LowCredit => data
            .low_credit
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    c.barcode.clone(),
                    Some(format!("{:.2} \u{20ac}", c.credit)),
                    c.barcode.clone(),
                )
            })
            .collect(),
        AlertType::Inactive => data
            .inactive
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    c.barcode.clone(),
                    c.last_visit.clone(),
                    c.barcode.clone(),
                )
            })
            .collect(),
    };

    view! {
        <Sheet
            on_close=on_close
            title=title
            testid="sheet-alert-detail".to_string()
        >
            <div class="group" data-testid="alert-detail-list">
                {rows.into_iter().map(|(name, barcode, sub, q)| {
                    let q_owned = q.clone();
                    let on_click = move |_| {
                        if q_owned.is_empty() {
                            return;
                        }
                        if let Some(w) = web_sys::window() {
                            let encoded = q_owned
                                .replace('%', "%25")
                                .replace(' ', "%20")
                                .replace('&', "%26");
                            let _ = w.location().set_href(&format!("/staff?q={encoded}"));
                        }
                    };
                    let detail_text = sub
                        .map(|s| format!(" · {s}"))
                        .unwrap_or_default();
                    view! {
                        <div class="list-row list-row--interactive"
                             data-testid="alert-detail-row"
                             on:click=on_click>
                            <div class="list-row__main">
                                <div class="list-row__title">{name}</div>
                                <div class="list-row__sub">
                                    <code>{barcode}</code>
                                    {detail_text}
                                </div>
                            </div>
                        </div>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </Sheet>
    }
}
