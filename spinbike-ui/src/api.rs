use gloo_net::http::RequestBuilder;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::auth::get_token;

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
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    Ok(())
}

pub async fn put_json<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
) -> Result<T, String> {
    let url = format!("{}{}", base_url(), path);
    let req = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::PUT))
        .json(body)
        .map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    resp.json::<T>().await.map_err(|e| e.to_string())
}

pub async fn delete(path: &str) -> Result<(), String> {
    let url = format!("{}{}", base_url(), path);
    let resp = add_auth(RequestBuilder::new(&url).method(gloo_net::http::Method::DELETE))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(&text, resp.status()));
    }

    Ok(())
}

fn extract_error(body: &str, status: u16) -> String {
    #[derive(serde::Deserialize)]
    struct ErrBody {
        error: Option<String>,
    }
    if let Ok(e) = serde_json::from_str::<ErrBody>(body) {
        if let Some(msg) = e.error {
            return msg;
        }
    }
    format!("Request failed (HTTP {status})")
}
