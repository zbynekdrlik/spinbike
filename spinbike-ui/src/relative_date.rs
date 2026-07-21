//! Slovak/English relative-time formatter for "last visit" displays.
//!
//! Smart granularity: today / yesterday / 2-7 days / 1-8 weeks / 2-12 months
//! / 1+ years. Slovak grammar uses two forms per unit (singular `_one` for
//! N=1 and instrumental plural `_few` for N>=2 — these specific words
//! collapse the 2-4 / 5+ distinction to one form).
//!
//! Public API: `format_last_visit` returns the combined string
//! `"<DD.MM.YYYY> (<relative>)"`; `relative` returns just the relative bucket
//! word (e.g. "yesterday" / "vcera") for callers that don't need the date
//! prefix.

use crate::i18n::{self, Lang};
use chrono::NaiveDate;

/// Bratislava-local "today" derived from an explicit UTC instant — the
/// testable core of `today_local()`, pinned to an instant argument so tests
/// don't depend on the host's wall-clock timezone.
///
/// Do NOT reach for the raw UTC calendar date: near midnight Bratislava-local
/// (Bratislava runs UTC+1/+2 AHEAD of UTC), the UTC date is one day BEHIND
/// the local wall date. See #239 — `today_local()` used to derive "today"
/// from `chrono::Local::now()` (the BROWSER's clock), which is only correct
/// because every device running this dashboard happens to be set to
/// Bratislava time; nothing enforced that assumption.
fn today_from_utc(now_utc: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    now_utc
        .with_timezone(&chrono_tz::Europe::Bratislava)
        .date_naive()
}

/// Locale-aware "today", anchored to Europe/Bratislava (not the host clock).
/// Centralised so future date-formatting features (and tests that mock
/// the current day) share a single call site. See #63, #239.
pub fn today_local() -> NaiveDate {
    today_from_utc(chrono::Utc::now())
}

/// Format `visited` as a date label combined with a relative-time hint
/// computed against `today`. Output examples:
///   - Slovak, 6 days ago, visited 2026-04-28 → "28.04.2026 (pred 6 dnami)"
///   - Slovak, today, 2026-05-04            → "04.05.2026 (dnes)"
///   - English, 2 weeks ago, 2026-04-20     → "20.04.2026 (2 weeks ago)"
///
/// `visited` MUST be <= `today`. Future visits are clamped to "today".
pub fn format_last_visit(visited: NaiveDate, today: NaiveDate, lang: Lang) -> String {
    let date_part = format_date(visited);
    let rel = relative(visited, today, lang);
    format!("{date_part} ({rel})")
}

/// DD.MM.YYYY date — same form for SK and EN (Slovak idiom; project's
/// existing English staff displays also use %d.%m.%Y). Deliberately
/// locale-INDEPENDENT — do NOT route this through `i18n::fmt_date` (which
/// returns ISO for English); it shares only the digit arithmetic via
/// `dates::format_ddmmyyyy` (#168).
fn format_date(d: NaiveDate) -> String {
    crate::dates::format_ddmmyyyy(d)
}

/// Relative-time bucket. See module docs for the exact thresholds.
pub fn relative(visited: NaiveDate, today: NaiveDate, lang: Lang) -> String {
    let days = (today - visited).num_days().max(0);
    if days == 0 {
        return i18n::t(lang, "rel_today").to_string();
    }
    if days == 1 {
        return i18n::t(lang, "rel_yesterday").to_string();
    }
    if days <= 7 {
        return plural(days as u32, "rel_days_one", "rel_days_few", lang);
    }
    if days <= 60 {
        let n = (days / 7) as u32;
        return plural(n, "rel_weeks_one", "rel_weeks_few", lang);
    }
    if days <= 364 {
        let n = (days / 30) as u32;
        return plural(n, "rel_months_one", "rel_months_few", lang);
    }
    let n = (days / 365) as u32;
    plural(n, "rel_years_one", "rel_years_few", lang)
}

