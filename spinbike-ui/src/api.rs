use gloo_net::http::RequestBuilder;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::auth::{clear_auth, get_token};

/// If the response is 401 AND we had a token stored (i.e., a previously-valid
/// session expired), clear stored auth and redirect to /login.
/// Returns true if the caller should bail.
///
/// This intentionally does NOT redirect on 401s from unauthenticated requests
/// (e.g., wrong password on login) — those show an inline error instead.
fn handle_unauthorized(status: u16, had_token: bool) -> bool {
    if status == 401 && had_token {
        clear_auth();
        if let Some(win) = web_sys::window() {
            let _ = win.location().set_href("/login");
        }
        return true;
    }
    false
}

fn base_url() -> String {
    String::new()
}

fn add_auth(req: RequestBuilder) -> RequestBuilder {
    if let Some(token) = get_token() {
        req.header("Authorization", &format!("Bearer {token}"))
    } else {
        req
    }
}

pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let url = format!("{}{}", base_url(), path);
    let resp = add_auth(RequestBuilder::new(&url))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(session_expired_message());
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

/// Like [`get`], but the `Err` carries the server's machine-readable
/// `error_code` (#158) alongside the raw message, so a render site can
/// localize by code (#145) instead of showing the server's raw English text.
/// Existing `get()` callers are left untouched — this is additive, used only
/// by the customer-facing render sites that need to localize their error.
pub async fn get_coded<T: DeserializeOwned>(path: &str) -> Result<T, CodedError> {
    let url = format!("{}{}", base_url(), path);
    let resp = add_auth(RequestBuilder::new(&url))
        .send()
        .await
        .map_err(CodedError::from_transport)?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(CodedError::msg(session_expired_message()));
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_coded_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(CodedError::from_transport)
}

pub async fn post<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> Result<T, String> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::POST))
        .json(body)
        .map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(session_expired_message());
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

/// Like [`post`], but for PUBLIC endpoints where a non-2xx response is an
/// expected, benign outcome — never triggers the "session expired, redirect
/// to /login" handling that [`post`] applies to any 401 while a token is
/// stored. Needed for magic-link redemption (`/api/auth/token-login`): an
/// already-used/expired token legitimately 401s, and that must NOT clear an
/// unrelated, still-valid session the browser happens to already hold (e.g.
/// re-opening an old invite email after already being logged in permanently
/// — see #109). Also skips attaching the Authorization header: these
/// endpoints never read it server-side.
pub async fn post_public<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
) -> Result<T, String> {
    let url = format!("{}{}", base_url(), path);
    let req = RequestBuilder::new(&url)
        .method(gloo_net::http::Method::POST)
        .json(body)
        .map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

/// Like [`post_public`], but the `Err` carries the server's machine-readable
/// `error_code` (#158) alongside the raw message — see [`get_coded`].
pub async fn post_public_coded<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
) -> Result<T, CodedError> {
    let url = format!("{}{}", base_url(), path);
    let req = RequestBuilder::new(&url)
        .method(gloo_net::http::Method::POST)
        .json(body)
        .map_err(CodedError::from_transport)?;

    let resp = req.send().await.map_err(CodedError::from_transport)?;

    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_coded_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(CodedError::from_transport)
}

pub async fn put<B: Serialize>(path: &str, body: &B) -> Result<(), String> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::PUT))
        .json(body)
        .map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(session_expired_message());
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    Ok(())
}

pub async fn patch<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> Result<T, String> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::PATCH))
        .json(body)
        .map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(session_expired_message());
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

