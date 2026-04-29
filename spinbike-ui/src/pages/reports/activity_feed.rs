use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::dashboard::helpers::urlencoding_light as url_encode;
use spinbike_core::reports::{EventKind, ReportEvent, ReportResponse};

use super::{FiltersState, RangeMode};

#[component]
pub fn ActivityFeed(
    events: ReadSignal<Vec<ReportEvent>>,
    loading: ReadSignal<bool>,
    has_more: ReadSignal<bool>,
    filters: ReadSignal<FiltersState>,
    anchor: ReadSignal<chrono::NaiveDate>,
    mode: ReadSignal<RangeMode>,
    set_events: WriteSignal<Vec<ReportEvent>>,
    set_has_more: WriteSignal<bool>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    let filtered = move || {
        let f = filters.get();
        let needle = f.search.to_lowercase();
        events
            .get()
            .into_iter()
            .filter(|e| {
                match f.event_kind.as_deref() {
                    Some("charge") => {
                        if !matches!(e.kind(), EventKind::Charge) {
                            return false;
                        }
                    }
                    Some("topup") => {
                        if !matches!(e.kind(), EventKind::TopUp) {
                            return false;
                        }
                    }
                    Some("pass") => {
                        if !matches!(e.kind(), EventKind::PassSale) {
                            return false;
                        }
                    }
                    _ => {}
                }
                if let Some(svc) = &f.service {
                    // Match against either language so the filter still works
                    // regardless of which name (sk/en) was clicked from.
                    let s = svc.as_str();
                    let sk = e.service_name_sk.as_deref();
                    let en = e.service_name_en.as_deref();
                    if sk != Some(s) && en != Some(s) {
                        return false;
                    }
                }
                if !needle.is_empty() {
                    let hay = format!(
                        "{} {}",
                        e.card_name.clone().unwrap_or_default(),
                        e.barcode.clone().unwrap_or_default()
                    )
                    .to_lowercase();
                    if !hay.contains(&needle) {
                        return false;
                    }
                }
                true
            })
            .collect::<Vec<_>>()
    };

    let load_older = move |_| {
        // Composite cursor "<created_at>|<id>"; URL-encode because created_at
        // contains a space.
        let before_encoded = events
            .get_untracked()
            .last()
            .map(|e| {
                let raw = format!("{}|{}", e.created_at, e.id);
                url_encode(&raw)
            })
            .unwrap_or_default();
        let url = match mode.get_untracked() {
            RangeMode::Day => format!(
                "/api/reports/day?date={}&before={}",
                anchor.get_untracked().format("%Y-%m-%d"),
                before_encoded
            ),
            other => {
                let (from, to) = match other {
                    RangeMode::Week => (
                        anchor.get_untracked() - chrono::Duration::days(6),
                        anchor.get_untracked(),
                    ),
                    RangeMode::Month => (
                        anchor.get_untracked() - chrono::Duration::days(29),
                        anchor.get_untracked(),
                    ),
                    RangeMode::Day => unreachable!(),
                };
                format!(
                    "/api/reports/range?from={}&to={}&before={}",
                    from.format("%Y-%m-%d"),
                    to.format("%Y-%m-%d"),
                    before_encoded
                )
            }
        };
        spawn_local(async move {
            if let Ok(r) = api::get::<ReportResponse>(&url).await {
                set_events.update(|v| v.extend(r.events));
                set_has_more.set(r.has_more);
            }
        });
    };

    view! {
        {move || if loading.get() {
            view! { <div class="group"><div class="list-row">"..."</div></div> }.into_any()
        } else {
            let rows = filtered();
            if rows.is_empty() {
                let msg_key = if filters.get().is_active() { "feed_empty_filter" } else { "feed_empty_day" };
                view! {
                    <div class="group" data-testid="activity-feed-empty">
                        <div class="list-row"><div class="list-row__main">{move || i18n::t(lang.get(), msg_key)}</div></div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="group" data-testid="activity-feed">
                        {rows.into_iter().map(|e| render_row(e)).collect::<Vec<_>>()}
                    </div>
                }.into_any()
            }
        }}
        {move || if has_more.get() {
            view! {
                <button class="btn btn--block btn--ghost"
                        data-testid="feed-load-older"
                        on:click=load_older>
                    {move || i18n::t(lang.get(), "feed_load_older")}
                </button>
            }.into_any()
        } else { ().into_any() }}
    }
}

fn render_row(e: ReportEvent) -> impl IntoView {
    let lang = use_context::<ReadSignal<crate::i18n::Lang>>().expect("Lang context");
    let kind = e.kind();
    let kind_class = match kind {
        EventKind::Charge => "feed-dot feed-dot--charge",
        EventKind::TopUp => "feed-dot feed-dot--topup",
        EventKind::PassSale => "feed-dot feed-dot--pass",
        EventKind::Visit => "feed-dot feed-dot--visit",
        EventKind::Other => "feed-dot feed-dot--voided",
    };
    let event_label_key = match kind {
        EventKind::Charge => "event_charge",
        EventKind::TopUp => "event_topup",
        EventKind::PassSale => "event_pass",
        EventKind::Visit => "event_charge", // placeholder — Task 7 replaces all keys
        EventKind::Other => "event_other",
    };
    let amount_class = if e.amount < 0.0 {
        "list-row__amount list-row__amount--neg"
    } else {
        "list-row__amount list-row__amount--pos"
    };
    let amount_display = if e.amount < 0.0 {
        format!("{:.2} \u{20ac}", e.amount)
    } else {
        format!("+{:.2} \u{20ac}", e.amount)
    };
    let time_only = i18n::fmt_time_str(&e.created_at);
    let voided_badge = if e.voided {
        view! { <span class="badge badge--voided">"voided"</span> }.into_any()
    } else {
        ().into_any()
    };

    // Click → jump to Desk and pre-fill search by barcode (or name fallback).
    let q_value = e
        .barcode
        .clone()
        .or_else(|| e.card_name.clone())
        .unwrap_or_default();
    let on_row_click = move |_| {
        if q_value.is_empty() {
            return;
        }
        if let Some(w) = web_sys::window() {
            let encoded = url_encode(&q_value);
            let _ = w.location().set_href(&format!("/staff?q={encoded}"));
        }
    };

    let name = e.card_name.clone().unwrap_or_else(|| "—".to_string());
    let service = match lang.get_untracked() {
        Lang::Sk => e.service_name_sk.clone(),
        Lang::En => e.service_name_en.clone(),
    };
    // Subtitle: "<event_label> · <service>" — never empty.
    let subtitle = move || {
        let svc_str = service
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let prefix = i18n::t(lang.get(), event_label_key);
        if svc_str.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix} · {svc_str}")
        }
    };

    view! {
        <div class="list-row list-row--interactive" data-testid="feed-row"
             on:click=on_row_click>
            <div class=kind_class></div>
            <div class="list-row__sub" style="min-width: 48px;">{time_only}</div>
            <div class="list-row__main">
                <div class="list-row__title">{name}</div>
                <div class="list-row__sub">{subtitle}</div>
            </div>
            <div class=amount_class>{amount_display}</div>
            {voided_badge}
        </div>
    }
}
