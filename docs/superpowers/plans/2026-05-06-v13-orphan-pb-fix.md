# V13 Orphan persistent_bookings Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop V13 step 7b from silently dropping orphan `persistent_bookings`, and add a unit-test seed that exercises both the orphan-transaction (#69) and orphan-PB (#70) fallback paths.

**Architecture:** One coherent edit to `const V13_USERS_REPLACE_CARDS` in `crates/spinbike-server/src/db/migrations.rs` — extend step 5's conditional `(deleted)` user creation, change step 7b from `INNER JOIN cards` to `LEFT JOIN cards` with a `COALESCE(c.user_id, deleted_user_id)` fallback. Bundled with two new seed rows + three new assertions in the existing `migration_13_users_replace_cards_full_round_trip` test, plus three updated assertions to reflect the new row counts.

**Tech Stack:** Rust + sqlx 0.8 + SQLite (in-memory pool for tests). No new dependencies.

---

## File Structure

| File | Responsibility | Change kind |
|---|---|---|
| `VERSION` | Single source of truth for app version | Bump `0.13.23` → `0.13.24` |
| `Cargo.toml` (workspace + crates) | Per-crate version mirrors of `VERSION` | Synced by `scripts/sync-version.sh` |
| `crates/spinbike-server/src/db/migrations.rs` | All schema migrations + their unit tests | Edit `const V13_USERS_REPLACE_CARDS` (step 5 + step 7b) and `migration_13_users_replace_cards_full_round_trip` (seed + assertions) |
| `docs/superpowers/specs/2026-05-06-v13-orphan-pb-fix-design.md` | Approved design (already committed at 793b4ab) | Reference only — do not edit |

**No other files touched.** No frontend, no E2E, no routes, no API.

---

## Task 1: VERSION bump 0.13.23 → 0.13.24

**Owner:** CONTROLLER (run directly, not a subagent).

**Files:**
- Modify: `VERSION`
- Modify (auto-synced): `Cargo.toml`, `crates/spinbike-core/Cargo.toml`, `crates/spinbike-server/Cargo.toml`, `spinbike-ui/Cargo.toml`

- [ ] **Step 1: Edit `VERSION`**

```bash
echo "0.13.24" > VERSION
```

- [ ] **Step 2: Sync to all Cargo.toml files**

Run: `bash scripts/sync-version.sh`

Expected output: lists each Cargo.toml updated to 0.13.24.

- [ ] **Step 3: Commit**

```bash
git add VERSION Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml spinbike-ui/Cargo.toml
git commit -m "$(cat <<'EOF'
chore: bump version 0.13.23 → 0.13.24

Pre-flight bump before V13 orphan-PB fix work. Required by CI
version-check job (dev > main).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: V13 SQL edits + test seed/assertions

**Owner:** Subagent (sonnet model).

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs` (one file, two regions: the `const V13_USERS_REPLACE_CARDS` SQL and the `migration_13_users_replace_cards_full_round_trip` test fn).

**Subagent prompt MUST start with:** "Read `docs/superpowers/specs/2026-05-06-v13-orphan-pb-fix-design.md` sections 'Migration sequencing safety' and 'Test changes' in full before writing any code. Ask any clarifying questions before starting."

### Subtask 2a — Update SQL in `const V13_USERS_REPLACE_CARDS`

- [ ] **Step 1: Locate step 5 conditional INSERT**

Grep: `grep -n "Step 5\|-- 5\." crates/spinbike-server/src/db/migrations.rs`

