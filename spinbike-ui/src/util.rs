//! App-wide utility helpers shared across pages (e.g. money parsing used by
//! both the dashboard charge/topup forms and the admin service price forms).

/// Parse a user-entered money string, accepting both `.` and `,` as the decimal
/// separator — Slovak keyboards produce comma by default, European users expect
/// it to work. Trims whitespace. Returns `None` on empty or invalid input so
/// callers can decide the fallback.
pub fn parse_money(s: &str) -> Option<f64> {
    let normalized = s.trim().replace(',', ".");
    if normalized.is_empty() {
        None
    } else {
        normalized.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod parse_money_tests {
    use super::parse_money;

    #[test]
    fn plain_integer() {
        assert_eq!(parse_money("40"), Some(40.0));
    }

    #[test]
    fn dot_decimal() {
        assert_eq!(parse_money("35.50"), Some(35.5));
    }

    #[test]
    fn comma_decimal_is_normalized() {
        assert_eq!(parse_money("35,50"), Some(35.5));
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(parse_money("  12.3  "), Some(12.3));
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(parse_money(""), None);
        assert_eq!(parse_money("   "), None);
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(parse_money("abc"), None);
        assert_eq!(parse_money("1,2,3"), None);
    }
}
