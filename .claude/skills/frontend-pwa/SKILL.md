---
name: spinbike-frontend-pwa
description: >
  SpinBike PWA-specific frontend gotchas: raw JS interop via js_sys::Reflect
  for untyped browser APIs (no web-sys binding exists), and iOS/iPadOS UA
  detection pitfalls. Load before touching anything in spinbike-ui that talks
  to a browser API without a typed web-sys binding, or any UA-sniffing logic.
triggers:
  - beforeinstallprompt
  - js_sys::Reflect
  - navigator.userAgent
  - iOS detection
  - manifest.json icons
---

# SpinBike Frontend / PWA gotchas

## Untyped JS interop via `js_sys::Reflect` (no prior use in this repo before #110)

Some browser APIs have **no typed `web-sys` binding** — `beforeinstallprompt`
is Chromium-only and non-standard, `navigator.standalone` is an iOS
Safari-only legacy flag. `spinbike-ui`'s existing `web-sys` feature list
(`Cargo.toml`) doesn't cover them and adding features wouldn't help (they
just aren't in the web-sys IDL at all). The pattern that works, with zero new
Cargo.toml dependencies (only `js-sys` + `wasm-bindgen`, both already deps):

```rust
fn get_prop(target: &JsValue, key: &str) -> JsValue {
    Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}
// navigator.userAgent, navigator.standalone, navigator.platform,
// navigator.maxTouchPoints, window.__myGlobal — all read this way.
let navigator = get_prop(&window, "navigator");
let ua = get_prop(&navigator, "userAgent").as_string().unwrap_or_default();
```

For calling an untyped method (e.g. a captured event's `.prompt()`):

```rust
if let Some(f) = get_prop(&event, "prompt").dyn_ref::<js_sys::Function>() {
    if let Ok(result) = f.call0(&event) {
        let _ = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&result)).await;
    }
}
```

`js_sys::Promise::resolve(&value)` safely wraps ANY `JsValue` into a
`Promise` (even if it's already one) — always use it before `JsFuture::from`
rather than trying `value.dyn_into::<Promise>()`, which can fail if the
runtime's `Promise` isn't recognized as the exact instance type in some
edge cases.

**Every `get_prop`/`dyn_ref`/`call` in this pattern degrades to a silent
no-op on failure** (`unwrap_or`, `if let Some`, `if let Ok`) — never
`.unwrap()`/`.expect()` on JS interop, since the property may legitimately
be absent (feature not supported on this browser) and that must never panic
the whole WASM app.

## iOS UA-sniffing: `"iPhone"`/`"iPad"` substring match MISSES real iPads

Since **iPadOS 13** (2019), Safari defaults to "Request Desktop Website" —
a real iPad's `navigator.userAgent` reports as a plain Mac
(`Macintosh; Intel Mac OS X ...`) with **no** `"iPad"` substring at all. A
bare `ua.contains("iPad")` check will never match a stock-configured iPad
(found in #110 code review, AFTER the buggy version had already merged +
deployed — caught it via an independent review agent, shipped the fix as a
fast-follow PR).

**Fix — the standard disambiguator:** a genuine Mac reports zero touch
points; an iPad (even UA-spoofed as a Mac) reports
`navigator.maxTouchPoints > 1`:

```rust
if ua.contains("iPhone") || ua.contains("iPad") {
    return true;
}
let platform = get_prop(&navigator, "platform").as_string().unwrap_or_default();
let max_touch_points = get_prop(&navigator, "maxTouchPoints").as_f64().unwrap_or(0.0);
platform == "MacIntel" && max_touch_points > 1.0
```

E2E-test this by overriding BOTH properties via `page.addInitScript` (a
Playwright device descriptor alone won't model this — none of the built-in
descriptors simulate iPadOS's desktop-UA default):

```ts
await page.addInitScript(() => {
    Object.defineProperty(window.navigator, 'platform', { get: () => 'MacIntel' });
    Object.defineProperty(window.navigator, 'maxTouchPoints', { get: () => 5 });
});
```

Always pair it with a **negative** test (`maxTouchPoints: 0`, i.e. a real
Mac) asserting neither install surface renders — the disambiguator is easy
to get backwards and silently show the guide to real Mac desktop users.

## Splitting a shared status signal into two (success/error) needs a structural mutual-exclusion Effect, not point-fixes

`#126` split the dashboard's single `msg`/`set_msg` status channel into
`msg` (green `.alert-success`) + `err` (red `.alert-error`) so errors would
stop rendering as green successes. The naive fix — repoint each error
branch's `set_msg.set(...)` to `set_err.set(...)` — is correct but
INCOMPLETE by construction: with two independent signals, nothing stops a
stale alert in one channel from surviving (or stacking with) a fresh value
in the other, and every writer of either signal (including ones outside the
files you're allowed to touch, e.g. `action_form.rs`'s own success calls
into the SHARED `set_msg`) is a place the bug can leak from. Three review
rounds on PR #132 kept finding one more leaking call site each time
(block/edit/transactions → panel-close/pick-card/search-effect →
`DeleteUserSheet`'s second close path) before landing on the actual fix: a
single reactive **mutual-exclusion `Effect`** at the top of the component
that owns both signals:

```rust
Effect::new(move |prev: Option<(String, String)>| {
    let m = msg.get();
    let e = err.get();
    if let Some((prev_m, prev_e)) = prev {
        if m != prev_m && !m.is_empty() && !e.is_empty() {
            set_err.set(String::new());
        } else if e != prev_e && !e.is_empty() && !m.is_empty() {
            set_msg.set(String::new());
        }
    }
    (m, e)
});
```

This makes "at most one alert visible" hold for EVERY writer — including
ones you're told not to touch — because the effect watches the signals
themselves, not the call sites. It converges in ≤2 re-runs per transition
(the `.set()` inside the effect re-triggers it once more, which then sees
the just-cleared value and no-ops) — safe, no infinite loop. **When
splitting any shared Leptos status/alert signal into two, write this
effect FIRST, then the point-fixes become defense-in-depth rather than the
whole fix.**

## An error for an action inside a `Sheet` MUST render INSIDE the sheet — the shared dashboard alert is occluded by the sheet backdrop

The dashboard's shared red/green alerts (`mod.rs`, the `err`/`msg` signals)
render in the page body. A `Sheet` is a full-viewport `position: fixed;
z-index: 200` blur backdrop laid OVER that body. So any alert routed to the
shared channel while a sheet is OPEN renders BEHIND the backdrop and is
invisible — the operator sees the action "do nothing" with no reason. This
bit `edit_info_form`'s Save: a rejected save (e.g. the 409 email-uniqueness
conflict) set `set_err` on the shared channel, the sheet stayed open, and the
error was never seen.

Two correct patterns, pick by whether the sheet stays open:
- **Sheet closes on the action's outcome** (like Invite): route to the shared
  channel AND close the sheet on either outcome, so the now-visible body shows
  it. `on_close.run(())` in both Ok and Err arms.
- **Sheet stays open to fix inline** (like Save): give the form its OWN local
  error signal and render it as `<div class="alert alert-error"
  data-testid="…">` INSIDE the sheet's `<form>` (it's sheet content, so it's
  above the backdrop). Clear it at submit-start and when switching to another
  in-sheet action. Do NOT rely on the shared channel — it's occluded.

Playwright's `toBeVisible()` does NOT detect z-index occlusion (the shared
alert is in the DOM, just covered), so an E2E test asserting the shared
`.alert-error` PASSES against this bug. Assert a sheet-scoped
`data-testid` inside the open sheet instead — that only exists once the error
renders in-sheet.

## Leptos disposal-ordering trap: write LOCAL signals BEFORE any disposal trigger

`reactive_graph` (0.1.8) **panics** — `"Tried to access a reactive value that
has already been disposed"` → a WASM `RuntimeError: unreachable` — when a
LOCAL/component-owned signal is `.set()` AFTER something that synchronously
disposes the component. In `edit_info_form` the disposal triggers are:

- `set_selected.set(Some(c))` — the parent (`mod.rs`) tracks `selected` and
  synchronously **rebuilds/disposes** the edit sheet subtree when it changes.
- `on_close.run(())` / `show=false` — also tears the sheet down.

So in a submit/invite handler, once you fire one of those, the component's own
signals (`set_loading`, `set_invite_loading`, a local `set_email_sig`, a local
error signal) are **already disposed** — writing them panics. The nasty part:
the panic fires AFTER the user-visible effects (the sheet has already closed and
shown its success alert), so it's **invisible in production** but shows up as
console errors that fail every clean-console Playwright assertion.

**Rule — order every outcome handler as:**

1. Write ALL local/component-owned signals FIRST (`set_loading.set(false)`,
   local email mirror, local error signal), THEN
2. fire the disposal triggers LAST (`set_selected.set(...)`, then `set_msg`/
   `set_err` which are PARENT-owned props → safe, then `on_close.run(())`).

Only **parent-owned signals passed in as props** (`set_msg`, `set_err`) and
**callbacks** (`on_close`) are safe to touch after disposal — they live in the
parent, which is not being disposed. A local signal is NOT.

```rust
// Ok arm of on_invite_click — LOCAL first, disposal LAST:
set_email_sig.set(saved.email.clone().unwrap_or_default()); // local
set_invite_loading.set(false);                              // local  ← BEFORE
set_selected.set(Some(saved));      // parent-tracked → DISPOSES this component
set_msg.set(t.invite_sent.into()); // parent-owned prop → safe after disposal
on_close_after_invite.run(());     // callback → safe
```

A reviewer may claim "setting a disposed signal is a silent no-op" — it is
**not**, it panics `unreachable`. Verify with an E2E test that clicks the
action and asserts `expect(consoleMessages).toEqual([])` AFTER the sheet closes
(found + fixed while shipping #141's one-click save-then-invite).

## Transaction/movement rows: reuse the shared classifier, never render the raw DB `action`

A transaction row's kind comes from `spinbike_core::reports::classify(action,
amount, valid_until) -> EventKind`, mapped to an i18n key via
`i18n::tx_label_key(kind)` (`tx_label_pass`/`tx_label_visit`/`tx_label_charge`/
`tx_label_topup`/`event_other`). ANY surface that lists transactions MUST route
through that — the DB stores raw English tokens (`topup`/`charge`/`visit`/
`storno`), so rendering `{t.action}` directly leaks English into the Slovak UI
(the exact `/my/balance` "nema slovencinu na pohyboch" bug, #144). The admin
`dashboard/transactions_list.rs` and the customer `pages/my_balance.rs` now both
call `tx_label_key` — add a new consumer the same way, don't re-inline the match.
Amounts: signed `{:+.2}` + `list-row__amount--pos`/`--neg` (theme-aware colour),
not an unsigned `€{:.2}` (which also misprints negatives as `€-5.00`). Pass-sale
rows append the expiry via `tx_until_short` + `fmt_date_short`. Reuse the
`.list-row`/`list-row__main`/`__title`/`__sub`/`__amount` primitive (theme vars,
56px tap height) — bespoke per-page row CSS tends to hardcode light-mode hex and
break dark mode.

## Door notes are stored English (`door: Nth`) — localize on DISPLAY only

`door.rs` writes the visit note as `"door: 1st"`/`"door: 2nd"` (English ordinals
via `util::ordinal`). Do NOT change the stored value to localize it: `door.rs`'s
same-day re-entry count query (`note LIKE 'door:%'`) AND the admin note view both
depend on that literal format. Instead localize at render: match a note starting
`"door: "`, take the leading digit run, and show it via the `door_note_reentry`
i18n key (`"Vstup c. {}"` / `"Entry #{}"`), falling back to the raw note when
there's no digit (`my_balance.rs`, #144). Same rule for any stored-English audit
string a customer sees.

## Manifest PNG icons: root `.gitignore` silently drops them

The repo's root `.gitignore` has `*.png` with an exception only for
`spinbike-ui/static/**`. Icons placed anywhere else (e.g.
`spinbike-ui/icon-192.png`, alongside `favicon.svg`) are silently excluded
from `git add` with no error. Use `git add -f` for these specific files and
verify with `git status --porcelain` that they show as staged (`A`, not
untracked `??`/`!!`) before committing — a missing PNG in the deployed
`dist/` means a broken manifest and an install-ineligible PWA, discovered
only by fetching the URL and getting 404 post-deploy.
