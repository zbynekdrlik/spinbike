use leptos::ev;
use leptos::prelude::*;

/// Bottom sheet on mobile, centered modal on desktop (breakpoint handled via CSS, not Rust).
///
/// Renders a `.sheet-backdrop` + `.sheet` with:
/// - `.sheet__grab`  — mobile drag-handle visual
/// - `.sheet__title` — heading
/// - `.sheet__body`  — slot for children
///
/// Accessibility: `role="dialog"` + `aria-modal="true"` on `.sheet`.
/// Keyboard: Escape on the sheet element triggers `on_close`.
///
/// **Mounting:** the Sheet renders unconditionally when instantiated.
/// Callers control visibility by mounting/unmounting the Sheet inside
/// a reactive closure, e.g.:
///
/// ```ignore
/// {move || if show.get() {
///     view! { <Sheet on_close title testid>{children}</Sheet> }.into_any()
/// } else {
///     ().into_any()
/// }}
/// ```
///
/// The `title` and any locale-dependent text are re-evaluated on each
/// re-instantiation, so toggling `show` after a language change yields
/// a fresh, correctly-localised sheet.
#[component]
pub fn Sheet(
    /// Called when the user closes the sheet (backdrop click or Escape key).
    #[prop(into)]
    on_close: Callback<()>,
    /// Heading text displayed in `.sheet__title`.
    #[prop(into)]
    title: String,
    /// Optional `data-testid` placed on the `.sheet` element for Playwright selectors.
    #[prop(optional, into)]
    testid: Option<String>,
    children: Children,
) -> impl IntoView {
    let on_close_backdrop = on_close.clone();
    let on_close_keyboard = on_close.clone();
    let testid_value = testid.unwrap_or_default();

    view! {
        <div
            class="sheet-backdrop"
            on:click=move |_| on_close_backdrop.run(())
        >
            <div
                class="sheet"
                role="dialog"
                aria-modal="true"
                tabindex="-1"
                data-testid=testid_value
                on:click=|ev: ev::MouseEvent| ev.stop_propagation()
                on:keydown=move |ev: ev::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        on_close_keyboard.run(());
                    }
                }
            >
                <div class="sheet__grab"></div>
                <div class="sheet__title">{title}</div>
                <div class="sheet__body">
                    {children()}
                </div>
            </div>
        </div>
    }
}
