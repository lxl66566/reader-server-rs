//! 前端静态资源服务。
//!
//! - release 构建：通过 rust-embed 将 `frontend/dist` 嵌入二进制直接服务；
//! - debug 构建：不内嵌前端，提示使用 vite dev server（`frontend/` 下 `pnpm dev`）。

#[cfg(debug_assertions)]
mod imp {
    use axum::{
        http::{StatusCode, Uri},
        response::{IntoResponse, Response},
    };

    /// debug 构建的兜底：前端由 vite dev server 提供，此处仅返回提示
    pub async fn fallback(_uri: Uri) -> Response {
        (
            StatusCode::NOT_FOUND,
            "debug build: frontend is not embedded. Run `pnpm dev` in frontend/ and visit \
             http://localhost:5173",
        )
            .into_response()
    }
}

#[cfg(not(debug_assertions))]
mod imp {
    use axum::{
        body::Body,
        http::{StatusCode, Uri, header},
        response::{IntoResponse, Response},
    };
    use rust_embed::RustEmbed;

    /// release 构建时嵌入的前端产物（相对 crate 根目录）
    #[derive(RustEmbed)]
    #[folder = "frontend/dist/"]
    struct Assets;

    /// 静态资源兜底：命中文件则返回，否则 SPA 回退到 index.html。
    /// API 路径未匹配时不应回退到前端页面，直接 404。
    pub async fn fallback(uri: Uri) -> Response {
        let path = uri.path().trim_start_matches('/');

        if path.starts_with("api/") {
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }

        // SPA：路径最后一段无扩展名时交给前端路由
        let is_file = path.rsplit('/').next().is_some_and(|seg| seg.contains('.'));
        let path = if path.is_empty() || !is_file {
            "index.html"
        } else {
            path
        };

        let Some(file) = Assets::get(path) else {
            return (StatusCode::NOT_FOUND, "not found").into_response();
        };

        // vite 产物 assets/ 下的文件名带内容 hash，可长缓存；其余（含 index.html）不缓存
        let cache_control = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };

        Response::builder()
            .header(header::CONTENT_TYPE, file.metadata.mimetype())
            .header(header::CACHE_CONTROL, cache_control)
            .body(Body::from(file.data))
            .expect("static response with valid headers always builds")
    }
}

pub use imp::fallback;
