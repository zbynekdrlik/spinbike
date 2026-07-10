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

    /// Staff-or-admin predicate for nav/routing gates (distinct from the
    /// `can_*` permission checks — this expresses "sees the staff surface",
    /// not a specific capability). `Unknown` is treated as non-privileged.
    pub fn is_staff_or_admin(&self) -> bool {
        matches!(self, Role::Admin | Role::Staff)
    }

    /// Admin-only predicate for nav/routing gates. `Unknown` → false.
    pub fn is_admin(&self) -> bool {
        matches!(self, Role::Admin)
    }
}

impl std::fmt::Display for Role {
    /// Lowercase wire form — MUST match the `#[serde(rename_all = "lowercase")]`
    /// serialization exactly so `Role` and the raw DB/JSON strings stay
    /// interchangeable at every boundary (server `UserResponse`/`UserInfo`,
    /// localStorage). `Unknown` renders as "unknown" (its serde form).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Role::Admin => "admin",
            Role::Staff => "staff",
            Role::Customer => "customer",
            Role::Unknown => "unknown",
        })
    }
}

impl From<&str> for Role {
    /// Total, infallible String → Role conversion that mirrors serde's
    /// `#[serde(other)]`: every string that isn't a known lowercase role maps
    /// to `Role::Unknown`. Use this at DB/wire boundaries so a `String` role
    /// round-trips to the exact same string it came from (for the three known
    /// roles) while gaining the forward-compat `Unknown` fallback.
    fn from(s: &str) -> Self {
        match s {
            "admin" => Role::Admin,
            "staff" => Role::Staff,
            "customer" => Role::Customer,
            _ => Role::Unknown,
        }
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

    /// `Display` MUST render exactly the serde-serialized string for every
    /// variant — this is the wire-compat invariant that lets a `Role` field
    /// replace a `String` role without changing any JSON payload.
    #[test]
    fn role_display_matches_serde_serialization() {
        for role in [Role::Admin, Role::Staff, Role::Customer, Role::Unknown] {
            let serde_str = serde_json::to_value(&role).unwrap();
            let serde_str = serde_str.as_str().unwrap();
            assert_eq!(
                role.to_string(),
                serde_str,
                "Display for {role:?} must equal its serde form"
            );
        }
    }

    /// `From<&str>` MUST agree with serde deserialization for known roles AND
    /// map any unknown string to `Role::Unknown` (mirrors `#[serde(other)]`).
    #[test]
    fn role_from_str_matches_serde_deserialization() {
        for s in ["admin", "staff", "customer", "trainer", "", "Admin"] {
            let via_from = Role::from(s);
            let via_serde: Role = serde_json::from_str(&format!("\"{s}\"")).unwrap();
            assert_eq!(via_from, via_serde, "From vs serde disagree for {s:?}");
        }
        // Explicit unknown-fallback pins.
        assert_eq!(Role::from("trainer"), Role::Unknown);
        assert_eq!(Role::from(""), Role::Unknown);
    }

    /// The known roles round-trip String → Role → String byte-identically,
    /// which is the property that keeps existing JSON/localStorage payloads
    /// unchanged after the migration.
    #[test]
    fn role_roundtrips_through_string() {
        for s in ["admin", "staff", "customer"] {
            assert_eq!(Role::from(s).to_string(), s);
        }
        for role in [Role::Admin, Role::Staff, Role::Customer] {
            assert_eq!(Role::from(role.to_string().as_str()), role);
        }
    }

    #[test]
    fn is_admin_and_is_staff_or_admin_helpers() {
        assert!(Role::Admin.is_admin());
        assert!(!Role::Staff.is_admin());
        assert!(!Role::Customer.is_admin());
        assert!(!Role::Unknown.is_admin());

        assert!(Role::Admin.is_staff_or_admin());
        assert!(Role::Staff.is_staff_or_admin());
        assert!(!Role::Customer.is_staff_or_admin());
        assert!(!Role::Unknown.is_staff_or_admin());
    }

    #[test]
    fn unknown_role_is_not_privileged() {
        // The fallback must NEVER widen permissions. Every can_* check is
        // an explicit Admin/Staff match; Unknown falls through to false.
        let r = Role::Unknown;
        assert!(!r.can_manage_cards());
        assert!(!r.can_book_for_others());
        assert!(!r.can_cancel_any_booking());
        assert!(!r.can_process_payments());
        assert!(!r.can_cancel_class());
        assert!(!r.can_manage_users());
    }
}
