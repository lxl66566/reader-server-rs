use anyhow::Result;
use axum::{
    body::Body,
    http::{Method, StatusCode},
};
use http_body_util::BodyExt;
use serial_test::serial;

use super::{make_request, setup_test_app};
use crate::models::{
        AdminSetupRequest,
        UpdateSettingsRequest,
    };
#[tokio::test]
#[serial]
async fn test_admin_setup_and_settings() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;

    // 测试管理员设置
    let setup_body = serde_json::to_string(&AdminSetupRequest {
        password: "admin123".to_string(),
    })?;

    let response = make_request(
        &app,
        Method::POST,
        "/api/auth/admin/setup",
        setup_body,
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    assert_eq!(json["code"], 0);
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
    assert_eq!(json["data"]["invite_code_required"], false);

    Ok(())
}
