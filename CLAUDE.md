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
- Any new day-boundary / local-time computation in a job or route → `.claude/rules/bratislava-tz.md` (auto-loads on `crates/spinbike-server/src/util.rs`, `crates/spinbike-server/src/jobs/**`, `crates/spinbike-server/src/routes/**` — never `chrono::Local`, always the shared `crate::util` helpers; the recurring #205/#222/#327/#330 bug class + the source-invariant regression-test pattern for it)
- Predaj permanentky za 0 € (a služba s nulovou cenou) → `.claude/rules/zero-price-is-deliberate.md` (auto-loads on `routes/payments.rs|users.rs|admin.rs`, `dashboard/action_form.rs` — ZÁMER majiteľa pre vyrovnanie v naturáliách, nikdy to neopravovať ako chybu, #342)
- Any new money-mutating write (`users.credit` / `transactions.amount`) → `.claude/rules/money-rounding.md` (auto-loads on `db/users.rs`, `routes/payments.rs|users.rs|door.rs|admin.rs|charger.rs` — round ONCE and reuse everywhere, check prod for real drift before assuming a backfill is needed, #325/#326)
- Adding a new `web_sys::` API surface to `spinbike-ui` → `.claude/rules/web-sys-features.md` (auto-loads on `spinbike-ui/Cargo.toml` — verify the Cargo feature gate without compiling, since local `cargo check` is Tier-0-banned, #320)
- Touching `Sheet`'s Tab-cycle focus trap (`trap_tab`/`refocus_if_orphaned` in `sheet.rs`) or any `Sheet` consumer's close/cancel handler → `.claude/rules/sheet-focus-trap.md` (auto-loads on `sheet.rs`, `nav.rs`, the dashboard sheets, `deleted_email_conflict.rs`, `calendar_picker.rs` — the container-itself-is-a-focus-state gotcha + the non-uniform close-defer pattern, #334)
- Identifying a class-visit service (Fitness/Spinning), or adding a new `services.kind` value → `.claude/rules/service-kind.md` (auto-loads on `crates/spinbike-core/src/services.rs`, `jobs/charger.rs`, `routes/payments.rs|users.rs|admin.rs`, `db/users.rs|reports.rs`, `dashboard/mod.rs|action_form.rs` — identify by the stable `kind` column, never admin-editable `name_en`/`name_sk`; a new kind needs its own i18n badge key or it renders "???", #186/#329)
- Creating a synthetic account on PROD (mail testing, auth verification) → `.claude/rules/prod-test-data.md` (auto-loads on `crates/spinbike-server/src/mail/**`, `crates/spinbike-server/src/auth/**`, `routes/auth.rs`, `e2e/tests/auth*.spec.ts` — clean up same-ticket, check `deleted_at IS NULL` filters first, prod-DELETE approval+backup, #333)
- Any admin/staff write handler or `api::*` call on a mutating endpoint → `.claude/rules/admin-ui-error-handling.md` (auto-loads on `spinbike-ui/src/pages/admin.rs|staff_dashboard.rs|dashboard/action_form.rs`, `spinbike-ui/src/api.rs`, `crates/spinbike-server/src/routes/admin.rs` — never `unwrap_or` a parse failure into a value, never `let _ =` a write, `post_no_content` for 204 routes, validate money at the server boundary)
- Touching `RequestId`/`util.rs`, or any closure-based helper shared by more than one Leptos event handler / reactive view child in `action_form.rs`/`users_by_movement.rs` → `.claude/rules/leptos-closure-captures.md` (auto-loads on those files — Leptos 0.8 CSR still needs `Send + Sync` on reactive-child closures, and clone-before-move needs its own wrapping block per consumer, not a chained same-scope shadow, #344 wasm32 follow-up)
- Changing the eWeLink `sequence` id, or diagnosing a door press that opens but records nothing → `.claude/rules/ewelink-ack.md` (auto-loads on `crates/spinbike-server/src/ewelink/**`, `routes/door.rs` — the ack is matched by exact string equality and must stay inside 2^53; uniqueness-only tests do not pin it, #353/#323)
- Monthly-pass auto-renewal (per-user flag + daily contiguous job; door/charger NO LONGER renew) → `.claude/rules/pass-auto-renewal.md` (auto-loads on `jobs/pass_renewal.rs`, `db/users.rs`, `routes/door.rs`, `jobs/charger.rs`, `routes/users.rs`, `tests/door_route.rs`, `spinbike-ui/.../edit_info_form.rs` — the `auto_renew_pass` flag (V28), the daily `jobs::pass_renewal` job, `renew_expired_pass` + `renewal_valid_until` continuity, staff_id-NULL distinguisher, idempotency, debit-into-negative, push notification, #374 replacing #365/#372)
- Reaching prod/dev at all — DB, jobs, routes, auth or the CI deploy workflow → `.claude/rules/vps-access.md` (auto-loads on `crates/spinbike-server/src/{db,jobs,routes,auth}/**`, `.github/workflows/ci.yml` — ssh recipe + path/unit/port table since prod+dev moved off dev1 onto the SpinBike VPS; also the hand-minted-JWT claim shapes, Cloudflare's 1010 block on scripted calls, and the host-move ordering/proof checklist, #350)

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

**Prod and dev both run on the SpinBike Hetzner VPS** (`spinbike`, cx23, Ubuntu 24.04, x86_64, `167.233.245.147`, project `spinbike` in Hetzner Cloud) — migrated off dev1 on 2026-08-12 (#350). Both `/opt/spinbike/prod/` and `/opt/spinbike/dev/` live there, together with the `spinbike` Cloudflare tunnel, the `spinbike-deploy` Actions runner, and the nightly prod→dev sync timer. Reach them with `ssh -i ~/.ssh/spinbike_vps root@167.233.245.147 '<cmd>'` and run `systemctl`/`sqlite3`/`journalctl` yourself over that — never ask the user to SSH or paste output. Ingress is the Cloudflare tunnel, not DNS: all four hostnames (`spinbike.newlevel.media`, `spinbike-dev.newlevel.media`, `spinbike.sk`, `www.spinbike.sk`) follow whichever box runs `spinbike-tunnel.service`, so a host move needs no DNS change — but the tunnel must run in exactly ONE place, and eWeLink likewise allows only ONE WebSocket session per account, so two live prod instances fight and break door unlock.

**Git staging: never `git add -A` or `git add .`** — untracked Playwright YAMLs and debug PNGs accumulate at the root. Always use explicit paths or `git add -u`.

**`[profile.release]` is `panic = "unwind"` (#172, not `"abort"`), and the router is wrapped in a `CatchPanicLayer`** (`build_router` in `crates/spinbike-server/src/lib.rs`) so a panicking HTTP handler returns 500 for that one request instead of aborting the whole single-instance process. Consequence: any `std::sync::Mutex` a handler locks with `.lock().expect("... poisoned")` can now actually become poisoned (a panic while holding the guard used to just abort the process; now it unwinds and leaves the mutex marked poisoned), and every LATER `.expect()` on that same mutex then panics too — caught by `CatchPanicLayer`, but as a *permanent* 500 on that endpoint for every user until restart. **Any new (or existing) shared-state `Mutex` a handler locks MUST recover instead of `.expect()`:** `.lock().unwrap_or_else(|poisoned| poisoned.into_inner())`, not `.lock().expect("... poisoned")` — see `door_rate_limit`/`login_link_rate_limit` in `door.rs`/`auth.rs` for the pattern. Only skip this for state where a poisoned/torn value is genuinely unsafe to keep using (none of the current in-memory rate-limit counters qualify).
