# Card History Clarity + Per-Transaction Notes — Design

**Issue:** #26 — *history of moves on card is unclear and not same on report and on card, note*

**Goal:** Make the card-history view and the report-activity feed use ONE consistent set of CEO/staff-friendly Slovak labels for every transaction movement, and let staff record a free-text note on every movement that's visible (and editable) on both surfaces.

**Version:** 0.13.9 (bumped on commit `0d5abd7`).

**Branch / PR:** new PR `dev` → `main`. Same-day cycle.

---

## Problem

Two distinct user-facing problems in one issue body:

### Problem 1 — Wording mismatch + accountant-y vocabulary

The same transaction row says different things on different screens:

| Movement | Card history label (today) | Report activity-feed label (today) |
|---|---|---|
| Top-up (`action=topup`, amount > 0) | `Dobitie` / Top-up | `Vklad` / Top-up |
| Charge (`action=charge`, amount < 0, no `valid_until`) | `Platba` / Charge | `Návšteva` / Visit |
| Visit (`action=visit`, amount = 0) | `Navsteva` / Visit | `Iné` / Other |
| Pass sold (`action=charge`, amount < 0, `valid_until` set) | `Platba` / Charge + " · do `<date>`" | `Permanentka` / Pass sale |

The CEO/staff complaint: vocabulary is for accountants, and the same movement reads differently depending on which screen you're on. The Visit row in particular lands in `Other` on the report because the report's `EventKind` classifier ignores `action='visit'`.

### Problem 2 — No place for staff notes

Today there's no way to record "what was actually bought" beyond the service name. Staff need to capture refreshment items, deals, agreed discounts — concrete free-form context. The CEO's examples: *"kúpil protein"*, *"deal medzi návštevníkom a CEO"*. Without a note, every special-case ends up as an opaque `Výdaj z kreditu -2.50 €` row.

---

## Design

### Single PR, four parts

1. Unified i18n labels (used identically on card and report).
2. Server-side `EventKind` classifier fix (visits stop landing in Other).
3. Schema: incremental migration adding `note TEXT` to `transactions`.
4. UI: note input on every create flow, inline display + inline edit on card history, read-only display on report.

### 1 — Unified labels

Four new i18n keys replace six existing ones. Both card history and report use the same key per movement.

| New i18n key | Slovak | English |
|---|---|---|
| `tx_label_topup` | `Dobitie kreditu` | `Top-up` |
| `tx_label_charge` | `Výdaj z kreditu` | `Spent from credit` |
| `tx_label_visit` | `Vstup s permanentkou` | `Entry with pass` |
| `tx_label_pass` | `Predaj permanentky` | `Sale of pass` |

Existing keys to remove: `tx_action_topup`, `tx_action_charge`, `tx_action_visit`, `event_charge`, `event_topup`, `event_pass`. The `event_other` key stays — it labels the residual `Other` bucket. The filter-dropdown keys `filters_event_topups` / `filters_event_passes` are independent and stay.

The card history's existing " · do `<date>`" suffix stays on Pass-sold rows.

### 2 — `EventKind` classifier fix

`crates/spinbike-core/src/reports.rs::EventKind::kind()` is rewritten to consider `action`. New variant `Visit` is added.

**New precedence (top-down, first-match-wins):**

1. `valid_until.is_some()` → `PassSale` *(rename from `PassSold` for symmetry with the new label)*
2. `action == "visit"` → `Visit` *(new variant; covers €0 pass attendance)*
3. `amount < 0.0` → `Charge`
4. `amount > 0.0` → `TopUp`
5. else → `Other`

UI mapping (single source of truth in `activity_feed.rs::render_row`):

| `EventKind` | i18n key |
|---|---|
| `PassSale` | `tx_label_pass` |
| `Visit` | `tx_label_visit` |
| `Charge` | `tx_label_charge` |
| `TopUp` | `tx_label_topup` |
| `Other` | `event_other` |

The `feed-dot` CSS class set gains `feed-dot--visit` (or reuses `feed-dot--charge` — implementation detail, picked in plan).

The `ReportEvent::action: String` field is the input; the existing struct already carries it, no schema change needed.

