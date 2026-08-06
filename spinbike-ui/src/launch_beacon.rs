//! Standalone-launch beacon (#260) — fired once on app boot, fire-and-
//! forget, whenever the app detects it is running installed (standalone /
//! home-screen), via `POST /api/metrics/launch`.
//!
//! This exists because round 4 of the iOS install auto-login fix (#258)
//! failed in prod and was undiagnosable: the server logged nothing, leaving
//! three competing hypotheses (wrong `start_url` captured at "Add to Home
//! Screen" time / manifest not re-read / the install gesture never
//! happened) that all fit the same two-line evidence identically. A beacon
//! line carrying the launch path with NO login token discriminates them —
//! see `spinbike_server::routes::metrics` on the server side for the full
//! reasoning.

use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

use crate::auth::get_token;
use crate::platform::is_standalone;
use spinbike_core::redact::redact_query;

#[derive(Serialize)]
struct LaunchBeaconRequest {
    path: String,
    query_redacted: String,
    had_token: bool,
    had_session: bool,
    src: Option<String>,
}

/// Read a single query-param VALUE from a raw (no leading `?`) query string,
/// e.g. `"a=1&b=2"` -> `query_param(_, "b") == Some("2")`. Used only for the
/// beacon's own non-sensitive `had_token`/`src` fields — the value actually
/// sent over the wire (`query_redacted`) always goes through
/// `spinbike_core::redact::redact_query` instead, never this raw lookup.
fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        pair.split_once('=')
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v)
    })
}

/// If running as an installed standalone app (`platform::is_standalone` —
/// the same `display-mode: standalone` / `navigator.standalone` check used
/// elsewhere in this crate), fire the launch beacon. Fire-and-forget: the
/// send error (if any) is silently dropped — a boot-time diagnostic beacon
/// must never affect, delay, or surface anything to the user.
pub fn maybe_fire() {
    if !is_standalone() {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let location = window.location();
    let path = location.pathname().unwrap_or_default();
    let raw_search = location.search().unwrap_or_default();
    let raw_query = raw_search.strip_prefix('?').unwrap_or(&raw_search);

    let had_token = query_param(raw_query, "t").is_some_and(|v| !v.is_empty())
        || query_param(raw_query, "it").is_some_and(|v| !v.is_empty());
    let src = query_param(raw_query, "src")
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());
    let had_session = get_token().is_some();
    let query_redacted = redact_query(raw_query);

    let body = LaunchBeaconRequest {
        path,
        query_redacted,
        had_token,
        had_session,
        src,
    };

    spawn_local(async move {
        use gloo_net::http::{Method, RequestBuilder};
        let Ok(req) = RequestBuilder::new("/api/metrics/launch")
            .method(Method::POST)
            .json(&body)
        else {
            return;
        };
        let _ = req.send().await;
    });
}

#[cfg(test)]
mod tests {
    use super::query_param;
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    #[wasm_bindgen_test]
    fn finds_value_for_present_key() {
        assert_eq!(query_param("a=1&b=2", "b"), Some("2"));
    }

    #[wasm_bindgen_test]
    fn returns_none_for_absent_key() {
        assert_eq!(query_param("a=1&b=2", "c"), None);
    }

    #[wasm_bindgen_test]
    fn returns_empty_string_value_when_key_present_but_blank() {
        assert_eq!(query_param("t=&src=install", "t"), Some(""));
    }

    #[wasm_bindgen_test]
    fn empty_query_returns_none() {
        assert_eq!(query_param("", "t"), None);
    }

    #[wasm_bindgen_test]
    fn single_pair_query_works() {
        assert_eq!(query_param("it=abc123", "it"), Some("abc123"));
    }
}
