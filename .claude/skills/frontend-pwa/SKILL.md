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
  - error_code
  - api.rs
  - localize error
---

# SpinBike Frontend / PWA gotchas

## Extending `api.rs` for only SOME callers: add an additive `_coded` variant, don't change the shared function's signature (#145)

`api.rs`'s `get`/`post`/`post_public`/`put`/`patch`/`delete`/`put_json` are
called from **~69 sites across 26 files** — changing any of their signatures
(e.g. `Result<T, String>` → `Result<T, SomethingRicher>`) ripples through
every caller even when only a handful actually need the richer error. #145
needed the server's `error_code` (#158) at exactly 5 customer-facing render
sites (login, my-balance, my-bookings x2, door, login-link-form) to localize
the banner — everywhere else (staff/admin pages, ~62 other call sites)
correctly keeps showing the server's raw English `error` text unchanged.

**Pattern: add a parallel `_coded` function per verb** (`get_coded`,
`post_public_coded`, `delete_coded`) that shares the transport logic but
returns a small named struct (`CodedError { code: Option<ErrorCode>,
message: String }`) instead of a bare `String`. The ORIGINAL function stays
byte-identical — zero ripple to the other ~62 sites. Only the render sites
that actually need to branch on the code switch their `Err(String)` →
`Err(CodedError)` handling (and their local error `Signal<String>` →
`Signal<Option<CodedError>>`).

