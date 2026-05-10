use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::reports::{KpiSummary, ReportEvent, ReportResponse};

mod activity_feed;
mod filters_bar;
mod kpi_cards;
mod sheets;
mod users_by_movement;

pub use activity_feed::ActivityFeed;
pub use filters_bar::{FiltersBar, FiltersState};
pub use kpi_cards::KpiCards;
use sheets::calendar_picker::CalendarPickerSheet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsersTab {
    DailyActivity,
    Users,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeMode {
    Day,
    Week,
    Month,
}

#[component]
pub fn ReportsPage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    let (tab, set_tab) = signal(UsersTab::DailyActivity);

    let (anchor, set_anchor) = signal(crate::relative_date::today_local());
    let (mode, set_mode) = signal(RangeMode::Day);
    let (filters, set_filters) = signal(FiltersState::default());

    let (kpi, set_kpi) = signal(KpiSummary {
        revenue_eur: 0.0,
        attendance: 0,
        passes_sold: 0,
        cash_in_eur: 0.0,
    });
    let (events, set_events) = signal::<Vec<ReportEvent>>(Vec::new());
    let (loading, set_loading) = signal(true);
    let (has_more, set_has_more) = signal(false);
    let (error, set_error) = signal(String::new());

    let (show_picker, set_show_picker) = signal(false);

    Effect::new(move |_| {
        let a = anchor.get();
        let m = mode.get();
        set_loading.set(true);
        set_error.set(String::new());
        let url = match m {
            RangeMode::Day => format!("/api/reports/day?date={}", a.format("%Y-%m-%d")),
            RangeMode::Week => {
                let from = a - chrono::Duration::days(6);
                format!(
                    "/api/reports/range?from={}&to={}",
                    from.format("%Y-%m-%d"),
                    a.format("%Y-%m-%d")
                )
            }
            RangeMode::Month => {
                let from = a - chrono::Duration::days(29);
                format!(
                    "/api/reports/range?from={}&to={}",
                    from.format("%Y-%m-%d"),
                    a.format("%Y-%m-%d")
                )
            }
        };
        spawn_local(async move {
            match api::get::<ReportResponse>(&url).await {
                Ok(r) => {
                    set_kpi.set(r.kpi);
                    set_events.set(r.events);
                    set_has_more.set(r.has_more);
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    });

    view! {
        <div class="reports-page" data-testid="reports-page">

            <div class="seg" role="tablist" data-testid="reports-tabs">
                <button class="seg__item" data-testid="reports-tab-daily"
                        aria-selected=move || (tab.get() == UsersTab::DailyActivity).to_string()
                        on:click=move |_| set_tab.set(UsersTab::DailyActivity)>
                    {move || i18n::t(lang.get(), "reports_tab_daily")}
                </button>
                <button class="seg__item" data-testid="reports-tab-users"
                        aria-selected=move || (tab.get() == UsersTab::Users).to_string()
                        on:click=move |_| set_tab.set(UsersTab::Users)>
                    {move || i18n::t(lang.get(), "reports_tab_users")}
                </button>
            </div>

            {move || if tab.get() == UsersTab::DailyActivity {
                view! {
                    <>
                    <div class="reports-date-strip">
                        // Quick toggles — Yesterday / Today / Week / Month
                        <div class="seg" role="tablist">
                            <button class="seg__item" data-testid="quick-yesterday"
                                    aria-selected=move || {
                                        let y = crate::relative_date::today_local() - chrono::Duration::days(1);
                                        (mode.get() == RangeMode::Day && anchor.get() == y).to_string()
                                    }
                                    on:click=move |_| {
                                        set_mode.set(RangeMode::Day);
                                        set_anchor.set(crate::relative_date::today_local() - chrono::Duration::days(1));
                                    }>
                                {move || i18n::t(lang.get(), "reports_yesterday")}
                            </button>
                            <button class="seg__item" data-testid="quick-today"
                                    aria-selected=move || {
                                        let t = crate::relative_date::today_local();
                                        (mode.get() == RangeMode::Day && anchor.get() == t).to_string()
                                    }
                                    on:click=move |_| {
                                        set_mode.set(RangeMode::Day);
                                        set_anchor.set(crate::relative_date::today_local());
                                    }>
                                {move || i18n::t(lang.get(), "reports_today")}
                            </button>
                            <button class="seg__item" data-testid="range-week"
                                    aria-selected=move || (mode.get() == RangeMode::Week).to_string()
                                    on:click=move |_| {
                                        set_mode.set(RangeMode::Week);
                                        set_anchor.set(crate::relative_date::today_local());
                                    }>
                                {move || i18n::t(lang.get(), "reports_week")}
                            </button>
                            <button class="seg__item" data-testid="range-month"
                                    aria-selected=move || (mode.get() == RangeMode::Month).to_string()
                                    on:click=move |_| {
                                        set_mode.set(RangeMode::Month);
                                        set_anchor.set(crate::relative_date::today_local());
                                    }>
                                {move || i18n::t(lang.get(), "reports_month")}
                            </button>
                        </div>

                        // Fine-grained day picker for any specific date
                        <div class="seg" role="tablist">
                            <button class="seg__item" data-testid="date-prev"
                                    on:click=move |_| {
                                        set_mode.set(RangeMode::Day);
                                        set_anchor.update(|d| *d = *d - chrono::Duration::days(1));
                                    }>
                                "‹"
                            </button>
                            <button class="seg__item" data-testid="date-label"
                                    on:click=move |_| set_show_picker.set(true)>
                                {move || i18n::fmt_date(anchor.get(), lang.get())}
                            </button>
                            <button class="seg__item" data-testid="date-next"
                                    on:click=move |_| {
                                        set_mode.set(RangeMode::Day);
                                        set_anchor.update(|d| *d = *d + chrono::Duration::days(1));
                                    }>
                                "›"
                            </button>
                        </div>
                    </div>

                    {move || if !error.get().is_empty() {
                        view! { <div class="alert alert-error" data-testid="reports-error">{move || error.get()}</div> }.into_any()
                    } else { ().into_any() }}

                    <KpiCards kpi=kpi />
                    <FiltersBar filters=filters set_filters=set_filters />
                    <ActivityFeed events=events loading=loading has_more=has_more filters=filters anchor=anchor mode=mode set_events=set_events set_has_more=set_has_more />

                    {move || if show_picker.get() {
                        view! {
                            <CalendarPickerSheet
                                current=anchor
                                on_close=Callback::new(move |_| set_show_picker.set(false))
                                on_pick=Callback::new(move |d: chrono::NaiveDate| {
                                    set_anchor.set(d);
                                    set_show_picker.set(false);
                                })
                            />
                        }.into_any()
                    } else { ().into_any() }}
                    </>
                }.into_any()
            } else {
                view! {
                    <users_by_movement::UsersByMovement />
                }.into_any()
            }}
        </div>
    }
}
