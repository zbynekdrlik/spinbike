# Legacy Services Backfill + Configurable Item Catalog — Design Spec

**Date:** 2026-04-27
**Status:** Approved (pending user review)

## Problem

Two related gaps in the SpinBike PWA:

1. **Historical data loss.** The legacy MS Access import preserved 87,734 transaction rows, but
   `migrate_legacy.rs::map_legacy_service_name` only maps three Slovak service names
   (Fitnes, Spinbike, Casova karta). Everything else gets `service_id = NULL`. Concretely lost:
   - 4,499 `Doplnky Vyzivy` rows (nutritional supplements)
   - 2,092 `Obcerstvenie` rows (refreshments / snacks)
   - 536 `AktivaciaKarty` rows (card activation fee)
   - **~7,100 transactions imported but stripped of "what was sold."**

2. **No way to sell anything but Spinning, Fitness, and Monthly pass.** Today's seeded
   catalogue is exactly those three rows. Admin already has full CRUD at
   `/api/admin/services`, but the system has never been seeded with refreshments or
   supplements — so staff cannot record those sales without manually adding services.

The user wants the historical record restored AND a clean, configurable item list usable
from day one of this change.

## Goals

- Restore service labels for the ~7,100 NULL-service legacy transactions in production,
  in-place and idempotently (post-import sales must remain untouched).
- Add three new sellable services seeded out of the box: **Občerstvenie/Refreshments**,
  **Doplnky výživy/Supplements**, **Aktivácia karty/Card activation fee**.
- Switch service names to dual-language (`name_sk` + `name_en`) so the staff UI shows
  whichever language the user has selected, and the backfilled history reads naturally
  to a Slovak operator (legacy strings are Slovak).
- Replace the fragile `WHERE name = 'Monthly pass'` pattern with a stable
  `WHERE kind = 'monthly_pass'` lookup so admin can rename display names freely.
- Fix `migrate_legacy.rs` at the source so any future re-import is correct.

## Non-Goals

- Translating UI labels other than service names — i18n for static labels (buttons,
  headers) already exists and isn't touched here.
- Adding any service `category` taxonomy beyond the `kind` enum (`generic` |
  `monthly_pass`). Future special-purpose kinds get added when they're needed, not now.
- Reseeding the `Iont` legacy service — it had zero historical sales, YAGNI.
- Changing how the `Storno` action works — those 72 legacy rows already have
  `action='storno'` so the meaning is preserved; promoting `Storno` to a service row
  would just duplicate that signal.
- Tracking legacy `id_data` per transaction. Match-by-tuple is enough for this one-shot
  backfill; storing legacy ids would be debt for no future benefit.

## Architecture

The work splits into four self-contained changes, ordered as a strict dependency chain:

1. **Schema migration** — add `name_sk`, `name_en`, `kind` columns; drop `name`. Seed
   the three new categories with `kind='generic'`. Mark the existing pass row
   `kind='monthly_pass'`. Partial unique index enforces "at most one monthly_pass row."

2. **Code refactor** — every service-name read switches to `name_sk` / `name_en` (per
   `Lang`). Every `WHERE name = 'Monthly pass'` becomes `WHERE kind = 'monthly_pass'`.
   `ServiceInfo` gains `name_sk`, `name_en`, `kind`. Admin form gets two name inputs
   plus a kind selector (read-only after create). Display helper:
   `ServiceInfo::display_name(lang) -> &str`.

3. **Migrator fix** — `map_legacy_service_name()` learns three new mappings; lookup
   joins on `name_sk`. Future re-imports are correct.

4. **Backfill subcommand** — `migrate-legacy --backfill --mdb-path X --target Y`.
   Walks the .mdb's `Data` table, matches each prod transaction by
   `(barcode, created_at_string, amount_eur)` with a `service_id IS NULL` guard, sets
   `service_id` and `legacy_backfilled = 1`. Idempotent. Reports
   matched / unmatched / already-set / ambiguous.

## Data model

Latest migration in `db/migrations.rs` is `V7`. This work adds two new migrations:
**V8** for the dual-language services schema, **V9** for the `legacy_backfilled` marker
on `transactions`.

