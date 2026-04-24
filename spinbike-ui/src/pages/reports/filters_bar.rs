use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::i18n::{self, Lang};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FiltersState {
    pub event_kind: Option<String>, // "charge" | "topup" | "pass" | None
    pub service: Option<String>,
    pub search: String,
}

impl FiltersState {
    pub fn is_active(&self) -> bool {
        self.event_kind.is_some() || self.service.is_some() || !self.search.is_empty()
    }
}

#[component]
pub fn FiltersBar(
    filters: ReadSignal<FiltersState>,
    set_filters: WriteSignal<FiltersState>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (expanded, set_expanded) = signal(false);

    view! {
        <div class="group" data-testid="filters-bar">
            <div class="list-row list-row--interactive"
                 on:click=move |_| set_expanded.update(|v| *v = !*v)>
                <div class="list-row__main">
                    <div class="list-row__title">{move || i18n::t(lang.get(), "filters_label")}</div>
                </div>
                <div class="list-row__end">
                    {move || if filters.get().is_active() {
                        view! { <span class="badge badge--info" data-testid="filters-active">"●"</span> }.into_any()
                    } else { ().into_any() }}
                    <span>{move || if expanded.get() { "▾" } else { "▸" }}</span>
                </div>
            </div>
            {move || if expanded.get() {
                view! {
                    <div style="padding: var(--s-3) var(--s-4); display: flex; flex-direction: column; gap: var(--s-3);">
                        <div class="seg" role="tablist" data-testid="filter-event-kind">
                            <FilterKindBtn kind="" label_key="filters_event_all" filters=filters set_filters=set_filters />
                            <FilterKindBtn kind="charge" label_key="filters_event_payments" filters=filters set_filters=set_filters />
                            <FilterKindBtn kind="topup" label_key="filters_event_topups" filters=filters set_filters=set_filters />
                            <FilterKindBtn kind="pass" label_key="filters_event_passes" filters=filters set_filters=set_filters />
                        </div>

                        <input class="form-control"
                               type="text"
                               data-testid="filter-search"
                               placeholder=move || i18n::t(lang.get(), "filters_search_placeholder").to_string()
                               prop:value=move || filters.get().search.clone()
                               on:input=move |ev: leptos::ev::Event| set_filters.update(|f| f.search = event_target_value(&ev)) />

                        <button class="btn btn--ghost btn--compact"
                                data-testid="filters-reset"
                                on:click=move |_| set_filters.set(FiltersState::default())>
                            {move || i18n::t(lang.get(), "filters_reset")}
                        </button>
                    </div>
                }.into_any()
            } else { ().into_any() }}
        </div>
    }
}

#[component]
fn FilterKindBtn(
    kind: &'static str,
    label_key: &'static str,
    filters: ReadSignal<FiltersState>,
    set_filters: WriteSignal<FiltersState>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let kind_s = kind.to_string();
    let testid = format!("filter-kind-{}", if kind.is_empty() { "all" } else { kind });
    view! {
        <button class="seg__item"
                data-testid=testid
                aria-selected=move || {
                    let current = filters.get().event_kind;
                    let matches = match current {
                        None => kind.is_empty(),
                        Some(c) => !kind.is_empty() && c == kind,
                    };
                    matches.to_string()
                }
                on:click={let kind_s = kind_s.clone(); move |_| set_filters.update(|f| {
                    f.event_kind = if kind_s.is_empty() { None } else { Some(kind_s.clone()) };
                })}>
            {move || i18n::t(lang.get(), label_key)}
        </button>
    }
}

fn event_target_value(ev: &leptos::ev::Event) -> String {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}
