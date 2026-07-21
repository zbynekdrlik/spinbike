//! Shared UI date helpers — the single source for parsing the server's
//! ISO-prefixed date strings and rendering the Slovak DD.MM.YYYY digit form.
//!
//! Consolidates the ~6 previously-duplicated inline ISO parsers and the two
//! copies of the DD.MM.YYYY renderer (#168). Two neighbouring concerns are
//! deliberately NOT folded in here — they are distinct by design:
//!   - `components::date_input::parse_user_date` is a 9-format LENIENT parser
//!     for interactive typing (2-digit years, slash/space variants); it must
//!     stay separate from this strict server parser.
//!   - `relative_date::format_date` is deliberately locale-INDEPENDENT (always
//!     DD.MM.YYYY, even for English staff). This module shares only the digit
//!     arithmetic via `format_ddmmyyyy`, never the locale policy — each caller
//!     decides when DD.MM.YYYY applies.

use chrono::NaiveDate;

/// Parse a server-supplied date string into a `NaiveDate`.
///
/// Accepts a bare ISO date (`"2026-04-25"`) or any ISO-prefixed timestamp the
/// server emits — space-separated (`"2026-04-25 18:00:00"`, SQLite
/// `datetime('now')`) or `T`-separated (`"2026-04-25T18:00:00Z"`, ISO 8601).
/// It trims, takes the first whitespace-delimited token, then the part before
/// any `T`, and parses that as `%Y-%m-%d`. Returns `None` if the leading token
/// is not a valid ISO date.
///
/// This is a safe superset of the six inline parsers it replaced: a plain
/// `"2026-04-25"` with no space or `T` is unaffected, while the trim/split only
/// ever strips a trailing time component that those parsers either also
/// stripped (my_balance) or would have rejected.
pub fn parse_server_date(s: &str) -> Option<NaiveDate> {
    let trimmed = s.trim();
    let date_str = trimmed.split_whitespace().next().unwrap_or(trimmed);
    let date_str = date_str.split('T').next().unwrap_or(date_str);
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

/// Render a `NaiveDate` in the Slovak DD.MM.YYYY digit form (e.g. `25.04.2026`),
/// zero-padded. This is *only* the shared digit arithmetic behind
/// `i18n::fmt_date`'s Slovak arm and `relative_date::format_date`; it carries no
/// locale policy of its own.
pub fn format_ddmmyyyy(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

/// Parse a server-supplied UTC timestamp (e.g. `last_visit_at`, itself a
/// `MAX(created_at)` UTC instant) into the Bratislava-LOCAL calendar date.
///
/// Do NOT reach for `parse_server_date` when the value needs a "was this
/// TODAY (Bratislava-local)?" bucket: that function takes the raw UTC date
/// token with NO timezone conversion, so a visit logged 00:00-02:00
/// Bratislava-local time (Bratislava runs UTC+1/+2 AHEAD of UTC) carries a
/// UTC date ONE DAY BEHIND the local wall date — it renders "vcera"
/// (yesterday), unhighlighted, in exactly the window the same-day
/// duplicate-visit signal (#234/#235) must fire. Review follow-up to #236.
///
/// This converts through the IANA tz database (DST-aware) via
/// `i18n::parse_to_local`, then takes the resulting local calendar date.
pub fn parse_server_date_local(s: &str) -> Option<NaiveDate> {
    crate::i18n::parse_to_local(s).map(|dt| dt.date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    fn ymd(y: i32, m: u32, d: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(y, m, d)
    }

    #[wasm_bindgen_test]
    fn parse_bare_iso_date() {
        assert_eq!(parse_server_date("2026-04-25"), ymd(2026, 4, 25));
    }

    #[wasm_bindgen_test]
    fn parse_space_separated_sqlite_timestamp() {
        assert_eq!(parse_server_date("2026-04-25 18:00:00"), ymd(2026, 4, 25));
    }

    #[wasm_bindgen_test]
    fn parse_t_separated_iso8601_with_zulu() {
        assert_eq!(parse_server_date("2026-04-25T18:00:00Z"), ymd(2026, 4, 25));
    }

    #[wasm_bindgen_test]
    fn parse_trims_surrounding_whitespace() {
        assert_eq!(parse_server_date("  2026-04-25  "), ymd(2026, 4, 25));
    }

    #[wasm_bindgen_test]
    fn parse_garbage_returns_none() {
        assert_eq!(parse_server_date("not-a-date"), None);
        assert_eq!(parse_server_date(""), None);
        // A Slovak display-form date is NOT a server date — must not parse.
        assert_eq!(parse_server_date("25.04.2026"), None);
    }

    #[wasm_bindgen_test]
    fn format_ddmmyyyy_zero_pads_single_digit_day_and_month() {
        let d = ymd(2026, 4, 5).unwrap();
        assert_eq!(format_ddmmyyyy(d), "05.04.2026");
    }

    #[wasm_bindgen_test]
    fn format_ddmmyyyy_two_digit_day_and_month() {
        let d = ymd(2026, 12, 25).unwrap();
        assert_eq!(format_ddmmyyyy(d), "25.12.2026");
    }

    #[wasm_bindgen_test]
    fn roundtrip_parse_then_format() {
        let d = parse_server_date("2026-01-09 07:30:00").unwrap();
        assert_eq!(format_ddmmyyyy(d), "09.01.2026");
    }

    // #236 review follow-up: `last_visit_at` is a UTC instant. Near
    // midnight Bratislava-local, the raw UTC date token and the local wall
    // date disagree — the "today" highlight must key off the LOCAL date.
    #[wasm_bindgen_test]
    fn parse_server_date_local_resolves_bratislava_wall_date_not_raw_utc_token() {
        // UTC 2026-07-20 22:30:00 = Bratislava-local 2026-07-21 00:30 (CEST,
        // UTC+2 in July) — the exact 00:00-02:00 local window the review
        // flagged. The OLD path (parse_server_date) takes the raw UTC date
        // token and lands one day behind local.
        assert_eq!(
            parse_server_date("2026-07-20 22:30:00"),
            ymd(2026, 7, 20),
            "documents the bug: raw UTC date token is one day behind local"
        );
        assert_eq!(
            parse_server_date_local("2026-07-20 22:30:00"),
            ymd(2026, 7, 21),
            "must resolve to the Bratislava-LOCAL calendar date, not the raw UTC token"
        );
    }

    #[wasm_bindgen_test]
    fn parse_server_date_local_agrees_with_utc_token_away_from_midnight() {
        // Mid-afternoon UTC: the UTC date token and the Bratislava-local
        // date are the same day — the fix must not regress the common case.
        assert_eq!(
            parse_server_date_local("2026-07-20 12:00:00"),
            ymd(2026, 7, 20)
        );
    }
}
