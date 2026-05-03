# Legacy Transaction Normalization

## Problem

Per-card transaction history shows almost every legacy row as **Top-up** — including
gym-visit debits and class-attendance entries. The CEO can't read history at a glance:
hundreds of green "Top-up" pills hide the actual debit/visit pattern.

The credit balance on each card is correct, so this is purely a presentation /
data-shape issue, not a money issue.

## Root cause

Two conventions coexist in `transactions`:

| Convention | Stored as | Source |
|---|---|---|
| **Legacy** (positive-magnitude + signed-by-action) | `action='debit'` / `'credit'` / `'activation'` / `'storno'`, **amount always ≥ 0** | `migrate-legacy` import from MS Access |
| **New** (signed-amount + neutral-action) | `action='charge'` / `'topup'` / `'visit'`, **amount may be negative** | All live writes since the rewrite |

`crates/spinbike-core/src/reports.rs::classify` (the function that drives the UI's
event-kind icon, color, and label) only knows the new vocabulary:

```rust
if valid_until.is_some()  → PassSale
else if action == "visit" → Visit
else if amount < 0.0      → Charge
else if amount > 0.0      → TopUp        // ← every legacy row hits this
else                       → Other
```

A 2014 `debit|3.0` (gym visit) carries positive amount and a non-`visit` action, so
it bottoms out at TopUp. Same for legacy `credit`, `activation`, and most `storno`
rows.

Two consequences:

1. **Per-card transaction list** mislabels the entire pre-2026 history.
2. **Reports activity feed and KPIs silently exclude legacy rows** — `db/reports.rs`
   filters charges as `action='charge' AND amount<0`, so legacy gym visits don't
   show up at all on /reports and don't count toward attendance / revenue KPIs.

## Goal

One vocabulary across all rows. After the fix, `classify()` and every SQL filter
that already targets the new convention work uniformly over the entire dataset.

## Approach

**One-time idempotent SQL backfill** that mutates legacy rows into the new
convention, run via the existing `db::migrations` framework on server start.

Plus a **defensive update** to `migrate_legacy.rs::map_action` so future re-imports
write the new convention directly (the backfill remains as the safety net for any
DB that still has legacy-shape rows).

No changes to `classify()`. No changes to live SQL queries — they already target
the new vocabulary; backfilled rows match automatically.

## Mutation table (verified against prod 2026-05-03)

| Source pattern | Row count | Becomes |
|---|---|---|
| `action='debit'` AND `amount > 0` | 77,989 (positive-amount debits, includes 4,512 with `valid_until`) | `action='charge'`, `amount = -amount` |
| `action='debit'` AND `amount = 0` AND `valid_until IS NULL` | 14,804 (4 without service) | `action='visit'`, amount unchanged (= 0) |
| `action='debit'` AND `amount = 0` AND `valid_until IS NOT NULL` | 876 | `action='charge'`, amount unchanged (= 0) |
| `action='credit'` AND `amount >= 0` | 9,395 | `action='topup'`, amount unchanged |
| `action='credit'` AND `amount < 0` | 1 (manual correction, 2010) | `action='charge'`, amount unchanged (already negative) |
| `action='activation'` | 568 | `action='topup'`, amount unchanged |
| `action='storno'` AND `amount > 0` | 64 | `action='topup'`, amount unchanged (refund = top-up shape) |
| `action='storno'` AND `amount = 0` | 8 | unchanged (already classifies as `Other`) |
| Any other action (`charge`, `topup`, `visit`) | already new convention | unchanged (not matched by any guard) |

`valid_until` is preserved verbatim on every row → PassSale classification
(driven by `valid_until.is_some()`) is unaffected.

The order of UPDATE statements matters (a later guard would otherwise re-match a
row a prior step already converted). The migration uses **AND guards on the
source action label** so each statement targets only legacy-shaped rows. After
the first successful run the source guards match zero rows; subsequent runs are
no-ops.

## Architecture

### 1. New migration in `db/migrations.rs`

Append a new versioned migration step (next sequential version after the most
recent one). Uses a single transaction so partial failure leaves the DB
unchanged. Pseudocode:

```sql
BEGIN;
UPDATE transactions SET action='charge', amount = -amount
  WHERE action='debit' AND amount > 0;

UPDATE transactions SET action='visit'
  WHERE action='debit' AND amount = 0 AND valid_until IS NULL;

UPDATE transactions SET action='charge'
  WHERE action='debit' AND amount = 0 AND valid_until IS NOT NULL;

UPDATE transactions SET action='charge'
  WHERE action='credit' AND amount < 0;

UPDATE transactions SET action='topup'
  WHERE action='credit';                    -- catches all remaining credits

UPDATE transactions SET action='topup'
  WHERE action='activation';

UPDATE transactions SET action='topup'
  WHERE action='storno' AND amount > 0;
COMMIT;
```

Idempotent: after the first run no rows match `'debit'` / `'credit'` /
`'activation'`, and only zero-amount `storno` rows remain (skipped by the guard).

