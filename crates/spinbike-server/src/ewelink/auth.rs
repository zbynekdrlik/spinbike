//! HMAC-SHA256 login + region routing — full implementation in Task 6.

use crate::ewelink::EwelinkError;

pub struct LoginResult {
    pub access_token: String,
    pub region: String,
    pub apikey: String,
}

/// Stub. Real impl in Task 6.
#[allow(dead_code)]
pub async fn login(
    _email: &str,
    _password: &str,
    _region_hint: Option<&str>,
) -> Result<LoginResult, EwelinkError> {
    Err(EwelinkError::Auth("not implemented yet (Task 6)".into()))
}
