pub mod test_integration;
pub mod test_quality;

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use sqlx::{Pool, Sqlite};
use tower::ServiceExt;

use crate::{app::create_app, config::Config, db::create_test_pool};

pub const TEST_BOOK_CONTENT: &str = "\
测试小说\n第一章 测试章节\n这是第一章的内容，用于测试书籍上传功能。\n这是一段普通的文本。\n\\
                                     n第二章 另一个章节\n这是第二章的内容，用于测试章节提取功能。\\
                                     n更多内容在这里。\n\n第三章 最终章\n这是第三章的内容。";

pub async fn setup_test_app() -> Result<(Router, Pool<Sqlite>)> {
    let pool = create_test_pool().await?;

    let book_dir = std::env::temp_dir().join("test_books");
    tokio::fs::create_dir_all(&book_dir).await?;

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
            book_dir,
            max_file_size: 10 * 1024 * 1024,
        },
        jwt: crate::config::JwtConfig {
            secret: "test_secret_key".to_string(),
            expiration: 3600,
            admin_expiration: 3600,
        },
        log: crate::config::LogConfig {
            dir: std::env::temp_dir().join("test_logs"),
            rotation: "never".to_string(),
        },
        rate_limit: crate::config::RateLimitConfig {
            per_second: std::num::NonZeroU32::new(1000).unwrap(),
            burst_size: std::num::NonZeroU32::new(1000).unwrap(),
        },
    };

    let app = create_app(pool.clone(), &config);

    Ok((app, pool))
}

pub async fn make_request<B>(
    app: &Router,
    method: Method,
    uri: &str,
    body: B,
    auth_token: Option<&str>,
) -> Response
where
    B: Into<Body>,
{
    // 测试用 oneshot 没有 peer_addr；tower_governor 默认从 X-Forwarded-For
    // 取 IP，故统一注入一个测试 IP 以避免被限流层拒绝
    let mut req_builder = Request::builder()
        .method(&method)
        .uri(uri)
        .header("X-Forwarded-For", "127.0.0.1");
    if let Some(token) = auth_token {
        req_builder = req_builder.header("Authorization", format!("Bearer {token}"));
    }
    let req = req_builder.body(body.into()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

pub async fn make_json_request(
    app: &Router,
    method: Method,
    uri: &str,
    body: &impl serde::Serialize,
    auth_token: Option<&str>,
) -> Response {
    let json = serde_json::to_string(body).unwrap();
    let mut req_builder = Request::builder()
        .method(&method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("X-Forwarded-For", "127.0.0.1");
    if let Some(token) = auth_token {
        req_builder = req_builder.header("Authorization", format!("Bearer {token}"));
    }
    let req = req_builder.body(Body::from(json)).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

pub async fn make_multipart_request(
    app: &Router,
    uri: &str,
    fields: Vec<(&str, &str)>,
    file_field: Option<(&str, &str, &[u8])>,
    auth_token: Option<&str>,
) -> Response {
    let boundary = "----FormBoundary7MA4YWxkTrZu0gW";
    let mut body = Vec::new();

    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; \
                 name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }

    if let Some((name, filename, data)) = file_field {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; \
                 filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let mut req_builder = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("X-Forwarded-For", "127.0.0.1");
    if let Some(token) = auth_token {
        req_builder = req_builder.header("Authorization", format!("Bearer {token}"));
    }
    let req = req_builder.body(Body::from(body)).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

pub async fn response_json(response: Response) -> serde_json::Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

pub async fn setup_admin(app: &Router) -> (String, serde_json::Value) {
    let resp = make_json_request(
        app,
        Method::POST,
        "/api/v1/auth/admin/setup",
        &serde_json::json!({"username": "admin", "password": "admin123"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);
    let token = json["data"]["admin_token"].as_str().unwrap().to_string();
    (token, json)
}

pub async fn register_user(
    app: &Router,
    username: &str,
    password: &str,
    invite_code: Option<&str>,
) -> Response {
    make_json_request(
        app,
        Method::POST,
        "/api/v1/auth/register",
        &serde_json::json!({
            "username": username,
            "password": password,
            "invite_code": invite_code
        }),
        None,
    )
    .await
}

pub async fn register_and_login(app: &Router, username: &str, password: &str) -> String {
    let resp = register_user(app, username, password, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);
    json["data"]["token"].as_str().unwrap().to_string()
}

pub async fn login(app: &Router, username: &str, password: &str, device_id: &str) -> Response {
    make_json_request(
        app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({
            "username": username,
            "password": password,
            "device_id": device_id
        }),
        None,
    )
    .await
}

pub async fn upload_test_book(
    app: &Router,
    token: &str,
    title: &str,
    content: &str,
    is_public: bool,
) -> serde_json::Value {
    let resp = make_multipart_request(
        app,
        "/api/v1/books/upload",
        vec![
            ("title", title),
            (
                "is_public",
                if is_public {
                    "true"
                } else {
                    "false"
                },
            ),
        ],
        Some(("file", "test.txt", content.as_bytes())),
        Some(token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "upload failed");
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0, "upload error: {json:?}");
    json["data"].clone()
}