```sql
-- V8_SERVICES_DUAL_LANG_KIND
CREATE TABLE services_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT    NOT NULL DEFAULT 'generic'
                  CHECK (kind IN ('generic', 'monthly_pass')),
    name_sk       TEXT    NOT NULL,
    name_en       TEXT    NOT NULL,
    default_price REAL    NOT NULL,
    active        INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX idx_services_monthly_pass
    ON services_new(kind) WHERE kind = 'monthly_pass';

-- Migrate existing rows preserving ids
INSERT INTO services_new (id, kind, name_sk, name_en, default_price, active)
SELECT id,
       CASE WHEN name = 'Monthly pass' THEN 'monthly_pass' ELSE 'generic' END,
       CASE name WHEN 'Spinning' THEN 'Spinning'
                 WHEN 'Fitness' THEN 'Fitness'
                 WHEN 'Monthly pass' THEN 'Mesačný preplatok'
                 ELSE name END,
       CASE name WHEN 'Spinning' THEN 'Spinning'
                 WHEN 'Fitness' THEN 'Fitness'
                 WHEN 'Monthly pass' THEN 'Monthly pass'
                 ELSE name END,
       default_price, active
FROM services;

DROP TABLE services;
ALTER TABLE services_new RENAME TO services;

-- Seed new generic categories (idempotent)
INSERT OR IGNORE INTO services (kind, name_sk, name_en, default_price, active)
VALUES ('generic', 'Občerstvenie',     'Refreshments',        0.0, 1),
       ('generic', 'Doplnky výživy',   'Supplements',         0.0, 1),
       ('generic', 'Aktivácia karty',  'Card activation fee', 0.0, 1);

-- V9_TRANSACTIONS_LEGACY_BACKFILL_MARKER
-- (dropped in a future cleanup migration after ≥2 weeks of stable operation)
ALTER TABLE transactions ADD COLUMN legacy_backfilled INTEGER NOT NULL DEFAULT 0;
```

`default_price = 0.0` because legacy snack/supplement amounts ranged from €0.66 to €278
— there's no useful default. Staff types the actual price each time.

## Backfill algorithm

CLI: `migrate-legacy --backfill --mdb-path zbynek/latest/db/db.mdb --target /var/lib/spinbike/spinbike.db`

```
1. Open target DB. Load services into a Slovak-name → service_id map.
2. Walk MDB `card` table once. Build legacy_card_id → barcode map.
3. Walk MDB `Data` table once. For each row:
     - Skip if action ∈ {Novy kredit, AKTIVACIA, BLOKOVANA}.
     - Skip if service is empty.
     - Look up barcode via legacy_card_id; skip + log if orphaned.
     - Look up service_id via Slovak name; skip + log if unknown.
     - Stash key (barcode, created_at_string, amount_eur) → service_id.
4. For each (key → service_id):
     UPDATE transactions
        SET service_id = ?, legacy_backfilled = 1
      WHERE id IN (
        SELECT t.id FROM transactions t
        JOIN cards c ON c.id = t.card_id
        WHERE c.barcode = ?
          AND t.created_at = ?
          AND ABS(t.amount + ?) < 0.005   -- prod stores negative
          AND t.service_id IS NULL
      )
      RETURNING id;
5. Print summary: matched, unmatched, already-set, ambiguous (>1 prod row), per service.
```

**Match dimensions.** Barcode (not card_id, robust against renumbering); exact
`created_at` string equality (the original importer stored MDB strings verbatim); amount
with epsilon 0.005 € (absorbs SK→EUR rounding); `service_id IS NULL` guard
(idempotency, never overwrites post-import sales).

**Ambiguity.** If one legacy row matches >1 prod row, update them all (same service_id)
and log the ids in the summary `ambiguous` count. The reverse — one prod row matched by
>1 legacy row — is impossible because the legacy `id_data` is unique within `Data`.

## UI changes

**Backend:**

- `routes/admin.rs` — `CreateServiceRequest`, `UpdateServiceRequest`, `ServiceRow`
  swap `name: String` for `name_sk: String, name_en: String, kind: String`. POST accepts
  `kind` (defaults to `'generic'`); PUT ignores `kind` (read-only).
- `routes/payments.rs:79, 279` — `WHERE name = 'Monthly pass'` →
  `WHERE kind = 'monthly_pass'`.
- `core/models.rs::ServiceInfo` — add `name_sk`, `name_en`, `kind`; remove `name`.
  Add `display_name(lang) -> &str` helper.
- `db/transactions.rs`, `db/reports.rs` — SELECTs joining `services` pull
  `s.name_sk, s.name_en` (and `s.kind` where the UI needs to detect the pass).

**Frontend (Leptos):**

- `pages/dashboard/action_form.rs` — `is_monthly_pass()` derives from
  `selected_service.kind == "monthly_pass"`. Dropdown option text uses
  `service.display_name(lang)`. Each `<option>` carries `data-kind={kind}` so test
  helpers can target the pass row by attribute.
- `pages/admin.rs::ServicesTab` — service form gets two name inputs (Slovak / English).
  Read-only "Kind" badge column. Create form has a kind selector; "Monthly pass" option
  disabled if a row already has it (the partial unique index enforces server-side too).
