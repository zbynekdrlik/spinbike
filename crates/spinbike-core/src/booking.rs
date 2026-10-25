//! Spinning booking constants shared by the server and the UI (#389).

/// The most guests one host may bring to a single class occurrence (owner
/// ROZHODNUTÉ on #389, 2026-10-25). The server rejects more (400
/// `too_many_guests`) and `bookings.guests` carries a CHECK backstop
/// (migration V32); the UI disables "+" at this value.
pub const MAX_GUESTS: i64 = 4;

/// Whether `guests` is an allowed guest count for one booking.
pub fn guests_in_range(guests: i64) -> bool {
    (0..=MAX_GUESTS).contains(&guests)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_range_is_zero_to_four() {
        assert!(guests_in_range(0));
        assert!(guests_in_range(4));
        assert!(!guests_in_range(5));
        assert!(!guests_in_range(-1));
        assert_eq!(MAX_GUESTS, 4);
    }
}
