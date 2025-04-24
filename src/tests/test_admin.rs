use anyhow::Result;
use axum::{
    body::Body,
    http::{Method, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serial_test::serial;

use super::{make_request, setup_test_app};
use crate::models::{
    AdminSetupRequest, CreateInviteCodeRequest, LoginRequest, ResetUserPasswordRequest,
    UpdateSettingsRequest,
};

/// 设置管理员，并返回响应
/// 密码：admin123
#[allow(unused)]
pub async fn setup_admin(app: &Router) -> Result<serde_json::Value> {
    let setup_body = serde_json::to_string(&AdminSetupRequest {
        password: "admin123".to_string(),
    })?;
    let response = make_request(app, Method::POST, "/api/auth/admin/setup", setup_body, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(json["code"], 0);
    Ok(json)
}

#[tokio::test]
#[serial]
async fn test_admin_setup_and_settings() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;

    let json = setup_admin(&app).await?;
    let admin_token = json["data"]["admin_token"].as_str().unwrap().to_string();

    // 测试检查管理员设置状态
    let response = make_request(
        &app,
        Method::GET,
        "/api/admin/check_setup",
        Body::empty(),
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["is_setup"], true);

    // 测试更新系统设置
    let settings_body = serde_json::to_string(&UpdateSettingsRequest {
        invite_code_required: true,
    })?;

    let response = make_request(
        &app,
        Method::PUT,
        "/api/admin/settings",
        settings_body,
        Some(&admin_token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    // 测试获取系统设置
    let response = make_request(
        &app,
        Method::GET,
        "/api/admin/settings",
        Body::empty(),
        Some(&admin_token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["invite_code_required"], true);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_invite_code_management() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;

    let json = setup_admin(&app).await?;
    let admin_token = json["data"]["admin_token"].as_str().unwrap().to_string();

    // 测试生成邀请码
    let invite_code_body = serde_json::to_string(&CreateInviteCodeRequest {
        limit_times: 1,
        description: Some("测试邀请码".to_string()),
    })?;

    let response = make_request(
        &app,
        Method::POST,
        "/api/admin/invite_code",
        invite_code_body,
        Some(&admin_token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(json["code"], 0);
    assert!(json["data"]["invite_code"].is_string());

    // 测试查看所有邀请码
    let response = make_request(
        &app,
        Method::GET,
        "/api/admin/invite_codes",
        Body::empty(),
        Some(&admin_token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(json["code"], 0);
    assert!(json["data"]["invite_codes"].is_array());
    assert_eq!(json["data"]["invite_codes"][0]["limit_times"], 1);
    assert_eq!(json["data"]["invite_codes"][0]["used_times"], 0);

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_user_management() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;

    let json = setup_admin(&app).await?;
    let admin_token = json["data"]["admin_token"].as_str().unwrap().to_string();

    // 设置邀请码为非必填
    let settings_body = serde_json::to_string(&UpdateSettingsRequest {
        invite_code_required: false,
    })?;
    let response = make_request(
        &app,
        Method::PUT,
        "/api/admin/settings",
        settings_body,
        Some(&admin_token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // 注册一个测试用户
    super::test_user::register_test_user(&app).await?;

    // 测试查看所有用户
    let response = make_request(
        &app,
        Method::GET,
        "/api/admin/users",
        Body::empty(),
        Some(&admin_token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    dbg!(&json);
    assert_eq!(json["code"], 0);
    assert!(json["data"]["users"].is_array());
    let user_id = json["data"]["users"][0]["user_id"].as_u64().unwrap();
    assert_eq!(json["data"]["users"][0]["username"], "testuser");

    // 测试重置用户密码
    let reset_password_body = serde_json::to_string(&ResetUserPasswordRequest {
        new_password: "newpassword123".to_string(),
    })?;

    let response = make_request(
        &app,
        Method::POST,
        &format!("/api/admin/users/{}/reset_password", user_id),
        reset_password_body,
        Some(&admin_token),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(json["code"], 0);

    // 验证密码已被重置（使用新密码登录）
    let login_body = serde_json::to_string(&LoginRequest {
        username: "testuser".to_string(),
        password: "newpassword123".to_string(),
        device_id: "test_device".to_string(),
    })?;

    let response = make_request(&app, Method::POST, "/api/auth/login", login_body, None).await;

    assert_eq!(response.status(), StatusCode::OK);

    Ok(())
}
