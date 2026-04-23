use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

use crate::pages::dashboard::helpers::event_target_value;
use crate::pages::dashboard::CardInfo;

#[component]
pub fn EditPassDateSheet(
    /// Whether the sheet is visible.
    show: RwSignal<bool>,
    /// Transaction id of the monthly-pass transaction to patch.
    tx_id: i64,
    /// Current valid_until date (pre-fills the date input).
    current_date: chrono::NaiveDate,
    /// Card barcode used to refresh the card after a successful save.
    barcode: String,
    /// Update the parent's selected card after save.
    set_selected: WriteSignal<Option<CardInfo>>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let barcode = StoredValue::new(barcode);

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            // Per-mount form state — each open of the sheet starts fresh from `current_date`.
            let (draft, set_draft) = signal(current_date);
            let (err, set_err) = signal(String::new());
            let (saving, set_saving) = signal(false);

            let on_date_input = move |ev: web_sys::Event| {
                let s = event_target_value(&ev);
                if let Ok(d) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                    set_draft.set(d);
                }
            };

            let on_save = move |_| {
                let vu = draft.get();
                let bc = barcode.get_value();
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
                            match api::get::<CardInfo>(&format!("/api/cards/lookup/{bc}")).await {
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
                set_err.set(String::new());
                show.set(false);
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| show.set(false))
                    title=i18n::t(lang.get(), "edit_pass_date").to_string()
                    testid="sheet-edit-pass-date"
                >
                    <div class="form-group">
                        <label>{i18n::t(lang.get(), "modal_valid_until")}</label>
                        <input
                            type="date"
                            class="form-control"
                            data-testid="pass-date-input"
                            prop:value=move || draft.get().format("%Y-%m-%d").to_string()
                            on:input=on_date_input
                        />
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
