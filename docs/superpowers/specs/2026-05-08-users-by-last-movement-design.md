# Users-by-last-movement report + soft-delete

**Issue:** [#56](https://github.com/zbynekdrlik/spinbike/issues/56)
**Date:** 2026-05-08
**Status:** Approved (brainstorm round)

## Goal

Give the CEO a report listing all users sorted by oldest activity first, with a way to soft-delete obsolete users from the same flow. Lives as a new tab inside the existing Reports page.

## Scope

- Reports page gains a tab switcher: **Daily activity** (existing KPIs/feed) / **Users** (new).
- Users tab shows a flat list, oldest-movement-first, paginated 50 + "Show more".
- Each row → opens that user's existing card panel.
- Card panel gains a **Delete user** affordance (admin gate). Confirmation modal lists name + balance + active permanentka warnings, then sets `users.deleted_at = datetime('now')`.
- Soft-deleted users disappear from search, dropdowns, negative-balance list, and the new report. Their transactions stay attached but invisible (existing per-user history queries already gate on `users.deleted_at IS NULL` after this change).
- The V13 synthetic `(deleted)` placeholder gets soft-deleted as part of V15 so it stops surfacing in UI (closes #68 as a side effect).

**Out of scope:**
- Filter chips, date pickers, year thresholds — CEO sorts oldest-first and decides per-row.
- Hard-delete / undelete UI — soft-delete is reversible only at the DB level.
- Restoring soft-deleted users — no UI flow planned.
- Reports for other entities (cards / transactions / passes) — separate future scope.

## UX

### Reports page tabs

```
[Daily activity] [Users]
```

`data-testid="reports-tab-daily"` and `data-testid="reports-tab-users"`. Default tab on first load = Daily activity (preserves existing behaviour). Tab state stays in component; no URL routing change.

### Users tab list

- Heading: **Pouzivatelia podla posledneho pohybu** / "Users by last movement".
- Each row: `[name]  [last-movement-date]` — date as `dd.mm.yyyy` (locale-aware via existing `format_local_date` helper). Users with no transactions show `—` instead of a date and bubble to the top.
- Sort: ascending by `MAX(transactions.created_at)`, NULLS FIRST. Stable secondary sort by `users.id` ascending so paging is deterministic.
- Pagination: load 50 rows initially, "Show more" button at the bottom loads the next 50. Button hides when fewer than 50 rows came back.
- Click row: `editing_user.set(Some(user.id))` — same context signal the Desk uses to open the card panel. Reports page mounts the existing card panel as a sibling so it overlays.

### Card panel — Delete user

- New button at the bottom of the panel, only visible if `claims.role.can_manage_cards()`.
- Slovak: **Zmazat pouzivatela**. English: "Delete user".
- `data-testid="delete-user-button"`.
- Click → `DeleteUserSheet` modal opens.

### Delete confirmation modal

- Title: **Zmazat `<name>`?** / "Delete `<name>`?".
- Body lines (rendered conditionally):
  - Always: "Tato akcia skryje pouzivatela vsade. Historia ostane v DB." / "Hides the user everywhere. History stays in the DB."
  - If `balance != 0`: warning row "Zostatok: `+12.50 EUR`" / "Balance: `+12.50 EUR`".
  - If active permanentka exists (`valid_until > today` AND `deleted_at IS NULL`): warning row "Aktivna permanentka do `<date>`" / "Active permanentka until `<date>`".
- Buttons: **Zrusit** / Cancel  +  **Zmazat** / Delete (destructive style).
- Confirm → PATCH (semantically a DELETE; see API) → on 200 close modal + close card panel + signal report refresh.

## Backend

### Migration V15

```sql
-- V15: soft-delete column on users + retire V13 synthetic placeholder
ALTER TABLE users ADD COLUMN deleted_at TEXT;

-- Retire the synthetic '(deleted)' placeholder from V13 so it stops surfacing
UPDATE users SET deleted_at = datetime('now')
 WHERE name = '(deleted)' AND deleted_at IS NULL;
```

Idempotency: `ALTER TABLE` is naturally non-idempotent in SQLite — wrap with the existing `PRAGMA table_info(users)` check (same pattern V7 used for `transactions.deleted_at`). The `UPDATE … WHERE deleted_at IS NULL` line is naturally idempotent.

### Read endpoint

`GET /api/admin/users/by-last-movement?limit=50&offset=0`

Auth: `claims.role.can_manage_cards()`; otherwise 403.

Response:

```json
[
  { "id": 42, "name": "Anna Novakova", "last_movement_at": "2024-03-12 18:21:00" },
  { "id": 17, "name": "Peter Kovac",   "last_movement_at": null }
]
```

SQL:

```sql
SELECT
    u.id,
    u.name,
    MAX(t.created_at) AS last_movement_at
  FROM users u
  LEFT JOIN transactions t
    ON t.user_id = u.id
   AND t.deleted_at IS NULL
 WHERE u.deleted_at IS NULL
 GROUP BY u.id
 ORDER BY last_movement_at IS NULL DESC,  -- NULLS FIRST
          last_movement_at ASC,
          u.id ASC
 LIMIT ?1 OFFSET ?2
```

`limit` clamped to `[1, 200]`; default 50. `offset` clamped to `>= 0`; default 0. Bad values → 400.

### Soft-delete endpoint

`DELETE /api/admin/users/{id}`

Auth: `claims.role.can_manage_cards()`; otherwise 403.

Body: none. Response: `{ "id": 42, "deleted_at": "2026-05-08 14:32:11" }`.

Errors:
- 404 if user does not exist
- 409 if `deleted_at IS NOT NULL` already (idempotency-failure semantics — caller should refresh)
- 403 if role gate fails

Behaviour: `UPDATE users SET deleted_at = datetime('now') WHERE id = ?`. Existing transactions for that user are NOT touched.

### Soft-delete filter ripple — exhaustive list

Every existing query that lists or fetches users must gain `WHERE u.deleted_at IS NULL` (or its alias-prefixed variant). Plan task will enumerate via `git grep` for: `FROM users`, `JOIN users`, `users WHERE`. Confirmed touch points to start the audit:

| File:line | Query purpose | Patch |
|---|---|---|
| `crates/spinbike-server/src/db/users.rs` (`search_users`, `list_users`, `get_user`, etc.) | All listing/lookup paths | add `AND u.deleted_at IS NULL` / `WHERE deleted_at IS NULL` |
| `crates/spinbike-server/src/routes/users.rs` (negative-balance route) | Negative-balance feed | add filter |
| `crates/spinbike-server/src/routes/transactions.rs` | If a JOIN to users feeds a list endpoint, gate it; per-row endpoints (e.g. card history) MAY return rows for soft-deleted users since the panel can still display the history if reached via a deep link. Decision: per-user-history endpoints DO show soft-deleted user rows so the report row stays clickable for inspection up until the panel closes; everywhere else hides. |

The plan will add a unit/integration test asserting that soft-deleted users vanish from each surface listed.

## i18n keys (Slovak unaccented + English)

| Key | Slovak | English |
|---|---|---|
| `reports_tab_daily` | Denna aktivita | Daily activity |
| `reports_tab_users` | Pouzivatelia | Users |
| `users_by_movement_heading` | Pouzivatelia podla posledneho pohybu | Users by last movement |
| `last_movement` | Posledny pohyb | Last movement |
| `no_movement_yet` | Bez pohybu | No movement yet |
| `show_more` | Zobrazit dalsie | Show more |
| `delete_user` | Zmazat pouzivatela | Delete user |
| `delete_user_confirm_title` | Zmazat {name}? | Delete {name}? |
| `delete_user_confirm_body` | Tato akcia skryje pouzivatela vsade. Historia ostane v DB. | Hides the user everywhere. History stays in the DB. |
| `delete_user_warning_balance` | Zostatok: {amount} EUR | Balance: {amount} EUR |
| `delete_user_warning_pass` | Aktivna permanentka do {date} | Active permanentka until {date} |
| `delete_user_cancel` | Zrusit | Cancel |
| `delete_user_confirm` | Zmazat | Delete |

## Frontend module layout

- New: `spinbike-ui/src/pages/reports/users_by_movement.rs` — component, fetch, load-more.
- New: `spinbike-ui/src/pages/dashboard/sheets/delete_user.rs` — confirmation modal with conditional warnings.
- Modify: `spinbike-ui/src/pages/reports/mod.rs` — tab switcher; mount card panel for clicked row.
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs` — Delete user button, gated on role + only when not already deleted (the row must be in scope, so this is mostly a render guard against future soft-deleted views).
- Modify: `spinbike-ui/src/i18n.rs` — 13 new keys.
- Modify: `spinbike-core` if a new shared type is needed (`UserByMovement` row); otherwise inline in route.

## Testing

### Backend integration tests (`crates/spinbike-server/tests/users_by_movement.rs`)

- `list_orders_by_oldest_movement_first` — seeds 3 users with varying transaction times; asserts order matches.
- `list_users_with_no_transactions_appear_first` — null `last_movement_at` rows precede dated rows.
- `list_paginates_with_show_more` — limit=2, offset=2 returns the next slice.
- `list_excludes_voided_transactions` — a user whose only txn is `deleted_at NOT NULL` shows null `last_movement_at`.
- `list_excludes_soft_deleted_users` — soft-deleted user does NOT appear.
- `list_requires_staff_role` — non-staff → 403.

### Backend integration tests (`crates/spinbike-server/tests/users_delete.rs`)

- `delete_user_happy_path_sets_deleted_at`.
- `delete_user_already_deleted_returns_409`.
- `delete_user_missing_id_returns_404`.
- `delete_user_non_staff_returns_403`.
- `delete_user_does_not_remove_transactions` — txn rows still exist after delete.
- `deleted_user_hidden_from_search` — search endpoint omits the deleted user.
- `deleted_user_hidden_from_negative_balance` — negative-balance feed omits the deleted user even if they had unpaid balance.

### Mutation testing target

Mutation testing on the SELECT must kill any mutant that drops `WHERE u.deleted_at IS NULL` — covered by `list_excludes_soft_deleted_users` + `deleted_user_hidden_from_search`.

### Playwright E2E (`e2e/tests/users-by-movement.spec.ts`)

1. Login via API helper (forces English).
2. Seed 3 users via API: A (no txns), B (charge dated 2 years ago), C (visit dated yesterday).
3. Navigate to `/staff` → click `[data-testid="nav-reports"]` → click `[data-testid="reports-tab-users"]`.
4. Assert row order: A (no movement), B, C.
5. Click row B → assert card panel opens.
6. Click `[data-testid="delete-user-button"]` → modal opens with B's name.
7. Click `[data-testid="delete-user-confirm"]` → modal closes, panel closes, row B disappears from the list.
8. `assertCleanConsole(msgs)` — zero console errors/warnings.

## Versioning + deploy

First commit on dev for this work bumps `VERSION` 0.13.27 → 0.13.28 + runs `bash scripts/sync-version.sh`. PR `dev` → `main` after CI green; never merge from agent.

## Open follow-ups (filed only when discovered)

None at design time. Plan-time discoveries (e.g. "search_users has 4 call sites that all need the filter") become explicit plan steps, not separate issues.
