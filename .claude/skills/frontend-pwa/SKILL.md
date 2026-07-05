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

## Manifest PNG icons: root `.gitignore` silently drops them

The repo's root `.gitignore` has `*.png` with an exception only for
`spinbike-ui/static/**`. Icons placed anywhere else (e.g.
`spinbike-ui/icon-192.png`, alongside `favicon.svg`) are silently excluded
from `git add` with no error. Use `git add -f` for these specific files and
verify with `git status --porcelain` that they show as staged (`A`, not
untracked `??`/`!!`) before committing — a missing PNG in the deployed
`dist/` means a broken manifest and an install-ineligible PWA, discovered
only by fetching the URL and getting 404 post-deploy.
