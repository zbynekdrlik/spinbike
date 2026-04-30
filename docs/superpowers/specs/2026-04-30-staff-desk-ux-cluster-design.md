# Staff Desk UX Cluster — Design

> Bundles four GitHub issues that all touch the staff "desk" workflow on the card dashboard:
> - **#29** — Fitness predefined in service combo
> - **#30** — Quick "Spinning 3.30€ from credit" button
> - **#31** — Block charge when no service is selected (data integrity)
> - **#32** — Card-dashboard layout cleanup (title, name, pass row, button readability)

**Target version:** `0.13.10` (dev currently equals main at `0.13.9`; first commit on dev MUST bump VERSION).

**Scope:** UI + CSS + one thin server validation. **No DB migration.** Out-of-band one-time data fix to `services.default_price` for Spinning, done by the operator via the existing `/admin/services` UI.

---

## Background

PR #27 (just merged as v0.13.9) cleared up card history and added per-transaction notes. Štefan (sole operator: CEO + admin + staff + desk) immediately filed four follow-up issues that all touch the **same surfaces** — the card-dashboard view and its inline action form. Bundling them is natural; splitting would force three reviews of overlapping CSS/markup.

The four asks share one underlying intent: **shorter clicks, cleaner data, more glanceable layout.** The desk workflow runs many times per day; every saved click compounds.

## Issue → Change

### #29 — Fitness predefined in combo box

**Today:** the service `<select>` opens with an empty placeholder option (`"…select service…"`). Staff must click the dropdown and pick Fitness every time, even though it is the dominant non-pass service.

**Change:**

- On `ActionForm` mount, look up the Fitness service by stable name (`spinbike_core::services::FITNESS_NAME_EN`) and set `selected_service_id` to its `id`.
- **Remove** the empty `<option value="">…</option>` placeholder so there is no way to pick "no service" from the UI.

**Why "remove the empty option" matters:** combined with the change below for #31, the UI can no longer submit a charge with `service_id = null`.

### #31 — Block payment when no service is selected

**Today:** `do_charge` reads `selected_service_id.get_untracked()` and posts whatever it gets — `Some(id)` or `None`. The server's `ChargeRequest.service_id` is `Option<i64>`, so it accepts `None`. Result: charges land in `transactions` with no service link, polluting the activity feed and reports.

**Change (UI side):** solved by #29 (no empty option exists, Fitness preselected).

**Change (server side, defense-in-depth):**

- `POST /api/payments/charge` validates: if `service_id` is `None`, return `400 Bad Request` with `{ "error": "service_id required for charge" }`.
- This mirrors the note-length cap pattern shipped in PR #27: server enforces the rule even if a future endpoint, curl test, or regressed UI tries to skip it.
- **Top-up is NOT affected.** `POST /api/cards/topup` continues to accept no service (top-up is service-independent — pure money on card). `sell-pass` and `log-visit` already require a service implicitly (the service IS the action), so no change there.

### #30 — Quick "Spinning 3.30€ from credit" button

**Today:** charging a Spinning visit takes 3 clicks: pick service in combo, type `3.30` into amount, click Charge. Spinning is one of the two most common operations.

**Change:**

- New `.quick-charge-row` chip row rendered **above the service combo**, always visible (regardless of monthly-pass status).
- One button: label `"Spinning {:.2} €"` formatted from the Spinning service's `default_price` field, read **live** from the services list signal (so admin edits propagate without a redeploy or page hard-refresh).
- One click → `POST /api/payments/charge` with `card_id`, `amount = default_price`, `service_id = spinning_id`. Same endpoint and response handling as the regular Charge submit.
- Loading state and error display reuse the existing `set_loading` / `set_err` signals.

**Why read `default_price` live (not hardcode 3.30):**

- The `/admin/services` UI already supports editing `default_price` (handler at `crates/spinbike-server/src/routes/admin.rs:567`). Štefan can change the price any time without code.
- It also fixes the unrelated complaint that "default_prices are wrong" — once Štefan updates Spinning's price to 3.30 via `/admin`, every consumer (this button, future quick-actions) reads the truth.

**Off-band data fix (deploy-day step, NOT in code):**

