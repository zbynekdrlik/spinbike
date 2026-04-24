use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::Sheet;
use crate::i18n::{self, Lang};

#[component]
pub fn CalendarPickerSheet(
    current: ReadSignal<chrono::NaiveDate>,
    #[prop(into)] on_close: Callback<()>,
    #[prop(into)] on_pick: Callback<chrono::NaiveDate>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (typed, set_typed) = signal(current.get_untracked().format("%Y-%m-%d").to_string());

    let on_close_cancel = on_close;
    view! {
        <Sheet
            on_close=on_close
            title=i18n::t(lang.get_untracked(), "reports_pick_date").to_string()
            testid="sheet-calendar-picker".to_string()
        >
            <div class="form-group">
                <input class="form-control"
                       type="date"
                       data-testid="calendar-picker-input"
                       prop:value=move || typed.get()
                       on:input=move |ev: leptos::ev::Event| {
                           if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                               set_typed.set(el.value());
                           }
                       }/>
            </div>
            <div class="sheet__actions">
                <button class="btn btn--ghost"
                        on:click=move |_| on_close_cancel.run(())>
                    {move || i18n::t(lang.get(), "modal_cancel")}
                </button>
                <button class="btn btn--primary"
                        data-testid="calendar-picker-confirm"
                        on:click=move |_| {
                            if let Ok(d) = chrono::NaiveDate::parse_from_str(&typed.get(), "%Y-%m-%d") {
                                on_pick.run(d);
                            }
                        }>
                    {move || i18n::t(lang.get(), "modal_confirm")}
                </button>
            </div>
        </Sheet>
    }
}
