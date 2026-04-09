use serde::{Deserialize, Serialize};

/// OAuth callback parameters from the provider redirect.
#[derive(Debug, Clone, Deserialize)]
pub struct OAuthCallback {
    pub code: String,
    pub state: Option<String>,
}

/// User info extracted from an OAuth provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthUserInfo {
    pub provider: String,
    pub oauth_id: String,
    pub email: String,
    pub name: String,
}
