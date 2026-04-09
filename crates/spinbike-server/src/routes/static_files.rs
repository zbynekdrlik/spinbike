use axum::{
    http::{StatusCode, header, Uri},
    response::{Html, IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../spinbike-ui/dist/"]
struct Asset;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try to serve the exact file.
    if let Some(content) = Asset::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let mut response = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref().to_string())],
            content.data.to_vec(),
        )
            .into_response();

        // Long cache for hashed assets.
        if path.starts_with("assets/") {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".parse().unwrap(),
            );
        }

        return response;
    }

    // SPA fallback: serve index.html for paths without file extensions.
    if !path.contains('.') || path.is_empty() {
        if let Some(index) = Asset::get("index.html") {
            return Html(String::from_utf8_lossy(&index.data).to_string()).into_response();
        }
    }

    (StatusCode::NOT_FOUND, "Not found").into_response()
}