/// Error from [`put_json`]. Beyond the display `message`, it optionally carries
/// the identity of a colliding account when the server rejects a value as
/// already-in-use (409). The server fills these ONLY for staff/admin callers,
/// so a self-editing customer never learns another account's identity — the UI
/// simply shows the generic message in that case.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct ApiError {
    pub message: String,
    /// The server's machine-readable `error_code` (#158), when present — lets a
    /// render site branch on the error kind (e.g. the #143 deleted-email
    /// conflict) instead of matching on human text.
    pub code: Option<spinbike_core::errors::ErrorCode>,
    pub conflict_name: Option<String>,
    pub conflict_card: Option<String>,
    /// #143: id of the SOFT-DELETED account holding the submitted email — the
    /// target of the restore / free-email resolution actions.
    pub conflict_id: Option<i64>,
    /// #143: when the colliding account was soft-deleted (raw server string).
    pub conflict_deleted_at: Option<String>,
}

impl ApiError {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Default::default()
        }
    }

    /// #143: if this error is the soft-deleted-email conflict
    /// (`email_belongs_to_deleted_account` WITH a `conflict_id`), return the
    /// archived account's `(id, name, deleted_at)` so the caller can raise the
    /// restore / free-email resolution dialog. `None` for any other error.
    pub fn deleted_email_conflict(&self) -> Option<(i64, String, Option<String>)> {
        if self.code == Some(spinbike_core::errors::ErrorCode::EmailBelongsToDeletedAccount) {
            if let Some(id) = self.conflict_id {
                return Some((
                    id,
                    self.conflict_name.clone().unwrap_or_default(),
                    self.conflict_deleted_at.clone(),
                ));
            }
        }
        None
    }
}

pub async fn put_json<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::PUT))
        .json(body)
        .map_err(|e| ApiError::msg(e.to_string()))?;

    let resp = req.send().await.map_err(|e| ApiError::msg(e.to_string()))?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(ApiError::msg(session_expired_message()));
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_api_error(&text, resp.status()));
    }

    resp.json::<T>()
        .await
        .map_err(|e| ApiError::msg(e.to_string()))
}

/// Like [`post`], but the `Err` is the richer [`ApiError`] (carrying the
/// server's `error_code` and the `conflict_*` identity fields) instead of a
/// bare `String`. Used by the create path so it can detect the #143
/// soft-deleted-email conflict and offer the restore / free-email resolution.
pub async fn post_json<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::POST))
        .json(body)
        .map_err(|e| ApiError::msg(e.to_string()))?;

    let resp = req.send().await.map_err(|e| ApiError::msg(e.to_string()))?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(ApiError::msg(session_expired_message()));
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_api_error(&text, resp.status()));
    }

    resp.json::<T>()
        .await
        .map_err(|e| ApiError::msg(e.to_string()))
}

pub async fn delete(path: &str) -> Result<(), String> {
    let url = format!("{}{}", base_url(), path);
    let resp = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::DELETE))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(session_expired_message());
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    Ok(())
}

/// DELETE request that expects a no-body success response (204). Alias over
/// [`delete`] with an explicit name matching `post` / `patch` conventions used
/// by call sites that want to emphasise the empty-body contract.
pub async fn delete_empty(path: &str) -> Result<(), String> {
    delete(path).await
}

/// Like [`delete`], but the `Err` carries the server's machine-readable
/// `error_code` (#158) alongside the raw message — see [`get_coded`].
pub async fn delete_coded(path: &str) -> Result<(), CodedError> {
    let url = format!("{}{}", base_url(), path);
    let resp = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::DELETE))
        .send()
        .await
        .map_err(CodedError::from_transport)?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err(CodedError::msg(session_expired_message()));
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_coded_error(&text, resp.status()));
    }

    Ok(())
}

/// Error from a "coded" API call (`get_coded` / `post_public_coded` /
/// `delete_coded`) — mirrors [`extract_error`]'s raw human message but also
/// carries the machine-readable `error_code` (#158) when the server attached
/// one, so a render site can localize by code (#145) while still falling
/// back to the raw server text for an unmapped or absent code.
#[derive(Debug, Clone, PartialEq)]
pub struct CodedError {
    pub code: Option<spinbike_core::errors::ErrorCode>,
    pub message: String,
}

