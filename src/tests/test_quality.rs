//! 阅读设置校验相关的质量测试
//!
//! 覆盖 PUT /api/v1/reading/settings 的数值范围、hex 颜色格式与枚举字段校验。

use axum::http::{Method, StatusCode};

use super::{make_json_request, register_and_login, response_json, setup_test_app};

/// 合法的全量更新应成功并持久化
#[tokio::test]
async fn test_update_reading_settings_valid() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "settings_ok", "pass123").await;

    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/reading/settings",
        &serde_json::json!({
            "font_size": 20,
            "background_color": "#a1B2c3",
            "text_color": "#FFF",
            "line_height": 1.8,
            "letter_spacing": 1.0,
            "paragraph_spacing": 0.5,
            "reading_width": 40,
            "text_indent": 1.0,
            "theme": "dark",
            "pagination_mode": "paginate"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 读回确认已持久化
    let resp = make_json_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        &serde_json::json!(null),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 20);
    assert_eq!(json["data"]["background_color"], "#a1B2c3");
    assert_eq!(json["data"]["reading_width"], 40);
    assert_eq!(json["data"]["theme"], "dark");
}

/// 数值越界（含负值）应返回 400
#[tokio::test]
async fn test_update_reading_settings_out_of_range() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "settings_range", "pass123").await;

    let cases = [
        serde_json::json!({"font_size": 5}),
        serde_json::json!({"font_size": 100}),
        serde_json::json!({"line_height": 0.5}),
        serde_json::json!({"letter_spacing": -1.0}),
        serde_json::json!({"paragraph_spacing": 20.0}),
        serde_json::json!({"reading_width": 0}),
        serde_json::json!({"text_indent": -2.0}),
    ];
    for body in cases {
        let resp = make_json_request(
            &app,
            Method::PUT,
            "/api/v1/reading/settings",
            &body,
            Some(&token),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 for body: {body}"
        );
    }
}

/// 非 hex 格式的颜色应返回 400
#[tokio::test]
async fn test_update_reading_settings_invalid_color() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "settings_color", "pass123").await;

    let cases = [
        serde_json::json!({"background_color": "red"}),
        serde_json::json!({"background_color": "123456"}),
        serde_json::json!({"background_color": "#12345"}),
        serde_json::json!({"background_color": "#GGGGGG"}),
        serde_json::json!({"text_color": ""}),
        serde_json::json!({"text_color": "#1234567"}),
    ];
    for body in cases {
        let resp = make_json_request(
            &app,
            Method::PUT,
            "/api/v1/reading/settings",
            &body,
            Some(&token),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 for body: {body}"
        );
    }
}

/// 非法枚举值在 JSON 反序列化阶段即被拒绝（theme/pagination_mode 为强类型枚举）
#[tokio::test]
async fn test_update_reading_settings_invalid_enum() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "settings_enum", "pass123").await;

    let cases = [
        serde_json::json!({"theme": "blue"}),
        serde_json::json!({"pagination_mode": "flip"}),
    ];
    for body in cases {
        let resp = make_json_request(
            &app,
            Method::PUT,
            "/api/v1/reading/settings",
            &body,
            Some(&token),
        )
        .await;
        // 枚举校验由 serde 在反序列化时完成，axum 对 JSON 数据错误返回 422
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "expected 422 for body: {body}"
        );
    }
}

/// 非法值不应污染已保存的设置
#[tokio::test]
async fn test_update_reading_settings_invalid_not_persisted() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "settings_persist", "pass123").await;

    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/reading/settings",
        &serde_json::json!({"font_size": 999}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = make_json_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        &serde_json::json!(null),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    // 仍是 schema 默认值 18，未被非法请求覆盖
    assert_eq!(json["data"]["font_size"], 18);
}
