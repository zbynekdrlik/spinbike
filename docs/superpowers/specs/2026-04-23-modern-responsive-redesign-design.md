# SpinBike 2026 Modern Responsive Redesign — Design

**Status:** Approved (brainstorming 2026-04-23, user authorised autonomous execution)
**Scope:** Whole app (staff card detail, staff search, schedule, client pages, admin) — single spec per user's explicit choice.

## Goal

Replace the current ad-hoc styling with a unified 2026 mobile-first design system. Fix the concrete complaints the user raised:

- Tabs with clashing light-theme colours on a dark app (`.tabbar/.tab/.tab--active`) vs. the original dark `.tabs/.tab-btn` system.
- Voided transaction rows with light-theme `#f5f5f5` / `#888` / `#b00020` clashing on dark surfaces.
- Micro edit buttons (`.btn-sm`, `.btn-icon`, inline `style="padding:2px 8px;font-size:0.85rem"`) well below the 44 px touch-target minimum.
- No responsive rules for `.upcoming-row` / `.persistent-row` grids (min 18em — overflows 360 px viewports).
- Transaction history renders every row in one list with no pagination.
- Flat vertical stack of sections with no visual hierarchy.

## Design principles (non-negotiable)

1. **Phone-first 375 px canonical.** Anything that works at 375 px widens gracefully; desktop is a progressive enhancement, not a separate layout.
2. **Adaptive theme via `prefers-color-scheme`.** No UI toggle, no stored preference. Both palettes expose identical token names; the OS picks.
3. **44 px minimum touch target for every interactive element.** Every `.btn-sm` use must be audited out.
4. **One primitive per behaviour.** One segmented-control class (not two tab systems). One sheet pattern. One list-row.
5. **No inline styles on interactive elements.** Inline styles move into a named class.
6. **Tokens over literals.** Components consume `var(--…)` names; palette values live only in `:root` + `@media (prefers-color-scheme: light)`.

## Tokens

### Spacing

```
--s-1:  4px   --s-2:  8px   --s-3: 12px   --s-4: 16px
--s-5: 24px   --s-6: 32px   --s-7: 48px
```

New 48 px for section gaps. Current max 32 is cramped between page sections on mobile.

### Radius

```
--r-sm:  8px   --r:    12px   --r-lg: 16px   --r-pill: 24px
```

Replaces 4/6/8 (the current "2019 Bootstrap" look).

### Font scale

```
--fs-xs:   12px   --fs-sm:   14px   --fs-base: 16px
--fs-md:   18px   --fs-lg:   22px   --fs-xl:   28px   --fs-2xl:  36px
```

Default body bumps from 15.2 → 16 px. Balance display uses `--fs-xl` (28 px) or `--fs-2xl` (36 px).

### Touch target

```
--tap-min:  44px
--tap-md:   48px
--tap-lg:   56px
```

Every button declares `min-height: var(--tap-min)` minimum.

### Motion

```
--dur-fast: 120ms
--dur:      180ms
--dur-slow: 280ms
--ease-out: cubic-bezier(0.2, 0, 0, 1)
--ease-spring: cubic-bezier(0.34, 1.56, 0.64, 1)
```

### Adaptive palette

| Token | Dark (default) | Light (`prefers-color-scheme: light`) |
|---|---|---|
| `--bg` | `#0a0b0e` | `#f6f7f9` |
| `--surface` | `#13151a` | `#ffffff` |
| `--surface-2` | `#1b1e25` | `#f1f3f7` |
| `--surface-3` | `#252932` | `#e5e8ef` |
| `--border` | `#2a2e37` | `#dfe2ea` |
| `--border-strong` | `#3c4250` | `#c1c6d2` |
| `--text` | `#ededf2` | `#14161b` |
| `--text-muted` | `#a8acb5` | `#545a67` |
| `--text-dim` | `#72767f` | `#8a8f9b` |
| `--brand` | `#22c55e` | `#16a34a` |
| `--brand-tint` | `rgba(34,197,94,0.14)` | `rgba(22,163,74,0.10)` |
| `--danger` | `#f87171` | `#dc2626` |
| `--info` | `#60a5fa` | `#2563eb` |
| `--pass` | `#84cc16` | `#65a30d` |
| `--shadow` | `0 4px 12px rgba(0,0,0,0.4)` | `0 2px 8px rgba(13,20,45,0.07)` |
| `--shadow-lg` | `0 12px 36px rgba(0,0,0,0.55)` | `0 16px 48px rgba(13,20,45,0.12)` |

