//! Machine-readable, stable error codes for the HTTP API surface.
//!
//! Every API error body carries a snake_case `error_code` (this enum)
//! alongside a human `error` message:
//!
//! ```json
//! { "error_code": "staff_required", "error": "Staff access required" }
//! ```
//!
//! `error_code` is the STABLE contract the UI branches on to localize error
//! banners (#145); `error` is server-authored English and may change without
//! breaking clients that key off the code. The enum lives in `spinbike-core`
//! (not the server crate) so the Leptos UI can deserialize `error_code` into
//! this same type and `match` on it.

use serde::{Deserialize, Serialize};

/// Stable, machine-readable API error code. Serializes as a snake_case string
/// (that string IS the wire contract — do not reorder or rename a variant
/// without updating the UI that matches on it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // --- 401 Unauthorized ---
    InvalidCredentials,
    OauthAccount,
    InvalidOrExpiredLink,
    /// The submitted 6-digit login code is wrong, expired, already used, or
    /// exhausted (#227). Uniform across all those causes — never leaks which.
    InvalidOrExpiredCode,
    // --- 403 Forbidden ---
    StaffRequired,
    AdminRequired,
    CardCodeStaffOnly,
    AllowSelfEntryAdminOnly,
    PasswordAdminOnly,
    BookingNotOwned,
    UserBlocked,
    // --- 404 Not Found ---
    UserNotFound,
    TransactionNotFound,
    TransactionAlreadyVoided,
    ServiceNotFound,
    BookingNotFound,
    // --- 409 Conflict ---
    EmailConflict,
    CardCodeConflict,
    EmailOrCardConflict,
    ClassFull,
    ClassCancelled,
    NoteOnVoidedTransaction,
    DateOnVoidedTransaction,
    NoActiveMonthlyPass,
    MonthlyPassExists,
    UserAlreadyDeleted,
    /// The submitted email is held by a SOFT-DELETED account (#143). Unlike
    /// `EmailConflict` (a live collision) this is RESOLVABLE by the staff UI:
    /// restore the old account, or free its email. The body carries the
    /// archived account's identity (`conflict_id` / `conflict_name` /
    /// `conflict_deleted_at`).
    EmailBelongsToDeletedAccount,
    // --- 400 Bad Request (the human message carries the specifics) ---
    BadRequest,
    // --- 429 Too Many Requests ---
    /// Rate limit hit on a public auth endpoint — e.g. too many login-code
    /// verify attempts (#227). Slow down and retry later.
    TooManyRequests,
    // --- 503 Service Unavailable ---
    MailNotConfigured,
    // --- 500 Internal ---
    Internal,
}

