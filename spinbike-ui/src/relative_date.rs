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
use chrono::{Datelike, NaiveDate};

/// Locale-aware "today" — wraps `chrono::Local::now().date_naive()`.
/// Centralised so future date-formatting features (and tests that mock
/// the current day) share a single call site. See #63.
pub fn today_local() -> NaiveDate {
    chrono::Local::now().date_naive()
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
/// existing English staff displays also use %d.%m.%Y).
fn format_date(d: NaiveDate) -> String {
    format!("{:02}.{:02}.{:04}", d.day(), d.month(), d.year())
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
    use chrono::NaiveDate;
    use wasm_bindgen_test::*;

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
        assert_eq!(format_last_visit(v, t, Lang::Sk), "28.04.2026 (pred 6 dnami)");
    }
    #[wasm_bindgen_test]
    fn combined_format_sk_today() {
        let (v, t) = mk(2026, 5, 4, 0);
        assert_eq!(format_last_visit(v, t, Lang::Sk), "04.05.2026 (dnes)");
    }
}
