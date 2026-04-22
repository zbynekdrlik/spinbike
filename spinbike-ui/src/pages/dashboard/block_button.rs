use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

use super::CardInfo;

#[component]
pub fn BlockButton(
    card_id: i64,
    blocked: bool,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (loading, set_loading) = signal(false);
    let btn_class = if blocked {
        "btn btn--primary btn--compact"
    } else {
        "btn btn--ghost btn--compact"
    };

    let on_click = move |_| {
        set_loading.set(true);
        let new_blocked = !blocked;
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                card_id: i64,
                blocked: bool,
            }
            match api::post::<Req, CardInfo>(
                "/api/cards/block",
                &Req {
                    card_id,
                    blocked: new_blocked,
                },
            )
            .await
            {
                Ok(c) => {
                    set_msg.set(if c.blocked {
                        i18n::t(lang.get_untracked(), "block_ok").to_string()
                    } else {
                        i18n::t(lang.get_untracked(), "unblock_ok").to_string()
                    });
                    set_selected.set(Some(c));
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <button class=btn_class disabled=move || loading.get() on:click=on_click>
            {move || if blocked { i18n::t(lang.get(), "unblock") } else { i18n::t(lang.get(), "block") }}
        </button>
    }
}