/// Return the i18n string for `key_one` if `n == 1`, otherwise the i18n
/// string for `key_few` with `{n}` replaced by `n`.
fn plural(n: u32, key_one: &str, key_few: &str, lang: Lang) -> String {
    if n == 1 {
        i18n::t(lang, key_one).to_string()
    } else {
        i18n::t(lang, key_few)
            .to_string()
            .replace("{n}", &n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};
    use wasm_bindgen_test::*;

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(y, m, d, h, mi, s).unwrap()
    }

    // #239: today_local() must anchor to Europe/Bratislava, not the raw UTC
    // calendar date (which is what the host clock returns when the browser
    // isn't Bratislava-local). Near midnight Bratislava-local the two
    // disagree — the exact window #236/#241 fixed for last_visit_at.
    #[wasm_bindgen_test]
    fn today_from_utc_resolves_bratislava_wall_date_not_raw_utc_token() {
        // UTC 2026-07-20 22:30:00 = Bratislava-local 2026-07-21 00:30 (CEST,
        // UTC+2 in July).
        let now = utc(2026, 7, 20, 22, 30, 0);
        assert_eq!(
            now.date_naive(),
            NaiveDate::from_ymd_opt(2026, 7, 20).unwrap(),
            "documents the bug: raw UTC calendar date is one day behind Bratislava-local"
        );
        assert_eq!(
            today_from_utc(now),
            NaiveDate::from_ymd_opt(2026, 7, 21).unwrap(),
            "must resolve to the Bratislava-LOCAL calendar date, not the raw UTC token"
        );
    }

    #[wasm_bindgen_test]
    fn today_from_utc_agrees_with_utc_token_away_from_midnight() {
        // Mid-afternoon UTC: the UTC date token and the Bratislava-local
        // date are the same day — the fix must not regress the common case.
        let now = utc(2026, 7, 20, 12, 0, 0);
        assert_eq!(today_from_utc(now), NaiveDate::from_ymd_opt(2026, 7, 20).unwrap());
    }

    fn mk(today_y: i32, today_m: u32, today_d: u32, days_ago: i64) -> (NaiveDate, NaiveDate) {
        let today = NaiveDate::from_ymd_opt(today_y, today_m, today_d).unwrap();
        let visited = today - chrono::Duration::days(days_ago);
        (visited, today)
    }

    #[wasm_bindgen_test]
    fn rel_0_days_today_sk() {
        let (v, t) = mk(2026, 5, 4, 0);
        assert_eq!(relative(v, t, Lang::Sk), "dnes");
    }
    #[wasm_bindgen_test]
    fn rel_0_days_today_en() {
        let (v, t) = mk(2026, 5, 4, 0);
        assert_eq!(relative(v, t, Lang::En), "today");
    }
    #[wasm_bindgen_test]
    fn rel_1_day_yesterday_sk() {
        let (v, t) = mk(2026, 5, 4, 1);
        assert_eq!(relative(v, t, Lang::Sk), "vcera");
    }
    #[wasm_bindgen_test]
    fn rel_1_day_yesterday_en() {
        let (v, t) = mk(2026, 5, 4, 1);
        assert_eq!(relative(v, t, Lang::En), "yesterday");
    }
    #[wasm_bindgen_test]
    fn rel_2_days_sk() {
        let (v, t) = mk(2026, 5, 4, 2);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_2_days_en() {
        let (v, t) = mk(2026, 5, 4, 2);
        assert_eq!(relative(v, t, Lang::En), "2 days ago");
    }
    #[wasm_bindgen_test]
    fn rel_4_days_sk() {
        let (v, t) = mk(2026, 5, 4, 4);
        assert_eq!(relative(v, t, Lang::Sk), "pred 4 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_5_days_sk() {
        let (v, t) = mk(2026, 5, 4, 5);
        assert_eq!(relative(v, t, Lang::Sk), "pred 5 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_7_days_sk() {
        let (v, t) = mk(2026, 5, 4, 7);
        assert_eq!(relative(v, t, Lang::Sk), "pred 7 dnami");
    }
    #[wasm_bindgen_test]
    fn rel_8_days_one_week_sk() {
        let (v, t) = mk(2026, 5, 4, 8);
        assert_eq!(relative(v, t, Lang::Sk), "pred 1 tyzdnom");
    }
    #[wasm_bindgen_test]
    fn rel_8_days_one_week_en() {
        let (v, t) = mk(2026, 5, 4, 8);
        assert_eq!(relative(v, t, Lang::En), "1 week ago");
    }
    #[wasm_bindgen_test]
    fn rel_14_days_two_weeks_sk() {
        let (v, t) = mk(2026, 5, 4, 14);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 tyzdnami");
    }
    #[wasm_bindgen_test]
    fn rel_60_days_eight_weeks_sk() {
        let (v, t) = mk(2026, 5, 4, 60);
        assert_eq!(relative(v, t, Lang::Sk), "pred 8 tyzdnami");
    }
    #[wasm_bindgen_test]
    fn rel_61_days_two_months_sk() {
        let (v, t) = mk(2026, 5, 4, 61);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 mesiacmi");
    }
    #[wasm_bindgen_test]
    fn rel_61_days_two_months_en() {
        let (v, t) = mk(2026, 5, 4, 61);
        assert_eq!(relative(v, t, Lang::En), "2 months ago");
    }
    #[wasm_bindgen_test]
    fn rel_364_days_twelve_months_sk() {
        let (v, t) = mk(2026, 5, 4, 364);
        assert_eq!(relative(v, t, Lang::Sk), "pred 12 mesiacmi");
    }
    #[wasm_bindgen_test]
    fn rel_365_days_one_year_sk() {
        let (v, t) = mk(2026, 5, 4, 365);
        assert_eq!(relative(v, t, Lang::Sk), "pred 1 rokom");
    }
    #[wasm_bindgen_test]
    fn rel_365_days_one_year_en() {
        let (v, t) = mk(2026, 5, 4, 365);
        assert_eq!(relative(v, t, Lang::En), "1 year ago");
    }
    #[wasm_bindgen_test]
    fn rel_730_days_two_years_sk() {
        let (v, t) = mk(2026, 5, 4, 730);
        assert_eq!(relative(v, t, Lang::Sk), "pred 2 rokmi");
    }
    #[wasm_bindgen_test]
    fn rel_1825_days_five_years_sk() {
        let (v, t) = mk(2026, 5, 4, 1825);
        assert_eq!(relative(v, t, Lang::Sk), "pred 5 rokmi");
    }

    #[wasm_bindgen_test]
    fn combined_format_sk_six_days_ago() {
        let (v, t) = mk(2026, 5, 4, 6);
        assert_eq!(
            format_last_visit(v, t, Lang::Sk),
            "28.04.2026 (pred 6 dnami)"
        );
    }
    #[wasm_bindgen_test]
    fn combined_format_sk_today() {
        let (v, t) = mk(2026, 5, 4, 0);
        assert_eq!(format_last_visit(v, t, Lang::Sk), "04.05.2026 (dnes)");
    }
}
