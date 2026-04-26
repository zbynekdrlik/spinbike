# SpinBike Staff/CEO Redesign — Design

**Status:** Approved (brainstorming 2026-04-24)
**Scope:** Admin surface only — adaptive navigation, new Reports module, Desk "Now" panel, Settings demotion, and Schedule consolidation. Customer pages (`/login`, `/link-card`, `/my/*`, public `/schedule`) are untouched.

## Context

The gym is run by a single person: **Sumerling Štefan** (`fitnescentrum.s.s@gmail.com`). He is simultaneously owner, admin, staff, and front-desk operator. His flat is part of the gym; he uses iPhone (primary) and laptop (secondary). Every design decision must minimise his overhead.

The previous visual redesign (`2026-04-23-modern-responsive-redesign-design.md`, shipped in v0.9.0+) introduced the design system — tokens, `.sheet`, `.group`, `.list-row`, `.seg`, `.btn` primitives. This spec is purely **information-architecture and new-feature**; it reuses all existing primitives and does not introduce new visual tokens.

## Goal

Give Štefan a coherent admin surface that:

1. Surfaces "what happened today / yesterday / any day" with money movements, attendance, filters, and actionable alerts.
2. Unifies fragmented top-level pages (`/staff`, `/staff/classes`, `/schedule`, `/admin`, `/reports`) into four clear task modes: **Desk**, **Schedule**, **Reports**, **Settings**.
3. Shows the "Now" state at the desk — which class is running or next, who is booked, who has arrived.
4. Keeps the iPhone PWA as the primary target while working well on laptop.

## Design principles

1. **One user, no role separation.** There is no CEO role. `admin` === Štefan. Do not add role tiers; do not add per-staff attribution.
2. **Mobile-first iPhone, adaptive desktop.** 375 px canonical. Bottom tab bar on phone, left sidebar on desktop — one nav definition, two layouts.
3. **Minimise overhead.** If a routine action needs more than 2 taps from home, it is too slow. If it needs more than 3 lines of text to explain, it is too complex.
4. **Reuse v0.9.0 primitives.** No new palette, no new tokens, no new layout primitives. Every new screen is assembled from existing building blocks.
5. **Data-testid discipline.** All existing `data-testid` attributes used by E2E tests stay stable. New surfaces get their own stable testids.
6. **No new tables, no new columns.** Reports are computed from the existing `transactions`, `bookings`, `cards`, `class_templates`, `services`, `users` schema.

## Information architecture

Four top-level admin destinations:

| Destination | Route     | Purpose |
|-------------|-----------|---------|
| Desk        | `/`       | Card search + card detail + **Now** panel. Default home. |
| Schedule    | `/schedule` | Week view + per-class rosters (admin) / public view (customer). |
| Reports     | `/reports`  | Day/week/month activity + KPIs + filters + alerts. **New.** |
| Settings ⚙  | `/settings` | Configuration: Centrum, Služby, Permanentky, Inštruktori, Používatelia. Demoted from `/admin`. |

Route changes:

- `/admin` → redirect to `/settings` (back-compat).
- `/staff/classes` → redirect to `/schedule` (functionality folded into admin view of `/schedule`).

Customer routes unchanged:

- `/login`, `/link-card`, `/my/bookings`, `/my/balance`, public `/schedule` (when not admin).

## Adaptive navigation

Single `<Nav>` component renders two layouts based on viewport:

### Mobile (< 768 px) — bottom tab bar

```
┌─────────────────────────────────────┐
│        (main content scrolls)       │
│                                      │
│                                      │
├─────────────────────────────────────┤
│ [🏠 Desk] [📅 Plán] [📊 Výkazy] [⚙]│
└─────────────────────────────────────┘
```

- Fixed at bottom: `position: fixed; bottom: 0; left: 0; right: 0;`
- Height: 56 px + `env(safe-area-inset-bottom)` padding (iOS Safari URL bar).
- Main content: `padding-bottom: calc(56px + env(safe-area-inset-bottom))` so nothing hides behind it.
- Active tab: `--brand` foreground + `--brand-tint` background pill.
- Icons + short labels (Slovak): `Desk`, `Plán`, `Výkazy`, `⚙`.

### Desktop (≥ 768 px) — left sidebar

```
┌────┬──────────────────────────────┐
│ 🏠 │                               │
│ 📅 │      (main content)           │
│ 📊 │                               │
│ ⚙  │                               │
│    │                               │
└────┴──────────────────────────────┘
```

