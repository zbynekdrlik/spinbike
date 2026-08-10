---
paths:
  - "spinbike-ui/Cargo.toml"
---

# Adding a new `web_sys::` API surface — verify the Cargo feature is actually declared (#320)

`spinbike-ui`'s `web-sys` dependency only compiles the specific browser
API bindings whose Cargo feature is enabled (e.g. `NodeList`, `HtmlElement`,
`Node`) — every `web_sys::Type::method()` is gated behind `#[cfg(feature =
"...")]` in the crate itself. **Feature unification across the whole
dependency graph means MANY features are already enabled transitively**
(leptos/tachys/gloo-net pull in a large set — `Document`, `Element`,
`HtmlElement`, `Node`, `Window`, etc. are usually already available even
though `spinbike-ui/Cargo.toml`'s own `web-sys` feature list looks short),
but a feature nothing else in the graph needs (e.g. `NodeList`, needed for
`Element::query_selector_all`) is genuinely NOT compiled in until you add
it explicitly, and using it without the feature is a **compile error CI
will catch** — costing a full CI cycle if missed, since this project's
Tier-0 local-build policy bans `cargo check`/`cargo build` from most
dispatched workers (see project CLAUDE.md's Pre-Push Checks).

**Before pushing code that uses a `web_sys::` type/method not already used
elsewhere in the codebase**, verify the feature gate WITHOUT compiling:

```bash
# 1. Find the exact feature(s) a method needs — grep the actual crate
#    source cached in the local registry (adjust the web-sys version to
#    match spinbike-ui/Cargo.lock's pinned version):
grep -n -B6 "pub fn query_selector_all" \
  ~/.cargo/registry/src/index.crates.io-*/web-sys-*/src/features/gen_Element.rs
# → look for the #[cfg(feature = "...")] line directly above the fn

# 2. Check whether that feature is ALREADY enabled transitively before
#    assuming you need to add it (cargo metadata resolution only, no
#    compile/codegen — safe under Tier-0, though it's a `cargo` invocation
#    outside the two sanctioned pre-push commands, so treat it as a
#    verification tool, not a routine local-build step):
cd spinbike-ui && cargo tree -e features -i web-sys | grep '"NodeList"'
# → no output means NOT enabled transitively; add it to Cargo.toml's
#   web-sys features list
```

If genuinely not present, add it to the `web-sys = { features = [...] }`
list in `spinbike-ui/Cargo.toml` (grouped near the other DOM-shape
features — `Document`/`Element`/`Node`/`NodeList`/`HtmlElement`).

Note: `cargo tree` can silently touch `spinbike-ui/Cargo.lock` (relocking a
path-dependency version bump, e.g. after `scripts/sync-version.sh`) — check
`git status` afterward and fold any resulting lockfile diff into your own
commit rather than leaving it stray.
