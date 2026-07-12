//! Small helpers shared across routes.

use chrono::{NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Europe::Bratislava;

/// Current wall-clock time in the gym's timezone (Europe/Bratislava), as a
/// naive local datetime.
///
/// The single source of truth for "what time is it at the gym". Derived from
/// `Utc::now()` and converted through the **named** IANA timezone
/// `Europe/Bratislava` (via `chrono-tz`), so:
///
/// * DST (CET/CEST, UTC+1/UTC+2) is applied automatically from the tz database
///   — never a hardcoded `+01:00`/`+02:00` offset.
/// * The result does NOT depend on the server process's OS/`TZ` configuration
///   (`chrono::Local` and SQLite's `'localtime'` both read the OS zone, which a
///   systemd unit may leave as UTC — the fragility #205 removes).
pub fn now_bratislava() -> NaiveDateTime {
    Utc::now().with_timezone(&Bratislava).naive_local()
}

/// Today's calendar date at the gym (Europe/Bratislava).
///
/// The single definition of "the gym-local day" used by every day-boundary
/// decision around monthly-pass expiry: the T-4h charger window, the door
/// pass check, the customer balance page, and the staff-list "days remaining".
/// A monthly pass is valid THROUGH the whole of its last calendar day in THIS
/// timezone — see #205 (owner's decision: the pass-expiry boundary is the
/// gym's local midnight, not SQLite's UTC `date('now')`).
pub fn today_bratislava() -> NaiveDate {
    now_bratislava().date_naive()
}

/// Format an integer as an English ordinal: 1 → "1st", 2 → "2nd", 3 → "3rd",
/// 4 → "4th", 11 → "11th", 21 → "21st", 100 → "100th".
///
/// Used in the door-route note column to label same-day re-entries
/// ("door: 2nd", "door: 3rd", ...). Capped at 999 by the caller's
/// rate limit; defensive for any u32 input.
pub fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{now_bratislava, ordinal, today_bratislava};
    use chrono::{NaiveDate, TimeZone, Utc};
    use chrono_tz::Europe::Bratislava;

    /// The helper's "today" must be the Europe/Bratislava calendar date, i.e.
    /// the named-tz conversion of the current UTC instant — NOT the naive UTC
    /// date. Also pins `today_bratislava == now_bratislava().date()`.
    #[test]
    fn today_bratislava_is_the_named_tz_date_not_utc() {
        let expected = Utc::now().with_timezone(&Bratislava).date_naive();
        assert_eq!(today_bratislava(), expected);
        assert_eq!(today_bratislava(), now_bratislava().date_naive());
    }

    /// Core #205 proof (winter / CET, UTC+1): 23:30 UTC is already 00:30 the
    /// NEXT calendar day in Bratislava. The gym-local date must roll over at
    /// the gym's midnight, so it DIFFERS from the naive UTC date at this
    /// instant — the exact off-by-one the UTC `date('now')` boundary produced.
    #[test]
    fn bratislava_date_rolls_over_at_local_midnight_winter() {
        let utc = Utc.with_ymd_and_hms(2026, 1, 15, 23, 30, 0).unwrap();
        let local_date = utc.with_timezone(&Bratislava).date_naive();
        assert_eq!(
            local_date,
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
            "23:30 UTC is already the next day at the gym (CET, +01:00)"
        );
        assert_ne!(
            local_date,
            utc.date_naive(),
            "gym-local date must differ from the naive UTC date at this boundary"
        );
    }

    /// DST proof (summer / CEST, UTC+2): 21:30 UTC is 23:30 in Bratislava —
    /// STILL the same day — while 22:30 UTC is already 00:30 the next day.
    /// A hardcoded +01:00 offset would put both at the wrong local hour, so
    /// this pins that `chrono-tz` applies the live summer offset from tzdata.
    #[test]
    fn bratislava_date_uses_summer_dst_offset() {
        let bratislava = chrono_tz::Europe::Bratislava;
        let same_day = Utc.with_ymd_and_hms(2026, 7, 15, 21, 30, 0).unwrap();
        assert_eq!(
            same_day.with_timezone(&bratislava).date_naive(),
            NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
            "21:30 UTC == 23:30 CEST — still the 15th at the gym"
        );
        let next_day = Utc.with_ymd_and_hms(2026, 7, 15, 22, 30, 0).unwrap();
        assert_eq!(
            next_day.with_timezone(&bratislava).date_naive(),
            NaiveDate::from_ymd_opt(2026, 7, 16).unwrap(),
            "22:30 UTC == 00:30 CEST — already the 16th at the gym"
        );
    }

    #[test]
    fn ordinal_basics() {
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(4), "4th");
        assert_eq!(ordinal(5), "5th");
    }

    #[test]
    fn ordinal_teens() {
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(12), "12th");
        assert_eq!(ordinal(13), "13th");
        assert_eq!(ordinal(14), "14th");
    }

    #[test]
    fn ordinal_twenties() {
        assert_eq!(ordinal(21), "21st");
        assert_eq!(ordinal(22), "22nd");
        assert_eq!(ordinal(23), "23rd");
        assert_eq!(ordinal(24), "24th");
    }

    #[test]
    fn ordinal_hundreds() {
        assert_eq!(ordinal(100), "100th");
        assert_eq!(ordinal(101), "101st");
        assert_eq!(ordinal(111), "111th");
        assert_eq!(ordinal(112), "112th");
        assert_eq!(ordinal(121), "121st");
    }

    #[test]
    fn ordinal_zero() {
        assert_eq!(ordinal(0), "0th");
    }
}
