# Monthly Pass (Time-Based Access) Design

## Summary

Add monthly pass ("Casova karta") as a first-class concept: staff can sell a time-based pass by deducting credit and picking a `valid_until` date. While the pass is active, per-class charges are waived and the dashboard shows a prominent "pass valid until" banner. Also fix the legacy importer to preserve the service name and `valid_until` date on imported transactions.

## Motivation

The legacy VB6 system supported "Casova karta" — a monthly pass where a customer pays a fixed amount and gains unlimited access until a staff-picked expiry date. The rewrite currently drops this capability:

1. The `services` table only has Spinning and Fitness — no Monthly Pass service.
2. The `transactions` table has no `valid_until` column, so there is no way to record when a pass expires.
3. The legacy importer (`migrate_legacy.rs` line 263) reads only `card_id`, `amount`, `action`, `date` from the legacy `Data` table — dropping both the `service` column (index 4) and the `EndDate` column (index 7).

Imported cards therefore show an incomplete history: legacy "Casova karta" rows appear as generic charges with no service label and no expiry date, and active pass holders who migrate from the old system can't see their pass status.

## Legacy behavior (reference)

- Staff opens the payment form, scans a card, selects "Casova karta" service.
- A `MonthView` calendar appears; staff picks the expiry date (typically today + 30 days).
- Staff enters the price (historically 600 SK / 19.92 EUR, today 35 EUR).
- System writes a `Data` row: `service="Casova karta"`, `action="Debet"`, `EndDate=<picked>`, `suma=<price>`.
- Credit is deducted from the card.
- On every subsequent scan, the UI reads the card's latest `EndDate` from `Data` and displays "valid until DATE" / "X days remaining" / "expired X days ago".

## User-facing behavior (new system)

### Three dashboard states

**No active pass.** Default state. `Sell service` section shows Spinning, Fitness, and a new `Monthly pass 35 EUR` button. History, credit, and existing actions unchanged.

**Active pass.** Green banner at top of action panel: `✓ Monthly pass valid until DD.MM.YYYY` + `N days remaining · unlimited access`. The per-class charge buttons switch to `Log visit (no charge)` buttons that record a 0 EUR transaction in history. Top-up is always available. Selling another pass is still possible (extends the end date).

**Expired pass.** Red banner at top: `Monthly pass expired N days ago` + `Last valid until DD.MM.YYYY · sell new pass?`. Charge buttons return to paid mode. The `Sell monthly pass` button is highlighted.

### Selling a pass

1. Staff clicks `Monthly pass 35 EUR` button.
2. Modal opens with: price field (default 35 EUR, editable), date picker (default = max(current valid_until, today) + 30 days), confirm button.
3. Staff confirms → server deducts price from credit, writes a transaction with `service_id = monthly pass`, `action = "charge"`, `valid_until = <picked>`, `amount = <price>`.
4. Dashboard refreshes with active-pass banner.

### Logging a pass-covered visit

1. Staff clicks `Log Spinning visit` or `Log Fitness visit`.
2. Server writes a transaction with `service_id = spinning|fitness`, `action = "visit"`, `amount = 0`, `valid_until = NULL`.
3. History row displays as `Spinning (pass) · visit · 0.00` in blue.

### Transaction history

Existing columns (date, service, action, amount) unchanged, with two additions:

- Monthly-pass purchase rows show service as `Monthly pass` and action as `charge · until DD.MM` (appending the end date to the action cell — no new column).
- `visit` rows (amount 0) are styled in blue to distinguish from paid charges.

## Data model

### New migration: V4

```sql
-- Add valid_until to transactions (nullable; only monthly-pass purchases set it)
ALTER TABLE transactions ADD COLUMN valid_until TEXT;

-- Seed the Monthly Pass service (idempotent)
INSERT OR IGNORE INTO services (name, default_price, active)
VALUES ('Monthly pass', 35.0, 1);
```

`valid_until` uses SQLite `TEXT` with ISO-8601 `YYYY-MM-DD` format (date-only, no time component — passes expire at end of day).

### Card pass status (computed, not stored)

No `valid_until` column on `cards`. Pass status is derived at read time:

```sql
SELECT MAX(valid_until)
FROM transactions
WHERE card_id = ? AND valid_until IS NOT NULL
```

Compare result against today's date:

- `NULL` → no pass ever purchased → state A
- `>= today` → active → state B
- `< today` → expired → state C

This matches the legacy approach (compute from transaction history rather than duplicate onto the card row), avoids data drift, and correctly handles the "extend from current end" semantic — the latest `valid_until` wins automatically.

### Card info API response

`GET /api/cards/:barcode` and `GET /api/cards/search` responses gain a `pass` field:

```rust
struct CardPass {
    valid_until: Option<NaiveDate>,  // None if never purchased
    days_remaining: Option<i32>,     // negative if expired
}
```

`days_remaining` is `(valid_until - today).days` — positive if active, zero on last day, negative once expired. Clients use `days_remaining >= 0` to decide active-vs-expired.

## API changes

### New: `POST /api/payments/sell-pass`

```rust
struct SellPassRequest {
    card_id: i64,
    price: f64,       // must be >= 0
    valid_until: NaiveDate,  // must be > today
}

struct SellPassResponse {
    transaction_id: i64,
    new_credit: f64,
    valid_until: NaiveDate,
    days_remaining: i32,
}
```

Server finds the "Monthly pass" service id by name (cached after first lookup), debits credit, writes transaction with `service_id`, `action="charge"`, `amount=price`, `valid_until=<date>`.

Validation errors: `price < 0` → 400; `valid_until <= today` → 400; card not found → 404; card blocked → 403.

### New: `POST /api/payments/log-visit`

