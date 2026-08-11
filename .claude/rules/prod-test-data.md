---
paths:
  - "crates/spinbike-server/src/mail/**"
  - "crates/spinbike-server/src/auth/**"
  - "crates/spinbike-server/src/routes/auth.rs"
  - "e2e/tests/auth*.spec.ts"
---

# Synthetic accounts touching PROD — clean up in the SAME ticket (#333)

Any time work in this area (magic-link/email flow, auth routes, or a manual
prod poke while verifying either) creates a row in the **prod** `users`
table that is not a real customer — a `@srv1.mail-tester.com`-style probe
account, a one-off you inserted to check a login/email flow — treat cleanup
as part of THAT ticket, not a future sweep. `#333` was exactly this: two
July test accounts (ids 571/572) verifying Resend + magic-link delivery
were left in prod and needed a whole separate cleanup ticket weeks later.

**Checklist for creating a synthetic row on prod:**

1. Use a greppable, unmistakable identifier (`@srv1.mail-tester.com`,
   `autopilot-verify-<N>@spinbike.local`, etc. — see the `prod-verification`
   skill for the full synthetic-customer recipe used during verification).
2. Before ending the ticket, delete it (or, for the throwaway-verification
   recipe, follow that skill's own cleanup step) — do not rely on a soft
   delete alone to make it "invisible enough".

**Before assuming a stale prod row is user-visible, check whether the read
path already filters it out.** `#333`'s premise ("two test accounts appear
in the client list") was wrong: `crates/spinbike-server/src/db/users.rs`'s
three client-list queries all filter `WHERE ... deleted_at IS NULL`, and
both rows already had `deleted_at` set from an earlier partial cleanup — so
they were already invisible in the app. Verifying that first is what turned
this from "fix the visible bug" into "just finish the hard delete", i.e. it
changed what the actual fix needed to be. Don't skip straight to a code fix
before confirming a report against a filtered read path is still true.

## Deleting rows from the PROD database — approval + evidence, every time

- Deleting from `/opt/spinbike/prod/spinbike.db` is irreversible. Get the
  owner's explicit go-ahead before running the `DELETE`
  (`no-destructive-remote-actions.md` — DB `DELETE`/`DROP` always needs
  approval, no exception for "it's just test rows").
- Take a `.backup` of the prod DB file immediately before the `DELETE`.
- After the `DELETE`, paste a `SELECT COUNT(*) ...` (or equivalent) proving
  zero rows remain, as evidence in the ticket/PR — not just "done".
- Confirm the row has zero linked FKs (`transactions`, `login_tokens`,
  `push_subscriptions`, ...) before deleting, same as any other prod
  `DELETE` — an orphaned reference is worse than a leftover row.

## Prod and dev both run LOCALLY on this machine

`/opt/spinbike/prod/spinbike.db` (service `spinbike.service`, port 8080)
and the dev instance (port 8081) are both on THIS box — never SSH, never
ask the user to paste `systemctl`/`sqlite3`/`journalctl` output. Run the
commands yourself via Bash (see project `CLAUDE.md`'s always-apply rules).
