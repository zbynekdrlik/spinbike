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

fn random_nonce() -> String {
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
    let ts = chrono::Utc::now().timestamp();
    let (body, sig) = build_request(email, password, ts, random_nonce());

    let url = format!("https://{region}-api.coolkit.cc:8080/api/user/login");
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
        region: parsed.region.unwrap_or_else(|| region.clone()),
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
}
