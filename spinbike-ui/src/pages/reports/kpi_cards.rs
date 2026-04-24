use leptos::prelude::*;
use spinbike_core::reports::KpiSummary;

use crate::i18n::{self, Lang};

#[component]
pub fn KpiCards(kpi: ReadSignal<KpiSummary>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    view! {
        <div class="kpi-grid" data-testid="kpi-grid">
            <div class="kpi-card" data-testid="kpi-revenue">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_revenue")}</div>
                <div class="kpi-card__value">{move || format!("{:.2} \u{20ac}", kpi.get().revenue_eur)}</div>
            </div>
            <div class="kpi-card" data-testid="kpi-attendance">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_attendance")}</div>
                <div class="kpi-card__value">{move || format!("{}", kpi.get().attendance)}</div>
            </div>
            <div class="kpi-card" data-testid="kpi-passes">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_passes")}</div>
                <div class="kpi-card__value">{move || format!("{}", kpi.get().passes_sold)}</div>
            </div>
            <div class="kpi-card" data-testid="kpi-cash-in">
                <div class="kpi-card__label">{move || i18n::t(lang.get(), "kpi_cash_in")}</div>
                <div class="kpi-card__value">{move || format!("{:.2} \u{20ac}", kpi.get().cash_in_eur)}</div>
            </div>
        </div>
    }
}
