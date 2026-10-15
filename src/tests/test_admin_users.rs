// 管理员删除用户接口的集成测试：权限、防呆约束（自己 / 其他管理员）、
// 级联清理（DB 逐表 + 磁盘文件 + 旧 token 失效）与审计日志。

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, StatusCode},
    response::Response,
};
use sqlx::{Pool, Sqlite};

use crate::tests::{
    TEST_BOOK_CONTENT, disable_invite_code, make_json_request, make_request, register_and_login,
    response_json, setup_admin, setup_test_app, upload_test_book,
};

async fn admin_delete_user(app: &Router, admin_token: &str, user_id: i64) -> Response {
    make_request(
        app,
        Method::DELETE,
        &format!("/api/v1/admin/users/{user_id}"),
        Body::empty(),
        Some(admin_token),
    )
    .await
}

/// 按用户名取 users.id（防呆用例里的目标行）
async fn user_id_by_name(pool: &Pool<Sqlite>, username: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 按列计数（表名/列名均为本文件字面量，id 走 bind；
/// sqlx 0.9 拒绝动态 SQL 字符串，运行期拼接统一走 QueryBuilder）
async fn count_where(pool: &Pool<Sqlite>, table: &str, column: &str, id: i64) -> i64 {
    let mut qb = sqlx::QueryBuilder::<Sqlite>::new(format!(
        "SELECT COUNT(*) FROM {table} WHERE {column} = "
    ));
    qb.push_bind(id);
    qb.build_query_scalar::<i64>()
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_admin_delete_user_access_control() -> Result<()> {
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;
    let user_token = register_and_login(&app, "plain_user", "pass1234").await;

    // 用户 token：403（角色不匹配）；无 token：401
    let resp = make_request(
        &app,
        Method::DELETE,
        "/api/v1/admin/users/1",
        Body::empty(),
        Some(&user_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = make_request(
        &app,
        Method::DELETE,
        "/api/v1/admin/users/1",
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 管理员 token 正常路径在级联用例中覆盖
    Ok(())
}

#[tokio::test]
async fn test_admin_delete_user_guards() -> Result<()> {
    let (app, pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    // 与当前管理员同名的用户行：删自己，400
    let _ = register_and_login(&app, "admin", "pass1234").await;
    let self_id = user_id_by_name(&pool, "admin").await;
    let resp = admin_delete_user(&app, &admin_token, self_id).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 与其他管理员同名的用户行：管理员账号受保护，400
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/admin/admins",
        &serde_json::json!({"username": "admin2", "password": "admin123"}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = register_and_login(&app, "admin2", "pass1234").await;
    let admin2_id = user_id_by_name(&pool, "admin2").await;
    let resp = admin_delete_user(&app, &admin_token, admin2_id).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 两个目标用户行都必须原样保留
    assert_eq!(count_where(&pool, "users", "id", self_id).await, 1);
    assert_eq!(count_where(&pool, "users", "id", admin2_id).await, 1);

    // 用户不存在：404
    let resp = admin_delete_user(&app, &admin_token, 999_999).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn test_admin_delete_user_cascade() -> Result<()> {
    let (app, pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;
    disable_invite_code(&app, &admin_token).await;

    let token = register_and_login(&app, "deletee", "pass1234").await;
    let user_id = user_id_by_name(&pool, "deletee").await;

    let book = upload_test_book(&app, &token, "被删用户的书", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();
    let chapter_count =
        i64::try_from(book["chapters"].as_array().unwrap().len()).expect("chapter count fits i64");
    assert!(chapter_count >= 1, "upload should parse chapters");

    // 批注、书签、标签、阅读设置、每日统计、进度行（上传时创建）
    let note = serde_json::json!({
        "kind": "note",
        "start_locator": "0",
        "start_percent": 0.0,
        "content": "笔记"
    });
    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &note,
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bookmark = serde_json::json!({"locator": "0", "percent": 0.0, "label": "书签"});
    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/bookmarks"),
        &bookmark,
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}/tags"),
        &serde_json::json!({"tags": ["经典"]}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/reading/settings",
        &serde_json::json!({"font_size": 20}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let daily = serde_json::json!({"entries": [{"date": "2026-01-01", "reading_seconds": 60}]});
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/reading/stats/daily/{book_id}"),
        &daily,
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 前置：书籍文件与各表行确实存在
    let (file_path,): (String,) = sqlx::query_as("SELECT file_path FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        tokio::fs::metadata(&file_path).await.is_ok(),
        "file on disk"
    );
    assert_eq!(count_where(&pool, "books", "user_id", user_id).await, 1);
    assert_eq!(
        count_where(&pool, "chapters", "book_id", book_id).await,
        chapter_count
    );
    assert_eq!(count_where(&pool, "book_tags", "book_id", book_id).await, 1);
    assert_eq!(count_where(&pool, "bookmarks", "user_id", user_id).await, 1);
    assert_eq!(
        count_where(&pool, "annotations", "user_id", user_id).await,
        1
    );
    assert_eq!(
        count_where(&pool, "reading_progress", "user_id", user_id).await,
        1
    );
    assert_eq!(
        count_where(&pool, "daily_reading_stats", "user_id", user_id).await,
        1
    );
    assert_eq!(
        count_where(&pool, "reading_settings", "user_id", user_id).await,
        1
    );

    // 管理员删除
    let resp = admin_delete_user(&app, &admin_token, user_id).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 磁盘文件消失
    assert!(
        tokio::fs::metadata(&file_path).await.is_err(),
        "book file removed from disk"
    );

    // 各表行逐表清零
    assert_eq!(count_where(&pool, "users", "id", user_id).await, 0);
    assert_eq!(count_where(&pool, "books", "user_id", user_id).await, 0);
    assert_eq!(count_where(&pool, "chapters", "book_id", book_id).await, 0);
    assert_eq!(count_where(&pool, "book_tags", "book_id", book_id).await, 0);
    assert_eq!(count_where(&pool, "bookmarks", "user_id", user_id).await, 0);
    assert_eq!(
        count_where(&pool, "annotations", "user_id", user_id).await,
        0
    );
    assert_eq!(
        count_where(&pool, "reading_progress", "user_id", user_id).await,
        0
    );
    assert_eq!(
        count_where(&pool, "daily_reading_stats", "user_id", user_id).await,
        0
    );
    assert_eq!(
        count_where(&pool, "reading_settings", "user_id", user_id).await,
        0
    );

    // 该用户的旧 token 失效：用户不存在按 401 处理
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 用户列表不再包含该用户；重复删除 404
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/users",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    let json = response_json(resp).await;
    assert!(
        json["data"]["users"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u["username"] != "deletee"),
        "deleted user still listed: {json:?}"
    );
    let resp = admin_delete_user(&app, &admin_token, user_id).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 审计日志记录了 delete_user
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
        .any(|l| l["action"] == "delete_user" && l["target_id"] == user_id);
    assert!(has_delete, "delete_user audit log missing: {logs:?}");
    Ok(())
}
