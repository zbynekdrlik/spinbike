//! Dynamic Web App Manifest handler (#258).
//!
//! `GET /manifest.json` is normally served verbatim from the embedded static
//! bundle (rust-embed, `static_files.rs`). This handler takes precedence over
//! that fallback for the exact `/manifest.json` path so it can OPTIONALLY
//! rewrite the manifest's `start_url` when the query carries an install token
//! (`?it=<raw>`):
//!
//! - No `it` (absent or empty) → the manifest is served **verbatim**, byte for
//!   byte identical to the embedded static asset, so a normal browser load is
//!   completely unchanged.
//! - `it=<raw>` present → the SAME manifest JSON with only `start_url` replaced
//!   by `/welcome?t=<raw>&src=install`. iOS reads `start_url` from the manifest
//!   at "Add to Home Screen" time, so the installed home-screen icon opens
//!   straight into the token-login flow and the app is already signed in on its
//!   first launch (the whole point of #258). The install token is a normal
//!   `purpose='login'` magic-link token minted for the caller's OWN session
//!   (see `install_token` in `auth.rs`) — same strength as an emailed login
//!   link, no new security model.
//!
//! **The token is NEVER validated when serving the manifest** — validation is
//! redemption (`POST /api/auth/token-login`, in `welcome.rs`). Serving it here
//! is a pure string substitution with no DB touch and no oracle: an
//! invalid/expired `it` simply yields a manifest whose `start_url` will fail to
//! redeem later, exactly like a stale emailed link.
//!
//! The manifest body is compiled in via `include_str!` of the single source of
//! truth (`spinbike-ui/manifest.json`, which Trunk also copies into `dist/`),
//! NOT read from the rust-embed asset — so the handler is deterministic and
//! testable even under the CI placeholder-`dist` (where `dist/manifest.json` is
//! a stub). The compiled-in copy is byte-identical to the deployed asset.

use axum::{
    Router,
    extract::Query,
    http::header,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use crate::AppState;

/// The canonical Web App Manifest, compiled in from the single source of truth
/// (`spinbike-ui/manifest.json`). Trunk copies this same file into `dist/`, so
/// this compiled-in copy equals the deployed static asset — but it is always
/// present (even under the CI placeholder `dist`), keeping the handler + its
/// tests deterministic.
const STATIC_MANIFEST: &str = include_str!("../../../../spinbike-ui/manifest.json");

const MANIFEST_CONTENT_TYPE: &str = "application/manifest+json";

pub fn routes() -> Router<AppState> {
    Router::new().route("/manifest.json", get(manifest))
}

#[derive(Deserialize)]
struct ManifestQuery {
    /// Raw install token (`?it=<raw>`). Absent or empty → serve verbatim.
    it: Option<String>,
}

/// Build the `start_url` an install-token manifest points at. The installed
/// home-screen app opens this on its first launch; `welcome.rs` redeems the
/// `t` param and stores a permanent session, so the app is signed in.
fn install_start_url(token: &str) -> String {
    format!("/welcome?t={token}&src=install")
}

/// Return the canonical manifest with only `start_url` replaced by the
/// install-token welcome URL. Built by parsing the static manifest into a
/// `serde_json::Value` and re-setting the ONE field (never string-splicing raw
/// JSON), so every other field — `name`, `icons`, `display`, colors — is
/// preserved exactly and the output is always well-formed JSON. Returns the
/// parse/serialize error only if the compiled-in manifest is itself malformed
/// (a build-time invariant, guarded by a unit test).
fn manifest_with_install_start_url(token: &str) -> Result<String, serde_json::Error> {
    let mut value: serde_json::Value = serde_json::from_str(STATIC_MANIFEST)?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "start_url".to_string(),
            serde_json::Value::String(install_start_url(token)),
        );
    }
    serde_json::to_string(&value)
}

/// `no-store` on EVERY manifest response, token-bearing or not (#261). A
/// cached tokenless manifest served after a token was minted — or a cached
/// stale install-token manifest served past that token's real state — is a
/// silent fourth failure mode the design explicitly calls out; the manifest
/// body is cheap to rebuild on every request (`include_str!` + one JSON
/// field swap), so there is no reason to ever cache it.
fn manifest_response(body: impl Into<axum::body::Body>) -> Response {
    (
        [
            (header::CONTENT_TYPE, MANIFEST_CONTENT_TYPE),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body.into(),
    )
        .into_response()
}

/// `GET /manifest.json[?it=<raw>]`. See the module doc for the two branches.
async fn manifest(Query(query): Query<ManifestQuery>) -> Response {
    let token = query.it.filter(|t| !t.is_empty());
    match token {
        None => manifest_response(STATIC_MANIFEST),
        Some(token) => match manifest_with_install_start_url(&token) {
            Ok(body) => manifest_response(body),
            // A malformed compiled-in manifest is a build invariant violation;
            // fall back to the verbatim static manifest rather than 500 — the
            // static one is still install-eligible, just without the token.
            Err(e) => {
                tracing::error!(error = %e, "manifest: failed to build install-token manifest");
                manifest_response(STATIC_MANIFEST)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_response_always_sets_cache_control_no_store() {
        // Body content is irrelevant here — only the header matters.
        let resp = manifest_response("{}");
        assert_eq!(
            resp.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "every manifest response must set Cache-Control: no-store (#261)"
        );
    }

    #[test]
    fn static_manifest_is_valid_json_with_expected_fields() {
        let v: serde_json::Value =
            serde_json::from_str(STATIC_MANIFEST).expect("compiled-in manifest must be valid JSON");
        assert_eq!(v["name"], "SpinBike", "manifest name must be SpinBike");
        assert!(
            v["icons"].as_array().is_some_and(|a| !a.is_empty()),
            "manifest must carry icon entries"
        );
        // The verbatim manifest points at the app root (no install token).
        assert_eq!(v["start_url"], "/", "static manifest start_url must be /");
    }

    #[test]
    fn install_start_url_carries_the_exact_token_and_src() {
        assert_eq!(
            install_start_url("abc-123_XYZ"),
            "/welcome?t=abc-123_XYZ&src=install"
        );
    }

    #[test]
    fn install_manifest_swaps_only_start_url_and_preserves_everything_else() {
        let install_tok = "tok_ENVR-9xZ";
        let original: serde_json::Value = serde_json::from_str(STATIC_MANIFEST).unwrap();
        let swapped: serde_json::Value =
            serde_json::from_str(&manifest_with_install_start_url(install_tok).unwrap()).unwrap();

        // start_url now carries the install token.
        assert_eq!(
            swapped["start_url"],
            format!("/welcome?t={install_tok}&src=install")
        );

        // Every other field is byte-identical to the original — compare the two
        // objects with start_url removed from both.
        let mut a = original.as_object().unwrap().clone();
        let mut b = swapped.as_object().unwrap().clone();
        a.remove("start_url");
        b.remove("start_url");
        assert_eq!(a, b, "no field other than start_url may change");

        // Explicit spot-check that icons survived intact (the field the
        // install-eligibility heuristics actually read).
        assert_eq!(swapped["icons"], original["icons"]);
    }
}
