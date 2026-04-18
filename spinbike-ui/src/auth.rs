use serde::{Deserialize, Serialize};

const TOKEN_KEY: &str = "spinbike_token";
const USER_KEY: &str = "spinbike_user";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInfo {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthData {
    pub token: String,
    pub user: UserInfo,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

pub fn get_token() -> Option<String> {
    storage()?.get_item(TOKEN_KEY).ok()?
}

pub fn get_user() -> Option<UserInfo> {
    let s = storage()?.get_item(USER_KEY).ok()??;
    serde_json::from_str(&s).ok()
}

pub fn set_auth(data: &AuthData) {
    if let Some(s) = storage() {
        let _ = s.set_item(TOKEN_KEY, &data.token);
        let _ = s.set_item(USER_KEY, &serde_json::to_string(&data.user).unwrap_or_default());
    }
}

pub fn clear_auth() {
    if let Some(s) = storage() {
        let _ = s.remove_item(TOKEN_KEY);
        let _ = s.remove_item(USER_KEY);
    }
}

pub fn is_staff_or_admin() -> bool {
    get_user()
        .map(|u| u.role == "staff" || u.role == "admin")
        .unwrap_or(false)
}

pub fn is_admin() -> bool {
    get_user()
        .map(|u| u.role == "admin")
        .unwrap_or(false)
}
