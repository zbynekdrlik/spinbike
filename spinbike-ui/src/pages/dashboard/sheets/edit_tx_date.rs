use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::{DateInput, Sheet};
use crate::i18n::{self, Lang};

#[component]
pub fn EditTxDateSheet(
    /// Whether the sheet is visible.
    show: RwSignal<bool>,
    /// Transaction id to PATCH.
    tx_id: i64,
    /// Current created_at date (pre-fills the date input).
    current_date: chrono::NaiveDate,
    /// Invoked after a successful save so the parent can refresh.
    on_saved: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

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
                let new_date = draft.get();
                let today = chrono::Local::now().date_naive();
                let earliest = today - chrono::Duration::days(30);
                if new_date < earliest || new_date > today {
                    set_err.set(i18n::t(lang.get_untracked(), "tx_date_window_error").to_string());
                    return;
                }
                set_err.set(String::new());
                set_saving.set(true);
                spawn_local(async move {
                    #[derive(serde::Serialize)]
                    struct Req {
                        created_at_date: chrono::NaiveDate,
                    }
                    match api::patch::<Req, serde_json::Value>(
                        &format!("/api/transactions/{tx_id}/created-at"),
                        &Req { created_at_date: new_date },
                    )
                    .await
                    {
                        Ok(_) => {
                            // Reset per-mount signals BEFORE unmount so we
                            // never write to a dropped subscriber. Then yield
                            // to the JS event loop so on_saved's parent-side
                            // refresh settles before the sheet vanishes.
                            // See #88.
                            set_saving.set(false);
                            on_saved.run(());
                            gloo_timers::future::TimeoutFuture::new(0).await;
                            show.set(false);
                        }
                        Err(e) => {
                            set_err.set(e);
                            set_saving.set(false);
                        }
                    }
                });
            };

            let on_cancel = move |_| {
                // See #89 — bare spawn_local without an await is synchronous
                // and does not defer. TimeoutFuture(0).await yields to the
                // JS event loop so the click event finishes dispatching
                // before the reactive tree unmounts.
                spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(0).await;
                    show.set(false);
                });
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| show.set(false))
                    title=i18n::t(lang.get(), "edit_tx_date").to_string()
                    testid="sheet-edit-tx-date"
                >
                    <div class="form-group">
                        <label>{i18n::t(lang.get(), "modal_date")}</label>
                        <DateInput value=draft set_value=set_draft testid="tx-date-input" />
                    </div>
                    {move || {
                        let e = err.get();
                        if e.is_empty() {
                            view! { <div></div> }.into_any()
                        } else {
                            view! { <div class="alert alert-error" data-testid="tx-date-error">{e}</div> }.into_any()
                        }
                    }}
                    <div class="sheet__actions">
                        <button
                            class="btn btn--ghost"
                            data-testid="edit-tx-date-cancel"
                            disabled=move || saving.get()
                            on:click=on_cancel
                        >
                            {i18n::t(lang.get(), "cancel")}
                        </button>
                        <button
                            class="btn btn--primary"
                            data-testid="tx-date-save"
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
