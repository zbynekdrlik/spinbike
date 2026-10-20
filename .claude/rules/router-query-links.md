---
paths:
  - "spinbike-ui/src/pages/**/*.rs"
  - "spinbike-ui/src/router.rs"
---

# In-app links with a `?query` — read the ROUTER's location, not `window.location` (#385)

A plain `<a href="/some/page?x=1">` inside the app is intercepted by
leptos_router 0.8 (client-side navigation). The router **renders the target
route first and pushes the URL afterwards**, so a component that parses
`web_sys::window().location().search()` while it is being built (or in an
untracked `Effect` queued during the build) still sees the OLD, empty query
— the deep link silently does nothing. A `page.goto(url)` E2E test (full
page load) does NOT catch this; only clicking the in-app link does.

Pick one, per target:

- The target reads the query through the router:
  `leptos_router::hooks::use_location().search.get_untracked()` (or
  `use_query_map()`) — `/staff/spinning?t=&d=` does this.
- The link forces a full load: `rel="external"` on the `<a>` (or
  `location().set_href(..)`), for targets that parse `window.location`
  themselves — the desk's `/staff?card=` / `?q=` jump (`pages/dashboard/
  mod.rs`) is one, so every link to it must be external.

Test the real path: click the in-app link in Playwright and assert the
preselected state, never only `page.goto` the URL.
