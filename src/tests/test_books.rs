// 书架列表排序/格式筛选、书籍简介编辑的集成测试。

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, StatusCode},
};

use crate::tests::{
    make_json_request, make_multipart_request, make_request, register_and_login, response_json,
    setup_test_app, upload_test_book,
};

// e2e 夹具中的多章节 epub（按内容签名识别格式，元数据无简介）
const SAMPLE_EPUB: &[u8] = include_bytes!("../../frontend/e2e/assets/sample.epub");

async fn list_books(app: &Router, token: &str, query: &str) -> (StatusCode, serde_json::Value) {
    let resp = make_request(
        app,
        Method::GET,
        &format!("/api/v1/books{query}"),
        Body::empty(),
        Some(token),
    )
    .await;
    let status = resp.status();
    (status, response_json(resp).await)
}

async fn list_titles(app: &Router, token: &str, query: &str) -> Vec<String> {
    let (status, json) = list_books(app, token, query).await;
    assert_eq!(status, StatusCode::OK, "list failed: {json:?}");
    json["data"]["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["title"].as_str().unwrap().to_string())
        .collect()
}

/// 上传带可选 author/description 字段的书
async fn upload_book_opts(
    app: &Router,
    token: &str,
    title: &str,
    author: Option<&str>,
    description: Option<&str>,
    file: (&str, &[u8]),
) -> serde_json::Value {
    let mut fields = vec![("title", title), ("is_public", "false")];
    if let Some(a) = author {
        fields.push(("author", a));
    }
    if let Some(d) = description {
        fields.push(("description", d));
    }
    let resp = make_multipart_request(
        app,
        "/api/v1/books/upload",
        fields,
        Some(("file", file.0, file.1)),
        Some(token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "upload failed");
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0, "upload error: {json:?}");
    json["data"].clone()
}

#[tokio::test]
async fn test_book_list_sort() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "sort_user", "password1").await;

    // 标题大小写混排验证 NOCASE；一本无作者验证 COALESCE 兜底
    upload_book_opts(
        &app,
        &token,
        "cherry",
        Some("cathy"),
        None,
        ("c.txt", "内容一".as_bytes()),
    )
    .await;
    upload_book_opts(
        &app,
        &token,
        "apple",
        Some("alice"),
        None,
        ("a.txt", "内容二".as_bytes()),
    )
    .await;
    upload_book_opts(
        &app,
        &token,
        "Banana",
        None,
        None,
        ("b.txt", "内容三".as_bytes()),
    )
    .await;

    // 默认（无参数）= 旧行为：全部未读（last_read NULL 排最后）→ 创建时间倒序
    assert_eq!(list_titles(&app, &token, "").await, [
        "Banana", "apple", "cherry"
    ]);
    // 显式默认值与缺省等价
    assert_eq!(
        list_titles(&app, &token, "?sort=last_read&order=desc").await,
        ["Banana", "apple", "cherry"]
    );

    // 标题 NOCASE 升序：apple < Banana < cherry
    assert_eq!(list_titles(&app, &token, "?sort=title&order=asc").await, [
        "apple", "Banana", "cherry"
    ]);
    assert_eq!(list_titles(&app, &token, "?sort=title&order=desc").await, [
        "cherry", "Banana", "apple"
    ]);

    // 作者升序：无作者（COALESCE ''）排最前；降序落最后
    assert_eq!(list_titles(&app, &token, "?sort=author&order=asc").await, [
        "Banana", "apple", "cherry"
    ]);
    assert_eq!(
        list_titles(&app, &token, "?sort=author&order=desc").await,
        ["cherry", "apple", "Banana"]
    );

    // 创建时间升序 = 插入顺序（id 兜底保证稳定）
    assert_eq!(
        list_titles(&app, &token, "?sort=created&order=asc").await,
        ["cherry", "apple", "Banana"]
    );
    assert_eq!(
        list_titles(&app, &token, "?sort=created&order=desc").await,
        ["Banana", "apple", "cherry"]
    );

    Ok(())
}

