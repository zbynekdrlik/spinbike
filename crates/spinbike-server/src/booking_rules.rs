//! Pure, clock-injected spinning booking rules (#382 — owner ROZHODNUTÉ
//! 2026-10-12, spec `docs/superpowers/specs/2026-10-12-spinning-recurring-bookings-design.md`).
//!
//! Every function takes the gym-local (Europe/Bratislava) `now` as an
//! argument instead of reading a clock, so the boundaries are unit-tested
//! exactly (08:59:59 vs 09:00:00, a class that started a second ago). The
//! callers obtain `now` from `crate::util::now_bratislava()` — never
//! `chrono::Local` (`.claude/rules/bratislava-tz.md`).
//!
//! - **R1 binding charge:** a booking is charged once `now >= 09:00` on the
//!   class day ([`charge_cutoff`] / [`is_charge_due`]). The charger's SQL
//!   uses [`CHARGE_CUTOFF_TIME`] for the same boundary.
//! - **R2 customer cancel deadline:** a customer may cancel their own
//!   booking only while `now < 09:00` on the class day and the booking is
//!   not yet charged ([`customer_may_cancel`]). Staff may always cancel.
//! - **R4 no past bookings:** a class can be booked only while it has not
//!   started yet ([`has_started`]).

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

/// Hour of the class day at which a booking becomes binding and is charged.
pub const CHARGE_CUTOFF_HOUR: u32 = 9;

/// [`CHARGE_CUTOFF_HOUR`] as the `HH:MM:SS` literal the charger's SQL
/// appends to a booking's bare `YYYY-MM-DD` date. Kept next to the hour so
/// the two can never drift (pinned by a unit test below).
pub const CHARGE_CUTOFF_TIME: &str = "09:00:00";

/// The binding moment of a class day: 09:00 gym-local on `date`.
pub fn charge_cutoff(date: NaiveDate) -> NaiveDateTime {
    date.and_time(
        NaiveTime::from_hms_opt(CHARGE_CUTOFF_HOUR, 0, 0).expect("09:00:00 is a valid time"),
    )
}

/// R1: whether a booking on `date` is due for its binding charge at `now`.
pub fn is_charge_due(date: NaiveDate, now: NaiveDateTime) -> bool {
    now >= charge_cutoff(date)
}

/// R2: whether the CUSTOMER may still cancel their own booking on `date`.
/// `false` once the 09:00 cutoff passed or the booking was already charged
/// (a charged booking is binding even if a clock skew made it look early).
pub fn customer_may_cancel(date: NaiveDate, charged: bool, now: NaiveDateTime) -> bool {
    !charged && !is_charge_due(date, now)
}

/// Parse a class occurrence's start (`date` + the template's `HH:MM`
/// `start_time`). `None` for a malformed start time.
pub fn class_start(date: NaiveDate, start_time: &str) -> Option<NaiveDateTime> {
    NaiveTime::parse_from_str(start_time, "%H:%M")
        .ok()
        .map(|t| date.and_time(t))
}

/// R4: whether the class occurrence already started at `now` (start
/// inclusive — a class starting this very minute can no longer be booked).
/// A malformed start time counts as started: refusing a booking is the safe
/// side of an unparseable schedule.
pub fn has_started(date: NaiveDate, start_time: &str, now: NaiveDateTime) -> bool {
    match class_start(date, start_time) {
        Some(start) => now >= start,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn cutoff_constants_agree() {
        assert_eq!(
            charge_cutoff(d("2026-10-12"))
                .format("%H:%M:%S")
                .to_string(),
            CHARGE_CUTOFF_TIME
        );
    }

    #[test]
    fn charge_is_due_from_0900_on_the_class_day() {
        let day = d("2026-10-12");
        assert!(!is_charge_due(day, dt("2026-10-11 23:59:59")));
        assert!(!is_charge_due(day, dt("2026-10-12 08:59:59")));
        assert!(is_charge_due(day, dt("2026-10-12 09:00:00")));
        assert!(is_charge_due(day, dt("2026-10-12 18:30:00")));
        assert!(is_charge_due(day, dt("2026-10-13 00:00:00")));
    }

    #[test]
    fn customer_may_cancel_until_0900_on_the_class_day() {
        let day = d("2026-10-12");
        // A week ahead is fine.
        assert!(customer_may_cancel(day, false, dt("2026-10-05 20:00:00")));
        assert!(customer_may_cancel(day, false, dt("2026-10-12 08:59:59")));
        assert!(!customer_may_cancel(day, false, dt("2026-10-12 09:00:00")));
        assert!(!customer_may_cancel(day, false, dt("2026-10-13 07:00:00")));
    }

    #[test]
    fn customer_may_never_cancel_a_charged_booking() {
        assert!(!customer_may_cancel(
            d("2026-10-12"),
            true,
            dt("2026-10-05 20:00:00")
        ));
    }

    #[test]
    fn has_started_is_start_inclusive() {
        let day = d("2026-10-12");
        assert!(!has_started(day, "18:30", dt("2026-10-12 18:29:59")));
        assert!(has_started(day, "18:30", dt("2026-10-12 18:30:00")));
        assert!(has_started(day, "18:30", dt("2026-10-13 08:00:00")));
        // Yesterday's class is past; tomorrow's is not.
        assert!(has_started(
            d("2026-10-11"),
            "18:00",
            dt("2026-10-12 07:00:00")
        ));
        assert!(!has_started(
            d("2026-10-13"),
            "18:00",
            dt("2026-10-12 23:00:00")
        ));
    }

    #[test]
    fn a_malformed_start_time_counts_as_started() {
        assert!(has_started(
            d("2026-10-20"),
            "bogus",
            dt("2026-10-12 07:00:00")
        ));
        assert_eq!(class_start(d("2026-10-20"), "25:99"), None);
        assert_eq!(
            class_start(d("2026-10-20"), "18:30"),
            Some(dt("2026-10-20 18:30:00"))
        );
    }
}
