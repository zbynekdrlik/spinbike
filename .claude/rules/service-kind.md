---
paths:
  - "crates/spinbike-core/src/services.rs"
  - "crates/spinbike-server/src/jobs/charger.rs"
  - "crates/spinbike-server/src/routes/payments.rs"
  - "crates/spinbike-server/src/routes/users.rs"
  - "crates/spinbike-server/src/db/users.rs"
  - "crates/spinbike-server/src/db/reports.rs"
  - "crates/spinbike-server/src/routes/admin.rs"
  - "spinbike-ui/src/pages/dashboard/mod.rs"
  - "spinbike-ui/src/pages/dashboard/action_form.rs"
---

# Class-visit services are identified by `services.kind`, never `name_en`/`name_sk` (#329)

Before #329, "is this service a class visit (Fitness/Spinning)?" and "which
service IS Spinning specifically?" were both answered by matching the
compile-time constants `FITNESS_NAME_EN`/`SPINNING_NAME_EN` against the DB
row's admin-editable `name_en`. Renaming a service via the admin Services
tab (`PUT /api/admin/services/{id}` — unguarded, no special-casing for these
rows) silently desynced every one of those sites: visit classification, the
attendance KPI, and the 4-hour Spinning auto-charger (which `fetch_one`s the
price by name — a miss errors the whole tick, silently stopping every
booking in that batch from being charged).

**The fix: `services.kind` is the stable, rename-proof identifier.** `kind`
is immutable after creation (`routes/admin.rs::UpdateServiceRequest` has no
`kind` field, and `CreateServiceRequest` only accepts `"generic"`/
`"monthly_pass"` via the API — `single_entry`/`group_class` can only ever be
set by a migration). Use `spinbike_core::services`:

- `FITNESS_KIND` = `"single_entry"` (set by migration V16 — this is the
  SAME row as the seeded 'Fitness' service, not a separate one; also the
  row `routes/door.rs` charges for self-entry).
- `SPINNING_KIND` = `"group_class"` (set by migration V27).
- `CLASS_VISIT_KINDS` = `[FITNESS_KIND, SPINNING_KIND]` — the union, for
  "is this a class visit" `IN` filters.

**Why two DISTINCT kind values, not one shared `class_visit` flag** (a
design speculated in `services.rs`'s own pre-#329 doc comment, and
deliberately rejected): `routes/door.rs`'s self-entry lookup (`WHERE kind =
'single_entry' ... LIMIT 1`) and `jobs/charger.rs`'s Spinning-price lookup
(`fetch_one`) each need to resolve exactly ONE row by kind alone — sharing a
value would make both queries ambiguous the moment both services carried it.

**When adding a NEW identification site for "is this a class visit" or "find
the Spinning/Fitness service": use `kind`, never `name_en`/`name_sk`.** A
name-based site is the exact regression class #329 exists to prevent — it
compiles and passes every test against the current seed data, then silently
breaks the moment the owner renames a service. Grep
`CLASS_VISIT_KINDS`/`FITNESS_KIND`/`SPINNING_KIND` for the full current call
list before adding a new one, so it's added to the SAME union rather than
duplicating an inline literal.

**A NEW class-visit-like service kind needs its own migration + a matching
i18n badge key.** `spinbike-ui/src/i18n.rs`'s admin Services-tab badge
renders `service_kind_<kind>` — a kind value with no matching key silently
shows `"???"` (the exact bug #186 fixed once for `single_entry`, and #329
had to fix again for `group_class`). Add the key in the SAME PR that adds
the kind value.
