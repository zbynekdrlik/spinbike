//! Per-card Overview tab — KPI grid + 12-month bar charts.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::stats::{MonthlyBucket, StatsResponse};

#[component]
pub fn OverviewTab(card_id: i64) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (stats, set_stats) = signal(None::<StatsResponse>);
    let (loading, set_loading) = signal(true);

    Effect::new(move |_| {
        spawn_local(async move {
            match api::get::<StatsResponse>(&format!("/api/cards/{card_id}/stats")).await {
                Ok(s) => {
                    set_stats.set(Some(s));
                    set_loading.set(false);
                }
                Err(_) => {
                    // Silent failure — UI shows nothing rather than spamming
                    // a global error banner for a side panel. Console errors
                    // (network, 5xx) still surface via Playwright assertions.
                    set_loading.set(false);
                }
            }
        });
    });

    view! {
        {move || {
            if loading.get() {
                return view! {
                    <div class="empty-state" data-testid="overview-loading">
                        {move || i18n::t(lang.get(), "overview_loading")}
                    </div>
                }.into_any();
            }
            let Some(s) = stats.get() else {
                return view! { <div data-testid="overview-empty"></div> }.into_any();
            };

            let l = lang.get();
            let row = |label_key: &'static str, visits: i64, topped: f64| {
                view! {
                    <tr>
                        <td>{move || i18n::t(lang.get(), label_key)}</td>
                        <td data-testid={format!("overview-visits-{label_key}")}>{visits}</td>
                        <td data-testid={format!("overview-topup-{label_key}")}>{format!("{:.2} \u{20ac}", topped)}</td>
                    </tr>
                }
            };

            let visits_max = s.monthly.iter().map(|b| b.visits).max().unwrap_or(0);
            let topup_max = s.monthly.iter().map(|b| b.topped_up_eur).fold(0.0_f64, f64::max);

            // Display newest first so the chart matches the History tab's
            // top-down recency. The API gives oldest→newest so reverse here.
            let mut visits_rows: Vec<&MonthlyBucket> = s.monthly.iter().collect();
            visits_rows.reverse();
            let topup_rows = visits_rows.clone();

            let visit_bar = |b: &MonthlyBucket| {
                let pct = if visits_max > 0 { b.visits as f64 / visits_max as f64 * 100.0 } else { 0.0 };
                let label = fmt_year_month(&b.year_month, l);
                let value = b.visits;
                view! {
                    <div class="stats-row" data-testid="stats-visits-row">
                        <span class="stats-row__label">{label}</span>
                        <div class="stats-row__bar-wrap">
                            <div class="stats-row__bar" style=format!("width: {:.1}%", pct)></div>
                        </div>
                        <span class="stats-row__value">{value}</span>
                    </div>
                }
            };
            let topup_bar = |b: &MonthlyBucket| {
                let pct = if topup_max > 0.0 { b.topped_up_eur / topup_max * 100.0 } else { 0.0 };
                let label = fmt_year_month(&b.year_month, l);
                let value = format!("{:.2} \u{20ac}", b.topped_up_eur);
                view! {
                    <div class="stats-row" data-testid="stats-topup-row">
                        <span class="stats-row__label">{label}</span>
                        <div class="stats-row__bar-wrap">
                            <div class="stats-row__bar" style=format!("width: {:.1}%", pct)></div>
                        </div>
                        <span class="stats-row__value">{value}</span>
                    </div>
                }
            };

            view! {
                <div data-testid="overview-tab">
                    <table class="stats-kpi">
                        <thead>
                            <tr>
                                <th></th>
                                <th>{move || i18n::t(lang.get(), "overview_col_visits")}</th>
                                <th>{move || i18n::t(lang.get(), "overview_col_topup")}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {row("overview_period_month", s.totals.this_month.visits, s.totals.this_month.topped_up_eur)}
                            {row("overview_period_year",  s.totals.this_year.visits,  s.totals.this_year.topped_up_eur)}
                            {row("overview_period_all",   s.totals.all_time.visits,   s.totals.all_time.topped_up_eur)}
                        </tbody>
                    </table>

                    <h3 class="stats-chart-title">{move || i18n::t(lang.get(), "overview_chart_visits")}</h3>
                    <div class="stats-chart" data-testid="stats-visits-chart">
                        {visits_rows.iter().map(|b| visit_bar(b)).collect::<Vec<_>>()}
                    </div>

                    <h3 class="stats-chart-title">{move || i18n::t(lang.get(), "overview_chart_topup")}</h3>
                    <div class="stats-chart" data-testid="stats-topup-chart">
                        {topup_rows.iter().map(|b| topup_bar(b)).collect::<Vec<_>>()}
                    </div>
                </div>
            }.into_any()
        }}
    }
}

/// "2026-05" → English "May'26", Slovak "Maj'26". Locale-friendly axis label
/// for the 12-bar charts. Falls back to the input string if it's malformed.
fn fmt_year_month(ym: &str, lang: Lang) -> String {
    let parts: Vec<&str> = ym.split('-').collect();
    if parts.len() != 2 {
        return ym.to_string();
    }
    let yr = parts[0];
    let yr_short = if yr.len() == 4 { &yr[2..4] } else { yr };
    let m: usize = match parts[1].parse() {
        Ok(n) if (1..=12).contains(&n) => n,
        _ => return ym.to_string(),
    };
    let names_en = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let names_sk = [
        "Jan", "Feb", "Mar", "Apr", "Maj", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dec",
    ];
    let name = match lang {
        Lang::En => names_en[m - 1],
        Lang::Sk => names_sk[m - 1],
    };
    format!("{}'{}", name, yr_short)
}

#[cfg(test)]
mod tests {
    use super::fmt_year_month;
    use crate::i18n::Lang;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn formats_english_may() {
        assert_eq!(fmt_year_month("2026-05", Lang::En), "May'26");
    }
    #[wasm_bindgen_test]
    fn formats_slovak_may() {
        assert_eq!(fmt_year_month("2026-05", Lang::Sk), "Maj'26");
    }
    #[wasm_bindgen_test]
    fn malformed_returns_input() {
        assert_eq!(fmt_year_month("not-a-date", Lang::En), "not-a-date");
    }
    #[wasm_bindgen_test]
    fn out_of_range_month_returns_input() {
        assert_eq!(fmt_year_month("2026-13", Lang::En), "2026-13");
    }
}