- Fixed at left: 72 px collapsed / 200 px expanded on hover.
- Same 4 destinations, same active-state styling.
- Icons + labels (when expanded).

### Shared top header

- Thin bar (40 px), contains: center name + language toggle (SK/EN) + user menu (logout).
- Not a navigation surface — just branding and utilities.
- Present on both mobile and desktop.

### Permission gating

- `Desk`, `Schedule`, `Reports`, `Settings` visible only when `role == admin`.
- Non-admin (customer) sees the existing customer-facing pages only. No admin tabs appear.
- Server enforces per-endpoint (existing pattern); client hides UI by role (existing pattern).

## Desk (Home) page

**Route:** `/`

**Layout (top to bottom):**

1. **Now panel** (collapsible, stored preference in localStorage key `desk_now_collapsed`)
   - Shows current class if one is running now, else next upcoming class within 3 h. If none: `"Ďalšia hodina: <day> <HH:mm> <service>"` or `"Dnes už žiadne hodiny."`
   - Header row: `18:00 Spinning — Jana K.` + badge `4/12`.
   - Expandable → roster `.group` with `.list-row` per booking (walk-ins are not shown here — they appear in the day's Reports feed instead):
     - Main: customer name, barcode.
     - Badge: `checked-in` (booking has a linked non-voided charge), `booked` (no charge yet), `cancelled` (cancelled booking).
     - Tap row → opens the customer's card detail in the card-detail section below (same as tapping a search result).
   - Action row inside expanded panel:
     - `Walk-in` button — opens card-search sheet → pick card → creates booking + charge.
     - `Cancel class` button (admin only) — confirms, then marks all bookings cancelled and the template occurrence cancelled for that date.
   - Collapse/expand chevron in header.

2. **Card search** (existing behaviour)
   - Sticky input at top of the rest of the page.
   - Real-time fuzzy search, keyboard nav preserved (existing tests guard this).

3. **Card detail** (restyled)
   - Hero row: name + barcode (muted) + balance (`--fs-2xl`) + block/pass badges inline.
   - Pass banner (if present): unchanged from v0.9.0.
   - **Primary action:** one dominant `.btn--hero.btn--primary` labelled `Platba` (Charge) — opens existing inline charge form.
   - **Secondary actions:** row of `.btn.btn--ghost` — `Vklad` (Top-up), `Predať permanentku` (Sell Pass), `Upraviť` (Edit Info).
   - **Tertiary actions:** in an "overflow" `.btn--compact` button or an inline menu — `Block`, `Delete`.
   - Company/phone/email: hidden under an `ℹ` toggle that expands `.list-row` containing those fields.
   - Tabs (`.seg`): `História | Nadchádzajúce | Permanentky` — unchanged from v0.9.0.

**Addressed pain points:**

- "Hard to see what's happening now" → Now panel.
- "Card detail too busy" → hierarchy with dominant primary, secondary row, tertiary collapsed, contact info hidden by default.
- "Too many pages" → /staff/classes folds into Schedule + Now panel absorbs "who's here right now".
- "Admin mixed with daily ops" → /admin demoted to ⚙.

## Reports page (new)

**Route:** `/reports`

**Layout (top to bottom):**

### 1. Date nav strip (sticky under header)

- Segmented control (`.seg`): `‹ Včera | Dnes | Zajtra ›`
  - Arrows navigate day-by-day.
  - Label is tappable → opens calendar sheet (reusable `<CalendarPickerSheet />` component, built on `.sheet`).
- Below the segment: two small buttons — `Týždeň` and `Mesiac` — switch to 7-day or 30-day aggregate view anchored at the selected date (ending today if selected day is today, else last 7/30 days back from selected day).
- Current mode is shown in a small subheading: `"Dnes · 2026-04-24"` or `"Týždeň · 2026-04-18 → 2026-04-24"`.

### 2. Needs-attention banner (conditional)

- `.group` with amber accent left border.
- Rendered only if at least one of the three alert types has a non-zero count AND has not been dismissed for today (per-day localStorage key `reports_alerts_dismissed_<YYYY-MM-DD>_<type>`).
- Up to three rows (each `.list-row`):
  - `N permanentiek vyprší do 7 dní` — tap → opens `<AlertSheet type="expiring" />` with the list of cards.
  - `N kariet s kreditom pod 5 €` — tap → same pattern, `type="low_credit"`.
  - `N zákazníkov neaktívnych 60+ dní` — tap → same pattern, `type="inactive"`.
- Each row has an × button on the end to dismiss that alert for today.

### 3. KPI cards row

- Four cards. On phone: 2×2 grid. On desktop: 4×1 row.
- Each card: large number (`--fs-2xl`), small uppercase label (`--fs-xs --text-dim`), appropriate unit (€, count).

Definitions (all exclude `deleted_at IS NOT NULL` rows):

- **Charge transaction** — `amount < 0` AND `valid_until IS NULL` (regular gym visit or class attendance).
- **Pass-sale transaction** — `valid_until IS NOT NULL` (monthly pass sold — per existing `create_transaction_with_valid_until` path in `db/transactions.rs`). Amount is typically negative for the pass price debit.
- **Top-up transaction** — `amount > 0`.

| Card        | Label (SK)      | Value |
|-------------|-----------------|-------|
| Revenue     | `TRŽBA`         | Sum of absolute values of all non-voided charge and pass-sale transactions for the date/range (both represent money earned). |
| Attendance  | `NÁVŠTEVY`      | Count of non-voided charge transactions for the date/range (one visit per charge). |
| Passes sold | `PERMANENTKY`   | Count of non-voided pass-sale transactions for the date/range. |
| Cash in     | `VKLADY`        | Sum of non-voided top-up transactions for the date/range (money deposited onto cards). |

- No trend arrows, no deltas, no charts in v1.

### 4. Filters bar (collapsed by default)

- Single `Filtre` button with a count badge if any filter is active.
- Expanded:
  - Event type chips (radio, single select): `Všetko | Platby | Vklady | Permanentky`.
  - Service chips (radio, single select): `Všetko | Spinning | Fitness | Permanentka`.
  - Search input: name / barcode / phone — narrows feed to matching customers.
- `Reset` button clears all filters.

### 5. Activity feed

- `.group` of `.list-row`s, chronological **descending** (newest first).
- Each row:
  - Time `HH:MM` (left, muted, fixed width).
  - Icon: `•` colored dot — red (charge), green (top-up), blue (pass sold), dim (voided).
  - Main: customer name + barcode (muted, smaller).
  - Sub: service name or pass validity or note.
  - End: amount — red for charges (shown as `-5.00 €`), green for top-ups (`+20.00 €`), blue for passes (`+35.00 €`), with `voided` badge when applicable.
  - Tap row → existing transaction-detail sheet (void button inside) — reuses existing pattern.
- Pagination: 50 per page. "Načítať staršie" button at end uses cursor pagination via `before` query param (matching existing `/api/cards/{id}/transactions` pattern).

### 6. Empty states

- No events and no filters: card `"Na tento deň nie je žiadna aktivita."` with `Zobraziť dnes` button.
- No events but filters active: card `"Žiadne výsledky pre tieto filtre."` with `Zrušiť filtre` button.

### Interactions

- Pull-to-refresh on the page re-fetches the day's data (native browser behaviour, no custom gesture).
- Date change → fetch new day's data, show skeleton (`.skeleton`) while loading.

## Schedule page

**Route:** `/schedule`

Role-aware:

- **Customer:** existing public schedule — week strip, class cards, book/cancel. Unchanged.
- **Admin:** same layout, extended:
  - Expandable per-class roster with `.list-row`s showing booked members + status badges.
  - Walk-in button per class → card-search sheet → selected card → booking + charge created.
  - Cancel-class button per class → confirmation → cancels all bookings + marks occurrence cancelled (if the data model supports it; otherwise flags the bookings only, existing behaviour).
  - All admin features from the deprecated `/staff/classes` page fold in here.

No new API endpoints; uses existing schedule / booking endpoints.

`/staff/classes` route redirects to `/schedule`. The `staff_dashboard.rs` file's admin-specific logic moves into `schedule.rs`.

## Settings page

**Route:** `/settings`

- Same 5 sub-tabs as today's `/admin`, same forms, same functionality, same translations.
- Visual refresh only: audit any remaining pre-v0.9.0 styling (inline styles, old `.tabs` usage) and replace with current primitives.
- `/admin` → redirect to `/settings`.

Tab names (Slovak):

- `Centrum` (Settings — name, bike count)
- `Služby` (Services — pricing)
- `Permanentky` (Class templates — weekday/time/capacity/instructor)
- `Inštruktori` (Instructors)
- `Používatelia` (Users — visible only if role allows; existing logic)

## Data & API

### New endpoints

```
GET /api/reports/day?date=YYYY-MM-DD
→ 200 {
    kpi: {
      revenue_eur: f64,       // absolute value of charge sum
      attendance: i64,        // count of non-voided charges
      passes_sold: i64,       // count of non-voided sell-pass transactions
      cash_in_eur: f64        // sum of non-voided top-up transactions
    },
    events: [TxnInfo, ...],   // chronological desc, capped at 50, with ?before cursor for paging
    alerts_count: i64         // sum across all three alert types (for badge on nav)
  }

GET /api/reports/range?from=YYYY-MM-DD&to=YYYY-MM-DD
→ 200 same shape as /day, for range inclusive.
→ 400 if (to - from) > 93 days.

GET /api/reports/alerts
→ 200 {
    expiring_passes: [ { card_id, name, barcode, valid_until, days_left }, ... ],
    low_credit:      [ { card_id, name, barcode, credit }, ... ],
    inactive:        [ { card_id, name, barcode, last_visit }, ... ]
  }

GET /api/reports/now
→ 200 {
    current_class: Option<{ template_id, start_at, end_at, roster: [RosterEntry, ...] }>,
    next_class:    Option<{ template_id, start_at, service_name, instructor_name, booked: i64, capacity: i64 }>
  }
```

All require `admin` role (checked in Axum middleware via existing `require_admin` extractor).

### Existing shapes reused

- `TxnInfo` — already defined in `crates/spinbike-core/src/transactions.rs`. Adds `service_name: Option<String>` if not already present (it is).
- `RosterEntry` — `{ card_id, name, barcode, booking_id, status: "booked" | "checked_in" | "cancelled" }`. New shape in `spinbike-core`.

### SQL implementation

- **Day report:** one query on `transactions` joined with `cards` and `services`, filtered `WHERE date(created_at) = ? AND deleted_at IS NULL`. KPIs aggregated in Rust (loop over rows — under 1000/day expected). Limit 50 + cursor `AND created_at < ?` for older page.
- **Range report:** same pattern with `WHERE created_at BETWEEN ? AND ?`. Rust validates 93-day cap.
- **Alerts:** three independent queries, each returning at most 100 rows (`LIMIT 100`):
  - Expiring: use the existing correlated subquery pattern from `crates/spinbike-server/src/db/cards.rs` — `SELECT c.id, c.first_name, c.last_name, c.barcode, (SELECT MAX(valid_until) FROM transactions WHERE card_id = c.id AND valid_until IS NOT NULL AND deleted_at IS NULL) AS pass_valid_until FROM cards c WHERE NOT c.blocked HAVING pass_valid_until IS NOT NULL AND pass_valid_until BETWEEN DATE('now') AND DATE('now','+7 days') ORDER BY pass_valid_until ASC LIMIT 100`. (Pass validity lives on the `transactions` table; the `cards` table does not store it.)
  - Low credit: `SELECT … FROM cards WHERE credit < 5 AND NOT blocked ORDER BY credit ASC`.
  - Inactive: `SELECT c.id, c.first_name, c.last_name, c.barcode, MAX(t.created_at) AS last_visit FROM cards c LEFT JOIN transactions t ON t.card_id = c.id AND t.deleted_at IS NULL AND t.amount < 0 WHERE NOT c.blocked AND c.credit > 0 GROUP BY c.id HAVING last_visit IS NULL OR last_visit < DATE('now','-60 days') ORDER BY last_visit ASC LIMIT 100`.
- **Now panel:** class-template lookup for current weekday + time window, roster join on `bookings` + `transactions` to infer `status`.

### Performance

- No caching layer for v1. SQLite WAL with current volume (<100k txns) handles sub-100ms queries for day/range.
- Frontend caches current day's response in a Leptos signal; re-fetched on date change or explicit pull-to-refresh.

### Schema changes

None. No new tables, no new columns, no new migrations.

## Error handling

- API errors surface as inline `.alert--error` banners at the top of the Reports page (or relevant panel).
- Empty responses render empty-state cards (see Reports / Empty states above).
- Network failure → "Nepodarilo sa načítať údaje. Skúste obnoviť stránku." with retry button.
- Invalid date range (>93 days): client prevents via disabled submit; if server rejects with 400, show alert and reset to last valid range.

## Testing plan

### Playwright E2E (new)

All tests MUST assert `consoleMessages === []` as the last step.

- **`reports-day.spec.ts`**
  - Seed 3 txns on a specific date via API helper → navigate `/reports` → verify KPI cards match computed values → verify activity feed shows 3 rows in descending time order.
  - Click `‹ Včera` → verify state/URL changes, different data loads.
  - Click date label → calendar sheet opens → pick past date → verify feed loads for that date.

- **`reports-filters.spec.ts`**
  - Seed charge + top-up + pass for one day → open Reports → expand filters → select `Platby` → only charge row shown → select service `Spinning` → only spinning charge shown → type customer name in search → only that customer.
  - Click `Reset` → all rows back.

- **`reports-alerts.spec.ts`**
  - Seed: one pass expiring in 3 days, one card credit €2, one card inactive 70 days (last charge 70 days ago) → open Reports → verify alerts banner shows 3 rows with counts 1/1/1 → tap "expiring" → verify `AlertSheet` opens showing that specific card → dismiss via × → reload → verify alert still present next day (per-day dismissal).

- **`reports-range.spec.ts`**
  - Seed 5 days of data → click `Týždeň` → verify aggregate KPIs = sum across 5 days → verify feed shows events from all 5 days.
  - Send `from=2026-01-01&to=2026-06-01` via API → verify 400.

- **`desk-now-panel.spec.ts`**
  - Seed a class template whose weekday and start_time fall within the next 3 hours from the actual test run time, plus 2 bookings on it → open Desk → verify Now panel shows the class with `2/<capacity>` badge → expand → verify both booked names visible → tap a name → verify card detail opens for that card.
  - Delete all active class templates (test helper) → verify Now panel shows `"Dnes už žiadne hodiny."` fallback.
  - Server clock is authoritative (we do not mock time on the server). Tests seed data relative to the real server `NOW()`.

- **`nav-adaptive.spec.ts`**
  - Viewport 375×812 (iPhone) → verify bottom tab bar visible, sidebar hidden.
  - Viewport 1280×800 → verify sidebar visible, bottom tabs hidden.
  - Tap each destination → verify URL and active-state change.

- **`schedule-roster-admin.spec.ts`**
  - Admin opens `/schedule` → expand a class with 2 bookings → verify roster shows 2 members.
  - Click `Walk-in` → card-search sheet → pick card → verify booking + charge created.

### Rust unit tests (`crates/spinbike-server/tests/reports.rs`)

- `day_kpi_aggregation` — given seeded mixed transactions (charge, top-up, pass, voided), verify KPIs are correct including voided-exclusion.
- `alerts_expiring_passes_range` — verify query returns only passes with `valid_until ≤ today+7`.
- `alerts_low_credit_excludes_blocked` — verify query excludes blocked cards regardless of credit.
- `alerts_inactive_excludes_zero_credit` — verify query excludes cards with credit ≤ 0 (they aren't customers we want to re-engage).
- `range_rejects_over_93_days` — GET /api/reports/range with 94-day range returns 400.
- `now_panel_picks_current_class` — given fixed clock and a template for "now", verify current_class is non-null.
- `now_panel_picks_next_within_3h` — given fixed clock and a template 2 h away, verify next_class is set.

### Mutation testing

Existing CI gate `cargo mutants --in-diff pr.diff --timeout 60` covers the new `reports.rs` module.

### Manual post-deploy verification

Via Playwright on production URL:

1. Navigate to `/reports` as Štefan → verify today's numbers match recent transaction activity.
2. Swipe to yesterday → verify yesterday's numbers visible.
3. Tap Week → verify 7-day aggregate shown.
4. Verify alerts banner content is sensible (no false-positive expired passes, no wrong low-credit flags).
5. Switch viewport to desktop via browser resize → verify sidebar appears.
6. Browser console: zero errors, zero warnings.

## Non-goals (explicit)

- Charts, graphs, trend lines.
- CSV or PDF exports.
- Multi-staff features (no per-staff attribution or shifts).
- New roles (no "CEO" or "Owner" role; existing `admin` is Štefan).
- Notifications / emails (alerts are in-app only).
- Gesture-based swiping beyond pull-to-refresh (Safari PWA conflicts).
- Real-time updates (no WebSocket / polling for live KPIs).
- Date ranges over 93 days.
- Customer-facing UI changes.
- Full card-detail rewrite (only hierarchy cleanup, testids preserved).
- New visual tokens or primitives.

## Version

Bump `VERSION` `0.9.7` → **`0.10.0`**. New top-level `/reports` page + adaptive nav + Desk "Now" panel is a significant user-facing change, minor version bump (no breaking API).

## File structure

### New files

```
spinbike-ui/src/pages/reports/mod.rs                       # page entry + date nav
spinbike-ui/src/pages/reports/kpi_cards.rs                  # 4 KPI cards
spinbike-ui/src/pages/reports/alerts_banner.rs              # needs-attention banner + dismissal
spinbike-ui/src/pages/reports/activity_feed.rs              # feed + pagination
spinbike-ui/src/pages/reports/filters_bar.rs                # collapsible filters
spinbike-ui/src/pages/reports/sheets/calendar_picker.rs     # calendar-pick sheet
spinbike-ui/src/pages/reports/sheets/alert_detail.rs        # per-alert card list sheet
spinbike-ui/src/pages/desk/now_panel.rs                     # Now panel + roster expansion + walk-in
spinbike-ui/src/components/adaptive_nav.rs                  # bottom tabs + sidebar (one component)

crates/spinbike-server/src/routes/reports.rs                # new /api/reports/* handlers
crates/spinbike-server/src/db/reports.rs                    # DB queries for day/range/alerts/now
crates/spinbike-core/src/reports.rs                         # shared response types

crates/spinbike-server/tests/reports.rs                      # Rust integration tests

e2e/tests/reports-day.spec.ts
e2e/tests/reports-filters.spec.ts
e2e/tests/reports-alerts.spec.ts
e2e/tests/reports-range.spec.ts
e2e/tests/desk-now-panel.spec.ts
e2e/tests/nav-adaptive.spec.ts
e2e/tests/schedule-roster-admin.spec.ts
```

### Modified files

```
spinbike-ui/src/app.rs                                     # new routes: /reports, /settings, redirects
spinbike-ui/src/components/nav.rs                          # replaced by adaptive_nav or becomes header-only
spinbike-ui/src/pages/dashboard/mod.rs                     # Now panel mount + card detail hierarchy tweak
spinbike-ui/src/pages/dashboard/card_panel.rs              # hierarchy: primary/secondary/tertiary button rows, collapsed contact info
spinbike-ui/src/pages/schedule.rs                          # admin-branch merges /staff/classes features
spinbike-ui/src/pages/staff_dashboard.rs                   # DELETED; logic lifted into schedule.rs + now_panel.rs
spinbike-ui/src/pages/admin.rs                             # renamed visually to Settings; translations updated
spinbike-ui/src/i18n.rs                                    # new keys: reports_label, revenue, attendance, passes_sold, cash_in, needs_attention, expiring_passes, low_credit, inactive_customers, week, month, calendar_title, filters_label, reset_filters, no_activity_today, no_filter_results, walk_in, cancel_class, ...
spinbike-ui/style.css                                      # safe-area padding + sidebar rules + now-panel + kpi-card
VERSION                                                    # 0.9.7 -> 0.10.0

crates/spinbike-server/src/routes/mod.rs                   # register reports module
crates/spinbike-server/src/app.rs                          # wire /api/reports/* routes
crates/spinbike-core/src/lib.rs                            # re-export reports module
```

## Rollout risk & mitigations

- **Risk:** breaking existing E2E tests via nav restructure.
  **Mitigation:** keep every `data-testid` used in existing specs stable; add new ones for new surfaces; run full E2E suite locally before PR.

- **Risk:** admin users with bookmarks to `/admin` or `/staff/classes`.
  **Mitigation:** 302 redirects from old routes → new routes.

- **Risk:** alerts produce false positives during legacy-data edge cases (e.g. cards with `valid_until` set but no real pass active).
  **Mitigation:** expiring-passes query excludes blocked cards and checks `pass_valid_until IS NOT NULL`; manual post-deploy verification confirms output sanity.

- **Risk:** Reports range query on 93-day window slow on production DB.
  **Mitigation:** measure on prod copy before launch; if slow, add an index on `transactions(created_at)` (free — SQLite accepts online index creation).

- **Risk:** bottom tab bar + iOS Safari URL bar visually conflicting.
  **Mitigation:** `env(safe-area-inset-bottom)` padding; manual verification on iPhone both in-browser and as installed PWA.

- **Risk:** big PR hard to review.
  **Mitigation:** implement task-by-task via `superpowers:subagent-driven-development`; commits segment cleanly (nav → reports skeleton → reports KPIs → reports alerts → reports feed → now panel → desk hierarchy → settings demotion → route redirects → tests).
