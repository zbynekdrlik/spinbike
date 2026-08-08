# SpinBike PWA

Spin bike class booking and prepaid card management system. Replaces a legacy VB6 + MS Access app.

## Architecture

Monolith: Axum 0.8 server + Leptos 0.7 CSR frontend compiled to WASM via Trunk, embedded in server binary via rust-embed.

```
crates/spinbike-core/    # Shared types (WASM-safe, no tokio)
crates/spinbike-server/  # Axum server + SQLite + auth + API
spinbike-ui/             # Leptos frontend (excluded from workspace)
```

## Building

```bash
# Server (includes all workspace crates)
cargo check
cargo test -p spinbike-core -p spinbike-server

# Frontend (separate workspace, WASM target)
cd spinbike-ui && trunk build

# Full release build
cd spinbike-ui && trunk build --release
cd .. && cargo build --release --bin spinbike-server
```

**IMPORTANT:** The server crate uses `rust-embed` pointing at `spinbike-ui/dist/`. For lint/clippy/test to work without building WASM first, create a placeholder:
```bash
mkdir -p spinbike-ui/dist && echo "placeholder" > spinbike-ui/dist/index.html
```

## Running Locally

```bash
# With defaults (port 8080, spinbike.db, dev JWT secret)
cargo run --bin spinbike-server

# With custom config
PORT=3000 DATABASE_PATH=./data.db JWT_SECRET=your-secret cargo run --bin spinbike-server
```

## Pre-Push Checks

```bash
cargo fmt --all --check
```

Do NOT run `cargo clippy`, `cargo test`, or `cargo build` locally unless debugging — these create large build artifacts. Clippy and tests run on CI.

## Version Management

Single source of truth: `VERSION` file. Sync to all Cargo.toml files:
```bash
scripts/sync-version.sh
```

Bump VERSION before any new work on dev. CI checks that dev version > main version on PRs.

## Branch Workflow

- `main` — production, deploy target
- `dev` — all development work
- PRs from `dev` to `main` only, merge commits only

## Database

SQLite via sqlx. Migrations in `crates/spinbike-server/src/db/migrations.rs`. Auto-applied on server start.

## Legacy Data Migration

```bash
cargo run --bin migrate-legacy -- --mdb-path path/to/db.mdb --output spinbike.db
```

Requires `mdbtools` installed on the system.

## Design Docs

- Spec: `docs/superpowers/specs/2026-04-09-spinbike-pwa-design.md`
- Plan: `docs/superpowers/plans/2026-04-09-spinbike-pwa.md`

## Playbook router

Load the skill for the area you're working in — each contains the full HOW-TO.
Path-scoped rules under `.claude/rules/` load themselves when you touch a matching file:

