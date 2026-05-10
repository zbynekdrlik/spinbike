//! App-wide utility helpers shared across pages (e.g. money parsing used by
//! both the dashboard charge/topup forms and the admin service price forms).

use std::cell::Cell;
use std::rc::Rc;

/// Stale-response guard for the `Effect::new + spawn_local` fetch pattern.
///
/// When a trigger signal increments faster than a fetch can complete, both
/// futures resolve and the last-arriving response wins — possibly stale.
/// Tag each fetch with a token from `next()`; the spawned task checks
/// `is_latest()` before applying the result and bails if a newer token has
/// been minted in the meantime. See #66.
///
/// Usage:
/// ```ignore
/// let req_id = RequestId::new();
/// Effect::new(move |_| {
///     let _ = trigger.get();
///     let token = req_id.next();
///     spawn_local(async move {
///         let result = api::get::<Foo>("/api/foo").await;
///         if !token.is_latest() {
///             return; // newer request superseded this one
///         }
///         // apply result
///     });
/// });
/// ```
#[derive(Clone, Default)]
pub struct RequestId(Rc<Cell<u32>>);

impl RequestId {
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(0)))
    }

    /// Mint a new token; the inner counter now holds this token's value.
    /// Subsequent calls to `next()` invalidate this token's `is_latest()`.
    pub fn next(&self) -> RequestToken {
        let v = self.0.get().wrapping_add(1);
        self.0.set(v);
        RequestToken {
            id: v,
            latest: self.0.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RequestToken {
    id: u32,
    latest: Rc<Cell<u32>>,
}

impl RequestToken {
    /// True iff no newer token has been minted from the parent `RequestId`.
    pub fn is_latest(&self) -> bool {
        self.latest.get() == self.id
    }
}

/// Parse a user-entered money string, accepting both `.` and `,` as the decimal
/// separator — Slovak keyboards produce comma by default, European users expect
/// it to work. Trims whitespace. Returns `None` on empty or invalid input so
/// callers can decide the fallback.
pub fn parse_money(s: &str) -> Option<f64> {
    let normalized = s.trim().replace(',', ".");
    if normalized.is_empty() {
        None
    } else {
        normalized.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod request_id_tests {
    use super::RequestId;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn first_token_is_latest() {
        let id = RequestId::new();
        let t = id.next();
        assert!(t.is_latest());
    }

    #[wasm_bindgen_test]
    fn newer_token_invalidates_older() {
        let id = RequestId::new();
        let t1 = id.next();
        let t2 = id.next();
        assert!(!t1.is_latest());
        assert!(t2.is_latest());
    }

    #[wasm_bindgen_test]
    fn cloned_request_id_shares_counter() {
        let id = RequestId::new();
        let id2 = id.clone();
        let t = id.next();
        // Mint via the clone — should still invalidate `t`.
        let _ = id2.next();
        assert!(!t.is_latest());
    }

    #[wasm_bindgen_test]
    fn token_is_clone_for_async_capture() {
        let id = RequestId::new();
        let t = id.next();
        let t2 = t.clone();
        assert!(t.is_latest());
        assert!(t2.is_latest());
    }
}

#[cfg(test)]
mod parse_money_tests {
    use super::parse_money;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn plain_integer() {
        assert_eq!(parse_money("40"), Some(40.0));
    }

    #[wasm_bindgen_test]
    fn dot_decimal() {
        assert_eq!(parse_money("35.50"), Some(35.5));
    }

    #[wasm_bindgen_test]
    fn comma_decimal_is_normalized() {
        assert_eq!(parse_money("35,50"), Some(35.5));
    }

    #[wasm_bindgen_test]
    fn whitespace_trimmed() {
        assert_eq!(parse_money("  12.3  "), Some(12.3));
    }

    #[wasm_bindgen_test]
    fn empty_is_none() {
        assert_eq!(parse_money(""), None);
        assert_eq!(parse_money("   "), None);
    }

    #[wasm_bindgen_test]
    fn garbage_is_none() {
        assert_eq!(parse_money("abc"), None);
        assert_eq!(parse_money("1,2,3"), None);
    }
}