- `pages/dashboard/transactions_list.rs` — service column renders
  `display_name(lang)`.
- `pages/dashboard/pass_banner.rs` — relies on `kind` from the API row instead of name
  matching.
- `pages/reports/*` — same `display_name(lang)` pattern.
- `i18n/*` — add keys: `service_kind_generic`, `service_kind_monthly_pass`.

**Helpers (E2E):** `helpers.ts::selectMonthlyPass` switches from text-matching
"Monthly pass" to selecting the option with `data-kind="monthly_pass"`. More robust
across language toggles.

## Error handling

| Failure | Behavior |
|---|---|
| `--mdb-path` missing or unreadable | Exit non-zero with explicit error. |
| `--target` DB has no `legacy_backfilled` column | Exit non-zero with "run schema migration first." |
| Legacy `id_card` not in MDB `card` table | Skip row, log warning, count as `unmatched`. |
| Legacy service name not in target services | Skip row, log warning, count as `unknown_service`. |
| Prod row matched by zero legacy rows | Acceptable — those are post-import sales. |
| Single legacy row matches >1 prod rows | Update all matched rows to same service_id; report `ambiguous` count. |
| Backfill interrupted | Idempotent — re-run resumes (NULL guard). |
| Server start finds migration partially applied | Wrapped in single transaction; commit-or-rollback. SQLite `IMMEDIATE` transaction blocks other writers. |
| Admin tries to create second `monthly_pass` row | Server rejects via partial unique index; UI hides the option. |

## Testing

**Unit (`migrate_legacy.rs::tests`):**
- `map_legacy_service_known_names` extended for the three new mappings.
- `backfill_idempotent` — seeds NULL + non-NULL rows, runs backfill twice, asserts
  expected updates.
- `backfill_skips_post_import_sales` — `service_id IS NULL` guard correctness.
- `backfill_ambiguous_match_logs_and_updates_all` — two prod rows, one legacy,
  both updated, ambiguous count = 1.

**Integration (`crates/spinbike-server/tests/`):**
- `admin_routes.rs` — POST/PUT/GET `/api/admin/services` with `name_sk`, `name_en`,
  `kind`. Unique-monthly_pass rejection. PUT cannot change `kind`.
- `payments.rs` — sell-pass after `kind` lookup swap; rename-and-still-sell regression.
- `reports.rs` — service-grouped reports return both names.

**Playwright E2E (`e2e/tests/`):**
- `services-admin.spec.ts` — create/edit/deactivate dual-language service.
- `card-action-form-language.spec.ts` — same row, two languages, both rendered.
- `card-action-form.spec.ts` — `selectMonthlyPass` updated to use `data-kind`.
- `legacy-history.spec.ts` — seed-fixture card with one transaction per backfilled
  service kind; assert all three appear in history.

All Playwright tests follow `setupConsoleCheck` + `assertCleanConsole` last-assertion
pattern.

**Mutation testing:** `cargo mutants --in-diff` covers `map_legacy_service_name`
branches and the backfill UPDATE predicate. Tests above hit each branch.

## Migration / rollout

1. PR merges to `main`. Schema migration runs on next server start. New services appear
   in admin. Existing data unchanged.
2. Post-deploy verification on prod: admin opens `/admin?tab=services`, sees rows in
   dual-language with the Monthly pass badged. Sell a Monthly pass on a test card
   (banner appears). Sell a refreshment on a test card (transaction shows
   `Občerstvenie` in history).
3. Run backfill on a copy of prod DB locally first. Compare
   `SELECT COUNT(*) FROM transactions WHERE service_id IS NULL` before/after.
4. Once verified, run backfill against prod. (Safe to run while live — only updates
   NULL rows.) Restart not required.
5. Verify on a card with known legacy snack purchases (sample 2008 data).
6. After ≥2 weeks of stable operation, follow-up migration drops
   `transactions.legacy_backfilled` column.

**Backout:** `UPDATE transactions SET service_id = NULL WHERE legacy_backfilled = 1`
reverses the backfill targeted (no risk to post-import sales). Schema migration
backout = restore from daily DB backup.

## Risks (summary)

| Risk | Mitigation |
|---|---|
| Backfill mislabels a row | Triple-key match + NULL guard; dry-run on prod copy first; ambiguous counter. |
| Schema migration loses service ids | Single transaction; `INSERT INTO services_new SELECT id, ...` preserves ids; no `transactions` rows touched. |
| Pass-banner / sell-pass regression | Integration + E2E tests cover the `kind` swap. |
| Admin creates duplicate monthly_pass | Partial unique index + UI option-disabled. |

## Open Questions

None at design time.
