//! Shared `sessionStorage` primitives (#287).
//!
//! Extracted out of `auth.rs` (where the one-shot session-expired flag
//! originally lived, #275) and `components::install_prompt` (where the
//! install-token mint guard originally lived, #261) — both hand-rolled the
//! identical `window()->session_storage()->get/set_item` JS interop chain
//! independently. Two shapes are needed:
//!
//! - **one-shot flag** ([`flag_set`]/[`flag_take`]): set once, read-and-
//!   cleared exactly once — a later read (e.g. a manual reload) never
//!   re-observes it. Used by `auth.rs`'s session-expired notice.
//! - **cache** ([`cache_get`]/[`cache_set`]): plain get/set, no removal on
//!   read — the value is meant to be reused across multiple reads within the
//!   same tab session. Used by `install_prompt.rs`'s install-token mint
//!   guard.
//!
//! Both degrade to a silent no-op / `None` on any storage failure (private
//! browsing, a security exception, or — in `wasm-pack test --node` unit
//! tests — no `window` at all), same discipline as every other storage call
//! in this crate. Mirrors the `platform.rs` precedent: a small shared
//! primitive extracted because two independent call sites hand-rolled the
//! same JS interop chain.

fn session_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.session_storage().ok()?
}

/// Set the one-shot flag at `key`. Silent no-op on any storage failure.
pub fn flag_set(key: &str) {
    if let Some(s) = session_storage() {
        let _ = s.set_item(key, "1");
    }
}

/// Read the one-shot flag at `key` AND clear it in the same call, so a later
/// read (e.g. a manual reload) never re-observes it.
pub fn flag_take(key: &str) -> bool {
    let Some(s) = session_storage() else {
        return false;
    };
    let present = matches!(s.get_item(key), Ok(Some(_)));
    if present {
        let _ = s.remove_item(key);
    }
    present
}

/// Read the cached value at `key` — never cleared on read.
pub fn cache_get(key: &str) -> Option<String> {
    session_storage()?.get_item(key).ok()?
}

/// Persist `value` into the cache at `key`. Silent no-op on any storage
/// failure — losing the write only means a future read misses the cache,
/// not a correctness break.
pub fn cache_set(key: &str, value: &str) {
    if let Some(s) = session_storage() {
        let _ = s.set_item(key, value);
    }
}

/// Explicitly clear the cached value at `key` (unlike [`cache_get`], which
/// never removes on read). Silent no-op on any storage failure or if the key
/// was never set. Added for `install_prompt.rs`'s cross-instance mint-claim
/// marker (#282 recurrence): unlike the one-shot [`flag_take`] shape
/// (read-and-clear together), that caller needs to WRITE the claim in one
/// call and clear it in a LATER, separate call (on an observed failure).
pub fn cache_remove(key: &str) {
    if let Some(s) = session_storage() {
        let _ = s.remove_item(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    /// `wasm-pack test --node` has no `window`/`sessionStorage` at all (a
    /// real Node.js runtime, not a browser — see `auth.rs`'s equivalent
    /// #275 test for the full explanation). Both shapes MUST gracefully
    /// no-op rather than panic when storage is unavailable, same as every
    /// other storage call in this crate. The real set-then-read(-then-
    /// cleared) round trips are verified by the Playwright E2E suites
    /// (`session-invalidation.spec.ts` #275, `install-prompt.spec.ts` #261),
    /// which `--node` unit tests structurally cannot cover.
    #[wasm_bindgen_test]
    fn flag_helpers_degrade_gracefully_without_a_window() {
        flag_set("test_flag");
        assert!(
            !flag_take("test_flag"),
            "no window/sessionStorage in this test runtime — must no-op, not panic"
        );
    }

    #[wasm_bindgen_test]
    fn cache_helpers_degrade_gracefully_without_a_window() {
        cache_set("test_cache", "v");
        assert_eq!(
            cache_get("test_cache"),
            None,
            "no window/sessionStorage in this test runtime — must no-op, not panic"
        );
    }

    #[wasm_bindgen_test]
    fn cache_remove_degrades_gracefully_without_a_window() {
        cache_set("test_cache_remove", "v");
        cache_remove("test_cache_remove");
        assert_eq!(
            cache_get("test_cache_remove"),
            None,
            "no window/sessionStorage in this test runtime — must no-op, not panic"
        );
    }
}
