use anyhow::Result;
use axum::{
    body::Body,
    http::{Method, StatusCode},
};
use http_body_util::BodyExt;
use serial_test::serial;

use super::{make_request, setup_test_app, test_user::register_test_user_and_login};
use crate::models::UpdateReadingSettingsRequest;

#[tokio::test]
#[serial]
async fn test_reading_settings() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;

    let token = register_test_user_and_login(&app).await?;

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

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)?;

    println!("json: {:?}", json);
    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["font_size"], 20);
    assert_eq!(json["data"]["background_color"], "#F0F0F0");
    assert_eq!(json["data"]["simplified_chinese"], false);

    Ok(())
}