The existing dark tokens are tuned up (near-black `#0a0b0e` bg, slightly deeper surfaces, modernised muted/dim); the light tokens are added fresh, WCAG AA contrast verified for every `{text, surface}` pair.

## Primitive components

### `.btn` (rationalised)

Three canonical sizes replace `.btn-sm` / default / `.btn-icon`:

```css
.btn               { min-height: var(--tap-min);  font-size: var(--fs-sm);   padding: 0 var(--s-4); }
.btn--hero         { min-height: var(--tap-lg);   font-size: var(--fs-md);   padding: 0 var(--s-5); }
.btn--compact      { min-height: 36px;            font-size: var(--fs-xs);   padding: 0 var(--s-3); }
```

Variants: `.btn--primary`, `.btn--danger`, `.btn--ghost`, `.btn--pass`. `.btn--block` stays (full width).

**All existing `.btn-sm` uses are audited out.** Where compactness is truly required (inline table action like void × button), `.btn--compact` is used — and only in rows that already have a larger row-level tap target.

### `.seg` (segmented control — replaces `.tabs` and `.tabbar`)

```css
.seg            { display:flex; background:var(--surface-2); border-radius:var(--r); padding:4px; gap:2px; }
.seg__item      { flex:1; min-height:40px; background:transparent; color:var(--text-muted); border:0; border-radius:calc(var(--r) - 4px); font-weight:500; }
.seg__item[aria-selected="true"] { background:var(--surface); color:var(--text); box-shadow: var(--shadow); }
```

iOS-style pill segments. Single source of truth — the light-theme `.tabbar/.tab--active` additions are removed.

### `.sheet` (bottom sheet on mobile, centered modal ≥768 px)

```css
.sheet-backdrop { position:fixed; inset:0; background:rgba(0,0,0,0.45); backdrop-filter:blur(4px); z-index:200; }
.sheet          { position:fixed; left:0; right:0; bottom:0; background:var(--surface); border-top-left-radius:var(--r-lg); border-top-right-radius:var(--r-lg);
                  padding:var(--s-4) var(--s-4) var(--s-5); z-index:210; box-shadow:var(--shadow-lg); max-height:90vh; overflow-y:auto;
                  animation:sheet-in var(--dur-slow) var(--ease-spring); }
.sheet__grab    { width:44px; height:4px; border-radius:2px; background:var(--border-strong); margin:0 auto var(--s-3); }
.sheet__title   { font-size:var(--fs-md); font-weight:600; margin-bottom:var(--s-4); }
.sheet__actions { display:flex; gap:var(--s-2); padding-top:var(--s-4); }

@media (min-width: 768px) {
  .sheet { position:fixed; left:50%; top:50%; right:auto; bottom:auto;
           transform:translate(-50%, -50%); width:min(480px, 90vw); border-radius:var(--r-lg); }
}

@keyframes sheet-in { from { transform: translateY(100%); } to { transform: translateY(0); } }
```

Click backdrop → closes (via Leptos signal). `Escape` key → closes. Swipe-down gesture deferred (YAGNI for v1).

Replaces the current `.modal-overlay` / `.modal`. The "Sell Pass" form, "Edit customer info" form, and "Edit pass end date" form all render through this primitive.

### `.group` + `.list-row` (unified list rows — replaces `.upcoming-row`, `.persistent-row`, transaction table, search result rows)

