use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

/// CSS selector for elements that are ACTUALLY reachable by native Tab
/// navigation — shared by `trap_tab` below (the boundary check) and
/// `components::nav::focus_first_in` (the #319 focus-on-open move), so the
/// two never drift out of sync (#320 review finding). Deliberately
/// stricter than a bare `"a, button, input, select, textarea, [tabindex]"`:
/// `a[href]` (a hrefless anchor isn't tabbable), `:not([disabled])` on
/// every form control (several `Sheet` consumers — e.g.
/// `dashboard/sheets/edit_pass_date.rs`, `edit_tx_date.rs`,
/// `delete_user.rs` — disable their Save/Delete button while a `saving`/
/// `loading` signal is true, and a disabled element is never actually
/// focusable), and `[tabindex]:not([tabindex='-1'])` (a `tabindex="-1"`
/// element is deliberately excluded from tab order). Using the loose
/// selector as `first`/`last` here would silently break the trap whenever
/// a non-tabbable element happened to be the DOM-order boundary match.
pub(crate) const FOCUSABLE_SELECTOR: &str = "a[href], button:not([disabled]), \
     input:not([disabled]), select:not([disabled]), textarea:not([disabled]), \
     [tabindex]:not([tabindex='-1'])";

/// #320: real WAI-ARIA dialog focus trap. `ev.current_target()` is the
/// `.sheet` element itself (this handler is bound directly on it via
/// `on:keydown`, not on a focused descendant), so this queries `.sheet`'s
/// OWN focusable descendants via `FOCUSABLE_SELECTOR` above. When the
/// currently-focused element is the LAST one and plain Tab was pressed, or
/// the FIRST one and Shift+Tab was pressed, wrap focus back around instead
/// of letting the browser's native tab order carry it out of the dialog
/// into page content behind the backdrop.
fn trap_tab(ev: &ev::KeyboardEvent) {
    let Some(container) = ev
        .current_target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return;
    };
    let Ok(list) = container.query_selector_all(FOCUSABLE_SELECTOR) else {
        return;
    };
    let len = list.length();
    if len == 0 {
        // No focusable descendant to trap around — assumed unreachable for
        // all 8 current Sheet call sites (each renders at least one
        // interactive control), but a future purely-informational Sheet
        // would silently regress to pre-#320 (untrapped) Tab behavior.
        return;
    }
    let first = list
        .get(0)
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok());
    let last = list
        .get(len - 1)
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok());
    let Some(active) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
    else {
        return;
    };
    let active_node: &web_sys::Node = active.as_ref();

    if ev.shift_key() {
        if let (Some(first_el), Some(last_el)) = (&first, last) {
            let first_node: &web_sys::Node = first_el.as_ref();
            if first_node.is_same_node(Some(active_node)) {
                ev.prevent_default();
                // `focus()` returns a `Result` (fails only if the target
                // detached mid-event) — vanishingly unlikely for an
                // element we just queried out of a live, attached `.sheet`,
                // and there's nothing more useful to do here than leave
                // focus wherever it currently is.
                let _ = last_el.focus();
            }
        }
    } else if let (Some(last_el), Some(first_el)) = (&last, first) {
        let last_node: &web_sys::Node = last_el.as_ref();
        if last_node.is_same_node(Some(active_node)) {
            ev.prevent_default();
            let _ = first_el.focus();
        }
    }
}

/// Bottom sheet on mobile, centered modal on desktop (breakpoint handled via CSS, not Rust).
///
/// Renders a `.sheet-backdrop` + `.sheet` with:
/// - `.sheet__grab`  — mobile drag-handle visual
/// - `.sheet__title` — heading
/// - `.sheet__body`  — slot for children
///
/// Accessibility: `role="dialog"` + `aria-modal="true"` on `.sheet`. An
/// optional `id` prop places a DOM id on `.sheet` so a trigger button can
/// reference it via `aria-controls` (#319). The keydown handler lives on
/// `.sheet` itself, so it only fires once focus is actually *inside* the
/// sheet — a caller that needs Escape/Tab to work without a prior mouse
/// click (a genuine keyboard-first open) must move focus into the sheet
/// itself right after mounting it (see `components::nav::Navbar`'s burger
/// menu for the pattern).
/// Keyboard: Escape triggers `on_close`. Tab/Shift+Tab cycle within the
/// sheet's own focusable descendants and wrap at the ends — a real
/// focus trap, per the WAI-ARIA dialog pattern (#320).
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
    let close_keyboard = move |ev: ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => {
            let cb = on_close_keyboard;
            spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(0).await;
                cb.run(());
            });
        }
        "Tab" => trap_tab(&ev),
        _ => {}
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
