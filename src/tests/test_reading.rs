use anyhow::Result;
use axum::{
    body::Body,
    http::{Method, StatusCode},
};
use http_body_util::BodyExt;
use serial_test::serial;

use super::{make_request, setup_test_app};
use crate::{
    models::{LoginRequest, UpdateReadingSettingsRequest},
    utils::hash_password,
};

#[tokio::test]
#[serial]
async fn test_reading_settings() -> Result<()> {
    let (app, pool) = setup_test_app().await?;

    // 创建测试用户
    let user_id = 1;
    let password_hash = hash_password("password123")?;

    sqlx::query!(
        "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
        user_id,
        "testuser",
        password_hash
    )
    .execute(&pool)
    .await?;

    // 获取认证令牌
    let login_body = serde_json::to_string(&LoginRequest {
        username: "testuser".to_string(),
        password: "password123".to_string(),
        device_id: "test_device".to_string(),
    })?;

    let response = make_request(&app, Method::POST, "/api/auth/login", login_body, None).await;

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    let token = json["data"]["token"].as_str().unwrap().to_string();

    // 测试获取阅读设置 (应该创建默认设置)
    let response = make_request(
        &app,
        Method::GET,
        "/api/reading/settings",
        Body::empty(),
        Some(&token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["font_size"], 18);
    assert_eq!(json["data"]["background_color"], "#F5F5DC");

    // 更新阅读设置
    let settings_body = serde_json::to_string(&UpdateReadingSettingsRequest {
        font_size: Some(20),
        background_color: Some("#F0F0F0".to_string()),
        text_color: Some("#333333".to_string()),
        line_height: Some(1.8),
        letter_spacing: Some(0.1),
        paragraph_spacing: Some(1.5),
        reading_width: Some(700),
        text_indent: Some(2.5),
        simplified_chinese: Some(false),
    })?;

    let response = make_request(
        &app,
        Method::PUT,
        "/api/reading/settings",
        settings_body,
        Some(&token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    // 再次获取阅读设置，验证更新成功
    let response = make_request(
        &app,
        Method::GET,
        "/api/reading/settings",
        Body::empty(),
        Some(&token),
    )
    .await;

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    println!("json: {:?}", json);
    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["font_size"], 20);
    assert_eq!(json["data"]["background_color"], "#F0F0F0");
    assert_eq!(json["data"]["simplified_chinese"], false);

    Ok(())
}