```css
.group          { background:var(--surface); border-radius:var(--r); overflow:hidden; margin-bottom:var(--s-4); }
.group__head    { padding:var(--s-3) var(--s-4); font-size:var(--fs-sm); color:var(--text-dim); text-transform:uppercase; letter-spacing:0.04em; border-bottom:1px solid var(--border); }
.list-row       { display:flex; align-items:center; gap:var(--s-3); padding:var(--s-3) var(--s-4); min-height:56px;
                  border-top:1px solid var(--border); }
.list-row:first-child { border-top:none; }
.list-row__main { flex:1; min-width:0; }
.list-row__title{ font-weight:500; }
.list-row__sub  { font-size:var(--fs-sm); color:var(--text-muted); }
.list-row__end  { display:flex; align-items:center; gap:var(--s-2); }

/* No grid templates — flex from the start, no column collapse on mobile. */
```

Replaces:
- `.upcoming-row` grid `8em 5em 1fr auto auto` → now a flex row with wrapping content on < 480 px
- `.persistent-row` grid `1fr auto` → same primitive
- `data-table tr` for transactions → switched to `.list-row` (table abandoned on mobile; kept as stacked rows with aligned amount)
- `.search-result` rows → same primitive

### `.chip` / `.badge` refresh

```css
.badge    { display:inline-flex; align-items:center; min-height:24px; padding:0 var(--s-2); border-radius:var(--r-pill); font-size:var(--fs-xs); font-weight:600; }
.badge--pass    { background:rgba(132, 204, 22, 0.14); color:var(--pass); }
.badge--booked  { background:rgba(96, 165, 250, 0.14); color:var(--info); }
.badge--full    { background:rgba(248, 113, 113, 0.14); color:var(--danger); }
.badge--cancelled{ background:var(--surface-3); color:var(--text-dim); }
.badge--voided  { background:rgba(248, 113, 113, 0.14); color:var(--danger); }
```

### Form inputs

`.form-control` bumps to 44 px min-height, 16 px body font size (prevents iOS Safari zooming on focus), larger label spacing. Otherwise same markup.

## Per-page application

### Staff card detail panel (primary target — the complaint)

New layout order on phone:

1. **Header bar** — full name + barcode + company/phone (muted), close × button on the right. Sticky at top of the panel (not the viewport).
2. **Pass banner** (if present) — `.group` with status badge, end date, days remaining. "Edit date" button opens sheet.
3. **Balance** — prominent `.fs-2xl` number, red tint when negative, blocked badge inline.
4. **Primary actions row** — two side-by-side buttons: `Charge` (opens Charge section inline, stays inline — a single form, not a sheet) and `Top-up` (same). Sizes: `.btn--hero`.
5. **Sell pass button** — full-width `.btn--hero.btn--pass`, price inline. Opens **sheet**.
6. **Secondary actions** — Edit customer info + Block — `.btn.btn--ghost`, side-by-side. Edit opens **sheet**.
7. **Segmented tabs** — History (default) / Upcoming / Subscriptions using `.seg`.
8. **Tab content** — a `.group` with `.list-row`s.
   - History: show 10 most recent. `Show older (N)` button at bottom loads next 20.
   - Upcoming: existing `<UpcomingClasses />` re-skinned to `.list-row`s.
   - Subscriptions: existing `<PersistentToggles />` re-skinned.

Code reshuffle:

- `dashboard.rs` is 1544 lines. Extract to `spinbike-ui/src/pages/dashboard/` module:
  - `mod.rs` — page entry (search + results + detail container)
  - `card_panel.rs` — main detail card
  - `charge_section.rs` — charge form (inline)
  - `topup_section.rs` — topup form (inline)
  - `pass_banner.rs` — pass group + edit date sheet trigger
  - `transactions_list.rs` — history with pagination
  - `sheets/edit_info.rs` — sheet for edit customer info
  - `sheets/sell_pass.rs` — sheet for sell pass
  - `sheets/edit_pass_date.rs` — sheet for edit pass end date

