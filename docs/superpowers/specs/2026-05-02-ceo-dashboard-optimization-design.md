# CEO Dashboard Optimization — Design

**Version target:** 0.13.15 (bump from 0.13.14 on first commit on dev)

**Branch strategy:** dev → main, single PR.

**Goal:** Trim wasted UI surface for the CEO's daily card-management workflow.
Four small, independent UI changes shipped as one PR.

## 1. Reports — drop "Needs attention" banner (delete-completely)

The `AlertsBanner` (expiring passes / low credit / inactive customers, with
dismiss + detail-sheet) is removed wholesale. The CEO doesn't act on it.

**Files to delete:**
- `spinbike-ui/src/pages/reports/alerts_banner.rs`
- `spinbike-ui/src/pages/reports/sheets/alert_detail.rs` (verify the
  `sheets/mod.rs` re-export is the only other reference and remove it)
- `e2e/tests/reports-alerts.spec.ts`

**Files to edit:**
- `spinbike-ui/src/pages/reports/mod.rs`:
  - Remove `mod alerts_banner;` and `pub use alerts_banner::AlertsBanner;`
  - Remove the `<AlertsBanner data=alerts />` render site (currently line 168)
  - Remove the `(alerts, set_alerts)` signal and the `Effect` that fetches
    `/api/reports/alerts` (currently lines 49-56)
  - Remove the `AlertsResponse` import from `spinbike_core::reports`
- `spinbike-ui/src/pages/reports/sheets/mod.rs` — remove the
  `alert_detail` re-export
- `crates/spinbike-server/src/routes/reports.rs`:
  - Remove the `.route("/api/reports/alerts", get(alerts))` line
  - Remove the `alerts()` handler (currently lines ~95-105)
  - Remove the `AlertsResponse` import
- `crates/spinbike-server/src/db/reports.rs`:
  - Remove the `alerts_report()` function (currently line 322 onwards
    until the next `pub async fn`)
  - Remove unused imports it pulled in (`AlertsResponse`, `ExpiringPass`,
    `LowCreditCard`, `InactiveCustomer`)
- `crates/spinbike-core/src/reports.rs`:
  - Remove `AlertsResponse`, `ExpiringPass`, `LowCreditCard`,
    `InactiveCustomer` types
- `crates/spinbike-server/tests/reports.rs`:
  - Remove the four `/api/reports/alerts` test cases (currently calls at
    lines 189, 218, 269, 492 — each lives in its own `#[tokio::test]`
    function; remove the whole function)
- `spinbike-ui/style.css`: remove the `.alerts-banner*` rules block
  (currently lines ~1350-1400; verify by `grep -n alerts-banner style.css`
  before edit to confirm the exact range)
- `spinbike-ui/src/i18n.rs`:
  - Remove keys `alerts_title`, `alerts_expiring_passes`,
    `alerts_low_credit`, `alerts_inactive`
  - Remove any keys referenced ONLY by `alert_detail.rs` (grep each
    candidate: `alert_expiring_pass_title`, `alert_low_credit_title`,
    `alert_inactive_title`, plus any helper strings — verify each is
    unused after deletion before removing)

**Orphaned data (no migration needed):**
- `localStorage` keys `reports_alerts_dismissed_*` — daily-keyed; will
  age out naturally.

**Acceptance:**
- `/reports` page renders without the banner.
- `cargo test -p spinbike-server` passes (alerts test cases gone, not
  failing).
- `grep -rn "AlertsBanner\|alerts-banner\|/api/reports/alerts" .` returns
  zero hits across `crates/`, `spinbike-ui/src/`, `e2e/tests/`,
  `spinbike-ui/style.css`.

## 2. Reports — direct jump from row to card panel

Today: row click → `/staff?q=<barcode>` → search dropdown → user must
click the dropdown row.

New: row click → `/staff?card=<barcode>` → desk page calls
`/api/cards/lookup/<barcode>` directly → `selected = Some(card)`
immediately. One tap, not three.

**Files to edit:**

