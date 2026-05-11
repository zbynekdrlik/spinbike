//! eWeLink Open API login.
//!
//! Authenticates with email + password + HMAC-SHA256-signed body.
//! Returns access token + region for use by the WS dispatcher.
//!
//! Protocol references:
//! - Public app credentials from HACS sonoffLAN (also documented at
//!   https://dev.ewelink.cc/). Constants must match exactly.
//! - HMAC input is the serialized JSON body; key is APP_SECRET.

use crate::ewelink::EwelinkError;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const APP_ID: &str = "oeVkj2lYFGnJu5XUtWisfW4utiN4u9Mq";
const APP_SECRET: &str = "6Nz4n0xA8s8qdxQf2GqurZj2Fs55FUvM";

/// Default region. If the login response says otherwise, the caller
/// re-issues to the indicated region.
const DEFAULT_REGION: &str = "eu";

#[derive(Debug, Clone)]
pub struct LoginResult {
    pub access_token: String,
    pub region: String,
    pub apikey: String,
}

#[derive(Serialize)]
struct LoginBody<'a> {
    email: &'a str,
    password: &'a str,
    #[serde(rename = "countryCode")]
    country_code: &'a str,
    ts: i64,
    version: u8,
    nonce: String,
    appid: &'a str,
}

#[derive(Deserialize)]
struct LoginResp {
    error: i64,
    region: Option<String>,
    #[serde(default)]
    at: String,
    #[serde(default)]
    user: UserPart,
}

#[derive(Default, Deserialize)]
struct UserPart {
    #[serde(default)]
    apikey: String,
}

