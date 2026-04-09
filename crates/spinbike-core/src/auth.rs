use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Staff,
    Customer,
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
