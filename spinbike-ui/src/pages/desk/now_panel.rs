use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::reports::{NowResponse, RosterEntry, RosterStatus};

const LS_KEY: &str = "desk_now_collapsed";

fn load_collapsed() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|ls| ls.get_item(LS_KEY).ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn save_collapsed(v: bool) {
    if let Some(ls) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = ls.set_item(LS_KEY, if v { "1" } else { "0" });
    }
}

#[component]
pub fn NowPanel() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<NowResponse>);
    let (collapsed, set_collapsed) = signal(load_collapsed());

    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(n) = api::get::<NowResponse>("/api/reports/now").await {
                set_data.set(Some(n));
            }
        });
    });

    view! {
        <div class="now-panel" data-testid="now-panel">
            {move || {
                let Some(n) = data.get() else {
                    return view! { <div class="now-panel__head"><div class="now-panel__title">"..."</div></div> }.into_any();
                };
                if let Some(cc) = n.current_class.clone() {
                    render_current(cc, collapsed, set_collapsed, lang)
                } else if let Some(nc) = n.next_class.clone() {
                    let l = lang.get();
                    let when = format!("{} {} {} ({})",
                        i18n::fmt_weekday_short(nc.date, l),
                        i18n::fmt_date(nc.date, l),
                        nc.start_time.clone(),
                        nc.service_name.clone());
                    view! {
                        <div class="now-panel__head" data-testid="now-panel-head-next">
                            <div class="now-panel__title">
                                {move || i18n::t(lang.get(), "now_next_on").replace("{when}", &when)}
                            </div>
                            <div class="now-panel__badge">{format!("{}/{}", nc.booked, nc.capacity)}</div>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="now-panel__head" data-testid="now-panel-head-empty">
                            <div class="now-panel__title">{move || i18n::t(lang.get(), "now_no_more_today")}</div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

fn render_current(
    cc: spinbike_core::reports::CurrentClass,
    collapsed: ReadSignal<bool>,
    set_collapsed: WriteSignal<bool>,
    lang: ReadSignal<Lang>,
) -> leptos::prelude::AnyView {
    let active_count = cc
        .roster
        .iter()
        .filter(|r| !matches!(r.status, RosterStatus::Cancelled))
        .count();
    let title = format!(
        "{} {} — {}",
        cc.start_time.clone(),
        cc.service_name.clone(),
        cc.instructor_name.clone().unwrap_or_default()
    );
    let badge = format!("{}/{}", active_count, cc.capacity);
    let roster_vec = cc.roster.clone();
    view! {
        <div class="now-panel__head now-panel__head--running"
             data-testid="now-panel-head-running"
             on:click=move |_| {
                 set_collapsed.update(|v| { *v = !*v; save_collapsed(*v); });
             }>
            <div class="now-panel__title">{title}</div>
            <div class="now-panel__badge" data-testid="now-panel-badge">{badge}</div>
            <span class="now-panel__chevron">{move || if collapsed.get() { "▸" } else { "▾" }}</span>
        </div>
        {move || if !collapsed.get() {
            let rows = roster_vec.clone();
            view! {
                <div class="now-panel__body" data-testid="now-panel-body">
                    <div class="group">
                        {rows.into_iter().map(|r| roster_row(r, lang)).collect::<Vec<_>>()}
                    </div>
                </div>
            }.into_any()
        } else { ().into_any() }}
    }.into_any()
}

fn roster_row(r: RosterEntry, lang: ReadSignal<Lang>) -> impl IntoView {
    let (status_key, badge_class) = match r.status {
        RosterStatus::Booked => ("status_booked", "badge badge--booked"),
        RosterStatus::CheckedIn => ("status_checked_in", "badge badge--pass"),
        RosterStatus::Cancelled => ("status_cancelled", "badge badge--cancelled"),
    };
    view! {
        <div class="list-row" data-testid="now-roster-row">
            <div class="list-row__main">
                <div class="list-row__title">{r.name.clone()}</div>
                <div class="list-row__sub">{r.barcode.clone().unwrap_or_default()}</div>
            </div>
            <span class=badge_class>{move || i18n::t(lang.get(), status_key)}</span>
        </div>
    }
}
