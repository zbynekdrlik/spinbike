//! Locale-aware date text input with `‹` / `›` step buttons and a "Today"
//! quick-set. Parses DD.MM.YYYY (Slovak), ISO, and a few flexible variants.
//!
//! Designed to replace `<input type="date">` whose visible format is
//! controlled by the browser/OS locale, not the app language.

use leptos::ev;
use leptos::prelude::*;

use crate::i18n::{self, Lang};
use crate::pages::dashboard::helpers::event_target_value;

/// Try a few common formats. Returns `None` if none match.
pub fn parse_user_date(s: &str) -> Option<chrono::NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    for fmt in &[
        "%d.%m.%Y", "%d. %m. %Y", "%-d.%-m.%Y", "%-d. %-m. %Y", "%d.%m.%y", "%-d.%-m.%y",
        "%Y-%m-%d", "%d/%m/%Y", "%d/%m/%y",
    ] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
    }
    None
}

#[component]
pub fn DateInput(
    /// Current value.
    value: ReadSignal<chrono::NaiveDate>,
    /// Setter — invoked when user types a valid date or clicks ‹/›/Today.
    set_value: WriteSignal<chrono::NaiveDate>,
    #[prop(optional, into)] testid: Option<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    // Local text mirror — kept in sync with `value` but allows mid-edit
    // intermediate states like "25.0" without snapping back.
    let (text, set_text) = signal(i18n::fmt_date(value.get_untracked(), lang.get_untracked()));
    let (last_synced, set_last_synced) = signal(value.get_untracked());

    // When external `value` changes (button click, parent resets), update text.
    Effect::new(move |_| {
        let v = value.get();
        if v != last_synced.get_untracked() {
            set_text.set(i18n::fmt_date(v, lang.get_untracked()));
            set_last_synced.set(v);
        }
    });

    let on_input = move |ev: ev::Event| {
        let s = event_target_value(&ev);
        set_text.set(s.clone());
        if let Some(d) = parse_user_date(&s) {
            if d != value.get_untracked() {
                set_value.set(d);
                set_last_synced.set(d);
            }
        }
    };

    let on_blur = move |_| {
        // On blur, snap text back to the canonical formatted form so the
        // user always sees a clean DD.MM.YYYY (or ISO in EN).
        set_text.set(i18n::fmt_date(value.get_untracked(), lang.get_untracked()));
    };

    let prev_day = move |_| {
        set_value.update(|d| *d = *d - chrono::Duration::days(1));
    };
    let next_day = move |_| {
        set_value.update(|d| *d = *d + chrono::Duration::days(1));
    };
    let set_today = move |_| {
        set_value.set(chrono::Local::now().date_naive());
    };

    let testid_value = testid.unwrap_or_default();
    let testid_input = if testid_value.is_empty() {
        String::new()
    } else {
        format!("{testid_value}-input")
    };

    view! {
        <div class="date-input" data-testid=testid_value>
            <button type="button"
                    class="btn btn--compact btn--ghost"
                    aria-label="prev day"
                    on:click=prev_day>"‹"</button>
            <input class="form-control date-input__field"
                   type="text"
                   inputmode="numeric"
                   autocomplete="off"
                   placeholder=move || match lang.get() {
                       Lang::Sk => "DD.MM.YYYY",
                       Lang::En => "YYYY-MM-DD",
                   }
                   data-testid=testid_input
                   prop:value=move || text.get()
                   on:input=on_input
                   on:blur=on_blur />
            <button type="button"
                    class="btn btn--compact btn--ghost"
                    aria-label="next day"
                    on:click=next_day>"›"</button>
            <button type="button"
                    class="btn btn--compact btn--ghost"
                    on:click=set_today>
                {move || i18n::t(lang.get(), "reports_today")}
            </button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::parse_user_date;
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_node);

    #[wasm_bindgen_test]
    fn parses_slovak_dot_format() {
        let d = parse_user_date("25.04.2026").unwrap();
        assert_eq!(d.to_string(), "2026-04-25");
    }

    #[wasm_bindgen_test]
    fn parses_slovak_with_spaces() {
        let d = parse_user_date("25. 04. 2026").unwrap();
        assert_eq!(d.to_string(), "2026-04-25");
    }

    #[wasm_bindgen_test]
    fn parses_iso() {
        let d = parse_user_date("2026-04-25").unwrap();
        assert_eq!(d.to_string(), "2026-04-25");
    }

    #[wasm_bindgen_test]
    fn parses_short_year() {
        let d = parse_user_date("25.04.26").unwrap();
        assert_eq!(d.to_string(), "2026-04-25");
    }

    #[wasm_bindgen_test]
    fn parses_single_digit_day_month() {
        let d = parse_user_date("5.4.2026").unwrap();
        assert_eq!(d.to_string(), "2026-04-05");
    }

    #[wasm_bindgen_test]
    fn rejects_garbage() {
        assert!(parse_user_date("not-a-date").is_none());
        assert!(parse_user_date("").is_none());
    }

    #[wasm_bindgen_test]
    fn rejects_logically_invalid_dates() {
        assert!(parse_user_date("31.04.2026").is_none(), "April has 30 days");
        assert!(parse_user_date("32.01.2026").is_none(), "Jan has 31 days");
        assert!(parse_user_date("00.01.2026").is_none(), "day 0 invalid");
    }

    #[wasm_bindgen_test]
    fn leap_year_handled_correctly() {
        assert_eq!(
            parse_user_date("29.02.2024").unwrap().to_string(),
            "2024-02-29",
            "2024 is a leap year"
        );
        assert!(
            parse_user_date("29.02.2025").is_none(),
            "2025 is not a leap year"
        );
    }

    #[wasm_bindgen_test]
    fn whitespace_padding_trimmed() {
        let d = parse_user_date("  25.04.2026  ").unwrap();
        assert_eq!(d.to_string(), "2026-04-25");
    }
}