/// Generate a stable HMAC signature over a serialized JSON body. Exposed
/// for unit tests against a known vector; production callers use `login`.
pub fn sign(body: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(APP_SECRET.as_bytes()).expect("hmac key");
    mac.update(body.as_bytes());
    let bytes = mac.finalize().into_bytes();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 8-char base36 nonce. `pub(crate)` so tests can assert uniqueness +
/// character set without going through the network round-trip.
pub(crate) fn random_nonce() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

/// Build the login payload + signature. Pure function — easy to test.
pub fn build_request(email: &str, password: &str, ts: i64, nonce: String) -> (String, String) {
    let body = LoginBody {
        email,
        password,
        country_code: "+421",
        ts,
        version: 8,
        nonce,
        appid: APP_ID,
    };
    let json = serde_json::to_string(&body).expect("serialize login body");
    let sig = sign(&json);
    (json, sig)
}

/// POST to the eWeLink login endpoint. On `error: 301` re-tries against
/// the indicated region. On any other non-zero error, returns Auth.
pub async fn login(
    email: &str,
    password: &str,
    region_hint: Option<&str>,
) -> Result<LoginResult, EwelinkError> {
    let region = region_hint.unwrap_or(DEFAULT_REGION).to_string();
    let base_url = format!("https://{region}-api.coolkit.cc:8080");
    login_with_base(&base_url, email, password, region_hint, &region).await
}

/// Test seam: same behaviour as `login` but with an injectable base URL
/// (used by httpmock tests). Production code always reaches it via
/// `login` which builds the real eWeLink regional endpoint.
///
/// `region_hint` and `region` are kept separate so we faithfully reproduce
/// `login`'s "pingpong" check (no second redirect allowed) even when the
/// base URL was injected by a test.
pub async fn login_with_base(
    base_url: &str,
    email: &str,
    password: &str,
    region_hint: Option<&str>,
    region: &str,
) -> Result<LoginResult, EwelinkError> {
    let ts = chrono::Utc::now().timestamp();
    let (body, sig) = build_request(email, password, ts, random_nonce());

    let url = format!("{base_url}/api/user/login");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| EwelinkError::Network(e.to_string()))?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Sign {sig}"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| EwelinkError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(EwelinkError::Auth(format!("HTTP {}", resp.status())));
    }
    let parsed: LoginResp = resp
        .json()
        .await
        .map_err(|e| EwelinkError::BadResponse(e.to_string()))?;

    if parsed.error == 301 {
        // Re-dispatch to the suggested region.
        if let Some(new_region) = parsed.region.as_deref() {
            if region_hint.is_some() {
                return Err(EwelinkError::Auth(format!(
                    "region pingpong (hint {region} → response {new_region})"
                )));
            }
            return Box::pin(login(email, password, Some(new_region))).await;
        }
        return Err(EwelinkError::Auth("error 301 without region".into()));
    }
    if parsed.error != 0 {
        return Err(EwelinkError::Auth(format!("error {}", parsed.error)));
    }
    Ok(LoginResult {
        access_token: parsed.at,
        region: parsed.region.unwrap_or_else(|| region.to_string()),
        apikey: parsed.user.apikey,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_matches_known_vector() {
        let body = r#"{"email":"x@x","password":"p","countryCode":"+421","ts":1715000000,"version":8,"nonce":"abcdefgh","appid":"oeVkj2lYFGnJu5XUtWisfW4utiN4u9Mq"}"#;
        let sig = sign(body);
        // Verified once against `openssl dgst -sha256 -hmac
        // 6Nz4n0xA8s8qdxQf2GqurZj2Fs55FUvM | base64`.
        // If this fails, the constants APP_SECRET or the JSON layout
        // changed and the change must be reviewed.
        assert_eq!(sig.len(), 44, "base64-encoded sha256 should be 44 chars");
        // Snapshot the exact vector so any future drift is loud.
        assert_eq!(sig, "9uhtXQCO/zWvmqsBLT5xDJ8o/ZY2hOu/M5QmVWNwLOc=");
    }

    #[test]
    fn build_request_round_trip() {
        let (body, sig) = build_request("x@x", "p", 1715000000, "abcdefgh".into());
        assert!(body.contains("\"email\":\"x@x\""));
        assert!(body.contains("\"appid\":\"oeVkj2lYFGnJu5XUtWisfW4utiN4u9Mq\""));
        assert!(body.contains("\"countryCode\":\"+421\""));
        assert!(body.contains("\"nonce\":\"abcdefgh\""));
        assert_eq!(sig, "9uhtXQCO/zWvmqsBLT5xDJ8o/ZY2hOu/M5QmVWNwLOc=");
    }

    /// random_nonce must vary between calls and stay inside [a-z0-9]{8}.
    /// Catches the L71 "return empty string" / "return constant" mutations.
    #[test]
    fn random_nonce_varies_and_is_base36_8() {
        let a = random_nonce();
        let b = random_nonce();
        let c = random_nonce();
        assert_eq!(a.len(), 8, "nonce should be 8 chars, got {a:?}");
        assert_eq!(b.len(), 8);
        assert_eq!(c.len(), 8);
        // Not all three identical (collision probability ~10^-24).
        assert!(
            !(a == b && b == c),
            "three consecutive nonces all identical: {a} {b} {c}"
        );
        for ch in a.chars().chain(b.chars()).chain(c.chars()) {
            assert!(
                ch.is_ascii_lowercase() || ch.is_ascii_digit(),
                "unexpected nonce char {ch:?}"
            );
        }
    }

    /// Success path: error=0, region "eu", returns the access token /
    /// region / apikey from the response. Kills the L139 "!= → ==" mutant
    /// (this test would fail if the success path were treated as an error).
    #[tokio::test]
    async fn login_success_returns_token_and_region() {
        use httpmock::prelude::*;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/user/login");
                then.status(200).json_body(serde_json::json!({
                    "error": 0,
                    "region": "eu",
                    "at": "tok123",
                    "user": { "apikey": "key456" }
                }));
            })
            .await;

        let result = login_with_base(&server.base_url(), "x@x", "p", None, "eu")
            .await
            .expect("login should succeed");
        assert_eq!(result.access_token, "tok123");
        assert_eq!(result.region, "eu");
        assert_eq!(result.apikey, "key456");
    }

    /// Non-zero error code (other than 301) must surface as Auth.
    /// Catches the L139 `!= → ==` mutant (under that mutation `error: 0`
    /// would become Auth and `error: 10` would succeed — flips both
    /// branches).
    #[tokio::test]
    async fn login_returns_auth_error_on_nonzero_error_code() {
        use httpmock::prelude::*;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/user/login");
                then.status(200).json_body(serde_json::json!({
                    "error": 10,
                    "at": "",
                    "user": { "apikey": "" }
                }));
            })
            .await;

        let result = login_with_base(&server.base_url(), "x@x", "p", None, "eu").await;
        match result {
            Err(EwelinkError::Auth(msg)) => {
                assert!(msg.contains("error 10"), "expected 'error 10' in {msg:?}")
            }
            other => panic!("expected Auth error, got {other:?}"),
        }
    }

    /// error=301 WITHOUT region falls through to the explicit
    /// "error 301 without region" Auth. Catches the L127 `== → !=` mutant
    /// (under that mutation this body would yield `error 301` via the
    /// != 0 branch instead).
    #[tokio::test]
    async fn login_fails_on_error_301_without_region() {
        use httpmock::prelude::*;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/user/login");
                then.status(200).json_body(serde_json::json!({
                    "error": 301,
                    "at": "",
                    "user": { "apikey": "" }
                }));
            })
            .await;

        let result = login_with_base(&server.base_url(), "x@x", "p", None, "eu").await;
        match result {
            Err(EwelinkError::Auth(msg)) => assert!(
                msg.contains("without region"),
                "expected 'without region' in {msg:?}"
            ),
            other => panic!("expected Auth(without region), got {other:?}"),
        }
    }

    /// HTTP 500 should hit the `!resp.status().is_success()` early-return.
    /// Catches the L119 `!` deletion (without `!` the success-check
    /// inverts and a 500 would parse as JSON instead of returning Auth).
    #[tokio::test]
    async fn login_fails_on_http_500() {
        use httpmock::prelude::*;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/user/login");
                then.status(500).body("internal server error");
            })
            .await;

        let result = login_with_base(&server.base_url(), "x@x", "p", None, "eu").await;
        match result {
            Err(EwelinkError::Auth(msg)) => {
                assert!(msg.contains("HTTP 500"), "expected 'HTTP 500' in {msg:?}")
            }
            other => panic!("expected Auth(HTTP 500), got {other:?}"),
        }
    }
}