The current step-5 conditional INSERT (created by PR #67) reads:

```sql
INSERT INTO users (email, name, role)
SELECT NULL, '(deleted)', 'customer'
 WHERE EXISTS (SELECT 1 FROM transactions WHERE user_id IS NULL);
```

- [ ] **Step 2: Replace step 5 conditional INSERT**

Use Edit tool. Old string is the three-line `INSERT … '(deleted)' … WHERE EXISTS (SELECT 1 FROM transactions WHERE user_id IS NULL);` shown above (match exactly to the file's whitespace). New string:

```sql
INSERT INTO users (email, name, role)
SELECT NULL, '(deleted)', 'customer'
 WHERE EXISTS (SELECT 1 FROM transactions WHERE user_id IS NULL)
    OR EXISTS (SELECT 1 FROM persistent_bookings pb
                LEFT JOIN cards c ON c.id = pb.card_id
                WHERE c.user_id IS NULL);
```

Why: spec section "SQL change — step 5". The `c.user_id IS NULL` predicate covers both `pb.card_id IS NULL` (LEFT JOIN nulls everything) and `pb.card_id` pointing to a non-existent cards row (no match → c.user_id NULL).

- [ ] **Step 3: Locate step 7b INSERT**

Grep: `grep -n "step 7b\|-- 7b\.\|persistent_bookings_new (id, user_id" crates/spinbike-server/src/db/migrations.rs`

Current step 7b INSERT (around line 535):

```sql
INSERT INTO persistent_bookings_new (id, user_id, template_id, created_at, ended_at)
SELECT pb.id, c.user_id, pb.template_id, pb.created_at, pb.ended_at
FROM persistent_bookings pb
JOIN cards c ON c.id = pb.card_id;
```

- [ ] **Step 4: Replace step 7b INSERT**

Use Edit tool. Old string is the four-line INSERT above. New string:

```sql
INSERT INTO persistent_bookings_new (id, user_id, template_id, created_at, ended_at)
SELECT pb.id,
       COALESCE(c.user_id,
                (SELECT id FROM users WHERE name = '(deleted)' ORDER BY id DESC LIMIT 1)),
       pb.template_id, pb.created_at, pb.ended_at
  FROM persistent_bookings pb
  LEFT JOIN cards c ON c.id = pb.card_id;
```

Why: spec section "SQL change — step 7b". `LEFT JOIN` keeps the orphan PB row; `COALESCE` falls back to the synthetic `(deleted)` user (guaranteed alive after the step-5 change above).

### Subtask 2b — Update `migration_13_users_replace_cards_full_round_trip`

- [ ] **Step 5: Locate the test fn**

Grep: `grep -n "fn migration_13_users_replace_cards_full_round_trip" crates/spinbike-server/src/db/migrations.rs`

Currently around line 1413. Test seeds: staff user, alice (linked card CODE1), bob (unlinked card CODE2), nameless (unlinked card CODE3), charlie (linked card CODE4 with blank names), one transaction tied to bob's card, one persistent_booking tied to bob's card.

- [ ] **Step 6: Add orphan transaction seed BEFORE the `// Apply V13.` comment**

Find the existing transaction seed block (around line 1533) — `INSERT INTO transactions(card_id, staff_id, amount, action) VALUES(?, ?, -1.50, 'charge')`. Immediately AFTER that block, BEFORE the `// Seed a persistent_booking …` comment, add:

```rust
        // Issue #69: orphan transaction (card_id NULL, user_id NULL) must
        // resolve to the synthetic '(deleted)' user via the step-5 fallback.
        let orphan_txn_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions(card_id, user_id, staff_id, amount, action)
             VALUES(NULL, NULL, ?, -5.00, 'charge') RETURNING id",
        )
        .bind(staff_id)
        .fetch_one(&pool)
        .await
        .unwrap();
```

Note: V12-era `transactions` schema columns the test must touch are `card_id`, `user_id`, `staff_id`, `amount`, `action`. Other columns (`service_id`, `valid_until`, `deleted_at`, `legacy_backfilled`, `note`, `created_at`) all have defaults / are nullable per existing test usage at line 1533 — no need to specify.

- [ ] **Step 7: Add orphan persistent_booking seed AFTER the existing PB seed**

Find the existing PB seed (around line 1549) — `INSERT INTO persistent_bookings(card_id, template_id) VALUES(?, ?)` with bob_card_id + template_id. Immediately AFTER that block, BEFORE the `// Apply V13.` comment, add:

```rust
        // Issue #70: orphan persistent_booking (card_id points to non-existent
        // cards row 999) must resolve to the synthetic '(deleted)' user via
        // the step-7b LEFT JOIN + COALESCE fallback. 999 is chosen because
        // CODE1..CODE4 occupy ids 1..4; 999 is guaranteed unused.
        let orphan_pb_id: i64 = sqlx::query_scalar(
            "INSERT INTO persistent_bookings(card_id, template_id)
             VALUES(999, ?) RETURNING id",
        )
        .bind(template_id)
        .fetch_one(&pool)
        .await
        .unwrap();
```

- [ ] **Step 8: Update `users_total` assertion (5 → 6)**

Find the existing assertion (around line 1567):

```rust
        let users_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(users_total, 5, "staff + alice + bob + nameless + charlie");
```

Replace the assertion line with:

```rust
        assert_eq!(
            users_total, 6,
            "staff + alice + bob + nameless + charlie + (deleted)"
        );
```

Why: orphan transaction (and orphan PB) cause step 5 to insert one synthetic `(deleted)` user.

- [ ] **Step 9: Update `pb_user` assertion to filter by Bob's PB explicitly**

Find the existing `pb_user` block (around line 1740):

```rust
        let pb_user: i64 = sqlx::query_scalar("SELECT user_id FROM persistent_bookings LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            pb_user, bob_user,
            "persistent_bookings.user_id must point to Bob's new user row"
        );
```

Replace the SELECT to scope by template_id + non-deleted user, or simpler — the existing valid PB has `id = 1` (first inserted), the orphan has `id = 2`. Filter by id explicitly using a captured value. Restructure to:

```rust
        // Bob's PB (the one seeded with valid card_id) — captured before V13.
        // It carries the same id across V13 because step 7b preserves pb.id.
        let bob_pb_user: i64 = sqlx::query_scalar(
            "SELECT user_id FROM persistent_bookings
             WHERE template_id = ? AND user_id = ?",
        )
        .bind(template_id)
        .bind(bob_user)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            bob_pb_user, bob_user,
            "persistent_bookings.user_id must point to Bob's new user row"
        );
```

Why: with two PB rows now (Bob's + orphan), `LIMIT 1` is nondeterministic. Filter by Bob's user_id directly so the assertion remains crisp.

- [ ] **Step 10: Update `pb_count` assertion (1 → 2)**

Find the existing assertion (around line 1750):

```rust
        let pb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM persistent_bookings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            pb_count, 1,
            "persistent_bookings count must be preserved across V13"
        );
```

Replace the assertion line:

```rust
        assert_eq!(
            pb_count, 2,
            "Bob's PB + orphan PB must both survive V13 (no INNER JOIN drop)"
        );
```

- [ ] **Step 11: Add three new orphan-fallback assertions**

After the existing `pb_count` assertion block (and before the `idx_persistent_bookings_user_id_template_id_active` index check around line 1759), insert:

```rust
        // ── Orphan-fallback assertions (#69, #70) ──────────────────────────
        // The synthetic '(deleted)' user exists.
        let deleted_user_id: i64 = sqlx::query_scalar(
            "SELECT id FROM users WHERE name = '(deleted)' ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Issue #69: the orphan transaction maps to the deleted user.
        let orphan_txn_user: i64 = sqlx::query_scalar(
            "SELECT user_id FROM transactions WHERE id = ?",
        )
        .bind(orphan_txn_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            orphan_txn_user, deleted_user_id,
            "orphan transaction (card_id=NULL) must map to '(deleted)' user via step-5 fallback"
        );

        // Issue #70: the orphan persistent_booking maps to the deleted user.
        let orphan_pb_user: i64 = sqlx::query_scalar(
            "SELECT user_id FROM persistent_bookings WHERE id = ?",
        )
        .bind(orphan_pb_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            orphan_pb_user, deleted_user_id,
            "orphan persistent_booking (card_id=999) must map to '(deleted)' user via step-7b LEFT JOIN + COALESCE"
        );
```

### Subtask 2c — Local lint + commit

- [ ] **Step 12: Local format check (only allowed local check)**

Run: `cargo fmt --all --check`
Expected: exits 0 with no output.

If it fails, run `cargo fmt --all` to fix and re-check.

- [ ] **Step 13: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "$(cat <<'EOF'
fix(db): V13 step 7b LEFT JOIN + COALESCE keeps orphan persistent_bookings

V13 step 7b previously used INNER JOIN cards, silently dropping any
persistent_booking row whose card_id did not resolve. Prod has zero
persistent_bookings today so the bug never bit, but a future fresh
import would lose rows without warning. Switch to LEFT JOIN with
COALESCE fallback to the synthetic '(deleted)' user that step 5
already creates.

Step 5's conditional INSERT now also fires when an orphan
persistent_booking exists, so the '(deleted)' user is guaranteed
alive when step 7b reads it. The OR-EXISTS uses LEFT JOIN +
WHERE c.user_id IS NULL to detect both NULL and dangling card_id.

V13 is past on prod (schema_version=14), so this edit only affects
fresh-DB chains (CI tests, future imports). Documented exception to
the frozen-migrations rule per
docs/superpowers/specs/2026-05-06-v13-orphan-pb-fix-design.md.

migration_13_users_replace_cards_full_round_trip gains:
- Orphan transaction seed (card_id=NULL, user_id=NULL) — covers #69
- Orphan persistent_booking seed (card_id=999) — covers #70
- '(deleted)' user existence check
- Both orphan rows asserted to map to '(deleted)' user_id
- users_total bumped 5 → 6, pb_count bumped 1 → 2
- pb_user query scoped by Bob's user_id (not LIMIT 1) since two PBs now exist

Closes #69
Closes #70

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Push + monitor CI to terminal state + open PR

**Owner:** CONTROLLER (run directly, not a subagent).

- [ ] **Step 1: Push dev branch**

```bash
git push origin dev
```

- [ ] **Step 2: Identify the latest run**

```bash
gh run list --branch dev --limit 1 --json databaseId,status,conclusion,headSha
```

Capture the `databaseId` as `RUN_ID`.

- [ ] **Step 3: Monitor to terminal state via single background poll**

Per `airuleset/modules/core/ci-monitoring.md`: ONE `sleep N && gh run view --json status,conclusion,jobs` background command. NO `/loop`, NO custom monitor scripts.

```bash
sleep 300 && gh run view <RUN_ID> --json status,conclusion,jobs
```

Run this with `Bash` tool, `run_in_background: true`. Read with `BashOutput` when notified. If still in progress, repeat with another 300s sleep.

Required terminal state: every job conclusion is `success` (or `skipped` for the deploy-prod / smoke-prod jobs that gate on push-to-main only). Specifically expect:
- Test Integrity ✅
- Lint ✅
- Test ✅
- Build WASM (UI) ✅
- Test (UI) ✅
- E2E Tests ✅
- Mutation Testing ✅
- Deploy (dev) ✅
- Smoke (dev) ✅
- Version Bump Check ✅ on the PR run (skipped on push-to-dev)
- Deploy (prod) / Smoke (prod) skipped on dev run

If any job FAILS:
1. `gh run view <RUN_ID> --log-failed` — investigate
2. Fix the root cause (no `--no-verify`, no timeout band-aids)
3. New commit on dev, push, restart monitoring with the new run id

- [ ] **Step 4: Open PR `dev` → `main`**

```bash
gh pr create --base main --head dev --title "fix(db): V13 LEFT JOIN keeps orphan persistent_bookings (#69 #70)" --body "$(cat <<'EOF'
## Summary

Fixes Issue #70: V13 step 7b previously used `INNER JOIN cards`, silently dropping any `persistent_booking` row whose `card_id` did not resolve. Switch to `LEFT JOIN` with a `COALESCE(c.user_id, deleted_user_id)` fallback to the synthetic `(deleted)` user that step 5 creates. Step 5's conditional INSERT now also fires when an orphan PB exists, so the deleted-user row is guaranteed when step 7b reads it.

Bundles Issue #69: `migration_13_users_replace_cards_full_round_trip` gains an orphan-transaction seed (`card_id=NULL, user_id=NULL`) plus the corresponding `(deleted)`-fallback assertion.

## Frozen-rule justification

V13 is past on prod (`schema_version=14`); this PR's edit only affects FRESH-DB chains (CI tests, future imports). See `docs/superpowers/specs/2026-05-06-v13-orphan-pb-fix-design.md` section "Frozen-rule justification".

## Test plan

- [x] `cargo fmt --all --check` clean locally
- [x] CI green on `dev` (link below)
- [x] New unit-test seed covers orphan transaction + orphan PB; new assertions confirm both map to `(deleted)` user
- [ ] Post-merge: prod CI green, version DOM check on https://spinbike.newlevel.media reads `v0.13.24`

## Out of scope

No frontend, no E2E, no routes touched. Single file (`crates/spinbike-server/src/db/migrations.rs`) + VERSION bump.

Closes #69
Closes #70

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Verify PR is mergeable + clean**

```bash
gh pr view --json number,mergeable,mergeStateStatus
```

Required: `mergeable: MERGEABLE` AND `mergeStateStatus: CLEAN`. If `UNSTABLE` / `BLOCKED` / `BEHIND` / `DIRTY`, investigate and fix before reporting done. Per `autonomous-quality-discipline.md`: UNSTABLE is not clean.

- [ ] **Step 6: STOP — never merge.** Per `pr-merge-policy.md`: provide the green PR URL and wait for the user's explicit "merge it".

---

## Task 4: Post-deploy verification on prod

**Owner:** CONTROLLER. **DO NOT START until the user explicitly says "merge it" AND the merge has happened AND main CI's Deploy (prod) + Smoke (prod) are both ✅.**

- [ ] **Step 1: Confirm merge happened**

```bash
gh pr view <PR-N> --json state,mergedAt,mergeCommit
```

Expected: `state: MERGED`, `mergedAt` populated.

- [ ] **Step 2: Watch main CI to terminal state**

```bash
gh run list --branch main --limit 1 --json databaseId,status,conclusion
sleep 300 && gh run view <MAIN_RUN_ID> --json status,conclusion,jobs
```

Required: Deploy (prod) ✅ AND Smoke (prod) ✅.

- [ ] **Step 3: Backend version check**

```bash
curl -s https://spinbike.newlevel.media/api/version
```

Expected: `{"version":"0.13.24"}`.

- [ ] **Step 4: Frontend DOM version check via Playwright MCP**

Navigate to `https://spinbike.newlevel.media`, read `[data-testid="version"]` text, confirm it reads `v0.13.24`. Confirm zero browser console errors. Close browser.

This is the only verification needed — V13 is past on prod (schema unchanged by this PR), and no UI surface changed.

- [ ] **Step 5: Send completion report**

Per `airuleset/modules/core/completion-report.md`. Audits at TOP, Goal/What changed/PR/URL at BOTTOM. Include both auto-closed issues (#69 + #70).

---

## Self-Review Checklist

Run before declaring the plan complete.

**Spec coverage:**
- [x] Spec "SQL change — step 5" → Task 2 Steps 1-2.
- [x] Spec "SQL change — step 7b" → Task 2 Steps 3-4.
- [x] Spec "Test changes" → Task 2 Steps 5-11.
- [x] Spec "Frozen-rule justification" → Task 2 commit message references the spec.
- [x] Spec "Out of scope" — plan touches only the listed files.
- [x] Spec "Validation gates" → Task 3 Steps 3-5.

**Type/identifier consistency:**
- [x] `staff_id`, `alice_user`, `bob_user`, `charlie_user`, `bob_card_id`, `template_id` — all match existing test fixture variable names.
- [x] `orphan_txn_id`, `orphan_pb_id`, `deleted_user_id`, `bob_pb_user`, `orphan_txn_user`, `orphan_pb_user` — new locals; consistent across Steps 6, 7, 9, 11.
- [x] `RETURNING id` — works on sqlx 0.8 + SQLite; same idiom used at lines 1467-1479 of the existing test.

**No placeholders:**
- [x] All steps include concrete code or commands.
- [x] All commit messages are spelled out, not "(write a good message)".
- [x] All grep patterns are exact strings.

**Mutation testing:** Spec section "Mutation-pressure considerations" enumerates COALESCE→NULL, COALESCE→c.user_id, LEFT JOIN→JOIN, EXISTS-extension removal. The test changes in Task 2 kill the first three by construction; the fourth has a defer-to-CI escape clause (only act if cargo-mutants reports a survivor).
