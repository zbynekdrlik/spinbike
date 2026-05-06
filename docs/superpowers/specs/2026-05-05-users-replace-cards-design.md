# Users Replace Cards — Design Spec

**Issue:** [#55](https://github.com/zbynekdrlik/spinbike/issues/55) — "rework cards to email registratered users"

**Date:** 2026-05-05

## Goal

Eliminate the legacy `cards` table as a primary customer entity. Customers are `users`. The "Activate New Card" desk flow is replaced with "Add Person". All credit, blocking, transactions, last-visit, negative-balance, and pass logic move from `cards` to `users`. Legacy chip codes survive as a vestigial reference column on `users`. Single PR, full migration, no parallel old/new paths.

## Why

The legacy VB6+MS Access system identified customers by physical RFID/NFC chip ("card barcode"). In reality, plastic chips were never issued to clients — the chip codes were a fiction internal to the legacy DB. CEO Štefan inherited that fiction and currently has to "activate a card" to onboard a new person, which is wrong. Now that legacy data has been imported into SpinBike, the architecture can be fixed: `users` becomes the canonical customer entity; `cards` is dropped.

## Constraints

- **One PR, one prod deploy.** No multi-phase migration, no dual-path "new vs legacy" coexistence.
- **Dev DB synced from prod on every dev deploy** (per `feedback_dev_ci_sync_prod_db.md`). Migration must succeed against real prod-shaped data on dev before main merge.
- **Email is OPTIONAL** at desk creation. CEO won't always have it; collection happens later via separate invitation flow (out of scope).
- **No password setup** at desk. New users get `password_hash=NULL` and cannot log in until a future feature wires up email+password / magic-link self-service (out of scope).
- **Customer self-register** endpoint at `POST /api/auth/register` continues to require email + password ≥ 8 chars (unchanged).
- **No destructive migration on prod data**: every existing card row must end up represented as a users row with credit, history, and chip code preserved.

## Schema after migration

```sql
CREATE TABLE users (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    email           TEXT    UNIQUE,                                 -- nullable (was NOT NULL)
    name            TEXT    NOT NULL DEFAULT '(no name)',           -- placeholder for nameless legacy
    password_hash   TEXT,
    phone           TEXT,
    company         TEXT,                                           -- NEW
    role            TEXT    NOT NULL DEFAULT 'customer',
    oauth_provider  TEXT,
    oauth_id        TEXT,
    credit          REAL    NOT NULL DEFAULT 0.0,                   -- NEW
    card_code       TEXT,                                           -- NEW (legacy chip reference)
    blocked         INTEGER NOT NULL DEFAULT 0,                     -- NEW
    allow_debit     INTEGER NOT NULL DEFAULT 0,                     -- NEW
    created_at      TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX idx_users_card_code ON users(card_code)
       WHERE card_code IS NOT NULL;

-- transactions: card_id column removed, user_id NOT NULL
-- bookings:     card_id column removed (user_id already exists)
-- cards:        DROPPED entirely
```

Email is now nullable but still UNIQUE — SQLite allows multiple NULLs in a UNIQUE column by default, so legacy users without email collide on nothing.

## Migration sequence (single sqlx migration step, idempotent, transactional)

The migration runs on server startup via the existing `apply_migrations()` runner. PRAGMA foreign_keys=OFF inside the migration; ON afterwards. Whole sequence wrapped in BEGIN…COMMIT.

Order matters because email becomes nullable (SQLite needs CREATE+COPY+DROP+RENAME) and step 3 inserts users with email=NULL — therefore recreate must precede the inserts.

```sql
-- 1. Recreate users with new schema (email nullable + new columns + default name)
CREATE TABLE users_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    email           TEXT    UNIQUE,
    name            TEXT    NOT NULL DEFAULT '(no name)',
    password_hash   TEXT,
    phone           TEXT,
    company         TEXT,
    role            TEXT    NOT NULL DEFAULT 'customer',
    oauth_provider  TEXT,
    oauth_id        TEXT,
    credit          REAL    NOT NULL DEFAULT 0.0,
    card_code       TEXT,
    blocked         INTEGER NOT NULL DEFAULT 0,
    allow_debit     INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO users_new (id, email, name, password_hash, phone, role,
                       oauth_provider, oauth_id, created_at)
SELECT id, email, COALESCE(name, '(no name)'), password_hash, phone, role,
       oauth_provider, oauth_id, created_at FROM users;
DROP TABLE users;
ALTER TABLE users_new RENAME TO users;
CREATE UNIQUE INDEX idx_users_card_code ON users(card_code)
       WHERE card_code IS NOT NULL;
-- Recreate any other indexes/triggers on users from existing schema.

-- 2. Promote linked cards (cards.user_id IS NOT NULL) into the existing user row
UPDATE users SET
    credit      = (SELECT credit      FROM cards WHERE cards.user_id = users.id),
    card_code   = (SELECT barcode     FROM cards WHERE cards.user_id = users.id),
    blocked     = (SELECT blocked     FROM cards WHERE cards.user_id = users.id),
    allow_debit = (SELECT allow_debit FROM cards WHERE cards.user_id = users.id),
    company     = (SELECT company     FROM cards WHERE cards.user_id = users.id)
 WHERE EXISTS (SELECT 1 FROM cards WHERE cards.user_id = users.id);

-- 3. Insert one users row per unlinked legacy card (email=NULL, name placeholder if blank)
INSERT INTO users (email, name, phone, role, credit, card_code,
                   blocked, allow_debit, company, created_at)
SELECT
    NULL,
    COALESCE(NULLIF(TRIM(COALESCE(c.first_name,'') || ' ' || COALESCE(c.last_name,'')), ''),
             '(no name)'),
    c.phone, 'customer', c.credit, c.barcode, c.blocked,
    c.allow_debit, c.company, c.created_at
FROM cards c WHERE c.user_id IS NULL;

-- 4. Backfill cards.user_id for the freshly-created users (so step 5 can map)
UPDATE cards SET user_id = (SELECT id FROM users WHERE users.card_code = cards.barcode)
 WHERE user_id IS NULL;

-- 5. Backfill transactions.user_id where missing
UPDATE transactions
   SET user_id = (SELECT user_id FROM cards WHERE cards.id = transactions.card_id)
 WHERE user_id IS NULL AND card_id IS NOT NULL;

-- 6. Recreate transactions without card_id, with user_id NOT NULL
CREATE TABLE transactions_new (...same columns minus card_id, user_id NOT NULL...);
INSERT INTO transactions_new SELECT id, user_id, staff_id, service_id, amount,
       action, created_at, ... FROM transactions;
DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;
-- Recreate indexes/triggers on transactions.

-- 7. Recreate bookings without card_id
CREATE TABLE bookings_new (...same columns minus card_id...);
INSERT INTO bookings_new SELECT id, template_id, date, user_id, ... FROM bookings;
DROP TABLE bookings;
ALTER TABLE bookings_new RENAME TO bookings;
-- Recreate indexes/triggers on bookings.

-- 8. Drop cards
DROP TABLE cards;

-- 9. Integrity check
PRAGMA integrity_check;
PRAGMA foreign_key_check;
```

Migration MUST be re-runnable safely (idempotent against partial-applied state) — the migration runner checks an already-applied flag, but the SQL inside should also be defensible if re-attempted on a half-migrated DB during local testing.

## API surface changes

### New endpoints (Staff-only)

- `POST /api/users` — create person at desk. Body: `{ name (required), email?, phone?, company?, card_code? }`. Returns full user. `password_hash=NULL`. Idempotent on email collision: 409 with existing user.
- `PATCH /api/users/{id}` — update name/email/phone/company/card_code/blocked/allow_debit. Email validated if present (must contain `@` and `.`).
- `GET /api/users/search?q=...` — name OR email OR card_code OR company LIKE match. Returns full user records with credit, blocked, pass info, last_visit_at.
- `GET /api/users/{id}` — single user with pass + last_visit_at.
- `GET /api/users/negative-balance` — replaces `/api/cards/negative-balance`.
- `POST /api/users/{id}/charge` — replaces `/api/payments/charge` (which currently takes card_id). Records transaction with `user_id`, NULL service_id is for generic charges, etc. — same business rules.
- `POST /api/users/{id}/topup` — replaces `/api/payments/topup`.
- `POST /api/users/{id}/log-visit` — replaces `/api/payments/log-visit`.
- `GET /api/users/{id}/transactions` — replaces card-scoped txn list.

### Removed endpoints

- All `/api/cards/*` paths and the legacy `/api/payments/*` variants that take `card_id` are removed. Callers in the UI all migrate.
- `POST /api/cards/activate` (the "activate new card" admin endpoint) is removed.

### Unchanged endpoints

- `POST /api/auth/register` (customer self-signup, requires email+password).
- `POST /api/auth/login`, `GET /api/auth/me`.
- `GET /api/version`, `/api/classes/*`, `/api/upcoming-classes/*`, `/api/persistent-bookings/*` — those already use `user_id`.
- `GET /api/admin/*` — admin endpoints not card-related.

## Code-level impact (estimate)

**Backend (`crates/spinbike-server/src/`):**
- `db/migrations.rs` — add new migration step (the SQL above).
- `db/cards.rs` — DELETED.
- `db/users.rs` — extended with all helpers previously on cards: `get_credit`, `set_credit`, `apply_credit_delta`, `find_by_card_code`, `find_by_email`, `search`, `list_negative_balance`, `last_visit_at`, `pass_info`, `block`, `unblock`.
- `routes/cards.rs` — DELETED.
- `routes/users.rs` — NEW, mirrors removed routes/cards.rs structure.
- `routes/payments.rs` — handlers updated to take user_id (not card_id). The same SQL underneath, just keyed differently.
- `routes/persistent_bookings.rs`, `routes/classes.rs`, `routes/upcoming_classes.rs`, `routes/transactions.rs` — drop any `card_id` references; use `user_id` only.
- `routes/test_fixtures.rs` — `seed_credit` keyed by user_id.
- `routes/mod.rs` — wire new router.
- `tests/cards_routes.rs` — port to `tests/users_routes.rs`.
- `tests/payments_routes.rs` — update body shape.

**Frontend (`spinbike-ui/src/`):**
- `pages/dashboard/mod.rs` — replace `Activate New Card` form + signal with `Add Person` form. Drop `show_activate`, replace with `show_add_person`.
- `pages/dashboard/action_form.rs` — calls switch from `/api/payments/charge` (card_id body) to `/api/users/{id}/charge` (path-keyed). Field rename `card_id` → `user_id` in any local types.
- `pages/dashboard/negative_balance_list.rs` — endpoint and types switch from `/api/cards/negative-balance` to `/api/users/negative-balance`. Display `name` (not `first_name`+`last_name`).
- `pages/dashboard/search_results.rs` (or wherever search-dropdown lives) — switch to `/api/users/search`. Row class still `result_row_class(highlighted, credit)` works because credit is on user.
- `pages/dashboard/helpers.rs` — `full_name_or_fallback` no longer needs first/last/company logic; collapse to `name.unwrap_or("(no name)")`.
- `pages/link_card.rs` — DELETED (link-card flow is obsolete; legacy chip codes are no longer attached to a user via separate flow).
- `i18n.rs` — add new keys: `add_person_heading`, `add_person_button`, `name_label`, `email_optional_label`, `phone_optional_label`, `company_optional_label`, `card_code_optional_label`, `add_person_ok`, `add_person_error_*`. Remove `activate_new_card`, `hide_activate`, `activate_ok`, `activate`, `deactivate` (or keep `deactivate` if used elsewhere). Keep `card_code` translations because existing UI still shows it on the user record.
- `app.rs` / router — drop `/customer/link-card` route.
- `version_footer.rs` — unchanged.

**E2E (`e2e/tests/`):**
- `last-visit-display.spec.ts` — seed via new user-create endpoint, no barcode used.
- `negative-balance.spec.ts` — same.
- `log-visit.spec.ts` (if present) — switch endpoint paths.
- `charge.spec.ts`, `topup.spec.ts` — same.
- `link-card.spec.ts` — DELETED.
- `add-person.spec.ts` — NEW. Open Desk, click Add Person, fill name+email, expect success banner + new row in search.
- All specs use `loginViaAPI`, `setupConsoleCheck`, `assertCleanConsole` (existing helpers). Console must remain zero-error.

**Test fixtures:**
- `routes/test_fixtures.rs::seed_credit` becomes `seed_user` (creates user with credit) and `seed_legacy_card` is dropped.

## Validation gate

- CI runs lint, test, build, E2E, mutation testing on PR.
- Self-hosted dev runner deploys binary; deploy-dev step `.backup`s prod → dev, then restarts dev binary which runs the migration on the prod-shaped data.
- Smoke tests on dev verify dashboard loads, search works, "Add Person" works, negative-balance list renders against migrated data.
- **Manual sanity** before merge: `sqlite3 dev.db` query a few rows from the migrated `users` to confirm name, credit, card_code, blocked match the original `cards` data.

## Out of scope (explicitly NOT in this PR)

- Email collection / invitation flow.
- Magic-link or password-reset auth.
- Online top-up (Stripe etc.).
- Self-service customer signup polish.
- Schedule view for logged-in customers.
- Removing `card_code` column entirely (planned for a later PR once codes truly aren't needed).
- Splitting `name` into first/last (CEO explicitly rejected).

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| Migration corrupts prod data | Validate on dev DB synced from prod first; smoke tests must pass before merge. |
| Email UNIQUE collisions in legacy data | Email left NULL for legacy → no collisions. New rows reject collisions with 409. |
| FK cleanup on transactions/bookings breaks history | Recreate tables preserving all rows; integrity_check + foreign_key_check inside migration. |
| Search performance regresses (no UNION needed but full-table scan) | Index on `card_code`; existing index on `email`; LIKE on `name` already accepted at current scale (~3k rows). |
| Mutation testing surfaces survivors in new code | Plan includes assertion-rich integration tests + E2E specs targeting the visible UX. |
| Subagents try to run cargo test/build/clippy locally | Plan explicitly forbids local cargo (CI-authoritative); only `cargo fmt --all --check` allowed locally. |

## Acceptance

- All E2E specs green.
- Mutation testing on diff: 0 surviving mutants in new code.
- Dev deploy: migration applies cleanly on prod-shaped DB; smoke tests pass; manual sanity check on `sqlite3 dev.db`.
- PR mergeable + clean. Awaits user merge.

## Implementation handoff

Next step: invoke `superpowers:writing-plans` to break this into bite-sized TDD tasks, then dispatch via `superpowers:subagent-driven-development`.
