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

## Router
Add a line to the project `CLAUDE.md` `## Playbook router` pointing here so a
future guard-adding ticket loads this BEFORE pushing, not after CI turns red.