impl CodedError {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }

    /// Build from a transport-level failure (network error, non-JSON
    /// response body, etc.) that never reached a server error body — no
    /// `error_code` to carry, just the raw `Display` text.
    fn from_transport(e: impl std::fmt::Display) -> Self {
        Self::msg(e.to_string())
    }
}

/// Shared parse of a JSON error body into (human message, optional
/// `error_code`). The message-only fallback behavior matches the pre-#145
/// `extract_error` exactly: an unparseable body or a body with no `error`
/// field yields the localized "request failed" text.
///
/// `error_code` is decoded defensively as a raw string first, THEN parsed
/// into `ErrorCode` — an unrecognized code string (e.g. version skew during
/// a deploy) degrades to `None` instead of failing the whole-body parse and
/// losing the (still perfectly good) human `error` message.
fn extract_error_parts(
    body: &str,
    status: u16,
) -> (String, Option<spinbike_core::errors::ErrorCode>) {
    #[derive(serde::Deserialize)]
    struct ErrBody {
        error: Option<String>,
        error_code: Option<String>,
    }
    match serde_json::from_str::<ErrBody>(body) {
        Ok(e) => {
            let code = e.error_code.as_deref().and_then(|c| {
                serde_json::from_value(serde_json::Value::String(c.to_string())).ok()
            });
            (
                e.error.unwrap_or_else(|| request_failed_message(status)),
                code,
            )
        }
        Err(_) => (request_failed_message(status), None),
    }
}

fn extract_error(body: &str, status: u16) -> String {
    extract_error_parts(body, status).0
}

fn extract_coded_error(body: &str, status: u16) -> CodedError {
    let (message, code) = extract_error_parts(body, status);
    CodedError { code, message }
}

/// Like [`extract_error`] but also pulls the optional `error_code` and the
/// `conflict_*` fields the server attaches to a 409 email-collision: the
/// `conflict_name` / `conflict_card` for a LIVE collision (staff/admin only),
/// and the `conflict_id` / `conflict_deleted_at` for the #143 soft-deleted
/// case, so the UI can name the account and offer restore / free-email.
fn extract_api_error(body: &str, status: u16) -> ApiError {
    #[derive(serde::Deserialize)]
    struct ErrBody {
        error: Option<String>,
        error_code: Option<String>,
        conflict_name: Option<String>,
        conflict_card: Option<String>,
        conflict_id: Option<i64>,
        conflict_deleted_at: Option<String>,
    }
    if let Ok(e) = serde_json::from_str::<ErrBody>(body) {
        // Decode error_code defensively (unknown string → None), same as
        // extract_error_parts — version skew must not lose the human message.
        let code = e
            .error_code
            .as_deref()
            .and_then(|c| serde_json::from_value(serde_json::Value::String(c.to_string())).ok());
        return ApiError {
            message: e.error.unwrap_or_else(|| request_failed_message(status)),
            code,
            conflict_name: e.conflict_name,
            conflict_card: e.conflict_card,
            conflict_id: e.conflict_id,
            conflict_deleted_at: e.conflict_deleted_at,
        };
    }
    ApiError::msg(request_failed_message(status))
}

/// Localized "session expired" text for the 401-while-token-stored redirect
/// path. This module has no reactive `Lang` context (it's not a Leptos
/// component) — `i18n::get_saved_lang()` reads the same `localStorage` key
/// the reactive `Lang` signal is initialized from and kept in sync with on
/// every toggle (`i18n::save_lang`), so this is safe outside a component.
fn session_expired_message() -> String {
    crate::i18n::t(crate::i18n::get_saved_lang(), "err_session_expired").to_string()
}

/// Localized generic "request failed" fallback for a response whose body
/// couldn't be parsed at all, or had no `error` field. See
/// [`session_expired_message`] for why `get_saved_lang()` is safe here.
fn request_failed_message(status: u16) -> String {
    crate::i18n::tf(
        crate::i18n::get_saved_lang(),
        "err_request_failed_format",
        &[&status.to_string()],
    )
}
