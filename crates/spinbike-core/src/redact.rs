//! Query-string token redaction, shared by the server (request-logging
//! middleware, #260) and the frontend (standalone-launch beacon, #260) so
//! the exact same truncation rule backs both — a security-relevant piece of
//! logic that must never drift between the two independent copies it would
//! otherwise become.
//!
//! Round 4 of the iOS install auto-login fix (#258) failed and was
//! undiagnosable because the server logged nothing at all. The fix is
//! request logging — but a login token (`t=`/`it=`) or a one-time login
//! code (`code=`) must NEVER reach a log line at full length. This module is
//! the ONE place that decides what "redacted" means for a query string.

/// Query-param keys whose VALUE gets truncated. Any other key passes through
/// untouched — this is a logging aid, not general PII scrubbing.
const REDACTED_KEYS: [&str; 3] = ["t", "it", "code"];

/// How many leading characters of a redacted value are kept.
const KEEP_CHARS: usize = 8;

/// Redact a raw (not percent-decoded) HTTP query string: for every `k=v`
/// pair whose key is `t`, `it`, or `code`, replace `v` with its first 8
/// characters followed by `...` (a shorter value is truncated to whatever it
/// has, `...` still appended — the marker itself signals "this was
/// redacted", not "there was more"). Every other pair — including a
/// redacted key with an EMPTY value, and any key/value with no `=` at all —
/// passes through byte-for-byte unchanged.
///
/// Deliberately operates on the raw string (no percent-decoding, no query
/// parser): a login token is URL-safe base64/hex and never percent-encodes
/// its own `&`/`=`, so this is exact for the real shape while staying a
/// trivial, obviously-correct string transform — the one thing that must
/// never itself become a source of bugs given how load-bearing it is.
///
/// An empty input returns an empty string.
pub fn redact_query(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) if REDACTED_KEYS.contains(&k) && !v.is_empty() => {
                let truncated: String = v.chars().take(KEEP_CHARS).collect();
                format!("{k}={truncated}...")
            }
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing property (#260 acceptance: "a full token never
    /// appears in the journal; only an 8-char prefix"): a long token value
    /// is truncated, and — critically — the ORIGINAL full value is provably
    /// absent from the output.
    #[test]
    fn long_token_value_is_truncated_to_eight_chars_and_full_value_is_gone() {
        let full = "abcdefghijklmnopqrstuvwxyz0123456789";
        let query = format!("t={full}");
        let redacted = redact_query(&query);
        assert_eq!(redacted, "t=abcdefgh...");
        assert!(
            !redacted.contains(full),
            "the full raw token must never appear in the redacted output"
        );
    }

    #[test]
    fn all_three_redacted_keys_are_truncated() {
        assert_eq!(redact_query("it=1234567890"), "it=12345678...");
        assert_eq!(redact_query("code=987654321000"), "code=98765432...");
    }

    #[test]
    fn value_shorter_than_keep_chars_is_kept_whole_but_still_marked_truncated() {
        // Fewer than 8 chars: take(8) just yields what's there; the "..."
        // marker still appears (it signals "this field was redacted", not
        // "there was more to cut").
        assert_eq!(redact_query("t=abc"), "t=abc...");
    }

    #[test]
    fn non_token_keys_pass_through_untouched() {
        assert_eq!(redact_query("src=install"), "src=install");
        assert_eq!(
            redact_query("path=/my/balance&src=install"),
            "path=/my/balance&src=install"
        );
    }

    #[test]
    fn mixed_query_redacts_only_the_token_shaped_keys() {
        let query = "src=install&t=abcdefgh12345&other=keep-me";
        assert_eq!(
            redact_query(query),
            "src=install&t=abcdefgh...&other=keep-me"
        );
    }

    #[test]
    fn empty_value_for_a_redacted_key_is_left_as_is() {
        // `t=` with nothing after it — nothing to redact, and no value
        // means no "..." marker should be invented either.
        assert_eq!(redact_query("t="), "t=");
    }

    #[test]
    fn key_with_no_value_at_all_passes_through() {
        assert_eq!(redact_query("flag"), "flag");
    }

    #[test]
    fn empty_query_returns_empty_string() {
        assert_eq!(redact_query(""), "");
    }
}
