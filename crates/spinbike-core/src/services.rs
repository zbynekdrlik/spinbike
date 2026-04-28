//! Service-name constants shared between the server (SQL queries, autocharger)
//! and the UI (visit-row predicates, color logic).
//!
//! The legacy DB seeds class services by their English name (`name_en`), and
//! several pieces of business logic key off those names: the staff dashboard's
//! "Log Visit" buttons, the visit-row color split between Fitness (solid blue)
//! and Spinning (soft blue), the 4-hour Spinning auto-charger, and the
//! attendance KPI on reports.
//!
//! Without this module those names lived as 5 independent string literals
//! across two crates. Renaming a service in admin would silently miscount or
//! mis-route. The constants here are the single source of truth — change
//! either string here and every Rust call site picks it up.
//!
//! # Limitation
//!
//! These are compile-time constants. They DO NOT prevent the runtime DB row's
//! `name_en` from drifting (e.g., admin renames "Spinning" via the admin
//! services CRUD). The proper fix for that is a `kind = 'class_visit'` flag
//! on the services table — see the doc comment on
//! `spinbike_ui::pages::dashboard::ServiceInfo::is_class_visit`. Adding that
//! flag is a schema migration and out of scope for this constants extraction.

/// English name of the Fitness class service. Matches the `services.name_en`
/// value seeded by `crates/spinbike-server/src/db/migrations.rs`.
pub const FITNESS_NAME_EN: &str = "Fitness";

/// English name of the Spinning class service.
pub const SPINNING_NAME_EN: &str = "Spinning";

/// All class-visit service `name_en` values. Used by SQL `IN` clauses,
/// `is_class_visit()` predicates, and dropdown filters.
pub const CLASS_VISIT_NAMES_EN: &[&str] = &[FITNESS_NAME_EN, SPINNING_NAME_EN];