- `spinbike-ui/src/pages/reports/activity_feed.rs`:
  - In `render_row(e: ReportEvent)`, gate clickability on
    `e.barcode.is_some()`. Drop the `card_name` fallback.
  - When barcode is present, navigate to `/staff?card=<bc-encoded>`
    instead of `/staff?q=<...>`.
  - When barcode is absent, render the row WITHOUT
    `list-row--interactive` class and WITHOUT `on:click`. Pure
    presentational row, no cursor pointer (CSS already handles this via
    the class gate, but verify by inspecting `.list-row--interactive`
    rules in `style.css`).

  Replacement code (replaces lines ~186-200, exact line range to be
  verified at execution time):

  ```rust
  // Click → jump to Desk in exact-card mode (skips dropdown). Only
  // available when barcode is known: rows for old/voided/orphan
  // transactions render presentationally.
  let interactive = e.barcode.is_some();
  let row_class = if interactive {
      "list-row list-row--interactive"
  } else {
      "list-row"
  };
  let bc = e.barcode.clone();
  let on_row_click = move |_| {
      let Some(bc) = bc.clone() else { return; };
      if let Some(w) = web_sys::window() {
          let encoded = url_encode(&bc);
          let _ = w.location().set_href(&format!("/staff?card={encoded}"));
      }
  };
  ```

  And in the returned view, only attach `on:click=on_row_click` when
  `interactive` is true. Implementation note: Leptos doesn't support
  conditional event handlers as elegantly as React; the pattern is to
  always attach the handler but make it a no-op when `bc` is None — the
  cursor/hover style still needs gating via `row_class`. The handler
  above is already a no-op when `bc.is_none()`, so attaching it
  unconditionally is acceptable; clickability comes from the class
  difference. Use `data-testid="feed-row"` regardless (existing tests
  rely on this selector).

