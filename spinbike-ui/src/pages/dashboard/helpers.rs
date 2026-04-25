//! Shared helper functions and types.


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

