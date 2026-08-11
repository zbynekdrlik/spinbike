---
paths:
  - "spinbike-ui/src/pages/admin.rs"
  - "spinbike-ui/src/pages/staff_dashboard.rs"
  - "spinbike-ui/src/pages/dashboard/action_form.rs"
  - "spinbike-ui/src/api.rs"
  - "crates/spinbike-server/src/routes/admin.rs"
---

# Admin/staff UI — never fall back to zero, never swallow an error

Two failure shapes kept recurring across the admin and staff pages, and both are
invisible in testing because the UI reports success either way. A `/code-review`
pass found six live instances at once (fixed 2026-08-11).

## 1. A parse failure must never fall back to a value

`parse_money(...).unwrap_or(0.0)` and `.parse().unwrap_or(0)` turn a typo into a
silently persisted zero:

- a service price saved as `0.00` makes quick-charge, the door single-entry flow
  and the 4-hour auto-charger all bill nothing — free classes until somebody
  reads the revenue report;
- a class-template capacity saved as `0` makes every generated class
  full-from-empty — customers get "class full" on an empty class.

**Rule:** on a parse failure, set the page's error banner and `return` WITHOUT
sending the request. Never `unwrap_or` a domain value into existence.

**The value zero is not the bug — the silent fallback is.** A deliberately free
service is plausible and must stay possible, so the server rejects only a
NEGATIVE price. A capacity of `0` is genuinely meaningless and is rejected on
both sides.

## 2. `let _ = api::...` is never acceptable on a write

Every discarded `Result` on a mutating call reports success on failure. The
worst instance: `on_cancel_class` discarded its result AND cleared the loading
flag AND bumped the refresh counter, so a failed cancel looked identical to a
successful one — while the class stayed live and the T-4h charger then billed
every booked customer for a class the gym believed it had cancelled.

**Rule:** `match res { Ok(_) => ..., Err(e) => set_<banner>.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])) }`,
the shape every correct sibling handler in these files already uses.

## 3. A 204 endpoint needs `api::post_no_content`, not `api::post`

`api::post` unconditionally calls `resp.json::<T>()` on any 2xx. gloo-net's
`Response::json` parses the body text, and an EMPTY body always fails with
"EOF while parsing a value" — so `api::post` against a `204 No Content` route
can never return `Ok`, even when the server fully succeeded.

Use `api::post_no_content` (added with this rule; mirrors how `put`/`delete`
already skip `.json()`). Its response handling lives in a separate
`handle_no_content_response` so the "empty body is success" contract is
unit-testable against a synthetic `gloo_net::http::Response` — the
`wasm-pack test --node` harness has no server to call.

Before adding a new client call, check what the handler actually returns:
`StatusCode::NO_CONTENT` means no body.

## 4. Validate money and counts at the SERVER boundary too

`create_service` / `update_service` used to bind `default_price` straight into
SQL with no validation and no rounding. A negative price makes the charger and
the door flow *credit* the customer on every visit; an unrounded price is an
upstream source of the ledger/credit drift `.claude/rules/money-rounding.md`
exists to prevent.

Fix at the boundary — once — rather than at each of the five downstream call
sites: reject negative, `round_cents()` ONCE, and reuse that rounded value for
both the INSERT and the response.

On `update_*`, only validate a field the caller is actually SETTING: an
untouched existing value on an unrelated edit (a rename) must not start failing
PUT requests it never asked to change. Use a let-chain
(`if let Some(dp) = body.default_price && dp < 0.0`) — a nested
`if { if let ... }` trips `clippy::collapsible_if`, which is deny-level in CI
and cannot be caught by the Tier-0 local checks.
