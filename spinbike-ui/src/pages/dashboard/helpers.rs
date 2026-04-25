//! Shared helper functions and types.

/// Format a server-side timestamp per the user's language preference.
/// Slovak: `dd.MM.yyyy HH:mm` (e.g. `14.04.2026 18:13`).
/// English: `yyyy-MM-dd HH:mm` (ISO).
/// Handles current SQLite output, ISO 8601, and legacy MS Access dumps
/// (`MM/dd/yy` or `MM/dd/yyyy`) imported via the migrate-legacy tool.
/// Falls back to the raw string so rows never disappear, even on unknown formats.
pub fn format_datetime(raw: &str, lang: crate::i18n::Lang) -> String {
    use chrono::NaiveDateTime;
    let trimmed = raw.trim();
    let patterns = [
        "%Y-%m-%d %H:%M:%S",    // SQLite datetime('now')
        "%Y-%m-%dT%H:%M:%S",    // ISO 8601 with T
        "%Y-%m-%d %H:%M:%S%.f", // SQLite with fractional seconds
        "%m/%d/%y %H:%M:%S",    // legacy MS Access, 2-digit year
        "%m/%d/%Y %H:%M:%S",    // legacy MS Access, 4-digit year
    ];
    for pattern in patterns {
        if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, pattern) {
            return match lang {
                crate::i18n::Lang::Sk => dt.format("%d.%m.%Y %H:%M").to_string(),
                crate::i18n::Lang::En => dt.format("%Y-%m-%d %H:%M").to_string(),
            };
        }
    }
    raw.to_string()
}

/// Backwards-compat alias — Slovak-specific version used by tests.
#[cfg(test)]
fn format_sk_datetime(raw: &str) -> String {
    format_datetime(raw, crate::i18n::Lang::Sk)
}

pub fn pass_is_active(card: &super::CardInfo) -> bool {
    card.pass
        .as_ref()
        .map(|p| p.days_remaining >= 0)
        .unwrap_or(false)
}

pub fn full_name(c: &super::CardInfo) -> String {
    let f = c.first_name.clone().unwrap_or_default();
    let l = c.last_name.clone().unwrap_or_default();
    let combined = format!("{f} {l}").trim().to_string();
    if combined.is_empty() {
        "—".into()
    } else {
        combined
    }
}

// tiny percent-encoder for the search query (avoids pulling urlencoding crate
// just for this — we only need to escape a handful of chars).
pub fn urlencoding_light(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            ' ' => out.push_str("%20"),
            _ => {
                let mut buf = [0u8; 4];
                for b in ch.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

pub fn event_target_value(ev: &web_sys::Event) -> String {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default()
}

#[cfg(test)]
mod date_tests {
    use super::format_sk_datetime;

    #[test]
    fn sqlite_format() {
        assert_eq!(
            format_sk_datetime("2026-04-14 18:13:11"),
            "14.04.2026 18:13"
        );
    }

    #[test]
    fn iso_8601_format() {
        assert_eq!(
            format_sk_datetime("2026-04-14T18:13:11"),
            "14.04.2026 18:13"
        );
    }

    #[test]
    fn legacy_two_digit_year() {
        assert_eq!(format_sk_datetime("03/24/26 18:59:08"), "24.03.2026 18:59");
    }

    #[test]
    fn legacy_four_digit_year() {
        assert_eq!(
            format_sk_datetime("03/24/2026 18:59:08"),
            "24.03.2026 18:59"
        );
    }

    #[test]
    fn unknown_returns_input() {
        assert_eq!(format_sk_datetime("not-a-date"), "not-a-date");
    }
}
