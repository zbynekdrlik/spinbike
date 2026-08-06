---
paths:
  - "crates/spinbike-core/src/errors.rs"
  - "crates/spinbike-server/src/routes/**"
  - "spinbike-ui/src/i18n.rs"
---

# Adding a new `ErrorCode` variant

`spinbike_core::errors::ErrorCode` is shared by the server (`ApiError`) and the
client (`spinbike-ui`), and is consumed by **two independent exhaustive
matches** that must BOTH be updated for a new variant to compile — and only
ONE of them is covered by the workspace's local pre-push gate (#268).

1. **`crates/spinbike-core/src/errors.rs`** — add the variant, its
   `message()` arm, and its entry in the `#[cfg(test)] const ALL` table
   (wire string + message — pins the `#[serde(rename_all = "snake_case")]`
   derivation so a future rename is a deliberate wire-format change, not an
   accident). `cargo check --workspace` / `cargo clippy --workspace` catch a
   missed `message()` arm immediately (exhaustive match, compile error).

2. **`spinbike-ui/src/i18n.rs`'s `error_code_key()`** — ALSO an exhaustive
   match over `ErrorCode` (no `_ =>` wildcard), used to decide whether a raw
   server error code gets a dedicated localized banner string or falls back
   to the server's raw English message. **`cargo check --workspace` does
   NOT catch a missing arm here** — `spinbike-ui` has its own top-level
   `[workspace]` in its `Cargo.toml` and is deliberately excluded from the
   root workspace (see project `CLAUDE.md`'s Architecture section), so
   `--workspace` never type-checks it. A new `ErrorCode` variant that isn't
   added to this match will ONLY fail in CI's `Build WASM (UI)` /
   `Test (UI)` job, costing a full CI cycle if missed locally. There is no
   local Tier-0-sanctioned way to catch this ahead of push (`cargo check`
   on `spinbike-ui` isn't in the allowed local command list, and
   `cargo build`/`trunk build` are banned) — the safest habit is to grep
   for `error_code_key` and manually walk the match by eye whenever you add
   an `ErrorCode` variant, deciding: does this code need its own
   `err_*` i18n key (customer-facing, no dedicated handler builds its own
   copy), or does it belong in the trailing `| ... => None` disjunction
   (staff/admin-only, or — like `SessionInvalid`, #268 — intercepted
   upstream before it ever reaches this lookup)? Either way it needs an
   explicit arm; there is no default case.

3. `cargo fmt --all --check` run from EITHER the repo root OR
   `spinbike-ui/` checks the SAME combined tree (both crates) — running it
   twice per the CLAUDE.md instruction is about covering `spinbike-ui`'s
   OWN `[workspace]` boundary for fmt, not two disjoint file sets; a
   formatting issue anywhere in `crates/` will show up from either
   directory.

## `sqlx::query_as` tuple size — clippy `type_complexity`

A `query_as::<_, (A, B, C, D, E, F, G)>` tuple of ~6+ elements trips
`clippy::type_complexity` under this project's `-D warnings` CI gate (and
under a local `cargo clippy --workspace --all-targets -- -D warnings` run,
which IS Tier-0-sanctioned and should be run before push on any multi-column
query change). Fix: a local `type FooRow = (A, B, C, ...);` alias right
above the query — no behavior change, same tuple, just named. Cheaper than
discovering it in CI.