impl ErrorCode {
    /// The canonical, server-authored English message for this code — the
    /// single place each error string lives. Used as the `error` body field
    /// for permission / not-found / conflict / unauthorized codes whose
    /// message is fixed. Dynamic errors (`BadRequest`, and conflicts built
    /// with an explicit message such as a full-class notice) carry their own
    /// message and ignore this default.
    pub const fn message(self) -> &'static str {
        match self {
            ErrorCode::InvalidCredentials => "Invalid email or password",
            ErrorCode::OauthAccount => "Account uses OAuth login",
            ErrorCode::InvalidOrExpiredLink => "Invalid or expired link",
            ErrorCode::InvalidOrExpiredCode => "Invalid or expired code",
            ErrorCode::StaffRequired => "Staff access required",
            ErrorCode::AdminRequired => "Admin access required",
            ErrorCode::CardCodeStaffOnly => "Only staff can modify card_code",
            ErrorCode::AllowSelfEntryAdminOnly => "Only admin can modify allow_self_entry",
            ErrorCode::PasswordAdminOnly => "Only admin can set another user's password",
            ErrorCode::BookingNotOwned => "Cannot cancel another user's booking",
            ErrorCode::UserBlocked => "User is blocked",
            ErrorCode::UserNotFound => "User not found",
            ErrorCode::TransactionNotFound => "Transaction not found",
            ErrorCode::TransactionAlreadyVoided => "Transaction already voided",
            ErrorCode::ServiceNotFound => "Service not found",
            ErrorCode::BookingNotFound => "Booking not found",
            ErrorCode::EmailConflict => "A user with this email already exists",
            ErrorCode::CardCodeConflict => "A user with this card code already exists",
            ErrorCode::EmailOrCardConflict => "A user with this email or card code already exists",
            ErrorCode::ClassFull => "Class is full",
            ErrorCode::ClassCancelled => "Class is cancelled",
            ErrorCode::NoteOnVoidedTransaction => "Cannot edit note on a voided transaction",
            ErrorCode::DateOnVoidedTransaction => "Cannot edit date on a voided transaction",
            ErrorCode::NoActiveMonthlyPass => {
                "User has no active monthly pass; use /api/payments/charge"
            }
            ErrorCode::MonthlyPassExists => "a monthly_pass service already exists",
            ErrorCode::UserAlreadyDeleted => "User already deleted",
            ErrorCode::EmailBelongsToDeletedAccount => "This email belongs to a deleted account",
            ErrorCode::BadRequest => "Bad request",
            ErrorCode::TooManyRequests => "Too many attempts, please try again later",
            ErrorCode::MailNotConfigured => "mail_not_configured",
            ErrorCode::Internal => "Internal server error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant, with the exact snake_case wire string it MUST serialize
    /// to and its canonical message. This table is the contract; a change to
    /// either column is a deliberate wire-format change, not an accident.
    /// Also serves as the mutation-testing kill for `message()` and the serde
    /// rename mapping.
    const ALL: &[(ErrorCode, &str, &str)] = &[
        (
            ErrorCode::InvalidCredentials,
            "invalid_credentials",
            "Invalid email or password",
        ),
        (
            ErrorCode::OauthAccount,
            "oauth_account",
            "Account uses OAuth login",
        ),
        (
            ErrorCode::InvalidOrExpiredLink,
            "invalid_or_expired_link",
            "Invalid or expired link",
        ),
        (
            ErrorCode::InvalidOrExpiredCode,
            "invalid_or_expired_code",
            "Invalid or expired code",
        ),
        (
            ErrorCode::StaffRequired,
            "staff_required",
            "Staff access required",
        ),
        (
            ErrorCode::AdminRequired,
            "admin_required",
            "Admin access required",
        ),
        (
            ErrorCode::CardCodeStaffOnly,
            "card_code_staff_only",
            "Only staff can modify card_code",
        ),
        (
            ErrorCode::AllowSelfEntryAdminOnly,
            "allow_self_entry_admin_only",
            "Only admin can modify allow_self_entry",
        ),
        (
            ErrorCode::PasswordAdminOnly,
            "password_admin_only",
            "Only admin can set another user's password",
        ),
        (
            ErrorCode::BookingNotOwned,
            "booking_not_owned",
            "Cannot cancel another user's booking",
        ),
        (ErrorCode::UserBlocked, "user_blocked", "User is blocked"),
        (ErrorCode::UserNotFound, "user_not_found", "User not found"),
        (
            ErrorCode::TransactionNotFound,
            "transaction_not_found",
            "Transaction not found",
        ),
        (
            ErrorCode::TransactionAlreadyVoided,
            "transaction_already_voided",
            "Transaction already voided",
        ),
        (
            ErrorCode::ServiceNotFound,
            "service_not_found",
            "Service not found",
        ),
        (
            ErrorCode::BookingNotFound,
            "booking_not_found",
            "Booking not found",
        ),
        (
            ErrorCode::EmailConflict,
            "email_conflict",
            "A user with this email already exists",
        ),
        (
            ErrorCode::CardCodeConflict,
            "card_code_conflict",
            "A user with this card code already exists",
        ),
        (
            ErrorCode::EmailOrCardConflict,
            "email_or_card_conflict",
            "A user with this email or card code already exists",
        ),
        (ErrorCode::ClassFull, "class_full", "Class is full"),
        (
            ErrorCode::ClassCancelled,
            "class_cancelled",
            "Class is cancelled",
        ),
        (
            ErrorCode::NoteOnVoidedTransaction,
            "note_on_voided_transaction",
            "Cannot edit note on a voided transaction",
        ),
        (
            ErrorCode::DateOnVoidedTransaction,
            "date_on_voided_transaction",
            "Cannot edit date on a voided transaction",
        ),
        (
            ErrorCode::NoActiveMonthlyPass,
            "no_active_monthly_pass",
            "User has no active monthly pass; use /api/payments/charge",
        ),
        (
            ErrorCode::MonthlyPassExists,
            "monthly_pass_exists",
            "a monthly_pass service already exists",
        ),
        (
            ErrorCode::UserAlreadyDeleted,
            "user_already_deleted",
            "User already deleted",
        ),
        (
            ErrorCode::EmailBelongsToDeletedAccount,
            "email_belongs_to_deleted_account",
            "This email belongs to a deleted account",
        ),
        (ErrorCode::BadRequest, "bad_request", "Bad request"),
        (
            ErrorCode::TooManyRequests,
            "too_many_requests",
            "Too many attempts, please try again later",
        ),
        (
            ErrorCode::MailNotConfigured,
            "mail_not_configured",
            "mail_not_configured",
        ),
        (ErrorCode::Internal, "internal", "Internal server error"),
    ];

    #[test]
    fn code_serializes_to_snake_case_and_roundtrips() {
        for (code, wire, _msg) in ALL {
            let json = serde_json::to_value(code).unwrap();
            assert_eq!(
                json,
                serde_json::Value::String((*wire).to_string()),
                "{code:?}"
            );
            let back: ErrorCode = serde_json::from_value(json).unwrap();
            assert_eq!(back, *code, "roundtrip {code:?}");
        }
    }

    #[test]
    fn message_matches_table() {
        for (code, _wire, msg) in ALL {
            assert_eq!(code.message(), *msg, "message for {code:?}");
        }
    }
}
