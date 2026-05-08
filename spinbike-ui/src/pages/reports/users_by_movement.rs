use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};
use crate::pages::dashboard::helpers::urlencoding_light;

#[derive(Debug, Clone, serde::Deserialize)]
struct Row {
    id: i64,
    name: String,
    #[serde(default)]
    card_code: Option<String>,
    #[serde(default)]
    last_movement_at: Option<String>,
}

#[component]
pub fn UsersByMovement() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (rows, set_rows) = signal::<Vec<Row>>(Vec::new());
    let (offset, set_offset) = signal(0i64);
    let (loading, set_loading) = signal(true);
    let (has_more, set_has_more) = signal(false);
    let (error, set_error) = signal(String::new());

    const PAGE: i64 = 50;

    Effect::new(move |_| {
        set_loading.set(true);
        set_error.set(String::new());
        spawn_local(async move {
            let url = format!("/api/users/by-last-movement?limit={PAGE}&offset=0");
            match api::get::<Vec<Row>>(&url).await {
                Ok(r) => {
                    let len = r.len() as i64;
                    set_rows.set(r);
                    set_has_more.set(len == PAGE);
                    set_offset.set(len);
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    });

    let on_show_more = move |_| {
        set_loading.set(true);
        let cur_offset = offset.get();
        spawn_local(async move {
            let url = format!("/api/users/by-last-movement?limit={PAGE}&offset={cur_offset}");
            match api::get::<Vec<Row>>(&url).await {
                Ok(r) => {
                    let len = r.len() as i64;
                    set_rows.update(|v| v.extend(r));
                    set_has_more.set(len == PAGE);
                    set_offset.update(|n| *n += len);
                }
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <section class="users-by-movement" data-testid="users-by-movement">
            <h2>{move || i18n::t(lang.get(), "users_by_movement_heading")}</h2>
            {move || if !error.get().is_empty() {
                view! { <div class="alert alert-error">{move || error.get()}</div> }.into_any()
            } else { ().into_any() }}
            <ul class="list" data-testid="users-by-movement-list">
                <For
                    each=move || rows.get()
                    key=|r| r.id
                    children=move |r| {
                        let id = r.id;
                        let name = r.name.clone();
                        let card_code = r.card_code.clone();
                        let display_date = match &r.last_movement_at {
                            Some(s) if s.len() >= 10 => s[..10].to_string(),
                            _ => i18n::t(lang.get(), "no_movement_yet").to_string(),
                        };
                        let on_click = move |_| {
                            // Navigate to Desk with card_code if known, else ?q=<name>.
                            let target = if let Some(code) = card_code.as_deref().filter(|s| !s.is_empty()) {
                                format!("/staff?card={}", urlencoding_light(code))
                            } else {
                                format!("/staff?q={}", urlencoding_light(&name))
                            };
                            if let Some(w) = web_sys::window() {
                                let _ = w.location().set_href(&target);
                            }
                        };
                        view! {
                            <li class="list-row" data-testid="user-row" data-user-id=id
                                on:click=on_click>
                                <div class="list-row__main">
                                    <div class="list-row__title">{r.name.clone()}</div>
                                    <div class="list-row__sub">{display_date}</div>
                                </div>
                            </li>
                        }
                    }
                />
            </ul>
            {move || if has_more.get() {
                view! {
                    <button class="btn btn--ghost"
                            data-testid="users-by-movement-show-more"
                            disabled=move || loading.get()
                            on:click=on_show_more.clone()>
                        {move || i18n::t(lang.get(), "show_more")}
                    </button>
                }.into_any()
            } else { ().into_any() }}
        </section>
    }
}
