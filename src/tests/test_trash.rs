// 回收站（软删除）集成测试：删除后全路径不可见、restore 恢复、
// purge 物理清理（文件 + 级联行）、trash 分页、admin include_deleted。

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, StatusCode},
};

use crate::tests::{
    disable_invite_code, make_json_request, make_request, register_and_login, response_json,
    setup_admin, setup_test_app, upload_test_book,
};

// 上传一本 public 书并返回 book_id
async fn upload_public(app: &Router, token: &str, title: &str) -> i64 {
    let book = upload_test_book(app, token, title, "第一章 内容\n正文", true).await;
    book["book_id"].as_i64().unwrap()
}

async fn req(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = make_request(app, method, uri, Body::empty(), Some(token)).await;
    let status = resp.status();
    (status, response_json(resp).await)
}

async fn soft_delete(app: &Router, book_id: i64, token: &str) {
    let (status, json) = req(
        app,
        Method::DELETE,
        &format!("/api/v1/books/{book_id}"),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "soft delete failed: {json:?}");
}

/// 删除后所有用户侧读取路径均不可见（含他人视角的公开书）
#[tokio::test]
async fn test_soft_delete_hides_book_everywhere() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token = register_and_login(&app, "trash_owner", "pass1234").await;
    let other = register_and_login(&app, "trash_other", "pass1234").await;
    let book_id = upload_public(&app, &token, "被删的书").await;

    soft_delete(&app, book_id, &token).await;

    // 非 owner 删除他人书仍是 403（且不产生软删）
    let other_book = upload_public(&app, &token, "别人的书").await;
    let (status, _) = req(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{other_book}"),
        &other,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 书架列表 / 搜索 / 公开列表 / 随机推荐 均不含已删书
    let (status, json) = req(&app, Method::GET, "/api/v1/books", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["total"], 1, "my list: {json:?}");
    assert_eq!(json["data"]["books"][0]["book_id"], other_book);

    let (_, json) = req(
        &app,
        Method::GET,
        "/api/v1/books/search?q=被删&scope=all",
        &token,
    )
    .await;
    assert_eq!(json["data"]["total"], 0, "search: {json:?}");

    // 公开列表 / 随机推荐：只含另一本未删的 public 书，不含已删书
    let (_, json) = req(&app, Method::GET, "/api/v1/books/public", &other).await;
    assert_eq!(json["data"]["total"], 1, "public list: {json:?}");
    assert_eq!(json["data"]["books"][0]["book_id"], other_book);

    let (_, json) = req(
        &app,
        Method::GET,
        "/api/v1/books/random_public?count=10",
        &other,
    )
    .await;
    let random_ids: Vec<i64> = json["data"]["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["book_id"].as_i64().unwrap())
        .collect();
    assert_eq!(random_ids, vec![other_book], "random_public: {json:?}");

    // 详情 / content / file / tags / 批注 / 书签：owner 与他人视角都 404
    for uri in [
        format!("/api/v1/books/{book_id}"),
        format!("/api/v1/books/{book_id}/content?position=0"),
        format!("/api/v1/books/{book_id}/file"),
        format!("/api/v1/books/{book_id}/tags"),
        format!("/api/v1/books/{book_id}/annotations"),
        format!("/api/v1/books/{book_id}/bookmarks"),
    ] {
        let (status, _) = req(&app, Method::GET, &uri, &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "owner GET {uri}");
        let (status, _) = req(&app, Method::GET, &uri, &other).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "other GET {uri}");
    }

    // 进度写入同样 404（check_book_access 收口）
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/reading/progress/{book_id}"),
        &serde_json::json!({"reading_time": 60}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 编辑与标签写入也 404
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"title": "改名"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}/tags"),
        &serde_json::json!({"tags": ["x"]}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    Ok(())
}

/// restore 后书架/详情恢复，进度数据保留
#[tokio::test]
async fn test_restore_book() -> Result<()> {
    let (app, pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token = register_and_login(&app, "restore_user", "pass1234").await;
    let book_id = upload_public(&app, &token, "要恢复的书").await;

    // 先制造一条进度数据
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/reading/progress/{book_id}"),
        &serde_json::json!({"reading_time": 120}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    soft_delete(&app, book_id, &token).await;

    // 他人不能恢复（404：不泄露他人回收站的存在性）
    let other = register_and_login(&app, "restore_other", "pass1234").await;
    let (status, _) = req(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/restore"),
        &other,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 正常书不在回收站，restore 404
    let normal = upload_public(&app, &token, "未删除的书").await;
    let (status, _) = req(
        &app,
        Method::POST,
        &format!("/api/v1/books/{normal}/restore"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, json) = req(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/restore"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "restore failed: {json:?}");
    // 重复 restore：已不在回收站 → 404
    let (status, _) = req(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/restore"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 列表与详情恢复可见，阅读时长保留
    let (_, json) = req(&app, Method::GET, "/api/v1/books", &token).await;
    assert_eq!(json["data"]["total"], 2);
    let (_, json) = req(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}"),
        &token,
    )
    .await;
    assert_eq!(json["data"]["reading_time"], 120, "progress preserved");

    // 进度行未被软删流程触碰
    let seconds: i64 = sqlx::query_scalar(
        "SELECT reading_time FROM reading_progress WHERE user_id = (SELECT id FROM users WHERE \
         username = 'restore_user') AND book_id = ?",
    )
    .bind(book_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(seconds, 120);

    Ok(())
}

/// purge：磁盘文件消失、annotations/进度行级联消失；仅回收站中的书可 purge
#[tokio::test]
async fn test_purge_removes_file_and_rows() -> Result<()> {
    let (app, pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token = register_and_login(&app, "purge_user", "pass1234").await;
    let book_id = upload_public(&app, &token, "要彻底删除的书").await;

    // 制造批注与书签
    for (method, uri, body) in [
        (
            Method::POST,
            format!("/api/v1/books/{book_id}/annotations"),
            serde_json::json!({"kind": "note", "start_locator": "1", "start_percent": 0.1, "content": "note"}),
        ),
        (
            Method::POST,
            format!("/api/v1/books/{book_id}/bookmarks"),
            serde_json::json!({"locator": "1", "percent": 0.2}),
        ),
    ] {
        let resp = make_json_request(&app, method, &uri, &body, Some(&token)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // 软删后行仍在，先取磁盘文件路径供 purge 后核对
    soft_delete(&app, book_id, &token).await;
    let file_path: String =
        sqlx::query_scalar("SELECT file_path FROM books WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(book_id)
            .fetch_one(&pool)
            .await?;
    assert!(
        std::path::Path::new(&file_path).exists(),
        "file exists before purge"
    );

    // 未软删的书不能 purge
    let alive = upload_public(&app, &token, "未删的书").await;
    let (status, _) = req(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{alive}/purge"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 他人不能 purge（404：不泄露他人回收站的存在性）
    let other = register_and_login(&app, "purge_other", "pass1234").await;
    let (status, _) = req(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book_id}/purge"),
        &other,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, json) = req(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book_id}/purge"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "purge failed: {json:?}");
    // 重复 purge 404
    let (status, _) = req(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book_id}/purge"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // books 行消失；级联行（进度/批注/书签）消失。
    // sqlx 0.9 禁止动态拼接 SQL 字符串：逐条静态查询
    let row: Option<i64> = sqlx::query_scalar("SELECT id FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(&pool)
        .await?;
    assert!(row.is_none(), "books row must be gone");
    let n_progress: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reading_progress WHERE book_id = ?")
            .bind(book_id)
            .fetch_one(&pool)
            .await?;
    let n_annotations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM annotations WHERE book_id = ?")
            .bind(book_id)
            .fetch_one(&pool)
            .await?;
    let n_bookmarks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bookmarks WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&pool)
        .await?;
    assert_eq!(n_progress, 0, "reading_progress rows must be cascaded");
    assert_eq!(n_annotations, 0, "annotations rows must be cascaded");
    assert_eq!(n_bookmarks, 0, "bookmarks rows must be cascaded");

    // 磁盘文件消失
    assert!(
        !std::path::Path::new(&file_path).exists(),
        "book file must be removed from disk"
    );

    Ok(())
}

/// trash 列表：分页 + 只含本人 + 行结构
#[tokio::test]
async fn test_trash_list_pagination() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token = register_and_login(&app, "trash_list_user", "pass1234").await;
    let other = register_and_login(&app, "trash_list_other", "pass1234").await;

    for i in 0..3 {
        let id = upload_public(&app, &token, &format!("回收站书{i}")).await;
        soft_delete(&app, id, &token).await;
    }
    // 他人的已删书不进我的回收站
    let other_id = upload_public(&app, &other, "别人的回收站书").await;
    soft_delete(&app, other_id, &other).await;

    let (status, json) = req(&app, Method::GET, "/api/v1/books/trash", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["total"], 3);
    let items = json["data"]["books"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    // 行结构：id/title/author/format/deleted_at
    assert!(items[0]["deleted_at"].as_str().is_some());
    assert_eq!(items[0]["format"], "txt");
    assert!(items[0]["title"].as_str().is_some());

    // 分页
    let (_, p1) = req(
        &app,
        Method::GET,
        "/api/v1/books/trash?page=1&limit=2",
        &token,
    )
    .await;
    assert_eq!(p1["data"]["books"].as_array().unwrap().len(), 2);
    let (_, p2) = req(
        &app,
        Method::GET,
        "/api/v1/books/trash?page=2&limit=2",
        &token,
    )
    .await;
    assert_eq!(p2["data"]["books"].as_array().unwrap().len(), 1);
    assert_eq!(p1["data"]["total"], 3);

    // 回收站书不出现在书架
    let (_, json) = req(&app, Method::GET, "/api/v1/books", &token).await;
    assert_eq!(json["data"]["total"], 0);

    Ok(())
}

/// admin 列表：默认不含已删书；include_deleted=true 时带 deleted_at；
/// admin DELETE 始终硬删（回收站中的书也能直接删）
#[tokio::test]
async fn test_admin_include_deleted() -> Result<()> {
    let (app, pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token = register_and_login(&app, "admin_del_user", "pass1234").await;
    let alive = upload_public(&app, &token, "存活的书").await;
    let deleted = upload_public(&app, &token, "已删的书").await;
    soft_delete(&app, deleted, &token).await;

    // 默认：只看到未删的
    let (status, json) = req(&app, Method::GET, "/api/v1/admin/books", &admin_token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["total"], 1);
    assert_eq!(json["data"]["books"][0]["book_id"], alive);
    assert!(json["data"]["books"][0]["deleted_at"].is_null());

    // include_deleted=true：两本都在，已删书带 deleted_at
    let (_, json) = req(
        &app,
        Method::GET,
        "/api/v1/admin/books?include_deleted=true",
        &admin_token,
    )
    .await;
    assert_eq!(json["data"]["total"], 2);
    let rows = json["data"]["books"].as_array().unwrap();
    let deleted_row = rows
        .iter()
        .find(|b| b["book_id"].as_i64() == Some(deleted))
        .unwrap();
    assert!(deleted_row["deleted_at"].as_str().is_some());

    // include_deleted 与搜索叠加
    let (_, json) = req(
        &app,
        Method::GET,
        "/api/v1/admin/books?include_deleted=true&q=已删",
        &admin_token,
    )
    .await;
    assert_eq!(json["data"]["total"], 1);

    // admin DELETE 对回收站中的书直接硬删
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/admin/books/{deleted}"),
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let row: Option<i64> = sqlx::query_scalar("SELECT id FROM books WHERE id = ?")
        .bind(deleted)
        .fetch_optional(&pool)
        .await?;
    assert!(row.is_none(), "admin delete must hard-delete");

    Ok(())
}