### 2. Update `crates/spinbike-server/src/bin/migrate_legacy.rs::map_action`

Change the mapping so future re-imports write the new convention directly. The
caller signature changes from `Option<&'static str>` to a tuple that also
encodes amount transformation, so the `Debet` / `Vstup` cases can request a
sign flip:

```rust
fn map_legacy(action: &str, amount: f64, has_valid_until: bool) -> Option<MappedAction> {
    match action.trim().trim_matches('"') {
        // Zero-fee class attendance: pure visit
        "Debet" | "Vstup" if amount == 0.0 && !has_valid_until => Some(MappedAction::visit()),
        // Real debit (paid visit, single-class purchase, pass purchase)
        "Debet" | "Vstup"                                       => Some(MappedAction::charge_neg()),
        "Kredit" | "Novy kredit" | "AKTIVACIA"                  => Some(MappedAction::topup()),
        "Storno" if amount > 0.0                                => Some(MappedAction::topup()),
        "Storno"                                                => Some(MappedAction::storno()),
        "BLOKOVANA"                                             => None,
        other => { warn!("Unknown legacy action: '{other}'"); Some(MappedAction::topup()) }
    }
}
```

(`MappedAction` is a small private struct: `{ action: &'static str, sign: f64 }`
to keep the call site tidy.)

### 3. No classifier changes

`reports.rs::classify` is unchanged. After backfill every row carries an action
the classifier already understands.

### 4. No live SQL changes

`db/reports.rs:72,202` (`action='charge' AND amount<0` clauses) already target
the new convention. After backfill the legacy rows match these clauses
automatically — the activity feed and KPI counts start including pre-2026
history correctly. This is a behavioral change worth highlighting:

- **Before:** /reports for a 2018 day shows zero events (legacy rows hidden).
- **After:** /reports for a 2018 day shows actual gym visits and top-ups.

The user has confirmed this is the desired behavior.

## Tests

### Unit — `db::migrations`

Seed one row of every pattern in the mutation table, run `run_migrations`, then
assert each row's resulting `(action, amount)`. Also assert the migration is
idempotent: run it twice, second run produces zero changes.

### Unit — `migrate_legacy::map_legacy`

Plain table-driven test for every Slovak input string and amount combination.
Includes the zero-amount `Vstup` → `visit` case and the `Storno` split.

### Playwright — `e2e/tests/legacy-history-classified.spec.ts`

Seed a card via test fixtures with rows that exercise the post-backfill
vocabulary: a `topup`, a `charge`, a `visit`, and a `charge` with `valid_until`
(pass sale). Open `/staff?card=<bc>`, scroll the per-card transaction list, and
assert the rendered pills include at least one Charge and at least one TopUp
(not all the same kind). Zero console errors.

A second test loads `/reports?date=2018-06-13` (a legacy date with known activity
on card 70701050) and asserts the activity feed renders at least one row —
proving legacy rows now appear on Reports.

## Verification plan

1. **Pre-deploy backup of prod DB.**
   `cp /opt/spinbike/prod/spinbike.db /opt/spinbike/prod/spinbike.db.bak-pre-normalize`
   (the plan includes this as Task 1 so it runs before the merge.)

2. **Dev validation first.** Per `feedback_validate_against_real_data.md`, the
   migration runs against the prod-synced dev DB during the standard
   deploy-dev cycle. Manual eyeball check of:
   - `https://spinbike-dev.newlevel.media/staff?card=70701712`
   - `https://spinbike-dev.newlevel.media/staff?card=70701050`
   Each card's history should show a mix of Charge (red dot, gym visits) and
   TopUp (green dot, real top-ups), not ~138 green pills.

3. **Balance invariant check.** A SQL spot-check before vs after the migration:
   `SELECT id, barcode, credit FROM cards;` — every `credit` value must be
   identical before and after (the migration must not touch the `cards` table).

4. **Prod deploy + same Playwright check** on `https://spinbike.newlevel.media`.

## Risks

- **One-way mutation.** Reversible only via the pre-deploy DB backup. Risk
  mitigated by (a) dev-DB rehearsal, (b) explicit backup step in the plan,
  (c) the migration runs in a single transaction so partial failure rolls back.

- **Storno semantics flattened.** 64 historical refund rows become
  indistinguishable from regular top-ups. Acceptable per CEO decision (the
  void semantic was never surfaced in the UI anyway).

- **KPI shift on historical days.** Reports dashboards for any day before
  2026-04 will show non-zero revenue / attendance starting at deploy time.
  This is the desired behavior, not a regression.

## Out of scope

- The `voided` flag on `ReportEvent` (separate code path, not affected by
  action vocabulary).
- Renaming `storno` to a clearer name. 8 historical zero-amount storno rows
  remain; classifier maps them to `Other`. Not worth a column-rename round
  trip.
- Backfilling `service_id` on legacy rows. That is a separate concern handled
  by the existing `db::backfill` module.