**Parse the code defensively, decoupled from the message.** Deserialize
`error_code` as `Option<String>` first, THEN try `serde_json::from_value` into
the typed `ErrorCode` enum and `.ok()` the result. If you instead deserialize
directly as `Option<ErrorCode>` in the same struct as `error`, an unrecognized
code string (stale client during a rolling deploy, a future code the UI
doesn't know yet) fails the WHOLE body parse — including the perfectly good
human `error` message — and the user sees a generic "request failed" instead
of the server's real text. Degrade the code to `None`, never the message.

**Map codes to i18n keys via an EXHAUSTIVE match that explicitly returns
`None` for out-of-scope codes** — same shape as `tx_label_key` below.
`error_code_key(code: ErrorCode) -> Option<&'static str>` forces a compile
error when a new `ErrorCode` variant is added upstream, until someone
decides whether it needs a customer translation; codes intentionally left
`None` (staff_required, conflict codes, etc.) fall back to the raw server
text — that's the scope boundary, enforced by the compiler, not a comment.

**`api.rs` has no reactive `Lang` — use `i18n::get_saved_lang()` for its
OWN hardcoded fallback strings** ("session expired", "request failed").
`get_saved_lang()` reads the same `localStorage` key the reactive `Lang`
signal is initialized from and kept in sync with via `i18n::save_lang()` on
every toggle (`components/nav.rs`, `adaptive_nav.rs`) — safe to call from a
non-component module. Render-site-specific error TEXT, by contrast, should
localize via the page's own `lang.get()` at render time (reactive), not
baked in at error-set time — pass the raw `code`/`message` through the
signal, localize in the `view!` closure.

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

## When two functions both need `navigator.userAgent` (or any other shared JS read): fetch it ONCE and pass it down (#226)

`is_ios_ua()` and a later `is_ios_webview_ua()` (in-app-browser detection,
#226) each independently did their own `window` -> `navigator` ->
`userAgent` `Reflect::get` round-trip — duplicated logic AND a wasted extra
JS/WASM FFI call on every mount for every iOS visitor. Caught by review, not
by CI (clippy has no lint for "you re-derived the same JS value twice").
Fix: pull the shared read into its own tiny function (`fn user_agent() ->
String`), call it ONCE at the top of the decision function that needs both
checks (`detect_kind()`), and pass the `&str` down to each predicate
(`is_ios_ua(ua: &str)`, `is_ios_webview_ua(ua: &str)`). Whenever a new
UA-sniffing (or any other `Reflect`-based) predicate is added alongside an
existing one, check whether they read the same underlying JS property
before writing a second independent fetch.

## Sharing platform-detection across components: a dedicated `src/platform.rs`, not a re-export from the first component that needed it (#228)

`is_standalone()`/`is_ios_ua()`/`user_agent()`/`get_prop()`/`window_value()`
originally lived private inside `components::install_prompt` (#110/#226).
#228 needed the SAME "installed standalone + iOS" detection from a second,
unrelated component (`CustomerLoginMethods` in `code_login_form.rs`, to
reorder its login-method toggle). Reaching into another component module
(`crate::components::install_prompt::is_standalone`) would work but wires two
components together for a concern that belongs to neither — and the fns
would need to go from private to `pub(crate)` on a component module anyway.

**Fix: promote to a crate-root module** (`spinbike-ui/src/platform.rs`,
`pub mod platform;` in `lib.rs`, every fn `pub(crate)`) — moved VERBATIM (no
rewrite) to avoid a refactor-introduces-a-bug risk, confirmed byte-identical
by an independent review pass. Added one new composite,
`is_ios_standalone() = is_standalone() && is_ios_ua(&user_agent())`, so a
THIRD call site never has to re-derive the AND itself. Whenever a THIRD
component needs UA/standalone detection, add to `platform.rs`, never
re-import from whichever component happened to define it first.

## E2E-testing "installed standalone PWA" state: `navigator.standalone` override is enough — `matchMedia` stubbing is NOT needed (#228)

`is_standalone()` checks the legacy iOS `navigator.standalone` flag FIRST and
only falls through to the `(display-mode: standalone)` media query if that
flag isn't `true` — so a Playwright test simulating "installed on iOS" only
needs the ONE override, via `page.addInitScript` (must run before the WASM
bundle loads):

```ts
await page.addInitScript(() => {
    Object.defineProperty(window.navigator, 'standalone', { get: () => true });
});
```

(Codified as `setIosStandalone(page)` in `e2e/tests/helpers.ts`.) Combine
with an iOS UA context (`devices['iPhone 13']`, same `test.use()` pattern as
the existing iOS-Safari-guide tests) for `is_ios_standalone()` to be true, and
ALWAYS pair with a negative case using an Android UA — since `standalone` is
an iOS-only flag with no real meaning on Android, applying the SAME override
under an Android UA and asserting nothing reorders proves the gate is keyed
on the UA check, not merely on the standalone flag being true.

## `navigator.clipboard.writeText()` must be dispatched SYNCHRONOUSLY from the click handler, not after a `spawn_local`/`.await` hop (#226)

The natural way to wire up a "copy to clipboard" button in this codebase's
established async-JS-interop style is `on:click = |_| spawn_local(async
move { clipboard.writeText(...).await })` — but that puts the actual
`writeText()` call one microtask hop AFTER the triggering click event, since
`wasm_bindgen_futures::spawn_local`'s first poll happens on a queued
microtask, not synchronously in the same call stack as the event handler.
Some stricter WebKit/Safari builds only honor the Clipboard API's required
user-activation ("this write is a direct result of a real user gesture")
when the call is dispatched synchronously from the originating event — an
async hop can silently lose that activation and the write fails with no
visible error (this codebase's interop rule already requires every JS call
to degrade to a silent no-op on failure, so a lost-activation failure looks
exactly like "the button did nothing").

**Fix: split the JS call from the await.** A sync function fires the actual
API call and returns the resulting `Promise` (or `None` on any unavailable
step); the click handler calls that function DIRECTLY (still inside the
event handler, no `spawn_local` yet); only the *returned Promise* is handed
to `spawn_local`/`.await` to update UI state once it resolves:

```rust
fn start_copy_current_url() -> Option<Promise> {
    // ... all the get_prop/dyn_ref/call1 JS calls happen HERE, synchronously ...
    Some(Promise::resolve(&result))
}

let on_copy_click = move |_| {
    let Some(promise) = start_copy_current_url() else { return; };  // sync, in the click handler
    spawn_local(async move {
        if JsFuture::from(promise).await.is_ok() { set_copied.set(true); }  // only the AWAIT is async
    });
};
```

Apply this split to ANY future button that calls a user-activation-gated
browser API (clipboard, fullscreen, payment sheets, etc.) from this
codebase's `spawn_local`-based click-handler idiom — not just clipboard.

## A component mounted on MULTIPLE pages must consider EVERY page's URL state, not just the one you're picturing

`InstallPrompt` mounts on both `/welcome` (right after a magic-link token is
redeemed) and `/my/balance`. A `start_copy_current_url()`-style "copy the
current URL" feature that reads `location.href` verbatim copies whatever is
in the address bar at that moment — including a one-time `?t=<token>` query
string that `pages/welcome.rs` never strips after redeeming it (no
`history.replaceState`/router navigate call exists there). A webview user
tapping the resulting copy-URL button on `/welcome` would copy their own
already-spent, now-invalid token, landing back on the "invalid link" screen
when pasted into Safari — silently defeating the whole point of the button,
for the EXACT page the feature was built to help with. This shipped past
the first review round because that round's E2E-reachability check only
covered `/my/balance` (no query string in sight); a SECOND, independent
deep-pass review caught it by walking through the issue's own stated
primary trigger scenario end-to-end.

**Rule: when building a "copy/read/report the current URL" feature on a
component mounted on more than one route, enumerate every mount site's
possible query-string/hash state BEFORE deciding what to read off
`location`.** Prefer `location.origin + location.pathname` (drops any query
string and hash) unless the query string is provably safe to keep. Add an
E2E test that exercises the mount site with the MOST state in its URL
(here: a page with a leftover token param), not just the cleanest one.

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

## Post-deploy version verification: clear the service worker BEFORE reading the DOM version

`sw.js` (fixed in #208 / PRs #210+#211, dev.75) routes by
**`request.mode === 'navigate'`** — the canonical SW discriminator:
- **navigations** (the app shell + EVERY SPA route: `/`, `/login`, `/dashboard`,
  `/my/balance`, …) → **network-first** (always picks up a fresh deploy, keeps an
  offline cache fallback). Self-adapts to any new route — no URL list to maintain.
- **everything else** (subresources) → **cache-first**. NOTE: this app's Trunk
  bundle is served at the **ROOT** (`/spinbike-ui-<hash>.js`, `_bg.wasm`), NOT
  under `/assets/` (`/assets/…` 404s; the 2.4 MB WASM has NO cache-control) — the
  first #208 attempt (#210) used a `/assets/` prefix and wrongly dropped the
  root bundle onto network-first (2.4 MB re-download per navigation); `request.mode`
  routing fixes that regardless of asset path. A `Content-Type: text/html` guard
  keeps a stray SPA-fallback HTML out of the cache-first store.
- `/api/*` + `/ws*` bypass the SW; `CACHE_NAME` (currently `spinbike-v3`) bumped
  on breaking changes → `activate` purges every non-current cache.

**Testing `sw.js` deterministically** (`e2e/tests/sw-cache.spec.ts`): a real
browser can't force a mid-run "new deploy", so the test loads the REAL
`spinbike-ui/sw.js` into a mocked `ServiceWorkerGlobalScope` via Node's `vm`
(mock `self`/`caches`/`fetch`, capture the `addEventListener` handlers, drive
synthetic FetchEvents with `request.mode`) and asserts network-first-vs-cache-first
outcomes across a simulated deploy. Deterministic, server-independent, runs in the
normal Playwright job. Set `request: { url, mode }` on the synthetic event — the
`'navigate'` mode is what routes to network-first. When editing `sw.js`, update
this test; it FAILS on the old URL-shape `isVolatile()` and on a `/assets/`-only
rewrite (the root-bundle regression).

**The pre-#208 bug (now fixed):** the old `isVolatile()` URL-shape check only
network-first'd `/` + `*.html`, so any SPA route got cache-first-pinned FOREVER
on its first-visited version (reproduced live #201: `/login` stuck on `.65` while
prod served `.71`). If you see a route stuck on an old version now, it is NOT this
bug — suspect either profile-staleness (below) or the (also now fixed) **#212**
CDN-caching issue described in the next section.

## `/sw.js` needs BOTH an origin `Cache-Control` header AND a Cloudflare Cache Rule — neither alone is enough on the Free plan (#212)

`/sw.js` used to be served with **no** `Cache-Control` header at all
(`static_handler` in `crates/spinbike-server/src/routes/static_files.rs` only
special-cased `assets/`-prefixed hashed files). Cloudflare's default
extension-based edge caching then applied its own `max-age=14400` (4h) to it —
confirmed live: `cf-cache-status: HIT`, `age` climbing toward 14400. A NEW SW
script (including SW-logic fixes like #208) could take up to 4h to reach real
users after a deploy.

**Fix has TWO layers, both required — this is the non-obvious part:**

1. **Origin header** — `static_handler` now special-cases the exact path
   `sw.js` (an `else if` sibling of the `assets/` branch) and sets
   `Cache-Control: no-cache` (not `no-store` — `no-cache` still permits cheap
   ETag/conditional-GET revalidation; `no-store` would forbid caching
   entirely and gains nothing here). `manifest.json` and HTML documents are
   deliberately untouched — they were already `cf-cache-status: DYNAMIC` on
   prod (Cloudflare doesn't classify them as cacheable by extension), so they
   never needed this.
2. **Cloudflare Cache Rule** — on the **Free "Website" plan**, a zone has NO
   "respect origin Cache-Control" toggle (that's Enterprise-only). Both
   zones this app is served from (`spinbike.sk` and the shared
   `newlevel.media` zone, which also carries `spinbike-dev.newlevel.media`)
   have a fixed `browser_cache_ttl = 14400` zone setting that Cloudflare
   injects into the EDGE response for any extension it classifies as
   cacheable (`.js` included) — **regardless of what the origin sends**.
   Verified live: even after the origin fix deployed, `curl spinbike.sk/sw.js`
   kept cycling `cf-cache-status: EXPIRED`/`HIT` with `cache-control:
   max-age=14400` in the response, completely ignoring the origin's
   `no-cache`. The origin header change alone does NOT stop Free-plan edge
   caching for a normally-cacheable extension.

   The fix: a **Cache Rule** (the modern Rulesets-API replacement for Page
   Rules — the legacy Page Rules REST endpoint (`/zones/{id}/pagerules`)
   rejects account-owned API tokens with `code 1011`, so use Rulesets)
   bypassing cache entirely for `/sw.js`, which makes Cloudflare treat it
   exactly like the already-`DYNAMIC` `manifest.json`/HTML paths — origin
   headers pass through untouched, never edge-cached:
   ```bash
   # mint a scoped token (account master token can only MINT tokens, not use them
   # directly — see reference_spinbike_infra_creds.md memory) with Zone Read +
   # Zone/Cache Settings Read+Write + Cache Purge, then:
   curl -X PUT "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/rulesets/phases/http_request_cache_settings/entrypoint" \
     -H "Authorization: Bearer $SCOPED_TOKEN" -H "Content-Type: application/json" \
     --data '{"name":"default","rules":[{
       "expression":"(http.request.uri.path eq \"/sw.js\")",
       "action":"set_cache_settings",
       "action_parameters":{"cache": false}
     }]}'
   # then purge the already-cached copy once:
   curl -X POST "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/purge_cache" \
     -H "Authorization: Bearer $SCOPED_TOKEN" -H "Content-Type: application/json" \
     --data '{"files":["https://spinbike.sk/sw.js"]}'
   ```
   `PUT .../entrypoint` REPLACES the whole cache-settings-phase ruleset for
   the zone — always `GET` it first to check for pre-existing rules before
   overwriting (both zones here had none for this phase, confirmed via a 404
   `"could not find entrypoint ruleset"` before creating it). On the shared
   `newlevel.media` zone (hosts unrelated services too — bakerion-ai,
   presenter, codex-bridge, etc.), scope the expression by hostname as well
   (`and (http.host eq "spinbike.newlevel.media" or http.host eq
   "spinbike-dev.newlevel.media")`) so the rule can't affect another
   service's `/sw.js`. **This CDN config lives ONLY in the Cloudflare zones —
   it is NOT in git.** If a future change ever needs to touch it again, the
   two zone IDs are `048113ccaacb5872c9af2df65eb5f0c8` (spinbike.sk) and
   `b9019ca528e573e62c2a110a45f45c74` (newlevel.media); mint a fresh scoped
   token each time (temp tokens used for #212 were revoked after use — don't
   leave a standing Cache-Settings-Write token lying around).

**Verifying it actually worked** needs BOTH the origin AND the edge check —
checking only one can lie:
```bash
curl -sD- -o /dev/null https://spinbike.sk/sw.js | grep -iE 'cache-control|cf-cache-status'
# want: cache-control: no-cache  AND  cf-cache-status: DYNAMIC (never HIT/EXPIRED)
```
If `cf-cache-status` still cycles `HIT`/`EXPIRED`, the Cache Rule is missing
or hasn't propagated (propagation is fast, seconds — if it's still wrong after
~30s, check the rule actually saved: `GET
.../rulesets/phases/http_request_cache_settings/entrypoint`). If
`cache-control` is missing entirely, the ORIGIN fix (the code) isn't deployed
yet — separate problem, redeploy.

Separately (pure profile staleness — a long-lived MCP profile carrying a stale
SW registration, unrelated to the fixed #208 strategy): a **long-lived Playwright
MCP browser profile** (reused
across many autopilot cycles/days, not a fresh per-run context like CI's
Smoke job) can carry an ALREADY-ACTIVE service worker registration from an
earlier session. Reloading/navigating in that profile is not guaranteed to
pick up the newly-deployed version on the very first read — the DOM's
`"Verzia aplikacie"` label can show a version several releases behind
`/api/version` (found post-#152-deploy: DOM showed `v0.15.0-dev.30` while the
backend already served `v0.15.0-dev.43`; recurred post-#201: DOM showed
`.65` vs backend `.71`).

**Before trusting a DOM version read in a long-lived Playwright session**,
clear any stale registration first:

```js
async () => {
  const regs = await navigator.serviceWorker.getRegistrations();
  for (const r of regs) { await r.unregister(); }
  if (window.caches) {
    const keys = await caches.keys();
    for (const k of keys) { await caches.delete(k); }
  }
}
```

...then re-navigate. Confirm the fix actually landed by comparing the fresh
HTML's hashed asset filename via `curl -s https://<host>/ | grep -oE
'<app>-[a-f0-9]+\.(js|wasm)'` against what the browser loaded — a mismatch
before the clear, matching after, confirms it was profile-local staleness,
not a real deploy failure. This does NOT affect Smoke (prod)/Smoke (dev) CI
— those jobs use a fresh Playwright browser context per run, so they never
carry a stale SW registration across deploys.

## Catching a fast-resolving loading state live: use in-page `requestAnimationFrame`, not a full MCP snapshot round-trip

Verifying a brief loading/in-flight UI state (e.g. #152's "sending" button
text) against a REAL prod backend is harder than against a Playwright E2E
test's artificial network delay — prod round-trips can resolve in well under
one MCP tool round-trip, so a `browser_click` followed by a separate
`browser_snapshot` call often already shows the POST-resolution state (the
success alert), missing the transient loading state entirely. Catch it with
a single `browser_evaluate` that clicks and polls in-page, no MCP round-trip
in between:

```js
async () => {
  const btn = document.querySelector('[data-testid="...-submit"]');
  btn.click();
  const results = [];
  for (let i = 0; i < 10; i++) {
    await new Promise(r => requestAnimationFrame(r));
    results.push({frame: i, text: btn.textContent, disabled: btn.disabled});
    if (!document.body.contains(btn)) break;
  }
  return JSON.stringify(results);
}
```

## Manifest PNG icons: root `.gitignore` silently drops them

The repo's root `.gitignore` has `*.png` with an exception only for
`spinbike-ui/static/**`. Icons placed anywhere else (e.g.
`spinbike-ui/icon-192.png`, alongside `favicon.svg`) are silently excluded
from `git add` with no error. Use `git add -f` for these specific files and
verify with `git status --porcelain` that they show as staged (`A`, not
untracked `??`/`!!`) before committing — a missing PNG in the deployed
`dist/` means a broken manifest and an install-ineligible PWA, discovered
only by fetching the URL and getting 404 post-deploy.

## UI date parsing/formatting lives in ONE place: `spinbike-ui/src/dates.rs` (#168)

Do NOT re-inline an ISO date parser or a `DD.MM.YYYY` renderer. Two shared helpers:

- `dates::parse_server_date(&str) -> Option<NaiveDate>` — trims + takes the
  first whitespace token + the part before any `T`, then parses `%Y-%m-%d`. Use
  it for ANY server-supplied date/timestamp (`"2026-04-25"`,
  `"2026-04-25 18:00:00"`, `"2026-04-25T18:00:00Z"`). It's a safe superset — a
  bare ISO date is unaffected; it only strips a trailing time component.
- `dates::format_ddmmyyyy(NaiveDate) -> String` — the shared `DD.MM.YYYY` digit
  renderer behind BOTH `i18n::fmt_date`'s Sk arm AND `relative_date::format_date`.

**Three neighbours are deliberately SEPARATE — do NOT fold them into the above
(it would be a bug):**
- `components::date_input::parse_user_date` — a 9-format LENIENT parser for
  interactive typing (2-digit years, slash/space). Distinct from the strict
  server parser.
- `relative_date::format_date` — deliberately locale-INDEPENDENT (always
  DD.MM.YYYY, even for English staff, because `card_panel` passes staff `lang`
  which can be `En` and `i18n::fmt_date` returns ISO for En). It shares only the
  DIGITS via `format_ddmmyyyy`, never the locale policy. Routing it through
  `i18n::fmt_date` regresses English staff display.
- `i18n::fmt_date_short` — already the canonical short-date (`DD.MM.`) formatter.

For a server timestamp you need converted UTC→Bratislava (not just the date),
use the now-`pub` `i18n::parse_to_local(&str) -> Option<DateTime<Tz>>` (DST-aware,
handles fractional-seconds + legacy MS-Access forms), then `.date_naive()` /
format off it — don't hand-roll the `from_utc_datetime` conversion.

## GOTCHA: the UI CI gate is `clippy --target wasm32 -D warnings` — `Test (UI)` passing does NOT mean the build passes

`Build WASM (UI)` runs `cargo clippy --manifest-path spinbike-ui/Cargo.toml
--all-targets --target wasm32-unknown-unknown -- -D warnings` BEFORE `trunk
build`. So a clippy LINT (not just a compile error) fails the whole UI build —
and `Test (UI)` (`wasm-pack test --node`) can go GREEN on the same commit
because it only *compiles* the lib, it doesn't run clippy. Under the Tier-0
no-local-builds policy you cannot run clippy locally (only `cargo fmt --all
--check` + `cd e2e && npx tsc --noEmit`), so these lints surface ONLY on CI and
cost a full ~15-min cycle. Write clippy-clean UI Rust the first time:

- **`clippy::collapsible_if`** — nested `if A { if let Some(x) = y { … } }` must be
  a let-chain: `if A && let Some(x) = y { … }` (edition 2024 UI, let-chains are
  supported — see `components::install_prompt.rs`, `api.rs`). This is the exact
  lint that failed #143's first UI push.
- Same for the other default `-D warnings` lints (`needless_return`,
  `redundant_clone`, `let_and_return`, …) — the UI is held to `-D warnings`, so
  any warning is a hard build failure.
- Multi-root `view! { <A/> {closure} }` (a fragment / tuple view) IS valid and
  compiles — if `Test (UI)` compiled it, `trunk build` will too. The wasm32
  clippy pass is the ONLY extra gate `trunk build` adds over `wasm-pack test`.
