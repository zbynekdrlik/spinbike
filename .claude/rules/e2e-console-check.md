---
paths:
  - "e2e/tests/helpers.ts"
  - "e2e/tests/console-check-4xx-scoping.spec.ts"
---

# `setupConsoleCheck`'s `allow4xxFor` scoping — match the RESOURCE URL, never `msg.text()`

**#278 (two rounds — the first fix looked reasonable and still broke CI 23/218 tests):**
`setupConsoleCheck` filters a 4xx-derived browser console message
("Failed to load resource: the server responded with a status of 4xx")
ONLY when `allow4xxFor` names the endpoint the caller's own test expects
that 4xx from — see `ConsoleCheckOptions.allow4xxFor` in `helpers.ts`. The
FIRST attempt at this scoping matched `allow4xxFor` needles against
`msg.text()` — plausible-looking, and it compiled — but is **structurally
impossible to match**: Chromium's browser-generated "Failed to load
resource" console message **never contains the resource URL in its
text**, only the status code. Confirmed empirically with a standalone
Playwright script against a local HTTP server (`msg.text()` was exactly
`"Failed to load resource: the server responded with a status of 401
(Unauthorized)"`, no URL anywhere).

**The URL lives in `msg.location().url`, not `msg.text()`.** These
browser-level network-error messages are Chromium `Log.entryAdded`
events (not JS `console.*` calls), and Playwright surfaces their resource
URL through the `location` field, which for a JS `console.log()` call
would normally be the call-site source location. `setupConsoleCheck`'s
`isFiltered(text, locationUrl)` now matches `allow4xxFor` against
`locationUrl.includes(needle)`, fed from `msg.location().url` in the
`page.on('console', ...)` handler. `page.on('pageerror')` (uncaught JS
exceptions) has no such resource location and stays text-only — never
pass `allow4xxFor` needles expecting them to match a `pageerror` there.

**If you ever need to widen or add a similar opt-in filter, verify the
match works with a throwaway standalone Playwright script BEFORE trusting
it** — a `text.includes(needle)` check that "should" match by inspection
can be silently, permanently dead if the needle-bearing data never
actually lives in the field you're checking. `console-check-4xx-scoping.spec.ts`
is the regression test proving the scoping actually discriminates
(an unrelated 4xx surfaces; an allow-listed one is filtered) — extend it
rather than trusting a new filter clause by inspection alone.

## A negative assertion ("this message never arrives") needs a REAL synchronization point, not a guessed sleep

The "allow-listed 4xx IS filtered" half of the scoping test is a
negative-space assertion — proving something does NOT show up. A bare
`page.waitForTimeout(500)` before checking is a guess at how long the
whole async chain takes; under CI load the check can run before the
message would have arrived, passing for the wrong reason. Tie the wait to
the actual network event instead (`page.waitForResponse` matching the
specific URL + status the test triggers), keeping only a SHORT, explicitly
justified trailing buffer for the browser's own internal
`Log.entryAdded` dispatch (which is not proven to be strictly ordered
before the `fetch()`/`Response` promise you can `await`).
