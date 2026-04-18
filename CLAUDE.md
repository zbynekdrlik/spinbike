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
