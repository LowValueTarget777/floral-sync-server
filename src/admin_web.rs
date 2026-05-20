use axum::{
    body::Body,
    extract::Path,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "admin-ui/dist/"]
struct AdminUiAssets;

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(root_redirect))
        .route("/admin", get(admin_index))
        .route("/admin/", get(admin_index))
        .route("/assets/*path", get(asset))
}

async fn root_redirect() -> Redirect {
    Redirect::temporary("/admin")
}

async fn admin_index() -> Response {
    embedded_file_response("index.html")
}

async fn asset(Path(path): Path<String>) -> Response {
    embedded_file_response(&format!("assets/{path}"))
}

fn embedded_file_response(path: &str) -> Response {
    let Some(asset) = AdminUiAssets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::new(Body::from(asset.data.into_owned()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.essence_str()).expect("valid content type"),
    );
    if path.starts_with("assets/") {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    response
}