Keep `data-testid` attributes **stable** to avoid breaking existing E2E tests: `action-panel`, `card-credit`, `sell-pass-btn`, `tab-history`, `tab-upcoming`, `tab-persistent`, `txn-void`, `pass-banner-active`, `pass-banner-expired`, etc.

### Staff dashboard search + results

- Sticky search input at top (`.form-control` new style).
- Results dropdown renders as `.group` of `.list-row`s (name, barcode, badges for blocked / pass / credit).
- Keyboard nav (arrow / enter) preserved.
- Empty state gets polished text ("Start typing a name, phone, or barcode…").

### Schedule (`/schedule`)

- Day picker — horizontal scroll strip of `.day-btn` cards, 64×72 px each (up from ~48). Active day uses `--brand-tint`.
- Class cards — switched from `.class-card` grid to `.group` containing `.list-row`s with left accent bar via `box-shadow inset 3px 0 0 var(--accent)`. Colours by state: available green, booked blue, full red, cancelled dim.
- Participants list (staff view) stays as existing `.participants-list` but modernised padding.

### Client pages

- `/me/balance` — `.group` layout. Pass banner (if any) + balance prominent + transaction history (same pagination as staff).
- `/me/bookings` — list of upcoming bookings as `.list-row`s, cancel buttons `.btn.btn--ghost.btn--compact` (small but 36 px min-height).
- `/login` — polished card, 16 px form controls, generous spacing.
- `/link-card` — same treatment.

### Admin

- Same tokens, same primitives, mostly consistency clean-up. Tables stay for dense data (users, cards list) but `.data-table` restyled.

### Nav

- Sticky top bar, `.navbar` restyled per tokens. Brand-green logo lockup. Language toggle `.btn.btn--compact.btn--ghost`.

## Data flow changes

### Transaction pagination

Add query parameters to the existing endpoint:

```
GET /api/cards/{id}/transactions?limit=10&before=2026-04-20T14:30:00
```

- `limit` — default **10** (no server-side default changes when caller omits param for backward compat? No — change default to 10). Legacy uses all migrate to the new API.
- `before` — ISO 8601 datetime, returns transactions with `created_at < before`. Used for "show older" cursor pagination.

Response shape unchanged: `Vec<TxnInfo>`. Client passes the oldest-shown timestamp as `before` on the next "Show older" click.

Server change: two lines in `crates/spinbike-server/src/db/transactions.rs` `list_by_card` — add optional `limit` and `before` params, append `AND created_at < ?` when present, `LIMIT ?` at the end.

All other data flow unchanged — this redesign is primarily presentational.

## File structure

**New files:**
```
spinbike-ui/src/components/sheet.rs          # <Sheet title show on_close children />
spinbike-ui/src/components/segmented.rs      # <Segmented items active on_change />
spinbike-ui/src/pages/dashboard/mod.rs       # (split from current dashboard.rs)
spinbike-ui/src/pages/dashboard/card_panel.rs
spinbike-ui/src/pages/dashboard/charge_section.rs
spinbike-ui/src/pages/dashboard/topup_section.rs
spinbike-ui/src/pages/dashboard/pass_banner.rs
spinbike-ui/src/pages/dashboard/transactions_list.rs
spinbike-ui/src/pages/dashboard/sheets/mod.rs
spinbike-ui/src/pages/dashboard/sheets/edit_info.rs
spinbike-ui/src/pages/dashboard/sheets/sell_pass.rs
spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs
```