- version label / `.app-version` styling → `.claude/rules/ui-version-label.md` (auto-loads on `spinbike-ui/style.css`, `e2e/tests/version-display.spec.ts`)
- adding a new `ErrorCode` variant → `.claude/rules/error-codes.md` (auto-loads on `crates/spinbike-core/src/errors.rs`, `crates/spinbike-server/src/routes/**`, `spinbike-ui/src/i18n.rs`)
- `AuthUser`-driven route acting on the caller's own account → `.claude/rules/session-invalidation.md` (auto-loads on `crates/spinbike-server/src/routes/**` — the blocked/deleted-user 401 contract, #268/#274/#277)
- writing/editing an E2E fixture generator or pagination loop → `.claude/rules/e2e-fixtures.md` (auto-loads on `e2e/tests/**` — dataset-growth-safe fixtures + rank-based pagination, #288/#39)
- card search ranking or the search-result row's digit display → `.claude/rules/search-ranking.md` (auto-loads on `crates/spinbike-server/src/db/users.rs`, `spinbike-ui/src/pages/dashboard/mod.rs`, `e2e/tests/dashboard.spec.ts`, `e2e/tests/negative-balance.spec.ts` — tail-match ranking + row-vs-panel digit display, #290/#39)
- `setupConsoleCheck`'s `allow4xxFor` opt-in filter → `.claude/rules/e2e-console-check.md` (auto-loads on `e2e/tests/helpers.ts`, `e2e/tests/console-check-4xx-scoping.spec.ts` — match `msg.location().url`, never `msg.text()`, #278)
- ordering a `transactions` query by `created_at` → `.claude/rules/transaction-ordering.md` (auto-loads on `crates/spinbike-server/src/db/transactions.rs`, `crates/spinbike-server/src/routes/my_balance.rs`, `crates/spinbike-server/src/routes/payments.rs` — same-second ties need an `id DESC` tiebreaker, #291)
- Web-Push notifications (daily job, VAPID key, anti-spam ledger) → `.claude/rules/push-notifications.md` (auto-loads on `crates/spinbike-server/src/jobs/notifications.rs`, `crates/spinbike-server/src/push.rs`, `crates/spinbike-server/src/routes/push.rs`, `spinbike-ui/sw.js` — #264)
- Scheduling a new daily (or longer) background job → `.claude/rules/daily-job-scheduling.md` (auto-loads on `crates/spinbike-server/src/bin/server.rs`, `crates/spinbike-server/src/jobs/**` — wall-clock alignment, never `tokio::time::interval(86400s)`, #264/#297)

| Area | Skill | When to load |
|---|---|---|
| DB migrations / queries | `.claude/skills/db-migrations/SKILL.md` | Any migration, backfill, visit count, or prod-DB validation |
| CI / deploy workflows | `.claude/skills/ci-deploy/SKILL.md` | Writing CI YAML, subagent prompts, staging commits, or monitoring a CI run (has the foreground-poll sandbox-block gotcha) |
| Domain / design | `.claude/skills/domain/SKILL.md` | Any design, spec, brainstorm, or feature touching roles/users/cards |
| Door unlock / eWeLink / Sonoff | `.claude/skills/ewelink-door/SKILL.md` | Any work on `ewelink/*`, `routes/door.rs`, `/api/door/*`, or door credentials |
| Frontend PWA / JS interop / post-deploy DOM checks | `.claude/skills/frontend-pwa/SKILL.md` | Untyped browser API access (`js_sys::Reflect`), UA sniffing, manifest icons, reading a version/feature off the live DOM after deploy (service-worker cache gotcha) |
| Auth / client onboarding | `.claude/skills/auth-onboarding/SKILL.md` | Magic-link tokens (`login_tokens`), `/api/auth/*`, `/welcome`, login/invite UI, register removal |
| Prod functional verification | `.claude/skills/prod-verification/SKILL.md` | Post-deploy verification of a customer-facing (`/my/*`) feature on live prod |
| E2E test writing | `.claude/skills/e2e-testing/SKILL.md` | Writing/editing `e2e/tests/*.spec.ts`, or right after adding a new 4xx/409 validation guard to any endpoint (audit existing specs for silent collisions before pushing) |

## Project-wide always-apply rules

**Prod and dev run on the same machine.** Both `/opt/spinbike/prod/` and `/opt/spinbike/dev/` are LOCAL. Never ask the user to SSH or paste `systemctl`/`sqlite3`/`journalctl` output — run those commands directly via Bash.

**Git staging: never `git add -A` or `git add .`** — untracked Playwright YAMLs and debug PNGs accumulate at the root. Always use explicit paths or `git add -u`.

**`[profile.release]` is `panic = "unwind"` (#172, not `"abort"`), and the router is wrapped in a `CatchPanicLayer`** (`build_router` in `crates/spinbike-server/src/lib.rs`) so a panicking HTTP handler returns 500 for that one request instead of aborting the whole single-instance process. Consequence: any `std::sync::Mutex` a handler locks with `.lock().expect("... poisoned")` can now actually become poisoned (a panic while holding the guard used to just abort the process; now it unwinds and leaves the mutex marked poisoned), and every LATER `.expect()` on that same mutex then panics too — caught by `CatchPanicLayer`, but as a *permanent* 500 on that endpoint for every user until restart. **Any new (or existing) shared-state `Mutex` a handler locks MUST recover instead of `.expect()`:** `.lock().unwrap_or_else(|poisoned| poisoned.into_inner())`, not `.lock().expect("... poisoned")` — see `door_rate_limit`/`login_link_rate_limit` in `door.rs`/`auth.rs` for the pattern. Only skip this for state where a poisoned/torn value is genuinely unsafe to keep using (none of the current in-memory rate-limit counters qualify).
