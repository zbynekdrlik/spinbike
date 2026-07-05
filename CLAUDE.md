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

Load the skill for the area you're working in — each contains the full HOW-TO:

| Area | Skill | When to load |
|---|---|---|
| DB migrations / queries | `.claude/skills/db-migrations/SKILL.md` | Any migration, backfill, visit count, or prod-DB validation |
| CI / deploy workflows | `.claude/skills/ci-deploy/SKILL.md` | Writing CI YAML, subagent prompts, or staging commits |
| Domain / design | `.claude/skills/domain/SKILL.md` | Any design, spec, brainstorm, or feature touching roles/users/cards |
| Door unlock / eWeLink / Sonoff | `.claude/skills/ewelink-door/SKILL.md` | Any work on `ewelink/*`, `routes/door.rs`, `/api/door/*`, or door credentials |
| Frontend PWA / JS interop | `.claude/skills/frontend-pwa/SKILL.md` | Untyped browser API access (`js_sys::Reflect`), UA sniffing, manifest icons |
| Auth / client onboarding | `.claude/skills/auth-onboarding/SKILL.md` | Magic-link tokens (`login_tokens`), `/api/auth/*`, `/welcome`, login/invite UI, register removal |

## Project-wide always-apply rules

**Prod and dev run on the same machine.** Both `/opt/spinbike/prod/` and `/opt/spinbike/dev/` are LOCAL. Never ask the user to SSH or paste `systemctl`/`sqlite3`/`journalctl` output — run those commands directly via Bash.

**Git staging: never `git add -A` or `git add .`** — untracked Playwright YAMLs and debug PNGs accumulate at the root. Always use explicit paths or `git add -u`.