#[tokio::test]
async fn test_book_list_sort_invalid_params() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "sort_bad_user", "password1").await;

    for (query, msg) in [
        ("?sort=priority", "Invalid sort"),
        ("?order=sideways", "Invalid order"),
        ("?format=mp3", "Invalid format"),
    ] {
        let (status, json) = list_books(&app, &token, query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "query {query}: {json:?}");
        assert!(
            json["message"].as_str().unwrap().contains(msg),
            "query {query}: {json:?}"
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_book_list_format_filter() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "filter_user", "password1").await;

    upload_book_opts(
        &app,
        &token,
        "文本书",
        None,
        None,
        ("a.txt", "内容".as_bytes()),
    )
    .await;
    upload_book_opts(&app, &token, "电子书", None, None, ("b.epub", SAMPLE_EPUB)).await;

    // txt 筛选：total 与列表同步过滤
    let (status, json) = list_books(&app, &token, "?format=txt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["total"], 1);
    assert_eq!(json["data"]["books"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"]["books"][0]["format"], "txt");

    // epub 筛选
    let (status, json) = list_books(&app, &token, "?format=epub").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["total"], 1);
    assert_eq!(json["data"]["books"][0]["title"], "电子书");

    // 筛选 + 排序组合
    let (status, json) = list_books(&app, &token, "?format=epub&sort=created&order=asc").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["books"][0]["title"], "电子书");

    Ok(())
}

#[tokio::test]
async fn test_update_book_description() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "desc_user", "password1").await;
    let book = upload_test_book(
        &app,
        &token,
        "有简介的书",
        crate::tests::TEST_BOOK_CONTENT,
        false,
    )
    .await;
    let book_id = book["book_id"].as_i64().unwrap();

    // 上传时未填：简介为 null
    let (_, json) = list_books(&app, &token, "").await;
    assert!(json["data"]["books"][0]["description"].is_null());

    // PUT 设置简介
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"description": "这是一个精彩的简介"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = list_books(&app, &token, "").await;
    assert_eq!(
        json["data"]["books"][0]["description"],
        "这是一个精彩的简介"
    );

    // 边界：2000 字符允许
    let boundary = "界".repeat(2000);
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"description": boundary}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 超限 400
    let over = "界".repeat(2001);
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"description": over}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(json["message"].as_str().unwrap().contains("'description'"));

    // 空串 = 清空；纯空白等价清空
    for value in ["", "   "] {
        let resp = make_json_request(
            &app,
            Method::PUT,
            &format!("/api/v1/books/{book_id}"),
            &serde_json::json!({"description": value}),
            Some(&token),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let (_, json) = list_books(&app, &token, "").await;
        assert!(
            json["data"]["books"][0]["description"].is_null(),
            "description should be cleared"
        );
    }

    // 缺省字段 = 不修改：先设置再 PUT 其它字段，简介保持不变
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"description": "保留我"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"title": "改名不简介"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, json) = list_books(&app, &token, "").await;
    assert_eq!(json["data"]["books"][0]["description"], "保留我");

    Ok(())
}

#[tokio::test]
async fn test_upload_book_with_description() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "upload_desc_user", "password1").await;

    // txt：表单提供的简介直接入库
    upload_book_opts(
        &app,
        &token,
        "手填简介",
        None,
        Some("  上传时手填的简介  "),
        ("a.txt", "内容".as_bytes()),
    )
    .await;
    let (_, json) = list_books(&app, &token, "?sort=created&order=desc").await;
    assert_eq!(json["data"]["books"][0]["description"], "上传时手填的简介");

    // epub：表单显式提供的简介生效（覆盖元数据提取；本夹具无元数据简介）
    upload_book_opts(
        &app,
        &token,
        "电子书",
        None,
        Some("epub 表单简介"),
        ("b.epub", SAMPLE_EPUB),
    )
    .await;
    let (_, json) = list_books(&app, &token, "?format=epub&sort=created&order=desc").await;
    assert_eq!(json["data"]["books"][0]["description"], "epub 表单简介");

    // 上传超限 400
    let over = "界".repeat(2001);
    let fields = vec![
        ("title", "超限"),
        ("is_public", "false"),
        ("description", over.as_str()),
    ];
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        fields,
        Some(("file", "c.txt", "内容".as_bytes())),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(json["message"].as_str().unwrap().contains("'description'"));

    Ok(())
}
