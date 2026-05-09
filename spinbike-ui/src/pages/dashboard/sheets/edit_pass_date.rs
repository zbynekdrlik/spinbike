use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::{DateInput, Sheet};
use crate::i18n::{self, Lang};

use crate::pages::dashboard::CardInfo;

#[component]
pub fn EditPassDateSheet(
    /// Whether the sheet is visible.
    show: RwSignal<bool>,
    /// Transaction id of the monthly-pass transaction to patch.
    tx_id: i64,
    /// Current valid_until date (pre-fills the date input).
    current_date: chrono::NaiveDate,
    /// Card code used to refresh the user after a successful save.
    card_code: String,
    /// Update the parent's selected card after save.
    set_selected: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let card_code = StoredValue::new(card_code);

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            // Per-mount form state — each open of the sheet starts fresh from `current_date`.
            let (draft, set_draft) = signal(current_date);
            let (err, set_err) = signal(String::new());
            let (saving, set_saving) = signal(false);

            let on_save = move |_| {
                let vu = draft.get();
                let code = card_code.get_value();
                set_err.set(String::new());
                set_saving.set(true);
                spawn_local(async move {
                    #[derive(serde::Serialize)]
                    struct Req {
                        valid_until: chrono::NaiveDate,
                    }
                    match api::patch::<Req, serde_json::Value>(
                        &format!("/api/transactions/{tx_id}/valid-until"),
                        &Req { valid_until: vu },
                    )
                    .await
                    {
                        Ok(_) => {
                            match api::get::<CardInfo>(&format!("/api/users/lookup/{code}")).await {
                                Ok(c) => {
                                    set_selected.set(Some(c));
                                    show.set(false);
                                }
                                Err(e) => set_err.set(e),
                            }
                        }
                        Err(e) => set_err.set(e),
                    }
                    set_saving.set(false);
                });
            };

            let on_cancel = move |_| {
                // See #89 — synchronous show.set(false) inside a click
                // handler tears down its own reactive scope mid-event and
                // Leptos emits "closure invoked recursively or after being
                // dropped". Defer the unmount to next microtask.
                spawn_local(async move {
                    show.set(false);
                });
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| show.set(false))
                    title=i18n::t(lang.get(), "edit_pass_date").to_string()
                    testid="sheet-edit-pass-date"
                >
                    <div class="form-group">
                        <label>{i18n::t(lang.get(), "modal_valid_until")}</label>
                        <DateInput value=draft set_value=set_draft testid="pass-date-input" />
                    </div>
                    {move || {
                        let e = err.get();
                        if e.is_empty() {
                            view! { <div></div> }.into_any()
                        } else {
                            view! { <div class="alert alert-error">{e}</div> }.into_any()
                        }
                    }}
                    <div class="sheet__actions">
                        <button
                            class="btn btn--ghost"
                            disabled=move || saving.get()
                            on:click=on_cancel
                        >
                            {i18n::t(lang.get(), "cancel")}
                        </button>
                        <button
                            class="btn btn--primary"
                            data-testid="pass-date-save"
                            disabled=move || saving.get()
                            on:click=on_save
                        >
                            {i18n::t(lang.get(), "save")}
                        </button>
                    </div>
                </Sheet>
            }.into_any()
        }}
    }
}
