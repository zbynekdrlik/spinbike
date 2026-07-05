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
            return Err("Session expired, redirecting to login...".into());
        }
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

pub async fn post<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> Result<T, String> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::POST))
        .json(body)
        .map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err("Session expired, redirecting to login...".into());
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

pub async fn put<B: Serialize>(path: &str, body: &B) -> Result<(), String> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::PUT))
        .json(body)
        .map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        if handle_unauthorized(resp.status(), get_token().is_some()) {
            return Err("Session expired, redirecting to login...".into());
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
            return Err("Session expired, redirecting to login...".into());
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
    pub conflict_name: Option<String>,
    pub conflict_card: Option<String>,
}

impl ApiError {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Default::default()
        }
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
            return Err(ApiError::msg("Session expired, redirecting to login..."));
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
            return Err("Session expired, redirecting to login...".into());
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

fn extract_error(body: &str, status: u16) -> String {
    #[derive(serde::Deserialize)]
    struct ErrBody {
        error: Option<String>,
    }
    if let Ok(e) = serde_json::from_str::<ErrBody>(body)
        && let Some(msg) = e.error
    {
        return msg;
    }
    format!("Request failed (HTTP {status})")
}

/// Like [`extract_error`] but also pulls the optional `conflict_name` /
/// `conflict_card` fields the server attaches to a 409 email-collision for
/// staff/admin callers, so the UI can name the account that holds the email.
fn extract_api_error(body: &str, status: u16) -> ApiError {
    #[derive(serde::Deserialize)]
    struct ErrBody {
        error: Option<String>,
        conflict_name: Option<String>,
        conflict_card: Option<String>,
    }
    if let Ok(e) = serde_json::from_str::<ErrBody>(body) {
        return ApiError {
            message: e
                .error
                .unwrap_or_else(|| format!("Request failed (HTTP {status})")),
            conflict_name: e.conflict_name,
            conflict_card: e.conflict_card,
        };
    }
    ApiError::msg(format!("Request failed (HTTP {status})"))
}