- `spinbike-ui/src/pages/dashboard/mod.rs`:
  - In the existing `?q=` parsing `Effect` (currently lines ~178-193),
    extend it to also parse `?card=<bc>`. If both are present, `?card=`
    wins (defensive — should not happen in practice since reports only
    sets `?card=`).
  - When `?card=<bc>` is present:
    - Call `api::get::<CardInfo>(&format!("/api/cards/lookup/{}", bc))`
    - On Ok(card), `set_selected.set(Some(card))` and `set_query.set("")`.
    - On Err (card deleted/404 race), `set_query.set(bc)` so the user
      sees the input populated and the existing search-empty UX kicks
      in (matches today's fallback flavor).

  Implementation outline (the actual function body added inside the
  existing `Effect::new`):

  ```rust
  // Pre-existing logic for ?q= stays. New branch for ?card=.
  if let Some(rest) = kv.strip_prefix("card=") {
      let bc = decode_uri_component(rest);
      if !bc.is_empty() {
          spawn_local(async move {
              match api::get::<CardInfo>(
                  &format!("/api/cards/lookup/{}", urlencoding_light(&bc))
              ).await {
                  Ok(card) => {
                      set_selected.set(Some(card));
                      set_query.set(String::new());
                  }
                  Err(_) => {
                      // 404 race: card deleted since report rendered.
                      // Fall back to populating search so user sees
                      // the populated input + empty result.
                      set_query.set(bc);
                  }
              }
          });
          break;
      }
  }
  ```

  The existing `?q=` branch remains untouched (other callers may
  still rely on it).

- `crates/spinbike-server/src/routes/cards.rs`: no change required —
  `/api/cards/lookup/{barcode}` already exists (line 168). Verify it
  returns the same `CardInfo` shape the UI expects (it does — UI's
  `CardInfo` struct matches the existing handler's response).

**Tests:**
- New `e2e/tests/reports-row-jump.spec.ts`:
  - Login as admin, seed at least one transaction with a known barcode
    (use the existing `/api/admin/topup` or test fixtures).
  - Navigate to `/reports`.
  - Click the first `[data-testid="feed-row"]` that has a barcode.
  - Assert URL becomes `/staff?card=<bc>`.
  - Assert the card panel `[data-testid="card-panel"]` is visible
    (the panel testid is set by `CardActionPanel` — verify by grep at
    plan-execution time).
  - Assert the search dropdown `[data-testid="search-result"]` is NOT
    visible (we skipped it).
  - Assert clean console (per project rule).

- For non-clickable rows: harder to fixture deterministically (would
  need a transaction with `card_id=NULL` and `barcode=NULL`). Skip the
  positive test of "non-clickable", document in the spec that it's
  covered by the existence check `interactive = e.barcode.is_some()`,
  and rely on cargo-mutants on the UI side to catch any regression that
  flips the boolean.

## 3. Desk — drop NowPanel ("next class" widget)

Delete `<crate::pages::NowPanel />` from `dashboard/mod.rs:306`. The
whole `desk` module is unused after removal (only file is
`now_panel.rs`).

**Files to delete:**
- `spinbike-ui/src/pages/desk/now_panel.rs`
- `spinbike-ui/src/pages/desk/mod.rs`
- `e2e/tests/desk-now-panel.spec.ts`

**Files to edit:**
- `spinbike-ui/src/pages/mod.rs`:
  - Remove `pub mod desk;` and `pub use desk::NowPanel;`
- `spinbike-ui/src/pages/dashboard/mod.rs`:
  - Remove `<crate::pages::NowPanel />` (currently line 306)
- `crates/spinbike-server/src/routes/reports.rs`:
  - Remove the `.route("/api/reports/now", get(now))` line
  - Remove the `now()` handler
  - Remove the `NowResponse` import
- `crates/spinbike-server/src/db/reports.rs`:
  - Remove the `now_panel()` function (currently line 418 onwards)
  - Remove unused imports it pulled in
- `crates/spinbike-core/src/reports.rs`:
  - Remove `NowResponse`, `CurrentClass`, `NextClass`, `RosterEntry`,
    `RosterStatus` types
  - Verify no other reports type uses them (grep before delete)
- `crates/spinbike-server/tests/reports.rs`:
  - Remove the four `/api/reports/now` test cases (calls at lines 319,
    370, 429, 538 — remove each enclosing `#[tokio::test]` function)
- `spinbike-ui/style.css`: remove `.now-panel*` rules
- `spinbike-ui/src/i18n.rs`:
  - Remove keys `now_next_on`, `now_no_more_today`, `status_booked`,
    `status_checked_in`, `status_cancelled` — but ONLY after grep
    confirms each is unused elsewhere. (`status_*` keys may be reused
    by other components — must verify per-key.)

**Orphaned data:**
- `localStorage` key `desk_now_collapsed` — orphaned, will not be read,
  no migration needed.

**Acceptance:**
- `/staff` page renders the search input directly under the page title
  (no NowPanel above it).
- `grep -rn "NowPanel\|now-panel\|/api/reports/now\|NowResponse\|now_panel" .`
  returns zero hits.

## 4. Phone — drop top navbar for staff/admin, add "More" to bottom bar

On phone, the top navbar burns 2 wrapped rows on rarely-used controls
(brand on row 1; username + Logout + EN/SK on row 2). Bottom adaptive-nav
(Desk / Schedule / Reports / Settings) is the real navigation. Hide the
top navbar entirely on phone when AdaptiveNav is rendered (= staff/admin
is logged in). Add a 5th "More" item to the bottom bar that opens a sheet
with username + EN/SK toggle + Logout.

### CSS

In the existing `@media (max-width: 540px)` block (style.css:1060+), add:

```css
@media (max-width: 540px) {
    /* Existing rules unchanged. Add: */
    body:has(.adaptive-nav) .navbar { display: none; }
}
```

The `:has()` selector is fully supported in evergreen browsers since
Safari 15.4 / Chrome 105 / Firefox 121 (all 2022-2023). No fallback
needed for SpinBike's deploy target.

This rule auto-handles the role split:
- staff/admin logged in: AdaptiveNav renders → top navbar hidden on phone.
- customer or logged-out: AdaptiveNav doesn't render → top navbar stays
  visible on phone (login/register/my-bookings/my-balance still
  reachable).
- desktop (≥768px): no `(max-width: 540px)` match → both bars render
  as today.

### AdaptiveNav: 5th item "More" + sheet

`spinbike-ui/src/components/adaptive_nav.rs` changes:

- Convert the component to manage a `(more_open, set_more_open)` signal
  (Leptos `signal(false)`).
- Append a 5th item AFTER the conditional admin items. The 5th item is
  a `<button>` (not `<a>` — no route navigation), styled the same as the
  other `.adaptive-nav__item`:
  ```
  <button class="adaptive-nav__item" data-testid="nav-more"
          on:click=move |_| set_more_open.update(|v| *v = !*v)>
      <span class="adaptive-nav__icon" inner_html=ICON_MORE />
      <span class="adaptive-nav__label">{i18n::t(lang.get(), "nav_more")}</span>
  </button>
  ```
  ICON_MORE is the Heroicons ellipsis-vertical (or grid) — pick
  `ellipsis-vertical` for visual consistency with other thin-stroke
  icons. Concrete SVG payload defined inline alongside `ICON_DESK` etc.

- Conditionally render the sheet next to the nav element (sibling, not
  child — nav contains only items):
  ```
  {move || if more_open.get() {
      view! {
          <Sheet
              testid="more-sheet".to_string()
              title=i18n::t(lang.get(), "nav_more").to_string()
              on_close=Callback::new(move |_| set_more_open.set(false))
          >
              <div class="more-sheet__user">{u.name.clone()}</div>
              <button class="btn btn--block btn--ghost"
                      data-testid="more-lang-toggle"
                      on:click=on_toggle_lang>
                  {move || match lang.get() {
                      Lang::Sk => "EN",
                      Lang::En => "SK",
                  }}
              </button>
              <button class="btn btn--block btn--danger"
                      data-testid="more-logout"
                      on:click=on_logout>
                  {move || i18n::t(lang.get(), "logout")}
              </button>
          </Sheet>
      }.into_any()
  } else { ().into_any() }}
  ```
  `on_logout` and `on_toggle_lang` logic is copied verbatim from
  `nav.rs:21-35` (clear_auth → bump auth_ver → location.set_href("/")
  for logout; save_lang → set_lang for toggle).

- The `auth_ver: ReadSignal<u32>` prop is already passed in — reuse it
  for `auth::clear_auth` reactivity.

- CSS (new, append to style.css):
  ```css
  .more-sheet__user {
      font-size: var(--fs-md);
      color: var(--text);
      font-weight: 600;
      padding: var(--s-2) 0;
      border-bottom: 1px solid var(--border);
      margin-bottom: var(--s-3);
  }
  ```

### `nav.rs` (top navbar): NO functional changes

The top navbar code stays as-is — customers and desktop still use it.
The CSS rule (`body:has(.adaptive-nav)`) is what hides it on phone for
staff/admin. This keeps the implementation surgical: no risk of breaking
the customer flow.

### i18n

Add to `spinbike-ui/src/i18n.rs`:
- `nav_more` → SK: "Viac" / EN: "More"

### E2E

`e2e/tests/nav-adaptive.spec.ts` modifications:

- In the existing mobile test (375×812 viewport), add:
  - `await expect(page.locator('[data-testid="nav-more"]')).toBeVisible();`
  - `await expect(page.locator('.navbar')).not.toBeVisible();` (the
    body-has rule must be active)
  - Click `nav-more` → assert `[data-testid="more-sheet"]` visible →
    click `more-logout` → assert URL became `/login` (or whatever the
    post-logout default is — verify via existing auth-spec).
- In the existing desktop test (1280×800 viewport), add:
  - `await expect(page.locator('.navbar')).toBeVisible();` (still
    present on desktop)

The existing assertions for `nav-desk`, `nav-schedule`, `nav-reports`,
`nav-settings`, `[data-testid="adaptive-nav"]` stay unchanged.

### Existing tests that touch `.navbar`

`e2e/tests/auth.spec.ts` references `.navbar-links` and `.navbar-user`
in 6 places (lines 28, 33, 52, 53, 63, 70, 95, 96). These all run at
default Playwright viewport (1280×720), which is ≥541px → the
body-has rule does NOT hide the navbar. They continue to pass without
change. Verify by running locally with `npx playwright test
auth.spec.ts` — but per project rules, CI is authoritative.

## Cross-cutting

### Version bump (FIRST commit on dev)

Per project version-bumping policy: VERSION is currently 0.13.14 on
both main and dev. The first commit on dev for this PR MUST bump:

```bash
echo "0.13.15" > VERSION
bash scripts/sync-version.sh
git add VERSION crates/*/Cargo.toml spinbike-ui/Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.13.15"
```

### Mutation testing

- Server-side cargo-mutants will run on the routes/reports.rs and
  db/reports.rs deletions. Deleting handlers can't survive mutants
  (the code is gone). The `/api/cards/lookup` route is unchanged and
  already has its mutants covered by existing test cases — no new
  server-side mitigation expected.
- UI side cargo-mutants will run since `spinbike-ui/src/` has
  non-test changes (activity_feed.rs, adaptive_nav.rs, dashboard/mod.rs,
  pages/mod.rs, reports/mod.rs). The new sanity check from PR #44
  passes without intervention. Surviving mutants on the new logic
  (e.g. `interactive = e.barcode.is_some()`) will be caught by the
  reports-row-jump E2E test (which only clicks rows with barcode).

### Out of scope

- Customer-facing top navbar restyling (user is the CEO; customer view
  unchanged).
- Reordering of bottom adaptive-nav items.
- New report event-type filters or KPI changes.
- Replacing the report-row navigation contract for any non-Reports
  caller of `/staff?q=`.

## Acceptance criteria (full PR)

1. CI green (Test Integrity, Lint, Test, Test (UI), Build WASM (UI),
   E2E Tests, Mutation Testing, Mutation Testing (UI), Deploy (dev),
   Smoke (dev)).
2. Post-deploy: dev frontend `[data-testid="version"]` reads `v0.13.15`
   and matches `/api/version`.
3. Manual / E2E spot-checks on dev:
   - `/reports` page has no "Needs attention" banner.
   - Clicking a report row with a barcode lands directly in the card
     panel (no dropdown click required).
   - `/staff` page shows the search input immediately under the title;
     no NowPanel.
   - On phone viewport (375px wide), the top navbar is hidden when
     logged in as admin; the bottom bar shows 5 items including "More";
     "More" sheet contains username + EN/SK + Logout.
4. Production parity verified the same way after merge to main.
