---
paths:
  - "spinbike-ui/src/components/sheet.rs"
  - "spinbike-ui/src/components/nav.rs"
  - "spinbike-ui/src/pages/dashboard/sheets/**"
  - "spinbike-ui/src/pages/dashboard/deleted_email_conflict.rs"
  - "spinbike-ui/src/pages/reports/sheets/calendar_picker.rs"
  - "e2e/tests/nav-burger-customer.spec.ts"
  - "e2e/tests/sheet-focus-trap-disabled-window.spec.ts"
  - "spinbike-ui/src/components/adaptive_nav.rs"
  - "spinbike-ui/src/pages/admin.rs"
  - "e2e/tests/ios-sheet-close.spec.ts"
  - "spinbike-ui/Trunk.toml"
---

# `Sheet`'s Tab-cycle focus trap — the container itself is a THIRD focus state, not just first/last descendant (#334)

`trap_tab` (`sheet.rs`) and `refocus_if_orphaned` (`sheet.rs`, #334) together
implement the WAI-ARIA dialog Tab-cycle trap. Both functions carry their own
deep doc comments explaining the exact event-timing reasoning — read those
in the file before changing either; this entry is the ONE-TIME gotcha that
cost a full review round to find, not a duplicate of that reasoning.

## `document.activeElement` can legitimately be `.sheet` ITSELF, not a descendant

`refocus_if_orphaned` (the `focusout` listener that recovers focus after the
browser auto-blurs a just-disabled control) falls back to focusing `.sheet`
itself — `tabindex="-1"`, the dialog container — whenever there is currently
NO real focusable descendant to prefer. `trap_tab`'s first/last-match wrap
logic **must** treat `active_node == container` as its own case (currently:
"before the first" — Tab enters at the first descendant, Shift+Tab wraps to
the last), never just `active_node == first_descendant` /
`active_node == last_descendant`. Missing this case is exactly how #334's
OWN fix briefly reintroduced its own bug in review: once the request that
triggered the all-disabled window later FAILED (controls re-enable, sheet
stays open), focus was left parked on the container and the very next Tab
press matched neither first nor last — no `prevent_default()`, so native tab
order escaped the dialog. **Any future change to `trap_tab`'s wrap logic
must re-check the container-focused case, not just the descendant-boundary
cases.**

## Not every `Sheet` consumer's `on_close`/`on_cancel` defers via `TimeoutFuture::new(0)`

Most do (this file's own `close_backdrop`/`close_button`/`close_keyboard`,
all through one shared `defer_close`, `nav.rs`'s
`on_close_menu`, `delete_user.rs`, `edit_pass_date.rs`, `edit_tx_date.rs`,
`edit_info_form.rs`) — but `calendar_picker.rs`'s Cancel/Confirm buttons and
`deleted_email_conflict.rs`'s Cancel button call their callback DIRECTLY,
synchronously, no macrotask defer at all. An earlier revision of
`refocus_if_orphaned`'s doc comment claimed the defer was universal across
all 8 call sites — wrong, caught in review. Before writing ANY new comment
or logic that assumes "every close path defers", `grep -n "on_close\.\|on_cancel\."`
across the 8 call sites and check each one, rather than assuming from the
majority pattern.

## Local verification limits — CI is authoritative for this file

`spinbike-ui` is Tier-0-restricted (no local `cargo build`/`cargo check`/
`cargo test`, no local Playwright) — any `web_sys::`/event-timing change
here can only be reasoned through by hand plus `cargo fmt --all --check`
(both repo root AND `spinbike-ui/`) and `cd e2e && npx tsc --noEmit`
locally; verify the actual DOM/focus behavior via a genuine second
(ideally independent, e.g. `superpowers:requesting-code-review` +
`/code-review`) review pass before merging, not just your own reasoning —
#334's fix needed a SECOND review round to catch the container-focus gap
above, which the implementer's own careful hand-timing analysis missed.

## Every overlay and sub-page needs an explicit on-screen exit — iPhone has no Escape, a standalone PWA has no back (#386)

The CEO got trapped on an installed iPhone PWA: the "Viac" sheet's only
exits were Escape (no key on iPhone) and a backdrop tap (iOS WebKit only
fires `click` on elements it deems clickable), and `/settings` had no way
back (standalone mode has no browser back button). Rules since #386:

- `Sheet` renders a header close button (`data-testid="sheet-close"`) for
  EVERY consumer. It has `tabindex="-1"`, and `FOCUSABLE_SELECTOR` excludes
  `[tabindex='-1']` on EVERY clause (not just the bare `[tabindex]` one) —
  that keeps it out of the #319 focus-on-open target and the #320/#334 trap
  boundaries, so the burger-menu focus specs stay valid. If you ever make it
  tabbable, those specs' first/last assertions change and must be updated
  in their own justified commit.
- That button ALSO needs `on:mousedown` → `prevent_default()`: Chromium
  focuses a clicked `tabindex="-1"` button, which is neither first, last nor
  the container for `trap_tab`, so a Shift+Tab from there escaped the
  dialog whenever the sheet stayed open (busy guard, drag-off release) —
  the same "third focus state" class as above, caught in #386's review.
  Script / screen-reader focus can still land there, so `trap_tab` treats
  ANY focused node outside the tab cycle like the focused container
  ("before the first"), not just `.sheet` itself.
- A link inside a sheet closes the sheet in its OWN `on:click` (synchronous
  `set_open.set(false)`, the `nav.rs` pattern) — never rely on the route
  change alone to unmount the dialog.
- `.sheet-backdrop` keeps `cursor: pointer` (the iOS tap insurance); `.sheet`
  resets `cursor: auto` because cursor is inherited.
- A sub-page reachable only from a sheet/menu (like `/settings`) carries its
  own back control in the page header (`settings-back`).
- Wide tables go inside `.table-scroll` (`overflow-x: auto`) — a page wider
  than the phone can be dragged sideways and displace the fixed bottom nav.
  All four admin tables are wrapped. The `.seg` segmented control scrolls on
  its own too: the SLOVAK admin tab labels (the owner's language) are ~391px
  of min-content at 390px — check width in `sk` too
  (`loginViaAPI(page, url, email, pw, 'sk')`; never stack a second language
  init script — Playwright does not define their order).
- iPhone behaviour is tested on the REAL WebKit engine: the `webkit-iphone`
  Playwright project (`devices['iPhone 13']`) runs only the specs matched by
  `WEBKIT_IPHONE_SPECS` in `e2e/playwright.config.ts`; CI's test-ui job
  installs `webkit`. Add an iPhone-only spec there, and use `tap()`.
  WebKit console gotchas (CI run 38712421537): do NOT emulate standalone
  (`setIosStandalone`) unless the code under test branches on it — every
  standalone boot fires the 2 s-min-gap launch beacon and back-to-back tests
  get a 429 console error; and never let Trunk preload the wasm
  (`spinbike-ui/Trunk.toml` `pattern_preload`) — WebKit never reuses an
  `as="fetch"` preload, downloads the wasm twice and warns.
  Local WebKit without root: `npx playwright install webkit`, `apt-get
  download` + `dpkg -x` the missing libs (list them with `ldd` on
  `~/.cache/ms-playwright/webkit-*/minibrowser-wpe/bin/MiniBrowser`, add the
  mesa EGL/dri packages), and launch via an `executablePath` wrapper that
  sets `LD_LIBRARY_PATH`, `__EGL_VENDOR_LIBRARY_DIRS`, `LIBGL_DRIVERS_PATH`,
  `EGL_PLATFORM=surfaceless` (the bundle's own wrapper overwrites
  `LD_LIBRARY_PATH`). Pair it with the CI `spinbike-server` artifact on
  :8099 to run the real spec.
