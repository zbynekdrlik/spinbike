//! Shared helper functions and types.

pub fn pass_is_active(card: &super::CardInfo) -> bool {
    card.pass
        .as_ref()
        .map(|p| p.days_remaining >= 0)
        .unwrap_or(false)
}

pub fn full_name(c: &super::CardInfo) -> String {
    let n = c.name.trim();
    if n.is_empty() || n == "(no name)" {
        "—".into()
    } else {
        n.to_string()
    }
}

/// Display name with a richer fallback chain: prefers `name`, then company,
/// then card_code in brackets. Useful for list rows where "—" would be
/// uninformative (e.g. a corporate card with no person name).
pub fn user_display_name(name: &str, company: Option<&str>, card_code: Option<&str>) -> String {
    let trimmed = name.trim();
    if !trimmed.is_empty() && trimmed != "(no name)" {
        return trimmed.to_string();
    }
    if let Some(c) = company.filter(|s| !s.trim().is_empty()) {
        return c.trim().to_string();
    }
    if let Some(code) = card_code.filter(|s| !s.trim().is_empty()) {
        return format!("[{}]", code.trim());
    }
    "(no name)".to_string()
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

/// Class string for a search-result row, combining keyboard-highlight state
/// with negative-credit highlight. Pure function — kept here so wasm-bindgen
/// tests can pin all four branches without having to drive the Leptos view.
pub fn result_row_class(highlighted: bool, credit: f64) -> &'static str {
    match (highlighted, credit < 0.0) {
        (false, false) => "search-result-row",
        (true, false) => "search-result-row search-result-active",
        (false, true) => "search-result-row search-result--negative",
        (true, true) => "search-result-row search-result-active search-result--negative",
    }
}

#[cfg(test)]
mod result_row_class_tests {
    use super::result_row_class;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn default_row() {
        assert_eq!(result_row_class(false, 1.0), "search-result-row");
    }

    #[wasm_bindgen_test]
    fn highlighted_row() {
        assert_eq!(
            result_row_class(true, 1.0),
            "search-result-row search-result-active",
        );
    }

    #[wasm_bindgen_test]
    fn negative_row() {
        assert_eq!(
            result_row_class(false, -0.01),
            "search-result-row search-result--negative",
        );
    }

    #[wasm_bindgen_test]
    fn highlighted_negative_row() {
        assert_eq!(
            result_row_class(true, -0.01),
            "search-result-row search-result-active search-result--negative",
        );
    }

    #[wasm_bindgen_test]
    fn zero_credit_is_not_negative() {
        // Boundary: 0.0 stays in the default class (kills `<= 0.0` mutant).
        assert_eq!(result_row_class(false, 0.0), "search-result-row");
    }
}
