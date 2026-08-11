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
        // #334: no focusable descendant to trap around right now — 2 of
        // the 8 current call sites (delete_user.rs,
        // deleted_email_conflict.rs) hit this during their transient
        // busy/saving window, where every button shares one disabled=...
        // signal. `refocus_if_orphaned` below (bound as `.sheet`'s
        // `on:focusout`) is what gets a keydown routed HERE at all in
        // that case — without it, the previously-focused (now-disabled)
        // button is auto-blurred by the browser straight to `<body>`
        // before any Tab could be pressed, and this handler never sees
        // the event (its `current_target` — `.sheet` — is no longer an
        // ancestor of `document.activeElement`). Once `refocus_if_orphaned`
        // has put focus back on `.sheet` itself, THIS keydown handler is
        // reachable again — but `.sheet` has `tabindex="-1"`, which is
        // excluded from the browser's own forward tab sequence, so a Tab
        // pressed from here with zero focusable descendants would
        // otherwise fall through to native tab order and carry focus
        // into whatever page content follows `.sheet` in DOM order,
        // behind the backdrop — the exact same escape, one step later.
        // prevent_default traps it in place instead: nothing to tab TO,
        // so Tab does nothing until a control re-enables.
        ev.prevent_default();
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
    // #334 review follow-up: `refocus_if_orphaned` below can park focus on
    // `.sheet` ITSELF (not any descendant) — e.g. once an in-flight
    // request that triggered the all-disabled window then FAILS, controls
    // re-enable again but nothing else ever moves focus off the container
    // onto a real descendant. Treat the container as sitting "before the
    // first" tabbable item: Tab from there enters at the first descendant,
    // Shift+Tab wraps to the last — same as pressing those keys from a
    // one-before-the-start position would, so the trap keeps working
    // immediately once something becomes focusable again, without waiting
    // for a second Tab press to hit a real descendant first.
    let container_node: &web_sys::Node = container.as_ref();
    let is_container_focused = container_node.is_same_node(Some(active_node));

    if ev.shift_key() {
        let first_is_active = first.as_ref().is_some_and(|el| is_active(el, active_node));
        if (first_is_active || is_container_focused)
            && let Some(last_el) = &last
        {
            ev.prevent_default();
            // `focus()` returns a `Result` (fails only if the target
            // detached mid-event) — vanishingly unlikely for an
            // element we just queried out of a live, attached `.sheet`,
            // and there's nothing more useful to do here than leave
            // focus wherever it currently is.
            let _ = last_el.focus();
        }
    } else {
        let last_is_active = last.as_ref().is_some_and(|el| is_active(el, active_node));
        if (last_is_active || is_container_focused)
            && let Some(first_el) = &first
        {
            ev.prevent_default();
            let _ = first_el.focus();
        }
    }
}

/// Whether `el` (a `first`/`last` focusable descendant) is the currently
/// active element — factored out so `trap_tab`'s shift/plain-Tab branches
/// share one comparison instead of re-deriving `&web_sys::Node` inline
/// twice each.
fn is_active(el: &web_sys::HtmlElement, active_node: &web_sys::Node) -> bool {
    let node: &web_sys::Node = el.as_ref();
    node.is_same_node(Some(active_node))
}

