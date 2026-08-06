---
name: spinbike-e2e-testing
description: >
  SpinBike Playwright e2e test-writing conventions and gotchas — auditing
  existing specs when adding a new validation guard, shared helper patterns.
  Load when writing/editing files under e2e/tests/, or right after adding a
  new 4xx/409 business-rule guard to any payments/booking/door endpoint
  (check every e2e call site that exercises that endpoint twice for one
  user/day before pushing).
triggers:
  - e2e test
  - playwright spec
  - e2e/tests
  - 409
  - duplicate guard
  - force: true
---

# SpinBike E2E Test-Writing Gotchas

## A new 4xx/409 guard on an endpoint breaks EXISTING e2e specs that assumed happy-path — audit every call site BEFORE pushing (#234/#235)

#234 added a same-day duplicate-visit guard to `POST /api/payments/log-visit`
(409 `already_visited_today` when the user already has a same-day class-visit
event — and a PAID Fitness/Spinning `charge` counts as one, per the canonical
attendance definition in `db-migrations` skill). The worker that implemented
it wrote its OWN new e2e spec (`log-visit-duplicate-warning.spec.ts`) correctly,
but did NOT audit `e2e/tests/reports-attendance.spec.ts` — an EXISTING spec
that seeded paid Fitness + paid Spinning charges for one synthetic user, then
called `log-visit` twice more for the SAME user on the SAME day (to prove the
attendance KPI sums every class-visit event). That existing spec's very FIRST
`log-visit` call now collided with the new guard and CI went red on `dev`
after the worker had already pushed and finished — a full extra CI cycle
spent diagnosing and fixing what a pre-push audit would have caught.

**Rule: whenever you add a new 4xx/409 validation guard to ANY endpoint,
`grep -rn` every e2e spec that calls that endpoint (or the UI action that
triggers it) BEFORE pushing** — not just the new spec you wrote to cover the
feature itself:

```bash
grep -rln "<endpoint-or-button-testid>" e2e/tests/*.spec.ts
```

For each hit, check whether the same user/day now double-triggers the new
guard's condition (not just the exact call you're thinking of — a PRIOR
seeded charge/transaction can ALSO satisfy the guard's "already happened"
check, as it did here: the guard fired on the FIRST `log-visit` call, not
the second, because the preceding paid-charge seeding already counted).

**Fix at the call site, not the guard.** When an existing spec's repeated
call is a genuine, INTENTIONAL second/third event for the same day (as
`reports-attendance.spec.ts` was — it deliberately seeds several class-visit
events to prove the KPI sums them all), pass the guard's documented
legitimate bypass (`force: true` here) with a comment explaining WHY it's
intentional test setup, not a workaround. NEVER weaken the new guard's own
assertions to make an unrelated existing spec pass — the guard is doing
its job; the OLD spec's assumption (never having imagined this state) is
what's stale.

## `today`/`tomorrow` for a day/range-bucketed endpoint: use `bratislavaToday()`/`bratislavaDateOffset()`, never `new Date().toISOString()` (#251)

