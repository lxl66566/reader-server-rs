pub mod test_admin;
pub mod test_reading;
pub mod test_user;

use anyhow::Result;
use axum::{
    body::Body,
    http::{Method, Request},
    response::Response,
    Router,
};
use sqlx::{Pool, Sqlite};
use tower::ServiceExt;

use crate::{
    app::create_app,
    config::Config,
    db::create_test_pool,
};

// 测试工具函数
async fn setup_test_app() -> Result<(axum::Router, Pool<Sqlite>)> {
    // 创建内存数据库
    let pool = create_test_pool().await?;

    // 创建一个测试配置
    let config = Config {
        server: crate::config::ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
        },
        db: crate::config::DbConfig {
            url: "sqlite::memory:".to_string(),
            max_connections: 5,
        },
        storage: crate::config::StorageConfig {
            book_dir: std::env::temp_dir().join("test_books"),
        },
        jwt: crate::config::JwtConfig {
            secret: "test_secret_key".to_string(),
            expiration: 3600,
            admin_expiration: 3600,
        },
    };

    // 创建应用
    let app = create_app(pool.clone(), config).await?;

    Ok((app, pool))
}

/// Helper function to make requests to the test Axum app.
///
/// # Arguments
/// * `app` - The Axum Router instance.
/// * `method` - The HTTP method (GET, POST, etc.).
/// * `uri` - The request URI path.
/// * `body` - The request body. For GET/DELETE or requests without a body, pass
///   `Body::empty()`. For POST/PUT, pass anything that implements `Into<Body>`
///   (e.g., `String`, `Vec<u8>`, `serde_json::to_vec(...)?.into()`).
/// * `auth_token` - Optional bearer token for the Authorization header.
///
/// # Returns
/// The `axum::response::Response`.
pub async fn make_request<B>(
    // Make the function generic over the body type B
    app: &Router,
    method: Method,
    uri: &str,
    body: B, // Accept B directly, not Option<B>
    auth_token: Option<&str>,
) -> Response
where
    B: Into<Body>, // Add trait bound: B must be convertible to Body
{
    let mut req_builder = Request::builder().method(&method).uri(uri);

    // Add authentication header if provided
    if let Some(token) = auth_token {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", token));
    }

    // Add Content-Type header for relevant methods (can be refined)
    // Note: This assumes JSON for POST/PUT. You might want to make this more
    // flexible or let the caller set it if needed, especially if `body` isn't
    // always JSON.
    if matches!(method, Method::POST | Method::PUT) {
        // Only add Content-Type if the body isn't explicitly empty? Might be tricky.
        // A common pattern is to require the caller to provide it if necessary,
        // or infer based on the type B if possible (advanced).
        // For simplicity, keeping the original logic, but be aware.
        req_builder = req_builder.header("Content-Type", "application/json");
    }

    // Build the request with the provided body
    // body.into() converts the input type B into axum::body::Body
    let req = req_builder.body(body.into()).unwrap(); // Use body.into()

    // Send the request using Tower's oneshot
    app.clone().oneshot(req).await.unwrap()
}
