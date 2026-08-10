use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

/// Bottom sheet on mobile, centered modal on desktop (breakpoint handled via CSS, not Rust).
///
/// Renders a `.sheet-backdrop` + `.sheet` with:
/// - `.sheet__grab`  — mobile drag-handle visual
/// - `.sheet__title` — heading
/// - `.sheet__body`  — slot for children
///
/// Accessibility: `role="dialog"` + `aria-modal="true"` on `.sheet`. An
/// optional `id` prop places a DOM id on `.sheet` so a trigger button can
/// reference it via `aria-controls` (#319). The keydown Escape handler
/// lives on `.sheet` itself, so it only fires once focus is actually
/// *inside* the sheet — a caller that needs Escape to work without a prior
/// mouse click (a genuine keyboard-first open) must move focus into the
/// sheet itself right after mounting it (see `components::nav::Navbar`'s
/// burger menu for the pattern).
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
    /// Optional DOM `id` placed on the `.sheet` element — lets a trigger
    /// button reference it via `aria-controls` (#319's burger menu).
    #[prop(optional, into)]
    id: Option<String>,
    children: Children,
) -> impl IntoView {
    let on_close_backdrop = on_close;
    let on_close_keyboard = on_close;
    let testid_value = testid.unwrap_or_default();
    let id_value = id.unwrap_or_default();

    // Defer on_close to next macrotask so the click / keydown event finishes
    // dispatching (and any focus/cleanup events on now-detaching DOM nodes
    // settle) before the consumer's reactive tree unmounts. Synchronous
    // on_close.run(()) here used to emit "closure invoked recursively or
    // after being dropped" via Leptos. See #89.
    let close_backdrop = move |_| {
        let cb = on_close_backdrop;
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            cb.run(());
        });
    };
    let close_keyboard = move |ev: ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            let cb = on_close_keyboard;
            spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(0).await;
                cb.run(());
            });
        }
    };

    view! {
        <div
            class="sheet-backdrop"
            on:click=close_backdrop
        >
            <div
                class="sheet"
                id=id_value
                role="dialog"
                aria-modal="true"
                tabindex="-1"
                data-testid=testid_value
                on:click=|ev: ev::MouseEvent| ev.stop_propagation()
                on:keydown=close_keyboard
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