### 3 — Note column

**Migration v8 (incremental, append-only — production rule):**

```sql
ALTER TABLE transactions ADD COLUMN note TEXT;
```

NULL means "no note". Empty string is treated identically to NULL on display.

**Plumbing through Rust types:**

- `db::transactions::TransactionRow` — add `pub note: Option<String>`.
- `routes::cards::TransactionResponse` — add `pub note: Option<String>`.
- `core::reports::ReportEvent` — add `pub note: Option<String>`.
- UI `pages::dashboard::TxnInfo` — add `pub note: Option<String>`.

Server-side `SELECT` lists in `db/transactions.rs` and `db/reports.rs` add the `note` column.

**Create payload changes (each accepts an optional note):**

- `POST /api/payments/charge` → body gets `note: Option<String>`.
- `POST /api/cards/topup` (existing top-up endpoint, lives under `cards`, not `payments`) → body gets `note: Option<String>`.
- `POST /api/payments/sell-pass` → body gets `note: Option<String>`.
- `POST /api/payments/log-visit` (existing visit-log endpoint) → body gets `note: Option<String>`.

`db::transactions::create_transaction` and `create_transaction_with_valid_until` both gain a `note: Option<&str>` parameter and `INSERT … note` accordingly.

**New endpoint for editing existing notes:**

```
PATCH /api/transactions/{id}/note
Authorization: staff
Body: { "note": "string or null, ≤200 chars" }
Response: 200 { id, note } | 400 (>200 chars) | 404 (not found) | 409 (voided) | 403 (non-staff)
```

Voided transactions cannot be edited (`409 Conflict` — symmetric with the existing void rules in `routes/transactions.rs`). Staff role required (`can_manage_cards`, like the existing void/valid-until endpoints).

**Length cap:** 200 chars. Enforced server-side (rejects >200 with 400) AND client-side (`maxlength="200"` on the input).

### 4 — UI

**Action form (`spinbike-ui/src/pages/dashboard/action_form.rs`):**

A small text input is added to every create flow:

```html
<input type="text"
       maxlength="200"
       data-testid="txn-note-input"
       placeholder="Poznámka (nepovinné)"
       class="input input--compact" />
```

(Slovak placeholder shown; English placeholder via `tx_note_placeholder` i18n key.)

Pre-fill is empty. On submit, the note value (or `None` if empty/whitespace) is included in the create payload. Shared placement across the three primary flows (charge / topup / sell-pass) keeps the muscle memory consistent. Visit-log gets the same input on the visit-log path.

**Card history (`spinbike-ui/src/pages/dashboard/transactions_list.rs`):**

When `tx.note` is `Some(non_empty)`, render a third line under the existing `date · service` subtitle:

```
Výdaj z kreditu                 -2.50 €
2026-04-29 14:30 · Občerstvenie
Proteinová tyčinka Trec
                                  ✎  ✕
```

The note line is in `.list-row__note` (new CSS class). The pencil icon (`✎`, button `data-testid="txn-note-edit"`) sits in `.list-row__end` next to the existing void button. Tap → the note line is replaced by an inline `<input maxlength="200">` with `Save` (`✓`) and `Cancel` (`✕`) buttons. Save dispatches `PATCH /api/transactions/{id}/note` and on success bumps the transactions refresh signal. Empty input + Save → sends `note: null`, server clears the column.

When the note is empty, the pencil icon is still shown (lets staff add a note later). The note row simply isn't rendered.

Voided transactions hide the pencil entirely (matches the existing void-button hide).

**Report activity feed (`spinbike-ui/src/pages/reports/activity_feed.rs`):**

Same inline-below-subtitle pattern but **read-only** (no pencil). Rationale: editing happens on the card history, where staff already manage the record. The report is a quick overview.

When `e.note` is `Some(non_empty)`, the row's subtitle becomes a 2-line block:

```
14:30   Anna Nováková
        Výdaj z kreditu · Občerstvenie
        Proteinová tyčinka Trec                  -2.50 €
```

---

## Testing

### Unit (Rust)

`crates/spinbike-core/src/reports.rs::tests`:

