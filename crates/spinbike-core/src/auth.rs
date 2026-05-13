use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Staff,
    Customer,
    /// Forward-compat catch-all. A newer server tier may introduce roles
    /// (e.g. Trainer, Receptionist) that an older WASM frontend doesn't
    /// know about yet. Without this variant, deserialization of any user
    /// payload carrying an unknown role would hard-fail the whole struct
    /// (breaking /api/users/lookup, dashboard search, etc.). Mapping
    /// unknowns to a neutral variant keeps the UI permissive: privilege
    /// checks (`can_manage_*` and the dashboard's role-gated controls)
    /// all rely on explicit `Admin | Staff` matches, so an Unknown user
    /// is naturally treated as customer-mode — the safe default.
    #[serde(other)]
    Unknown,
}

impl Role {
    pub fn can_manage_templates(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_manage_cards(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_book_for_others(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_cancel_any_booking(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_process_payments(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_cancel_class(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    pub fn can_manage_users(&self) -> bool {
        matches!(self, Role::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub email: String,
    pub role: Role,
    pub exp: i64,
    pub iat: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_deserializes_known_variants_lowercase() {
        assert_eq!(
            serde_json::from_str::<Role>(r#""admin""#).unwrap(),
            Role::Admin
        );
        assert_eq!(
            serde_json::from_str::<Role>(r#""staff""#).unwrap(),
            Role::Staff
        );
        assert_eq!(
            serde_json::from_str::<Role>(r#""customer""#).unwrap(),
            Role::Customer
        );
    }

    #[test]
    fn role_deserializes_unknown_string_as_unknown_variant() {
        // Forward-compat: a future server adds Trainer/Receptionist; the
        // older WASM frontend MUST still deserialize the surrounding user
        // payload instead of hard-failing the whole struct.
        assert_eq!(
            serde_json::from_str::<Role>(r#""trainer""#).unwrap(),
            Role::Unknown
        );
        assert_eq!(
            serde_json::from_str::<Role>(r#""receptionist""#).unwrap(),
            Role::Unknown
        );
    }

    #[test]
    fn unknown_role_is_not_privileged() {
        // The fallback must NEVER widen permissions. Every can_* check is
        // an explicit Admin/Staff match; Unknown falls through to false.
        let r = Role::Unknown;
        assert!(!r.can_manage_templates());
        assert!(!r.can_manage_cards());
        assert!(!r.can_book_for_others());
        assert!(!r.can_cancel_any_booking());
        assert!(!r.can_process_payments());
        assert!(!r.can_cancel_class());
        assert!(!r.can_manage_users());
    }
}
