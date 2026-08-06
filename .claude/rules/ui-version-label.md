---
paths:
  - "spinbike-ui/style.css"
  - "spinbike-ui/src/app.rs"
  - "spinbike-ui/src/components/**"
  - "e2e/tests/version-display.spec.ts"
---

# The version label is a verification CONTRACT, not decoration

`.app-version` renders `v<semver>` with `data-testid="version"` as the last child of
`.app-shell`, on every route. Two independent consumers read it:

- **Post-deploy verification** (`post-deploy-verification.md`, the `frontend-pwa` skill) reads
  the version off the LIVE DOM to prove the new build is actually serving — the whole
  deploy-verified claim rests on it.
- **`e2e/tests/version-display.spec.ts`** asserts it exists, is visible, matches `v<semver>`,
  and — since #267 — that its computed `position` is not `fixed` and its background is
  transparent.

So when restyling or moving it:

- **NEVER drop or rename `data-testid="version"`, and never change the `v<semver>` text
  format.** Both are load-bearing; a "cleanup" that removes either silently breaks every
  future post-deploy check.
- **NEVER make it a floating overlay again.** #267 was the user's angry report about exactly
  that: it had been `position: fixed; bottom: 60px; right: 12px; z-index: 50;` with a
  `rgba(0,0,0,0.35)` chip background — a grey rectangle painted over the app content on a
  phone ("najhnusnejsia vec co som kedy videl"). It now lives in normal document flow,
  centered, muted text, no background, after `.page` and above the reserved mobile
  bottom-nav padding.
- A version label that overlaps content is a **bug**, not a style preference — the E2E guard
  exists to make that regression fail CI instead of shipping.

## Verifying a change to it

Read the live DOM at a **phone viewport (390x844)**, not desktop — the overlay bug was
invisible at desktop width. Clear the service worker + `spinbike-v3` cache first (the
`frontend-pwa` skill's stale-cache gotcha) or you will measure the previous build. Check on
BOTH a logged-out route (`/login`) and a logged-in customer route (`/my/balance`, synthetic
account per the `prod-verification` skill) — only the logged-in shell has the bottom nav the
label could collide with. Assert: computed `position` not `fixed`, transparent background,
`elementFromPoint` at the label's center returns the label itself, zero overlap against
`.card` / nav elements, and `scrollWidth == clientWidth` (no horizontal scroll).
