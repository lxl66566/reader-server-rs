use anyhow::Result;
use axum::{
    body::Body,
    http::{Method, StatusCode},
};
use bytes::Bytes;
use http_body_util::BodyExt;
use serial_test::serial;

use super::{make_request, setup_test_app};
use crate::{
    models::{ChangePasswordRequest, CreateUserRequest, LoginRequest},
    utils::hash_password,
};
#[tokio::test]
#[serial]
async fn test_user_registration_and_login() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;

    // 注册新用户
    let register_body = serde_json::to_string(&CreateUserRequest {
        username: "testuser".to_string(),
        password: "password123".to_string(),
        invite_code: None,
    })?;

    let response = make_request(
        &app,
        Method::POST,
        "/api/auth/register",
        register_body,
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    assert_eq!(json["code"], 0);
    assert!(json["data"]["token"].is_string());
    assert_eq!(json["data"]["username"], "testuser");

    // 测试登录
    let login_body = serde_json::to_string(&LoginRequest {
        username: "testuser".to_string(),
        password: "password123".to_string(),
        device_id: "test_device".to_string(),
    })?;

    let response = make_request(&app, Method::POST, "/api/auth/login", login_body, None).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Bytes = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    assert_eq!(json["code"], 0);
    assert!(json["data"]["token"].is_string());

    // 测试使用错误的密码登录
    let login_body = serde_json::to_string(&LoginRequest {
        username: "testuser".to_string(),
        password: "wrongpassword".to_string(),
        device_id: "test_device".to_string(),
    })?;

    let response = make_request(&app, Method::POST, "/api/auth/login", login_body, None).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_user_info_and_password_change() -> Result<()> {
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

    // 测试获取用户信息
    let response = make_request(
        &app,
        Method::GET,
        "/api/auth/user_info",
        Body::empty(),
        Some(&token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["username"], "testuser");
    assert_eq!(json["data"]["user_id"], 1);

    // 测试修改密码
    let change_pwd_body = serde_json::to_string(&ChangePasswordRequest {
        old_password: "password123".to_string(),
        new_password: "newpassword123".to_string(),
    })?;

    let response = make_request(
        &app,
        Method::POST,
        "/api/auth/change_password",
        change_pwd_body,
        Some(&token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    // 使用新密码登录
    let login_body = serde_json::to_string(&LoginRequest {
        username: "testuser".to_string(),
        password: "newpassword123".to_string(),
        device_id: "test_device".to_string(),
    })?;

    let response = make_request(&app, Method::POST, "/api/auth/login", login_body, None).await;

    assert_eq!(response.status(), StatusCode::OK);

    // 使用旧密码登录 (应该失败)
    let login_body = serde_json::to_string(&LoginRequest {
        username: "testuser".to_string(),
        password: "password123".to_string(),
        device_id: "test_device".to_string(),
    })?;

    let response = make_request(&app, Method::POST, "/api/auth/login", login_body, None).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    Ok(())
}