/// #334: catches focus LEAVING `.sheet` without a legitimate close — the
/// gap `trap_tab`'s own `len == 0` branch above documents. The concrete
/// trigger: 2 of the 8 current call sites (`delete_user.rs`,
/// `deleted_email_conflict.rs`) disable EVERY focusable descendant off one
/// shared busy/saving signal. The instant that flips `true`, the browser
/// auto-blurs whatever was focused (now `disabled`) — to `<body>`, or
/// nowhere reachable — BEFORE any keydown can fire, so `trap_tab`'s
/// `keydown` listener never even runs (its `current_target`, `.sheet`, is
/// no longer an ancestor of `document.activeElement`).
///
/// `focusout` (unlike `blur`/`focus`) bubbles, so binding it directly on
/// `.sheet` (same binding point as `trap_tab`'s `keydown`) catches focus
/// leaving ANY descendant, for ANY reason — not just Tab.
/// `event.related_target()` is the element focus is MOVING TO (`None` when
/// it went nowhere reachable).
///
/// A `focusout` with a `related_target` outside `.sheet` ALSO fires on
/// every LEGITIMATE close (Escape, backdrop click, an item link navigating
/// away). Most close paths (this file's own `close_backdrop`/
/// `close_keyboard`, `nav.rs`'s `on_close_menu`, `delete_user.rs`,
/// `edit_pass_date.rs`, `edit_tx_date.rs`, `edit_info_form.rs`) unmount
/// `.sheet` via a `TimeoutFuture::new(0)`-deferred `show.set(false)` —
/// but NOT all of them: `calendar_picker.rs`'s Cancel/Confirm and
/// `deleted_email_conflict.rs`'s Cancel button call their `on_close`/
/// `on_cancel` directly, with no macrotask defer at all (review finding on
/// #334 — an earlier revision of this comment claimed uniformly deferred
/// unmounts here, which was wrong). Telling a legitimate close apart from
/// the transient-disabled-window case (where `.sheet` stays mounted) needs
/// a check, not just a defer: after our own deferred tick, re-verify
/// `.sheet` is STILL CONNECTED to the document (`Node::is_connected()` —
/// any close, deferred or not, has unmounted `.sheet` well before this
/// point for every path EXCEPT a backdrop click specifically) AND that
/// focus hasn't already landed back inside `.sheet` on its own.
///
/// A backdrop click is the one path where this check does NOT catch the
/// close before we act: the native `focusout` from the browser's
/// `mousedown`-driven blur is dispatched (and schedules our deferred
/// check) strictly BEFORE the later `click` event whose handler schedules
/// `close_backdrop`'s own deferred unmount — two equal-delay timers
/// resolve FIFO, so `is_connected()` is deterministically still `true`
/// when we check, not merely possibly true. We refocus `.sheet` a moment
/// before it closes anyway. This is harmless regardless: `.focus()` on a
/// node that gets detached moments later has no lasting effect once it
/// IS detached, and any consumer with its own explicit focus-restore-to-
/// trigger step (only `nav.rs`'s burger menu, currently) schedules that
/// through the close callback's own LATER macrotask, so the final
/// settled focus state is unaffected either way.
fn refocus_if_orphaned(ev: ev::FocusEvent) {
    let Some(container) = ev
        .current_target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return;
    };
    let left_sheet = match ev
        .related_target()
        .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
    {
        Some(node) => !container.contains(Some(&node)),
        None => true,
    };
    if !left_sheet {
        return;
    }
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        if !container.is_connected() {
            // A legitimate close already unmounted `.sheet` — nothing to do.
            return;
        }
        if let Some(active) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element())
        {
            let active_node: &web_sys::Node = active.as_ref();
            if container.contains(Some(active_node)) {
                // Focus already landed back inside `.sheet` on its own —
                // e.g. a click that focused a DIFFERENT still-enabled
                // descendant, which is a completely normal focus move,
                // not an orphaning.
                return;
            }
        }
        // #334 review follow-up: prefer a REAL focusable descendant over
        // the bare container when one exists. `left_sheet` above only
        // means focus moved outside `.sheet` — NOT that every descendant
        // is disabled (e.g. clicking inert content like `.sheet__title`
        // can blur the previously-focused button to `<body>` even while
        // other buttons are still enabled). Parking focus on `.sheet`
        // itself in that case would needlessly leave a real, clickable
        // control unfocused; querying fresh (rather than trusting
        // whatever `trap_tab` last saw) also self-heals the exact
        // container-focused state `trap_tab`'s own container fallback
        // exists for, the moment something becomes focusable again.
        let target = container
            .query_selector_all(FOCUSABLE_SELECTOR)
            .ok()
            .and_then(|list| list.get(0))
            .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
            .unwrap_or(container);
        if let Ok(html_el) = target.dyn_into::<web_sys::HtmlElement>() {
            let _ = html_el.focus();
        }
    });
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
                on:focusout=refocus_if_orphaned
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
