//! 安全加固回归测试
//!
//! 覆盖：密码非空校验、损坏文件上传语义（400 非 500）、空文件拒绝、
//! 章节数上限、被删用户 token 语义、无效 JWT 401 语义、/health 限流豁免。

use std::{fmt::Write as _, io::Write as _, num::NonZeroU32};

use axum::{
    body::Body,
    http::{Method, StatusCode},
};

use super::*;

/// 各密码设置入口对空新密码统一返回 400
#[tokio::test]
async fn test_empty_password_rejected_on_all_endpoints() {
    let (app, pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "pwuser", "password1").await;
    let (admin_token, _) = setup_admin(&app).await;

    // 用户改密
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/change_password",
        &serde_json::json!({"old_password": "password1", "new_password": ""}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 管理员改密
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/admin/password",
        &serde_json::json!({"old_password": "admin123", "new_password": ""}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 管理员重置用户密码
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'pwuser'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/reset_password"),
        &serde_json::json!({"new_password": ""}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 新建管理员
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/admin/admins",
        &serde_json::json!({"username": "extra_admin", "password": ""}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 初始 setup（独立实例）：空密码被拒，且未创建任何管理员（可重新 setup）
    let (app2, _pool2) = setup_test_app().await.unwrap();
    let resp = make_json_request(
        &app2,
        Method::POST,
        "/api/v1/auth/admin/setup",
        &serde_json::json!({"username": "root", "password": ""}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let (admin_token2, _) = setup_admin(&app2).await;
    let resp = make_request(
        &app2,
        Method::GET,
        "/api/v1/admin/settings",
        Body::empty(),
        Some(&admin_token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// 损坏（无法解析）的 epub / pdf 上传应返回 400 而不是 500
#[tokio::test]
async fn test_corrupted_file_upload_returns_400() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "corrupt", "password1").await;

    // 损坏 PDF：签名头 + 垃圾字节，xref 结构不完整
    let bad_pdf: &[u8] = b"%PDF-1.4\n%%\xff\xff garbage without xref";
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "bad pdf"), ("is_public", "false")],
        Some(("file", "bad.pdf", bad_pdf)),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 损坏 EPUB：mimetype 条目合法（通过格式检测）但 container.xml 截断
    let bad_epub = corrupt_epub_bytes();
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "bad epub"), ("is_public", "false")],
        Some(("file", "bad.epub", &bad_epub)),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// 构造能通过 zip/epub 签名检测但内部结构损坏的 EPUB
fn corrupt_epub_bytes() -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        // epub 规范要求 mimetype 为首个条目且不压缩（同时满足 infer 的签名检测）
        writer.start_file("mimetype", stored).unwrap();
        writer.write_all(b"application/epub+zip").unwrap();
        writer.start_file("META-INF/container.xml", stored).unwrap();
        writer.write_all(b"<container truncated").unwrap();
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

/// 0 字节文件必须被拒绝，不允许建出“不可读之书”
#[tokio::test]
async fn test_empty_file_upload_rejected() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "emptyfile", "password1").await;

    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "空文件书"), ("is_public", "false")],
        Some(("file", "empty.txt", b"")),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// 章节数超过上限（50_000）的构造性 txt 返回 400
#[tokio::test]
async fn test_too_many_chapters_rejected() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "manychapters", "password1").await;

    let mut content = String::with_capacity(50_001 * 8);
    for i in 0..=50_000u32 {
        let _ = writeln!(content, "第{i}章");
    }

    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "章节过多"), ("is_public", "false")],
        Some(("file", "many.txt", content.as_bytes())),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("Too many chapters"),
        "unexpected error: {json:?}"
    );
}

/// 被删用户的有效 JWT 必须返回 401（而非 404）
#[tokio::test]
async fn test_deleted_user_token_returns_401() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "gone", "password1").await;

    // 注销前 token 可用
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 自助注销
    let resp = make_json_request(
        &app,
        Method::DELETE,
        "/api/v1/auth/account",
        &serde_json::json!({"password": "password1"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 旧 token：用户已删除 → 401
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// 无效 JWT（垃圾字符串 / 结构合法但已过期）请求需认证接口返回 401 而非 500；
/// 未携带 Authorization 头同为 401
#[tokio::test]
async fn test_invalid_jwt_returns_401() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let _token = register_and_login(&app, "jwtuser", "password1").await;

    // 垃圾字符串（无法 base64/JSON 解码）
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some("garbage-not-a-jwt"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 结构合法但签名用错密钥（伪造 token）
    let forged = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &serde_json::json!({"sub": "1", "exp": 9_999_999_999u64, "iat": 1_000_000_000u64,
                            "role": "user", "ver": 0}),
        &jsonwebtoken::EncodingKey::from_secret(b"wrong_secret"),
    )
    .unwrap();
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&forged),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 结构合法但 exp 已过期
    let expired = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &crate::auth::Claims {
            sub: "1".to_string(),
            exp: 1_000_000_000, // 2001 年，早已过期
            iat: 900_000_000,
            role: "user".to_string(),
            ver: 0,
        },
        // 与 setup_test_app 的测试密钥一致
        &jsonwebtoken::EncodingKey::from_secret(b"test_secret_key"),
    )
    .unwrap();
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&expired),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 未携带 Authorization 头
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// /health 真正不受限流：极小配额下仍可连续访问，
/// 对照组（受限流路由）同 IP 连续请求会命中 429
#[tokio::test]
async fn test_health_exempt_from_rate_limit() {
    let (app, _pool) = setup_test_app_with_rate_limit(crate::config::RateLimitConfig {
        per_second: NonZeroU32::new(1).unwrap(),
        burst_size: NonZeroU32::new(1).unwrap(),
        trust_proxy_headers: true,
    })
    .await
    .unwrap();

    for _ in 0..20 {
        let resp = make_request(&app, Method::GET, "/health", Body::empty(), None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let mut saw_429 = false;
    for _ in 0..5 {
        let resp = make_request(&app, Method::GET, "/api/v1/books", Body::empty(), None).await;
        if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            saw_429 = true;
        }
    }
    assert!(saw_429, "rate-limited route must eventually return 429");
}
