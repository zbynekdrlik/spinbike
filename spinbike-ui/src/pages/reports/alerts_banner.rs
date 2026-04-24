use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use spinbike_core::reports::AlertsResponse;

const LS_PREFIX: &str = "reports_alerts_dismissed";

fn today_key() -> String {
    chrono::Local::now().date_naive().format("%Y-%m-%d").to_string()
}

fn is_dismissed(kind: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|ls| {
            ls.get_item(&format!("{}_{}_{}", LS_PREFIX, today_key(), kind))
                .ok()
                .flatten()
        })
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn dismiss(kind: &str) {
    if let Some(ls) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = ls.set_item(&format!("{}_{}_{}", LS_PREFIX, today_key(), kind), "1");
    }
}

#[component]
pub fn AlertsBanner() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<AlertsResponse>);
    let (ver, set_ver) = signal(0u32);

    Effect::new(move |_| {
        let _ = ver.get();
        spawn_local(async move {
            if let Ok(a) = api::get::<AlertsResponse>("/api/reports/alerts").await {
                set_data.set(Some(a));
            }
        });
    });

    view! {
        {move || {
            let Some(a) = data.get() else { return ().into_any(); };
            let expiring_n = a.expiring_passes.len();
            let low_n = a.low_credit.len();
            let inactive_n = a.inactive.len();

            let show_expiring = expiring_n > 0 && !is_dismissed("expiring");
            let show_low = low_n > 0 && !is_dismissed("low");
            let show_inactive = inactive_n > 0 && !is_dismissed("inactive");

            if !show_expiring && !show_low && !show_inactive {
                return ().into_any();
            }

            view! {
                <div class="alerts-banner" data-testid="alerts-banner">
                    <div class="alerts-banner__head">{move || i18n::t(lang.get(), "alerts_title")}</div>

                    {if show_expiring {
                        let n = expiring_n;
                        view! {
                            <div class="alerts-banner__row" data-testid="alert-expiring">
                                <div class="alerts-banner__body">
                                    {move || i18n::t(lang.get(), "alerts_expiring_passes").replace("{n}", &n.to_string())}
                                </div>
                                <button class="alerts-banner__dismiss"
                                        data-testid="alert-expiring-dismiss"
                                        on:click=move |ev: leptos::ev::MouseEvent| {
                                            ev.stop_propagation();
                                            dismiss("expiring");
                                            set_ver.update(|v| *v += 1);
                                        }>"×"</button>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}

                    {if show_low {
                        let n = low_n;
                        view! {
                            <div class="alerts-banner__row" data-testid="alert-low-credit">
                                <div class="alerts-banner__body">
                                    {move || i18n::t(lang.get(), "alerts_low_credit").replace("{n}", &n.to_string())}
                                </div>
                                <button class="alerts-banner__dismiss"
                                        data-testid="alert-low-credit-dismiss"
                                        on:click=move |ev: leptos::ev::MouseEvent| {
                                            ev.stop_propagation();
                                            dismiss("low");
                                            set_ver.update(|v| *v += 1);
                                        }>"×"</button>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}

                    {if show_inactive {
                        let n = inactive_n;
                        view! {
                            <div class="alerts-banner__row" data-testid="alert-inactive">
                                <div class="alerts-banner__body">
                                    {move || i18n::t(lang.get(), "alerts_inactive").replace("{n}", &n.to_string())}
                                </div>
                                <button class="alerts-banner__dismiss"
                                        data-testid="alert-inactive-dismiss"
                                        on:click=move |ev: leptos::ev::MouseEvent| {
                                            ev.stop_propagation();
                                            dismiss("inactive");
                                            set_ver.update(|v| *v += 1);
                                        }>"×"</button>
                            </div>
                        }.into_any()
                    } else { ().into_any() }}
                </div>
            }.into_any()
        }}
    }
}
