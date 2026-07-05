use serde::{Deserialize, Serialize};
use spinbike_core::auth::Role;

const TOKEN_KEY: &str = "spinbike_token";
const USER_KEY: &str = "spinbike_user";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInfo {
    pub id: i64,
    pub email: String,
    pub name: String,
    /// Typed role. Serialized/deserialized as the same lowercase string the
    /// server sends and localStorage already holds (`Role`'s serde form), so
    /// existing stored sessions parse unchanged; an unrecognised legacy value
    /// falls back to `Role::Unknown` (treated as non-privileged) instead of
    /// hard-failing the whole `get_user()` deserialization (#98).
    pub role: Role,
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
        let _ = s.set_item(
            USER_KEY,
            &serde_json::to_string(&data.user).unwrap_or_default(),
        );
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
        .map(|u| u.role.is_staff_or_admin())
        .unwrap_or(false)
}

pub fn is_admin() -> bool {
    get_user().map(|u| u.role.is_admin()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    /// Legacy localStorage payloads store `role` as a lowercase string
    /// (`"admin"` / `"staff"` / `"customer"`). Deserializing that shape into
    /// the now-typed `UserInfo` MUST yield the matching `Role` variant, so
    /// existing signed-in sessions keep working after the migration (#98).
    #[wasm_bindgen_test]
    fn user_info_parses_legacy_localstorage_role_strings() {
        for (s, expected) in [
            ("admin", Role::Admin),
            ("staff", Role::Staff),
            ("customer", Role::Customer),
        ] {
            let json = format!(r#"{{"id":1,"email":"a@b.com","name":"N","role":"{s}"}}"#);
            let ui: UserInfo = serde_json::from_str(&json).unwrap();
            assert_eq!(ui.role, expected);
        }
    }

    /// Forward-compat: a role the WASM frontend doesn't know (a future server
    /// tier) MUST deserialize to `Role::Unknown` rather than failing the whole
    /// `UserInfo` parse — otherwise `get_user()` would return `None` and log
    /// the user out.
    #[wasm_bindgen_test]
    fn user_info_parses_unknown_role_as_unknown() {
        let json = r#"{"id":1,"email":"a@b.com","name":"N","role":"trainer"}"#;
        let ui: UserInfo = serde_json::from_str(json).unwrap();
        assert_eq!(ui.role, Role::Unknown);
        assert!(!ui.role.is_staff_or_admin());
        assert!(!ui.role.is_admin());
    }

    /// Round-trip through `set_auth`'s serialization: a `UserInfo` serializes
    /// `role` to the same lowercase string, so what we write to localStorage
    /// is the exact shape the server produced.
    #[wasm_bindgen_test]
    fn user_info_serializes_role_to_lowercase() {
        let ui = UserInfo {
            id: 1,
            email: "a@b.com".into(),
            name: "N".into(),
            role: Role::Admin,
        };
        let json = serde_json::to_string(&ui).unwrap();
        assert!(json.contains(r#""role":"admin""#), "got {json}");
    }
}