Any endpoint that buckets by the GYM-LOCAL day (`/api/reports/day`,
`/api/reports/range`, `sell-pass`'s `valid_until` future-check, and any
future one — see the `db-migrations` skill's Bratislava-day-boundary
gotchas) compares against `today_bratislava()` server-side, NOT a raw UTC
date. A spec deriving "today"/"tomorrow" via
`new Date().toISOString().slice(0, 10)` or `Date.now() + N * 3600_000`
silently disagrees with the server during the 00:00-02:00 Bratislava-local
window (a UTC CI runner can still be on yesterday's UTC date while
Bratislava has already rolled over) — an intermittent, CI-only flake
(confirmed live, #251): a `sell-pass` call rejected a genuinely-future date
as "must be in the future", and separately a before/after attendance delta
read 0 instead of 4 because the before/after snapshots and the seeded
transactions landed in DIFFERENT Bratislava-day buckets than the UTC date
string the test queried.

**Fix: use the shared `helpers.ts` exports, never hand-roll the date.**

```ts
import { bratislavaToday, bratislavaDateOffset } from './helpers';

const today = bratislavaToday();          // 'YYYY-MM-DD', Intl-based, mirrors today_bratislava()
const tomorrow = bratislavaDateOffset(1);  // pure calendar-date arithmetic, no UTC-instant ambiguity
```

**A wider UTC-instant margin (e.g. `Date.now() + 48 * 3600_000`) is a
band-aid, not a fix** — it happened to mask the FIRST symptom (the sell-pass
rejection) live during #251's own investigation, but the SECOND symptom
(the before/after delta reading the wrong day's bucket) still failed right
after, because the underlying `today` used for the query was still a raw
UTC date. Fix the DATE DERIVATION itself (Bratislava-anchored), not the
size of an offset.

**When you touch ANY spec computing a date for a reports/day-bucketed
assertion, grep for the anti-pattern first:**

```bash
grep -n "toISOString().slice(0, 10)\|Date.now() + .* 3600" e2e/tests/*.spec.ts
```

Not every hit needs fixing — a spec using fixed historical dates
(`reports-range.spec.ts`) or one that never computes its own date (relies
on the frontend's own Bratislava-anchored default, like
`reports-day.spec.ts`/`txn-note.spec.ts`) is unaffected. Only a spec that
computes "today"/"tomorrow" itself AND asserts against a Bratislava-bucketed
endpoint needs the helper.

## `loginViaAPI` STORES a session — a "reads localStorage session" change collides with welcome/invite specs that look logged-out (#258)

`loginViaAPI(page, ..., 'admin@test.com', ...)` in `helpers.ts` does NOT
just return a JWT — it also `page.goto('/')` and writes `spinbike_token` /
`spinbike_user` into localStorage. The welcome/invite specs
(`welcome.spec.ts`, `inviteAndGetWelcomeLink`) call it ONLY to reach the
admin `POST /api/users` + `/invite` APIs, then navigate to the emailed
`testLink` — but they are NOT logged-out at that point, they carry the ADMIN
session. This is invisible until a change READS that session before acting.

#258 added a `/welcome` session short-circuit (a stored session redirects
home instead of redeeming `?t`). That silently broke 3 existing welcome
specs: their FIRST `goto(testLink)` short-circuited to `/staff` on the admin
session instead of redeeming the invite → `welcome-success` never appeared.

**Rule: any change that branches on "is a session already in localStorage"
(a short-circuit, an auth gate, a redirect) must audit every spec that
redeems a magic link / `?t` and clear the session first** — the invite must
redeem from a genuinely logged-out state (a real customer clicking their
emailed link is not logged in). The clear (already the established pattern in
`code-login.spec.ts`):

```ts
await page.evaluate(() => {
    localStorage.removeItem('spinbike_token');
    localStorage.removeItem('spinbike_user');
});
```

`grep -n "loginViaAPI" e2e/tests/*.spec.ts` and for each hit that later
`goto`s a `/welcome`/`?t` link, confirm the session is cleared between them.
The ticket author's "existing specs start clean" assumption is FALSE for any
spec that used `loginViaAPI` for setup.

## A test's FIRST page action touching `localStorage` MUST navigate first — `about:blank` throws a SecurityError (#263)

A new spec whose FIRST action is a customer-session switch (no prior admin
`loginViaAPI` call to establish a real origin) and calls
`page.evaluate(() => localStorage.clear())` before any `page.goto()` fails
immediately on a fresh Playwright context: `SecurityError: Failed to read
the 'localStorage' property from 'Window': Access is denied for this
document.` — `about:blank` has an opaque origin, and `localStorage` access
there throws. Every OTHER spec in this suite is silently protected from this
because their first action is always an admin `loginViaAPI` call, which
internally does `page.goto('/')` before touching storage — so the trap only
bites a NEW test whose scenario genuinely doesn't need an admin step first
(e.g. #263's "customer with no seeded state" case).

**Fix: `page.goto('/')` before the FIRST `localStorage` touch in any new
per-test helper**, even if `loginViaAPI` (called right after) will navigate
again — the second navigation is a harmless no-op re-navigation, not a
double cost worth avoiding:

```ts
async function loginAsCustomerSk(page: Page, baseURL: string, email: string, password: string): Promise<void> {
    await page.goto('/');                          // <-- establish a real origin FIRST
    await page.evaluate(() => { localStorage.clear(); });
    await loginViaAPI(page, baseURL, email, password);
    // ... addInitScript for lang override, etc.
}
```

Whenever a new spec's test body does NOT start with `loginViaAPI(page, ..., 'admin@test.com', ...)`, check whether its own custom helper touches `localStorage` before any navigation — this is invisible until CI actually runs it (local `npx tsc --noEmit` type-checks fine; the failure is a runtime browser security error, not a type error).

## Router
Add a line to the project `CLAUDE.md` `## Playbook router` pointing here so a
future guard-adding ticket loads this BEFORE pushing, not after CI turns red.
