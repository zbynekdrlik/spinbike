use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::auth::Role;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub phone: Option<String>,
    pub role: Role,
    pub password_hash: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: i64,
    pub user_id: i64,
    pub total_entries: i32,
    pub remaining_entries: i32,
    pub service_id: i64,
    pub purchased_at: NaiveDateTime,
    pub expires_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: i64,
    pub name: String,
    pub entries: i32,
    pub price_czk: i32,
    pub valid_days: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionAction {
    Purchase,
    Deduct,
    Refund,
    Expire,
    ManualAdjust,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub card_id: i64,
    pub action: TransactionAction,
    pub entries_delta: i32,
    pub note: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instructor {
    pub id: i64,
    pub user_id: i64,
    pub bio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassTemplate {
    pub id: i64,
    pub name: String,
    pub instructor_id: i64,
    pub day_of_week: i32,
    pub start_time: String,
    pub duration_minutes: i32,
    pub capacity: i32,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassOccurrence {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub cancelled: bool,
    pub cancel_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Booking {
    pub id: i64,
    pub occurrence_id: i64,
    pub user_id: i64,
    pub card_id: i64,
    pub booked_at: NaiveDateTime,
    pub cancelled_at: Option<NaiveDateTime>,
}