- `kind_passsale_when_valid_until_set_regardless_of_action_or_amount` — preserves existing precedence.
- `kind_visit_when_action_is_visit_and_no_valid_until` (NEW) — `action='visit'` AND `valid_until=None` AND `amount=0.0` → `Visit`. Critical case: today this lands in `Other`.
- `kind_visit_overrides_charge_when_amount_negative_and_action_visit` (NEW, defensive) — guards against future code that lets a visit have a negative amount.
- `kind_charge_when_amount_negative_and_action_charge_no_pass` — preserves `Charge` for non-visit non-pass charges.
- `kind_topup_when_amount_positive_and_no_pass` — preserves.
- `kind_other_when_amount_zero_no_pass_and_action_neither_visit_nor_charge` — guards `Other` as the residual bucket.

Each test must use a discriminating fixture (existing `ev()` helper extended with an `action` arg).

`crates/spinbike-server/src/db/migrations.rs::tests`:

- `v8_adds_note_to_transactions` — applies migrations through v8, asserts `PRAGMA table_info(transactions)` contains `note TEXT`.

### Integration (sqlx)

`crates/spinbike-server/tests/transactions_note.rs` (NEW):

- `create_charge_persists_note` — POST charge with note, verify DB row.
- `patch_note_updates_existing_row` — POST charge, then PATCH, verify DB.
- `patch_note_rejects_voided` — void txn, then PATCH → 409.
- `patch_note_rejects_over_200_chars` — 201-char body → 400.
- `patch_note_clears_on_null` — PATCH `{"note": null}` → DB note becomes NULL.
- `patch_note_requires_staff` — non-staff token → 403.

### E2E (Playwright)

`e2e/tests/txn-note.spec.ts` (NEW):

- **Create with note:** seed a card, charge €2.50 with `note="Proteinová tyčinka"`, navigate to card history, assert note line visible.
- **Note appears on report:** same charge, navigate to `/reports?date=<today>`, assert note line visible on the matching feed row.
- **Edit note inline:** click pencil, change note, save → reload card history, assert new note text.
- **Clear note inline:** click pencil, clear input, save → reload, assert note line is gone (subtitle still has date·service).
- **Empty note doesn't render note line:** charge without note, assert no `.list-row__note` element on that row.
- **Voided transaction hides pencil:** void a charge, assert no `.txn-note-edit` button, but the existing note text (if any) still shows.

Each test asserts zero browser console errors at the end (project-wide rule).

### Mutation testing

The new core tests must each cover ONE precedence rule with a discriminating amount/action/valid_until combo so that `cargo mutants --in-diff` catches operator swaps and condition flips on `EventKind::kind()`.

---

## Out of scope (not in this PR)

- Audit history of note edits (who changed what, when) — single-staff app today, no need.
- Markdown / rich text in notes — plain text only.
- Search-by-note in the reports activity feed — possible follow-up if CEO asks for it.
- Per-service templates ("press F2 for protein-bar template") — premature.
- Showing notes on the customer's `/my-balance` page — staff-only feature; customers see their existing transaction list which is already public-friendly.

---

## Acceptance criteria

1. Both card history and report activity feed show the SAME label for the same transaction row, drawn from the four new i18n keys (`tx_label_topup`, `tx_label_charge`, `tx_label_visit`, `tx_label_pass`).
2. A €0 visit transaction reads as `Vstup s permanentkou` on both surfaces (today: `Navsteva` on card, `Iné` on report).
3. Staff can type a note when creating any of the four transaction types, the note persists, and the card-history row shows it inline below the subtitle.
4. Staff can edit a note inline on a non-voided card-history row via a pencil icon → text input → save (PATCH).
5. Notes >200 characters are rejected with HTTP 400, both server-side and client-side (`maxlength` attribute).
6. Notes on voided transactions cannot be edited (409 Conflict from server; pencil icon hidden client-side).
7. Migration v8 runs cleanly on the production-synced dev database AND on a fresh CI test database. Existing rows get `note=NULL` (no backfill).
8. CI green: lint + test + integrity + e2e + mutation + deploy.
