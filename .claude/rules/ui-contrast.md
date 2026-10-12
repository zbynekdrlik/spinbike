---
paths:
  - "spinbike-ui/style.css"
  - "spinbike-ui/src/pages/staff_dashboard.rs"
  - "e2e/tests/staff-participant-chip.spec.ts"
---

# Text contrast — theme tokens only, pinned by a Playwright test (#379)

- **Never put an inline `background:`/`color:` on themed UI.** Theming is
  `:root` (dark default) + `@media (prefers-color-scheme: light)` in
  `style.css` — there is NO `data-theme` attribute. An inline colour mixed
  with a token colour (#379: inline `#e0e7ff` bg + `var(--text-muted)` text)
  is right in one scheme and unreadable (1.85:1) in the other.
- Safe pairs (WCAG, measured): `--text` on `--surface-3` 12.5:1 dark /
  14.8:1 light; `--text-muted` on `--surface-3` 6.4 / 5.6. `--danger` on
  `--surface-3` is only 3.9:1 in LIGHT — use it for hover/focus, not as the
  resting text colour.
- **Test both schemes**: `page.emulateMedia({ colorScheme: 'dark' | 'light' })`
  before `goto`, then compute contrast from `getComputedStyle` walking up to
  the first opaque background (helper `contrastOf` in
  `e2e/tests/staff-participant-chip.spec.ts` — reuse it).
- Prefer plain tokens over `color-mix(...)` for anything a contrast test
  reads: Chromium returns a computed `color-mix` as `color(srgb r g b)`
  (0-1 floats), not `rgb()` — the helper parses both, ad-hoc regexes won't.
