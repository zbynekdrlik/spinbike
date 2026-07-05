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
    /// Red-alert channel (#126) — block/unblock failures render here, not
    /// in the green success alert.
    set_err: WriteSignal<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (loading, set_loading) = signal(false);
    let btn_class = if blocked {
        "btn btn--primary btn--compact"
    } else {
        "btn btn--ghost btn--compact"
    };

    let on_click = move |_| {
        // Clear any stale alert from a PREVIOUS action (in this or a
        // sibling component sharing the panel's msg/err channels) before
        // starting a new one — otherwise a stale red error from an earlier
        // failure can still be showing when this action succeeds (or vice
        // versa), rendering both alerts at once (#126 follow-up).
        set_msg.set(String::new());
        set_err.set(String::new());
        set_loading.set(true);
        let new_blocked = !blocked;
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                user_id: i64,
                blocked: bool,
            }
            match api::post::<Req, CardInfo>(
                "/api/users/block",
                &Req {
                    user_id: card_id,
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
                Err(e) => set_err.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
            }
            set_loading.set(false);
        });
    };

    view! {
        <button class=btn_class data-testid="block-button" disabled=move || loading.get() on:click=on_click>
            {move || if blocked { i18n::t(lang.get(), "unblock") } else { i18n::t(lang.get(), "block") }}
        </button>
    }
}