```rust
struct LogVisitRequest {
    card_id: i64,
    service_id: i64,  // spinning or fitness (not monthly pass)
}

struct LogVisitResponse {
    transaction_id: i64,
}
```

Server validates the card has an active pass (`days_remaining >= 0`) — if not, returns 409 Conflict with message "card has no active pass; use /charge instead". Writes transaction with `amount=0`, `action="visit"`, `valid_until=NULL`.

### Modified: card info endpoints

`/api/cards/:barcode`, `/api/cards/search`, `/api/cards/:id` all gain the `pass` field in their response. Implementation: single subquery or join returning `MAX(valid_until)`, computed in the same transaction as the card fetch.

### Unchanged: `POST /api/payments/charge`

Existing charge flow unchanged — still usable for pass holders who want to pay for a one-off service, but dashboard UI will prefer the visit endpoint when pass is active.

## Legacy importer fix

Modify `migrate_legacy.rs` (around line 239):

1. Read `service` column (index 4) and `EndDate` column (index 7) from the `Data` CSV.
2. Map legacy service names to service IDs:
   - `"Casova karta"` → Monthly pass service id
   - `"Fitnes"` → Fitness service id
   - `"Spinbike"` → Spinning service id
   - anything else → `NULL`
3. Parse `EndDate` (legacy format `MM/DD/YY HH:MM:SS`, e.g., `"12/05/08 00:00:00"`) to ISO `YYYY-MM-DD`. Blank/empty strings → `NULL`.
4. Insert with service_id and valid_until populated:

```sql
INSERT INTO transactions (card_id, amount, action, created_at, service_id, valid_until)
VALUES (?, ?, ?, ?, ?, ?)
```

Imported legacy pass purchases from 2008 will have `valid_until` dates in the past — the dashboard correctly shows them as expired in history but does not affect the "current pass" derivation (which takes MAX — latest wins). A customer migrating over with a recent (still-active) legacy pass will see the banner immediately.

## Testing

### Unit tests (`cards.rs`, `transactions.rs`, `payments.rs`)

- `sell_pass_debits_credit_and_sets_valid_until` — sell pass, assert credit dropped by price and transaction has `valid_until`.
- `sell_pass_extends_from_current_end_date` — card has active pass until 05-15, sell another until 06-15, assert history has both rows with distinct valid_until.
- `sell_pass_rejects_past_date` — 400 on `valid_until <= today`.
- `sell_pass_rejects_negative_price` — 400 on `price < 0`.
- `sell_pass_rejects_blocked_card` — 403.
- `card_info_derives_pass_status` — no transactions → `pass.valid_until=None`; one past transaction → negative days_remaining; one future → positive.
- `log_visit_rejects_card_without_pass` — 409 if card has no active pass.
- `log_visit_writes_zero_amount` — amount is 0, service_id points to spinning/fitness, action is "visit".
- `card_info_with_multiple_passes_returns_max_date` — card has two active passes (overlap), returned valid_until is the later one.

### E2E tests (Playwright)

- `monthly-pass-sell-and-banner.spec.ts`:
  1. Login as staff, navigate to card dashboard.
  2. Search for a card with credit, open it.
  3. Click `Monthly pass 35 EUR`, pick date (today + 30), confirm.
  4. Assert banner appears: `Monthly pass valid until <DD.MM.YYYY>`.
  5. Assert credit decreased by 35.
  6. Assert history has new row with `Monthly pass` service and `charge · until` in action.
  7. Assert per-class buttons now say `Log visit`.
  8. Click `Log Spinning visit`, assert 0 EUR row appears in history.
  9. Assert zero browser console errors.
- `monthly-pass-expired-state.spec.ts`:
  1. Seed DB with a transaction `valid_until` in the past.
  2. Login, open card.
  3. Assert red banner: `Monthly pass expired N days ago`.
  4. Assert `Monthly pass 35 EUR` button visible and prominent.
  5. Per-class buttons show paid amounts (not "Log visit").

### Legacy importer tests

- `migrate_preserves_service_id` — import fixture with a `Casova karta` row, assert transaction has the monthly pass `service_id`.
- `migrate_parses_end_date` — fixture with `EndDate="12/05/08 00:00:00"`, assert stored as `2008-12-05`.
- `migrate_empty_end_date_becomes_null` — fixture with empty EndDate, assert NULL in DB.

### Mutation testing

Existing cargo-mutants CI will cover new code automatically. Any surviving mutants in the new logic (e.g., `days_remaining >= 0` mutated to `>`) must be killed with sharper test assertions.

## Out of scope

- **Pass pause / freeze** — no ability to pause a pass mid-period. If needed later, can be added as a `pass_pauses` table.
- **Multiple pass tiers** — single "Monthly pass" product only. Quarterly/annual or varied-price packages are explicitly not in this design.
- **Refunds / storno for passes** — existing `/api/payments/storno` works if called on the pass transaction, but the UI does not yet surface a "cancel pass" button. Out of scope.
- **Automatic renewal / recurring billing** — customer must physically come in to renew.
- **Email/SMS expiry reminders** — no messaging infrastructure.
- **Admin price configuration UI** — price default lives in the `services` table and is changeable via SQL; no UI edit today.

## Open questions resolved during brainstorming

- Scope: new feature + fix migration (both).
- Payment: deduct from card credit (customer must top up first).
- Date: staff picks via calendar (legacy parity).
- Price: 35 EUR default, staff can override per sale.
- Stacking: new purchase extends from current end date (MAX semantics via latest `valid_until` winning).
- Usage: active pass = unlimited access, per-class charges waived.
- Visit logging: yes, log as 0 EUR "visit" row so attendance is visible in history.
- Banner: big green/red banner at top of action panel.
