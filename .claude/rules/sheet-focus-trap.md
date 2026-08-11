---
paths:
  - "spinbike-ui/src/components/sheet.rs"
  - "spinbike-ui/src/components/nav.rs"
  - "spinbike-ui/src/pages/dashboard/sheets/**"
  - "spinbike-ui/src/pages/dashboard/deleted_email_conflict.rs"
  - "spinbike-ui/src/pages/reports/sheets/calendar_picker.rs"
  - "e2e/tests/nav-burger-customer.spec.ts"
  - "e2e/tests/sheet-focus-trap-disabled-window.spec.ts"
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

Most do (this file's own `close_backdrop`/`close_keyboard`, `nav.rs`'s
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
