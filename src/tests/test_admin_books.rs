// 管理员全局书籍管理接口的集成测试：权限、列表分页/搜索、编辑、删除级联。

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, StatusCode},
};
use sqlx::{Pool, Sqlite};

use crate::tests::{
    disable_invite_code, make_json_request, make_multipart_request, make_request,
    register_and_login, response_json, setup_admin, setup_test_app, upload_test_book,
};

// 上传带作者的书（upload_test_book 不支持 author 字段）
async fn upload_with_author(
    app: &Router,
    token: &str,
    title: &str,
    author: &str,
    content: &str,
    is_public: bool,
) -> serde_json::Value {
    let resp = make_multipart_request(
        app,
        "/api/v1/books/upload",
        vec![
            ("title", title),
            ("author", author),
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

async fn admin_books_list(
    app: &Router,
    admin_token: &str,
    query: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = make_request(
        app,
        Method::GET,
        &format!("/api/v1/admin/books{query}"),
        Body::empty(),
        Some(admin_token),
    )
    .await;
    let status = resp.status();
    (status, response_json(resp).await)
}

async fn book_row(pool: &Pool<Sqlite>, book_id: i64) -> Option<(String,)> {
    sqlx::query_as("SELECT file_path FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_admin_books_access_control() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;
    let user_token = register_and_login(&app, "plain_user", "pass1234").await;

    for (method, path) in [
        (Method::GET, "/api/v1/admin/books"),
        (Method::PUT, "/api/v1/admin/books/1"),
        (Method::DELETE, "/api/v1/admin/books/1"),
    ] {
        // 用户 token：403（角色不匹配）
        let resp = make_request(&app, method.clone(), path, Body::empty(), Some(&user_token)).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{method} {path}");
        // 无 token：401
        let resp = make_request(&app, method, path, Body::empty(), None).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{path}");
    }

    // 管理员 token 正常访问（空列表）
    let (status, json) = admin_books_list(&app, &admin_token, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["total"], 0);
    Ok(())
}

#[tokio::test]
async fn test_admin_books_list_search_and_update() -> Result<()> {
    let (app, pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token1 = register_and_login(&app, "owner_a", "pass1234").await;
    let token2 = register_and_login(&app, "owner_b", "pass1234").await;

    upload_with_author(&app, &token1, "三体", "刘慈欣", "第一章节 内容", false).await;
    upload_with_author(&app, &token1, "红楼梦", "曹雪芹", "第一章节 内容", true).await;
    upload_with_author(&app, &token1, "围城", "钱钟书", "第一章节 内容", false).await;
    upload_with_author(&app, &token2, "活着", "余华", "第一章节 内容", true).await;

    // 全量列表：含 owner 用户名与磁盘文件大小
    let (status, json) = admin_books_list(&app, &admin_token, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["total"], 4);
    let first = &json["data"]["books"][0];
    assert!(first["file_size"].as_i64().unwrap() > 0);
    assert_eq!(first["format"], "txt");
    assert!(first["owner_username"].as_str().is_some());

    // 分页：limit=2 的两页合计覆盖全部 4 本，total 不变
    let (_, page1) = admin_books_list(&app, &admin_token, "?page=1&limit=2").await;
    let (_, page2) = admin_books_list(&app, &admin_token, "?page=2&limit=2").await;
    assert_eq!(page1["data"]["books"].as_array().unwrap().len(), 2);
    assert_eq!(page2["data"]["books"].as_array().unwrap().len(), 2);
    assert_eq!(page1["data"]["total"], 4);
    // page 越界（无数据）返回空数组而非错误
    let (_, page3) = admin_books_list(&app, &admin_token, "?page=3&limit=2").await;
    assert_eq!(page3["data"]["books"].as_array().unwrap().len(), 0);

    // 搜索命中 标题 / 作者 / 所有者用户名
    let (_, by_title) = admin_books_list(&app, &admin_token, "?q=三体").await;
    assert_eq!(by_title["data"]["total"], 1);
    assert_eq!(by_title["data"]["books"][0]["title"], "三体");

    let (_, by_author) = admin_books_list(&app, &admin_token, "?q=曹雪芹").await;
    assert_eq!(by_author["data"]["total"], 1);
    assert_eq!(by_author["data"]["books"][0]["title"], "红楼梦");

    let (_, by_owner) = admin_books_list(&app, &admin_token, "?q=owner_b").await;
    assert_eq!(by_owner["data"]["total"], 1);
    assert_eq!(by_owner["data"]["books"][0]["owner_username"], "owner_b");

    // LIKE 特殊字符转义：字面 `%` 不当通配符用
    let (_, by_percent) = admin_books_list(&app, &admin_token, "?q=%").await;
    assert_eq!(by_percent["data"]["total"], 0);

    let book_id = by_title["data"]["books"][0]["book_id"].as_i64().unwrap();

    // 编辑：title/author/is_public 均生效（owner 侧可见）
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/admin/books/{book_id}"),
        &serde_json::json!({"title": "三体（修订版）", "author": "刘慈欣", "is_public": true}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    let detail = response_json(resp).await;
    assert_eq!(detail["data"]["title"], "三体（修订版）");
    assert_eq!(detail["data"]["is_public"], true);

    // 部分更新：仅 is_public，title 不回退
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/admin/books/{book_id}"),
        &serde_json::json!({"is_public": false}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, after) = admin_books_list(&app, &admin_token, "?q=修订版").await;
    assert_eq!(after["data"]["books"][0]["is_public"], false);
    assert_eq!(after["data"]["books"][0]["title"], "三体（修订版）");

    // 空更新体：200 No updates
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/admin/books/{book_id}"),
        &serde_json::json!({}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 不存在的书：404
    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/admin/books/999999",
        &serde_json::json!({"title": "x"}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 审计日志记录了 update_book
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/audit_logs?limit=100",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    let logs = response_json(resp).await;
    let has_update = logs["data"]["logs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["action"] == "update_book" && l["target_type"] == "book");
    assert!(has_update, "update_book audit log missing: {logs:?}");

    let _ = pool;
    Ok(())
}

#[tokio::test]
async fn test_admin_delete_book_cascade() -> Result<()> {
    let (app, pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token1 = register_and_login(&app, "book_owner", "pass1234").await;
    let book = upload_test_book(
        &app,
        &token1,
        "待删的书",
        "第一章 一\n内容\n第二章 二\n更多",
        false,
    )
    .await;
    let book_id = book["book_id"].as_i64().unwrap();

    // owner 产生批注与书签；进度行在上传时已创建
    for path in [
        "/api/v1/books/{book_id}/annotations",
        "/api/v1/books/{book_id}/bookmarks",
    ] {
        let url = path.replace("{book_id}", &book_id.to_string());
        let body = if url.ends_with("annotations") {
            serde_json::json!({"kind": "note", "start_locator": "0", "start_percent": 0.0, "content": "笔记"})
        } else {
            serde_json::json!({"locator": "0", "percent": 0.0, "label": "书签"})
        };
        let resp = make_json_request(&app, Method::POST, &url, &body, Some(&token1)).await;
        assert_eq!(resp.status(), StatusCode::OK, "create at {url}");
    }

    let (file_path,) = book_row(&pool, book_id).await.expect("book row exists");
    assert!(
        tokio::fs::metadata(&file_path).await.is_ok(),
        "file on disk"
    );

    // 非 admin 无法走管理删除
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/admin/books/{book_id}"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 管理员删除
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/admin/books/{book_id}"),
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 磁盘文件消失
    assert!(
        tokio::fs::metadata(&file_path).await.is_err(),
        "file removed from disk"
    );
    // DB 行全部级联消失
    assert!(book_row(&pool, book_id).await.is_none(), "books row gone");
    let (annotations, bookmarks, progress): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM annotations WHERE book_id = ?), (SELECT COUNT(*) FROM \
         bookmarks WHERE book_id = ?), (SELECT COUNT(*) FROM reading_progress WHERE book_id = ?)",
    )
    .bind(book_id)
    .bind(book_id)
    .bind(book_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((annotations, bookmarks, progress), (0, 0, 0));

    // owner 侧访问与重复删除均 404
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/admin/books/{book_id}"),
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 审计日志记录了 delete_book
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/audit_logs?limit=100",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    let logs = response_json(resp).await;
    let has_delete = logs["data"]["logs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["action"] == "delete_book" && l["target_id"] == book_id);
    assert!(has_delete, "delete_book audit log missing: {logs:?}");
    Ok(())
}