- After PR merges and prod deploys, Štefan opens `/admin/services` → Spinning → set `default_price = 3.30` → save.
- Or, equivalently: `sqlite3 /opt/spinbike/prod/spinbike.db "UPDATE services SET default_price=3.30 WHERE name_en='Spinning';"`.
- Dev DB picks the new value up automatically on the next deploy via the prod→dev sync added in v0.13.9 (per `feedback_dev_ci_sync_prod_db.md`).

**Edge cases:**

- If the Spinning service is missing or inactive, the row is empty (no chip rendered). Should never happen in practice; defensive only.
- If the card credit is below `default_price`, the server still allows the charge (this matches existing behavior — credit can go negative, intentionally, and PR #19 already shows the negative-credit warning style).

### #32 — Dashboard layout cleanup

Four sub-asks from the issue body, addressed in order:

**(a) Remove "Cards — Quick Dashboard" title.**
- `mod.rs:307` `<h1 class="page-title">{i18n::t("card_dashboard")}</h1>` → deleted.
- The i18n key `card_dashboard` ("Karty — rychly prehlad" / "Cards — Quick Dashboard") becomes unused; remove it from `i18n.rs` to avoid dead keys.

**(b) Card name + barcode on ONE line, name much bigger.**
- `card_panel.rs:48-53` currently stacks `.card-title` (name) above `.card-header__meta` (barcode in `<code>`).
- Restructure into a single line: `<div class="card-title"><span class="card-title__name">{name}</span> <code class="card-title__barcode">{barcode}</code></div>`.
- CSS: `.card-title__name` becomes the dominant element (e.g. `font-size: 1.75rem`, `font-weight: 700`); `.card-title__barcode` stays smaller and muted (`font-size: 1rem`, `color: var(--text-muted)`), aligned by baseline.

**(c) Monthly-pass row collapsed to ONE line.**
- Today (active pass): two stacked rows
  ```
  ✓ Mesačný lístok platný do 14.5.2026     [Edit date]
  14 dní zostáva · neobmedzený prístup
  ```
- Target (active pass): single line
  ```
  ✓ Mesačný lístok do 14.5.2026 (14 dní) ✏
  ```
- Implementation in `pass_banner.rs`:
  - Replace `pass-banner-title` + `pass-banner-sub` two-div structure with a single-line `pass-banner__line` div.
  - New i18n key `pass_active_oneline_format` with placeholders for date and days, e.g. Slovak: `"✓ Mesačný lístok do {date} ({days} dní)"`. Use existing `i18n::tf` helper.
  - Replace the text "Edit date" button with a pencil icon button. Use Unicode `✏` (U+270F) — no new icon dependency, already monospace-friendly. Keep `data-testid="pass-date-edit"` for test stability.
  - Drop the now-unused i18n keys after migrating both branches: `pass_valid_until`, `pass_days_remaining`, `pass_expired`, `pass_days_ago`, `pass_last_valid_until`. (Grep first to confirm nothing else still references them.)
- Expired-pass case: collapse symmetrically. Single line, e.g. Slovak: `"⚠ Mesačný lístok vypršal pred {n} dňami (do {date})"` — single i18n key `pass_expired_oneline_format`.

**(d) Log-visit class buttons: bigger + bolder + darker brand shade.**
- The buttons in `action_form.rs:301-308` currently use `btn btn--compact btn--info` (Fitness, solid blue) or `btn--info-soft` (Spinning, soft blue), with white text.
- CSS changes in `style.css`:
  - Remove `btn--compact` from these specific buttons (revert to standard padding) — done at the JSX site, not in CSS.
  - For the parent `.chip-row` or a new modifier `.chip-row--readable`: `font-size: 1.125rem` (~18px), `font-weight: 700`.
  - Darken `--info` and `--info-soft` brand variables by ~10% to bump white-on-blue contrast. **Verify with WCAG AA** (contrast ratio ≥ 4.5 for normal text or ≥ 3.0 for ≥18px bold text). The existing soft variant may already pass; the goal is subjective readability bump, but we should not break the contrast guarantee.

## Architecture

No structural change. All edits land inside the existing `dashboard/` module. The change has three layers:

| Layer | Files | What changes |
|---|---|---|
| **UI markup** | `spinbike-ui/src/pages/dashboard/{action_form,card_panel,pass_banner,mod}.rs` | Form default, quick-charge chip row, single-line card header, single-line pass banner, page-title removal |
| **UI strings** | `spinbike-ui/src/i18n.rs` | Drop unused keys; add `pass_active_oneline_format`, `pass_expired_oneline_format`, `quick_charge_spinning_label` |
| **UI styles** | `spinbike-ui/style.css` | `.card-title*`, `.pass-banner__line`, `.quick-charge-row`, `.btn--info` (darker), font-size on visit buttons |
| **Server** | `crates/spinbike-server/src/routes/payments.rs` | Reject charge with `service_id=null` (400) |
| **Data (off-band)** | prod `services` table | Štefan sets Spinning `default_price = 3.30` via `/admin/services` |

There is no DB migration. There is no schema change.

## Tests

Per `tdd-workflow.md` and `e2e-real-user-testing.md`, every user-visible change gets an E2E. Server-side changes also get an integration test.

### Server integration tests

`crates/spinbike-server/tests/payments_charge_validation.rs` (NEW):

1. `charge_rejects_null_service_id_with_400` — POST `/api/payments/charge` with `service_id: null`, assert status `400`, body contains `service_id required for charge`.
2. `charge_with_valid_service_id_still_succeeds` — regression guard.
3. `topup_still_accepts_null_service_id` — confirms top-up is unaffected by the new rule.

### Playwright E2E

`e2e/tests/desk-ux.spec.ts` (NEW), eight cases. Each test sets up a console listener and asserts `expect(consoleMessages).toEqual([])` at the end (per `browser-console-zero-errors.md`).

**Deferred to follow-up issues:**

- Cases 1 ("fitness preselected on form open") and 2 ("charge form has no empty service option") — moved to **issue #33** (Fitness preselect redesign).
- Case 3 ("quick spinning charge button charges card") — moved to **issue #34** (Spinning quick-charge chip redesign).

Cases 4-8 (card header one line, pass banner active/expired one line, h1 gone, log-visit buttons styling) are implemented in `e2e/tests/desk-ux.spec.ts` and ship in this PR.

1. **`fitness preselected on form open`** — search for a card → action form opens → `select[data-testid="charge-service"]` has `value === fitness_id` immediately, before any user interaction.
2. **`quick spinning charge button charges card`** — click `[data-testid="quick-charge-spinning"]` → wait for `/api/payments/charge` 200 response → assert credit decreased by service price → assert new transaction row appears in card history with action=charge and service=Spinning.
3. **`charge form has no empty service option`** — verify the `<option>` count under `[data-testid="charge-service"]` matches the count of active services exactly (no placeholder).
4. **`card header shows name and barcode on one line`** — assert `.card-title` contains both name and barcode in a single child row (no `.card-header__meta` div); assert font-size of `.card-title__name` is greater than baseline (e.g. ≥ 24px via `getComputedStyle`).
5. **`pass banner active is one line`** — for a card with active monthly pass, assert `[data-testid="pass-banner-active"]` text matches the new format `/^✓ Mesačný lístok do \d{1,2}\.\d{1,2}\.\d{4} \(\d+ dní\)/u` and the pencil icon button (`[data-testid="pass-date-edit"]`) is present at the end of the same line (single child row, not stacked).
6. **`pass banner expired is one line`** — for a card with an expired pass, assert `[data-testid="pass-banner-expired"]` is also a single line and matches the expired-oneline format. (Symmetry guard so we don't ship one-line active + two-line expired by accident.)
7. **`Cards — Quick Dashboard h1 is gone`** — navigate to `/staff` and assert the body does NOT contain "Cards — Quick Dashboard" or "Karty — rychly prehlad" (case-insensitive).
8. **`log-visit class buttons are bigger and bolder`** — for a card with active pass, assert the computed `font-size` of `[data-testid="log-visit-btn"]` is ≥ 18px and `font-weight` is ≥ 700.

### Unit/property tests

- **`spinbike-core` services constants:** if we add `SPINNING_NAME_EN` / `SPINNING_NAME_SK`, add a 1-line test that asserts they match the seeded service rows the migration produces.
- **i18n key audit:** the existing test `i18n_keys_present` (or equivalent) should be updated to drop removed keys and add new ones.

### Mutation testing

Per `mutation-testing.md`, the new server validation is per-call-site and must kill mutants. Mutants likely to surface:
- The `if service_id.is_none()` guard → `is_some()` swap. The test `charge_rejects_null_service_id_with_400` kills this.
- The status code `400` → `200` swap. Same test kills this.
- The error string `"service_id required for charge"` → empty string. The body-contains assertion kills this.

Add an extra test if any mutant survives in CI (`cargo mutants` runs on PR diff, will surface them).

## Acceptance Criteria

The PR is ready when:

1. ✅ VERSION bumped to `0.13.10` as the first commit on dev.
2. ✅ Form opens with Fitness selected by default; service combo has no empty placeholder option.
3. ✅ Quick-charge "Spinning {price}€" chip is visible above the combo on every card view, regardless of pass status.
4. ✅ Quick-charge button reads `default_price` from the Spinning service live (verified by editing the price in `/admin` and seeing the chip update on next dashboard load).
5. ✅ Server rejects `POST /api/payments/charge` with `service_id=null` and 400. Top-up still accepts null.
6. ✅ Dashboard view: no "Cards — Quick Dashboard" title; card name + barcode on one line with name visibly larger; monthly-pass active row is a single line ending with a pencil-icon edit button.
7. ✅ Log-visit class buttons (when pass active) are bigger, bolder, and darker — visibly more readable than v0.13.9.
8. ✅ All E2E + server tests green; mutation testing on PR diff reports no surviving mutants; browser console is clean (zero errors / warnings) on the dashboard route.
9. ✅ Post-deploy: Štefan updates Spinning's `default_price` to `3.30` via `/admin/services`. Verified by re-loading the dashboard and seeing `Spinning 3.30 €` on the chip.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Removing the empty `<option>` breaks reactive code that reads `selected_service_id == None` to render special UI | `selected_service_id` is initialised to `Some(fitness_id)` immediately; the only `None` path is "before init", which never renders. Verify with E2E test #1. |
| Reading `default_price=0.0` for Spinning (current prod value is 5.0, not 3.30) means the chip ships saying "Spinning 5.00 €" until Štefan updates it | Acceptable — the chip is correctly priced as soon as Štefan saves the new price in `/admin`. The acceptance criterion #9 makes this an explicit deploy-day step. |
| Pencil icon glyph renders differently across systems / fonts | Use a CSS-controlled icon class if one already exists in the codebase; fall back to Unicode `✏` only if no icon system is in place. The existing date-edit button text label can also be replaced with the icon while keeping `data-testid="pass-date-edit"` for stability. |
| Darkening `--info` reduces contrast on dark mode | Verify both light and dark themes manually if dark mode is enabled in the codebase; if dark mode not supported, no concern. |
| The new chip row pushes the rest of the form down on small screens | The chip row is one row of buttons; existing layout is already a vertical stack so this just adds one row at the top. Verify on the smallest tested viewport (Štefan's iPad). |

## Out of Scope

- **Cleaning up other stale `default_price` values** — Štefan handles this in `/admin` whenever convenient.
- **Adding a Fitness quick-button** — only Spinning was asked for; Fitness has variable pricing per the existing form workflow.
- **Adding `CHECK(length(note) <= 200)` constraint on transactions.note** — this is issue **#28**, separate PR (needs table-recreate migration on 88k+ row prod table).
- **Root-causing the E2E flake** — issue **#24**, separate PR.
- **Mutation testing for `spinbike-ui`** — issue **#22**, separate PR.

## References

- Recently merged: PR #27 (issue #26 — card history clarity + per-transaction notes, v0.13.9).
- Project memory: `feedback_decision_per_item.md` (per-item label decisions), `feedback_design_presentation.md` (short business-focused choices), `feedback_dev_ci_sync_prod_db.md` (prod→dev sync), `feedback_validate_against_real_data.md`, `feedback_subagent_no_local_build.md`.
- Airuleset: `tdd-workflow.md`, `version-bumping.md`, `version-on-dashboard.md`, `e2e-real-user-testing.md`, `browser-console-zero-errors.md`, `mutation-testing.md`, `pr-merge-policy.md`.
