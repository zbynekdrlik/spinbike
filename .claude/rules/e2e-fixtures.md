---
paths:
  - "e2e/tests/**"
---

# E2E fixtures must stay correct as the shared CI database grows

The shared single-worker CI E2E DB is cumulative across the whole 215+-test
run — it never resets between specs. A fixture or assertion that only holds
for a "small/fresh" dataset silently breaks once enough EARLIER tests in the
same run have piled up rows. Two recurring shapes, both hit live on the
#286/#287/#289 + #288 + #39-recurrence batch (2026-08-07):

## Fixed-count pagination loops rot as the DB grows (#288)

A "Show more" / pagination loop that clicks a fixed number of times hunting
a seeded row by text match (e.g. "click up to 5 times, 250 rows, look for my
row") works when the DB is small and stops working once enough earlier tests
have pushed the seeded row's rank past that fixed bound — a real, reproduced
CI flake (`users-by-movement.spec.ts`, run history in #288).

**Fix: never guess a click count — compute it from the row's own known
rank.** The test already (or can) fetch the row's exact rank via the
ordering-check API call before touching the UI; click "Show more" exactly
`floor(rank / PAGE_SIZE)` times (matching the server's own page size), never
a fixed upper bound. Deterministic regardless of how large the shared DB
grows.

## `Date.now()`-derived digit suffixes collide with any short digit-substring search (#39, recurred twice)

Any spec generating a "unique" fixture name/card_code/barcode via
`Date.now()` produces a PURE-DIGIT string. Any OTHER spec's short digit
query against an unanchored `search_text LIKE '%query%'` (e.g. `dashboard
.spec.ts`'s `'1001'` search) can substring-collide with that digit string,
and when neither row's card_code exactly prefix/suffix-matches, ordering
falls through to plain name ASC — a `.first()` assertion can then pick the
WRONG row. This is issue #39's original mechanism; it recurred TWICE
(`card-action-form-language.spec.ts`'s own local generator, then
`category-revenue.spec.ts`'s `Date.now()`-based card_code, CI run
31178634087) because the original May fix (a letters-only suffix) never
fully propagated to every spec file with its own inline generator.

**Fix — producer side (root cause, do this for any NEW unique-fixture
generator):** use the shared `uniqueLetterSuffix()` export from
`helpers.ts` (pure a-z, zero digits — a digit-substring collision is
impossible by construction) instead of `Date.now()` or a local hand-rolled
generator. Never write a second inline `createUniqueUser`-style helper —
import the real one.

**Fix — consumer side (defense in depth, do this for any spec asserting
against a short-digit search result):** scope the result locator by the
expected NAME (`{ hasText: 'Jana Testova' }`), never trust raw `.first()`
position for a query that could match multiple rows.

```bash
# Before adding a new fixture generator or a short-digit search assertion,
# check for the anti-patterns:
grep -n "Date.now()" e2e/tests/*.spec.ts | grep -v uniqueLetterSuffix
grep -n "\.first()" e2e/tests/dashboard.spec.ts
```
