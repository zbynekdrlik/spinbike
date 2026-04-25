use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::components::{DateInput, Sheet};
use crate::i18n::{self, Lang};

#[component]
pub fn CalendarPickerSheet(
    current: ReadSignal<chrono::NaiveDate>,
    #[prop(into)] on_close: Callback<()>,
    #[prop(into)] on_pick: Callback<chrono::NaiveDate>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (draft, set_draft) = signal(current.get_untracked());

    let on_close_cancel = on_close;
    view! {
        <Sheet
            on_close=on_close
            title=i18n::t(lang.get_untracked(), "reports_pick_date").to_string()
            testid="sheet-calendar-picker".to_string()
        >
            <div class="form-group">
                <DateInput value=draft set_value=set_draft testid="calendar-picker-input" />
            </div>
            <div class="sheet__actions">
                <button class="btn btn--ghost"
                        on:click=move |ev: leptos::ev::MouseEvent| {
                            ev.stop_propagation();
                            spawn_local(async move {
                                on_close_cancel.run(());
                            });
                        }>
                    {move || i18n::t(lang.get(), "modal_cancel")}
                </button>
                <button class="btn btn--primary"
                        data-testid="calendar-picker-confirm"
                        on:click=move |ev: leptos::ev::MouseEvent| {
                            ev.stop_propagation();
                            let d = draft.get_untracked();
                            spawn_local(async move {
                                on_pick.run(d);
                            });
                        }>
                    {move || i18n::t(lang.get(), "modal_confirm")}
                </button>
            </div>
        </Sheet>
    }
}
