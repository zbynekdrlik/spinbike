//! Service-kind constants shared between the server (SQL queries, autocharger)
//! and the UI (visit-row predicates, color logic).
//!
//! Class-visit services (Fitness, Spinning) are identified by their stable
//! `services.kind` value, NOT by their (admin-editable) `name_en`/`name_sk`
//! display strings. Several pieces of business logic key off "is this a
//! class visit": the staff dashboard's "Log Visit" buttons, the visit-row
//! color split between Fitness (solid blue) and Spinning (soft blue), the
//! 4-hour Spinning auto-charger, and the attendance KPI on reports.
//!
//! # History (#329)
//!
//! Before #329, this module matched on `name_en` string literals
//! ("Fitness"/"Spinning"). `services.kind` already existed and was used for
//! exactly this purpose for the OTHER service kinds (`monthly_pass`), but
//! only Fitness had a distinct class-visit kind (`single_entry`, added by
//! migration V16 for the door self-entry feature) — Spinning still shared
//! `kind='generic'` with unrelated sellable items (Refreshments,
//! Supplements, Card activation fee). Migration V27 gave Spinning its own
//! kind (`group_class`), which is what let every identification site here
//! switch from `name_en` to `kind`. `kind` is immutable after a service is
//! created (`routes/admin.rs::UpdateServiceRequest` has no `kind` field),
//! so — unlike `name_en` — renaming a service via the admin Services tab
//! can never desync it from these constants.
//!
//! `single_entry` and `group_class` are deliberately TWO DISTINCT values,
//! not one shared `class_visit` flag: `routes/door.rs`'s self-entry lookup
//! (`WHERE kind = 'single_entry' ... LIMIT 1`) and `jobs/charger.rs`'s
//! Spinning-price lookup (`fetch_one`) each need to resolve exactly ONE row
//! by kind alone — a shared value would make both queries ambiguous.

/// Stable `services.kind` value identifying the Fitness (door/walk-in
/// single-visit) service. Set by migration V16.
pub const FITNESS_KIND: &str = "single_entry";

/// Stable `services.kind` value identifying the Spinning (scheduled group
/// class) service. Set by migration V27.
pub const SPINNING_KIND: &str = "group_class";

/// All class-visit service `kind` values. Used by SQL `IN` clauses,
/// `is_class_visit()` predicates, and dropdown filters.
pub const CLASS_VISIT_KINDS: &[&str] = &[FITNESS_KIND, SPINNING_KIND];

/// The `<column> IN (SELECT id FROM services WHERE kind IN (...))` SQL
/// fragment that identifies a class-visit service (Fitness or Spinning) by
/// the stable `kind` column — built with exactly `CLASS_VISIT_KINDS.len()`
/// anonymous `?` placeholders, so it stays correct with no source edit if a
/// THIRD class-visit kind is ever added. `column` is whatever the caller's
/// query needs (`"service_id"`, `"t.service_id"`, ...).
///
/// Callers MUST `.bind()` each of `CLASS_VISIT_KINDS`, in that exact order,
/// ONCE PER OCCURRENCE of this fragment in their SQL text — unlike
/// SQLite's numbered `?N` parameters, anonymous `?` placeholders never
/// share a bound value across occurrences.
///
/// Extracted (#339) from independently hand-rolled copies of the same
/// placeholder-building + subquery text in `routes/payments.rs`,
/// `routes/users.rs`, and `db/users.rs`. `db/reports.rs`'s KPI query uses
/// SQLite's numbered `?N` params instead (so `?3` can be referenced twice
/// in the SQL text while bound only once) and is deliberately NOT routed
/// through this helper — see `db::reports::kpi_between`'s own doc comment.
pub fn class_visit_filter_sql(column: &str) -> String {
    let placeholders = std::iter::repeat_n("?", CLASS_VISIT_KINDS.len())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{column} IN (SELECT id FROM services WHERE kind IN ({placeholders}))")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_visit_filter_sql_builds_one_placeholder_per_kind() {
        let sql = class_visit_filter_sql("service_id");
        assert_eq!(
            sql,
            "service_id IN (SELECT id FROM services WHERE kind IN (?, ?))"
        );
        // Anti-drift: kills a mutant that hardcodes "?, ?" instead of
        // deriving the placeholder count from CLASS_VISIT_KINDS.len().
        assert_eq!(
            sql.matches('?').count(),
            CLASS_VISIT_KINDS.len(),
            "placeholder count must track CLASS_VISIT_KINDS.len()"
        );
    }

    #[test]
    fn class_visit_filter_sql_uses_the_given_column() {
        assert!(class_visit_filter_sql("t.service_id").starts_with("t.service_id IN"));
    }
}