**Modified files:**
```
spinbike-ui/style.css               (full rewrite — 942 -> ~1400 lines)
spinbike-ui/src/pages/dashboard.rs  (DELETED, split into dashboard/ module)
spinbike-ui/src/pages/mod.rs        (imports adjusted)
spinbike-ui/src/pages/my_balance.rs (restyled)
spinbike-ui/src/pages/my_bookings.rs (restyled)
spinbike-ui/src/pages/login.rs      (restyled)
spinbike-ui/src/pages/link_card.rs  (restyled)
spinbike-ui/src/pages/schedule.rs   (restyled)
spinbike-ui/src/pages/admin.rs      (restyled)
spinbike-ui/src/pages/staff_dashboard.rs (restyled if distinct)
spinbike-ui/src/components/nav.rs
spinbike-ui/src/components/class_card.rs
spinbike-ui/src/components/day_picker.rs
spinbike-ui/src/components/upcoming_classes.rs
spinbike-ui/src/components/persistent_toggles.rs
spinbike-ui/src/i18n.rs             (+show_older, +close, +edit_info, +expired_warning, +sell_pass_label)

crates/spinbike-server/src/db/transactions.rs  (list_by_card gains limit+before)
crates/spinbike-server/src/routes/cards.rs     (query params piped through)

VERSION                             0.8.0 -> 0.9.0
```

## Error handling

- Form validation errors surface inline inside sheets (red alert at top of sheet content).
- `.alert--error / .alert--success / .alert--info` restyled per tokens; existing markup preserved.
- Loading states: skeleton `.skeleton` class (animated pulse) for list-heavy views (transactions, upcoming). Replaces plain spinner where it fits.
- Sheet close during in-flight save: save button shows spinner; backdrop click is disabled while `saving`.

## Testing plan

### Functional (Playwright E2E)

Existing tests MUST stay green. Key tests touched:
- `card-search.spec.ts` — keyboard nav, focus restore (no behavioural change, only styling — should survive).
- `spin-booking.spec.ts` — uses tab-upcoming and tab-persistent (kept stable).
- `credit-improvements.spec.ts` — tab-history, txn-void, pass banner edit (stable testids).

**New tests (required):**
- `redesign-sheets.spec.ts`:
  - Sell pass: open sheet → form visible → cancel → sheet gone.
  - Sell pass: open → submit valid → sheet closes → credit updated.
  - Edit customer info: open sheet → edit name → save → dashboard reflects.
  - Edit pass date: open sheet → change date → save → banner reflects.
  - Sheet backdrop click → closes.
  - `Escape` key → closes sheet.
- `redesign-history-pagination.spec.ts`:
  - Open card with >10 transactions → only 10 visible.
  - Click "Show older" → 30 visible.
  - Button hides when all loaded.
- `redesign-theme.spec.ts`:
  - Emulate `prefers-color-scheme: light` → `<html>` computed background equals light token.
  - Emulate dark → dark token.
- All new and existing tests must assert `consoleMessages === []`.

### Unit (Rust)

- `transactions::list_by_card` — test `limit` and `before` cursor behaviour.
- Unit tests for `sheet.rs` and `segmented.rs` components render expected structure.

### Visual verification

Post-deploy: staff card detail panel Playwright screenshot at 375 / 768 / 1280 px, assert key selectors exist. No pixel-diff baselines (YAGNI for v1).

## Non-goals

- Business logic changes.
- Schema changes (only `limit` + `before` query params).
- Authentication / permissions changes.
- Theme toggle UI (adaptive follows OS only — explicit user decision).
- Swipe-down gesture on sheets (YAGNI).
- Pixel-diff visual regression baseline (YAGNI).
- Internationalisation additions beyond the minimum new keys.

## Version

Bump `VERSION` 0.8.0 → **0.9.0**. User-visible redesign, no breaking API, no schema break.

## Rollout risk & mitigations

- **Risk: breaking existing E2E tests.** Mitigation: keep every `data-testid` attribute stable; verify via searching for `data-testid=` in tests before renaming anything.
- **Risk: CSS regression in admin pages (rarely used, might look broken after token shift).** Mitigation: admin pages get explicit Playwright smoke test (nav, open users list, open settings).
- **Risk: light-theme contrast issues for tinted surfaces.** Mitigation: WCAG AA contrast verified for each `{text, surface}` pair during token definition.
- **Risk: big PR hard to review.** Mitigation: task-by-task commits via subagent-driven-development keep a clean history.
