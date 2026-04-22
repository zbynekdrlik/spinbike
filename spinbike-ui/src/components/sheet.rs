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
#[component]
pub fn Sheet(
    /// Whether the sheet is visible.
    #[prop(into)]
    show: Signal<bool>,
    /// Called when the user closes the sheet (backdrop click or Escape key).
    #[prop(into)]
    on_close: Callback<()>,
    /// Heading text displayed in `.sheet__title`.
    #[prop(into)]
    title: String,
    /// Optional `data-testid` placed on the `.sheet` element for Playwright selectors.
    #[prop(optional, into)]
    testid: Option<String>,
    children: ChildrenFn,
) -> impl IntoView {
    // Clone values that need to be moved into the closures below.
    let title_stored = StoredValue::new(title);
    let testid_stored = StoredValue::new(testid);
    let on_close_backdrop = on_close.clone();

    view! {
        <Show when=move || show.get() fallback=|| view! { <span></span> }>
            <div
                class="sheet-backdrop"
                on:click=move |_| on_close_backdrop.run(())
            >
                <div
                    class="sheet"
                    role="dialog"
                    aria-modal="true"
                    tabindex="-1"
                    data-testid=move || testid_stored.get_value()
                    on:click=|ev| ev.stop_propagation()
                    on:keydown=move |ev: ev::KeyboardEvent| {
                        if ev.key() == "Escape" {
                            on_close.run(());
                        }
                    }
                >
                    <div class="sheet__grab"></div>
                    <div class="sheet__title">{move || title_stored.get_value()}</div>
                    <div class="sheet__body">
                        {children()}
                    </div>
                </div>
            </div>
        </Show>
    }
}
