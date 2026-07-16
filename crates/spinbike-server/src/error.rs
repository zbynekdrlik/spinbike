//! Typed HTTP error layer for the API surface.
//!
//! One `ApiError` type replaces the ad-hoc `(StatusCode, Json<serde_json::Value>)`
//! tuples every handler used to hand-build. It implements `IntoResponse` and
//! serializes a consistent body:
//!
//! ```json
//! { "error_code": "staff_required", "error": "Staff access required" }
//! ```
//!
//! - `error_code` is the STABLE machine code (`spinbike_core::errors::ErrorCode`)
//!   the UI branches on to localize banners (#145).
//! - `error` is the human message — kept for backward compatibility, since
//!   many integration tests and the current UI read it.
//!
//! A `Conflict` may carry extra flattened fields (e.g. `conflict_name` /
//! `conflict_card` for the email-collision staff UI).

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use spinbike_core::errors::ErrorCode;

/// A typed HTTP error. Every handler returns `Result<Json<T>, ApiError>`.
#[derive(Debug)]
pub enum ApiError {
    /// 401 — message from the code.
    Unauthorized(ErrorCode),
    /// 403 — message from the code.
    Forbidden(ErrorCode),
    /// 404 — message from the code.
    NotFound(ErrorCode),
    /// 409 — message from the code unless `message` overrides it (dynamic
    /// conflicts); `extra` fields are flattened into the body.
    Conflict {
        code: ErrorCode,
        message: Option<String>,
        extra: Option<serde_json::Value>,
    },
    /// 400 — the message carries the validation specifics; code is `bad_request`.
    BadRequest(String),
    /// 429 — rate limit hit on a public endpoint; message from the code.
    TooManyRequests(ErrorCode),
    /// 503 — message from the code.
    ServiceUnavailable(ErrorCode),
    /// 500 — the real error is logged at construction (`routes::internal_error`);
    /// the client gets a generic body with no implementation detail leaked.
    Internal,
}

impl ApiError {
    /// 409 with the code's default message and no extra fields.
    pub fn conflict(code: ErrorCode) -> Self {
        ApiError::Conflict {
            code,
            message: None,
            extra: None,
        }
    }

    /// 409 with an explicit (dynamic) message — e.g. a full-class notice whose
    /// text comes from the booking layer.
    pub fn conflict_message(code: ErrorCode, message: impl Into<String>) -> Self {
        ApiError::Conflict {
            code,
            message: Some(message.into()),
            extra: None,
        }
    }

    /// 409 with the code's default message plus extra flattened fields (e.g.
    /// `conflict_name` / `conflict_card` for the staff email-collision UI).
    pub fn conflict_extra(code: ErrorCode, extra: serde_json::Value) -> Self {
        ApiError::Conflict {
            code,
            message: None,
            extra: Some(extra),
        }
    }

    /// Resolve to the wire status + JSON body. Split out from `into_response`
    /// so the mapping is unit-testable without reading an async body.
    fn parts(self) -> (StatusCode, serde_json::Value) {
        let (status, code, message, extra) = match self {
            ApiError::Unauthorized(c) => {
                (StatusCode::UNAUTHORIZED, c, c.message().to_string(), None)
            }
            ApiError::Forbidden(c) => (StatusCode::FORBIDDEN, c, c.message().to_string(), None),
            ApiError::NotFound(c) => (StatusCode::NOT_FOUND, c, c.message().to_string(), None),
            ApiError::Conflict {
                code,
                message,
                extra,
            } => (
                StatusCode::CONFLICT,
                code,
                message.unwrap_or_else(|| code.message().to_string()),
                extra,
            ),
            ApiError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, ErrorCode::BadRequest, msg, None)
            }
            ApiError::TooManyRequests(c) => (
                StatusCode::TOO_MANY_REQUESTS,
                c,
                c.message().to_string(),
                None,
            ),
            ApiError::ServiceUnavailable(c) => (
                StatusCode::SERVICE_UNAVAILABLE,
                c,
                c.message().to_string(),
                None,
            ),
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                ErrorCode::Internal.message().to_string(),
                None,
            ),
        };

        let mut body = json!({ "error_code": code, "error": message });
        if let Some(serde_json::Value::Object(fields)) = extra
            && let Some(obj) = body.as_object_mut()
        {
            for (k, v) in fields {
                obj.insert(k, v);
            }
        }
        (status, body)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = self.parts();
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts_of(err: ApiError) -> (StatusCode, serde_json::Value) {
        err.parts()
    }

    #[test]
    fn forbidden_maps_status_code_and_message() {
        let (status, body) = parts_of(ApiError::Forbidden(ErrorCode::StaffRequired));
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error_code"], "staff_required");
        assert_eq!(body["error"], "Staff access required");
    }

    #[test]
    fn unauthorized_maps_status_and_message() {
        let (status, body) = parts_of(ApiError::Unauthorized(ErrorCode::InvalidCredentials));
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error_code"], "invalid_credentials");
        assert_eq!(body["error"], "Invalid email or password");
    }

    #[test]
    fn not_found_maps_status_and_message() {
        let (status, body) = parts_of(ApiError::NotFound(ErrorCode::UserNotFound));
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error_code"], "user_not_found");
        assert_eq!(body["error"], "User not found");
    }

    #[test]
    fn bad_request_uses_dynamic_message_and_generic_code() {
        let (status, body) = parts_of(ApiError::BadRequest(
            "Amount must be greater than zero".into(),
        ));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error_code"], "bad_request");
        assert_eq!(body["error"], "Amount must be greater than zero");
    }

    #[test]
    fn too_many_requests_maps_status_and_message() {
        let (status, body) = parts_of(ApiError::TooManyRequests(ErrorCode::TooManyRequests));
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error_code"], "too_many_requests");
        assert_eq!(body["error"], "Too many attempts, please try again later");
    }

    #[test]
    fn service_unavailable_maps_status_and_message() {
        let (status, body) = parts_of(ApiError::ServiceUnavailable(ErrorCode::MailNotConfigured));
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error_code"], "mail_not_configured");
        assert_eq!(body["error"], "mail_not_configured");
    }

    #[test]
    fn internal_leaks_no_detail() {
        let (status, body) = parts_of(ApiError::Internal);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error_code"], "internal");
        assert_eq!(body["error"], "Internal server error");
    }

    #[test]
    fn conflict_default_message() {
        let (status, body) = parts_of(ApiError::conflict(ErrorCode::ClassCancelled));
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error_code"], "class_cancelled");
        assert_eq!(body["error"], "Class is cancelled");
    }

    #[test]
    fn conflict_explicit_message_overrides_default() {
        let (_status, body) = parts_of(ApiError::conflict_message(
            ErrorCode::ClassFull,
            "Class is full (12/12)",
        ));
        assert_eq!(body["error_code"], "class_full");
        assert_eq!(body["error"], "Class is full (12/12)");
    }

    #[test]
    fn conflict_extra_fields_are_flattened() {
        let (status, body) = parts_of(ApiError::conflict_extra(
            ErrorCode::EmailConflict,
            json!({ "conflict_name": "Jane", "conflict_card": "C7" }),
        ));
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error_code"], "email_conflict");
        assert_eq!(body["error"], "A user with this email already exists");
        assert_eq!(body["conflict_name"], "Jane");
        assert_eq!(body["conflict_card"], "C7");
    }

    #[test]
    fn into_response_carries_the_mapped_status() {
        let resp = ApiError::Forbidden(ErrorCode::AdminRequired).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
