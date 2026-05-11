//! Small helpers shared across routes.

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
    use super::ordinal;

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
