use axum::{
    body::Body,
    http::{Method, StatusCode},
};

use super::*;

macro_rules! section {
    ($name:expr) => {
        println!("\n===== {} =====", $name);
    };
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_full_integration() {
    let (app, pool) = setup_test_app().await.unwrap();

    // ================================================================
    //  A. Admin & Setup
    // ================================================================
    section!("A. Admin & Setup");

    // A1: check_setup 未初始化
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/check_setup",
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["is_setup"], false);

    // A2: setup admin
    let (admin_token, _) = setup_admin(&app).await;

    // A3: check_setup 已初始化
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/check_setup",
        Body::empty(),
        None,
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["is_setup"], true);

    // A4: 重复设置管理员 (error 1006)
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/admin/setup",
        &serde_json::json!({"username": "another", "password": "another"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("Admin already set up")
    );

    // A5: admin login
    let resp = login_as_admin(&app).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // A6: update settings (关闭邀请码)
    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/admin/settings",
        &serde_json::json!({"invite_code_required": false}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // A7: get settings
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/settings",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["invite_code_required"], false);

    // A8: create invite code
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/admin/invite_code",
        &serde_json::json!({"limit_times": 1, "description": "test invite"}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);
    let invite_code = json["data"]["invite_code"].as_str().unwrap().to_string();

    // A9: list invite codes
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/invite_codes",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert!(json["data"]["invite_codes"].is_array());

    // ================================================================
    //  B. User Registration & Auth
    // ================================================================
    section!("B. User Registration & Auth");

    // B1: register user1
    let token1 = register_and_login(&app, "user1", "password1").await;

    // B2: register user2
    let token2 = register_and_login(&app, "user2", "password2").await;

    // B3: login user1
    let resp = login(&app, "user1", "password1", "device1").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);

    // B4: login with wrong password
    let resp = login(&app, "user1", "wrongpassword", "device1").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let json = response_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("Invalid credentials")
    );

    // B5: get user_info
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["username"], "user1");
    assert_eq!(json["data"]["book_count"], 0);

    // B6: duplicate username
    let resp = register_user(&app, "user1", "password1", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("Username already exists")
    );

    // B7: change password
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/change_password",
        &serde_json::json!({"old_password": "password1", "new_password": "newpassword1"}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // B8: login with new password
    let resp = login(&app, "user1", "newpassword1", "device1").await;
    assert_eq!(resp.status(), StatusCode::OK);
    // refresh token1
    let json = response_json(resp).await;
    let token1 = json["data"]["token"].as_str().unwrap().to_string();

    // B9: login with old password fails
    let resp = login(&app, "user1", "password1", "device1").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // ================================================================
    //  C. Invite Code Flow
    // ================================================================
    section!("C. Invite Code Flow");

    // C1: enable invite_code_required
    let _resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/admin/settings",
        &serde_json::json!({"invite_code_required": true}),
        Some(&admin_token),
    )
    .await;

    // C2: register with valid invite code
    let resp = register_user(&app, "user3", "password3", Some(&invite_code)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // C3: register with same (used) invite code
    let resp = register_user(&app, "user4", "password4", Some(&invite_code)).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(json["message"].as_str().unwrap().contains("Invite code"));

    // C4: register without invite code (required)
    let resp = register_user(&app, "user5", "password5", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(json["message"].as_str().unwrap().contains("Invite code"));

    // C5: disable invite_code_required
    let _resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/admin/settings",
        &serde_json::json!({"invite_code_required": false}),
        Some(&admin_token),
    )
    .await;

    // ================================================================
    //  D. Book Management
    // ================================================================
    section!("D. Book Management");

    // D1: upload public book
    let book1 = upload_test_book(&app, &token1, "测试书籍1", TEST_BOOK_CONTENT, true).await;
    let book1_id = book1["book_id"].as_i64().unwrap();

    // D2: upload private book
    let book2 = upload_test_book(&app, &token1, "测试书籍2", TEST_BOOK_CONTENT, false).await;
    let book2_id = book2["book_id"].as_i64().unwrap();

    // D3: list books (user1 should have 2)
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books",
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 2);
    let book_list = json["data"]["books"].as_array().unwrap();
    assert_eq!(book_list.len(), 2);

    // D4: get book detail
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book1_id}"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["book_id"], book1_id);
    assert!(json["data"]["chapters"].is_array());

    // D5: get book content
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book1_id}/content?position=0&length=100"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_ne!(json["data"]["content"].as_str().unwrap(), "");
    assert!(json["data"]["next_position"].as_i64().unwrap() > 0);

    // D6: update book (change title + is_public=false)
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book1_id}"),
        &serde_json::json!({"title": "修改后的标题", "is_public": false}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // D7: verify update
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book1_id}"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["title"], "修改后的标题");
    assert_eq!(json["data"]["is_public"], false);

    // D8: user2 tries to access user1's now-private book (should fail)
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book1_id}"),
        Body::empty(),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // D9: user2 tries to delete user1's book (should fail)
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book1_id}"),
        Body::empty(),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // D10: user1 deletes book2
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book2_id}"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // D11: user1 lists books (should have 1)
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books",
        Body::empty(),
        Some(&token1),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 1);

    // ================================================================
    //  E. Public Books
    // ================================================================
    section!("E. Public Books");

    // re-upload a public book for public listing tests
    let book3 = upload_test_book(&app, &token1, "公开书籍", TEST_BOOK_CONTENT, true).await;
    let _book3_id = book3["book_id"].as_i64().unwrap();

    // E1: list public books
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/public",
        Body::empty(),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert!(json["data"]["total"].as_i64().unwrap() >= 1);

    // E2: random public books
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/random_public?count=1",
        Body::empty(),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);

    // ================================================================
    //  F. Reading Settings
    // ================================================================
    section!("F. Reading Settings");

    // F1: get default settings
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 18);
    assert_eq!(json["data"]["background_color"], "#F5F5DC");
    assert_eq!(json["data"]["simplified_chinese"], true);

    // F2: update all fields
    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/reading/settings",
        &serde_json::json!({
            "font_size": 22,
            "background_color": "#FFFFFF",
            "text_color": "#333333",
            "line_height": 2.0,
            "letter_spacing": 0.1,
            "paragraph_spacing": 1.5,
            "reading_width": 700,
            "text_indent": 3.0,
            "simplified_chinese": false
        }),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // F3: verify all updates
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        Body::empty(),
        Some(&token1),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 22);
    assert_eq!(json["data"]["background_color"], "#FFFFFF");
    assert_eq!(json["data"]["text_color"], "#333333");
    assert_eq!(json["data"]["simplified_chinese"], false);

    // F4: partial update (only font_size)
    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/reading/settings",
        &serde_json::json!({"font_size": 24}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // F5: verify only font_size changed
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        Body::empty(),
        Some(&token1),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 24);
    assert_eq!(json["data"]["background_color"], "#FFFFFF"); // unchanged

    // ================================================================
    //  G. Reading Progress & Heartbeat
    // ================================================================
    section!("G. Reading Progress & Heartbeat");

    // 心跳核心逻辑测试：直接调用 process_heartbeat_logic
    // （协议层 WS 端到端测试见 test_ws_reading_session）
    let user1_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'user1'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // G1: 首次心跳创建进度记录
    let result = crate::routes::reading::process_heartbeat_logic(
        &pool, user1_id, book1_id, "100", 0.05, "device1",
    )
    .await
    .unwrap();
    assert!(result.synced);
    assert_eq!(result.locator, "100");
    assert!((result.percent * 100.0 - 5.0).abs() < f64::EPSILON);
    assert_eq!(result.reading_time, 0);
    assert!(result.updated);

    // G2: 同设备再次心跳，进度更新（updated=true）
    let result = crate::routes::reading::process_heartbeat_logic(
        &pool, user1_id, book1_id, "200", 0.10, "device1",
    )
    .await
    .unwrap();
    assert!(result.synced);
    assert_eq!(result.locator, "200");
    assert!(result.updated);

    // G3: 不同设备心跳：返回服务端进度（synced=false），不更新（updated=false）
    let result = crate::routes::reading::process_heartbeat_logic(
        &pool, user1_id, book1_id, "999", 0.99, "device2",
    )
    .await
    .unwrap();
    assert!(!result.synced);
    assert_eq!(result.locator, "200"); // server's saved progress
    assert!(!result.updated);

    // G4: user2 accesses user1's public book
    let public_book = upload_test_book(&app, &token1, "公开书", TEST_BOOK_CONTENT, true).await;
    let public_book_id = public_book["book_id"].as_i64().unwrap();

    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{public_book_id}"),
        Body::empty(),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let chapters = json["data"]["chapters"].as_array().unwrap();

    // G5: jump to chapter (if chapters exist)
    if !chapters.is_empty() {
        let chapter_id = chapters[0]["chapter_id"].as_i64().unwrap();
        let resp = make_request(
            &app,
            Method::GET,
            &format!("/api/v1/books/{public_book_id}/jump_to_chapter?chapter_id={chapter_id}"),
            Body::empty(),
            Some(&token2),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["code"], 0);
        assert!(json["data"]["position"].is_number());
    }

    // ================================================================
    //  H. Admin User Management
    // ================================================================
    section!("H. Admin User Management");

    // H1: list users
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/users",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let users = json["data"]["users"].as_array().unwrap();
    assert!(users.len() >= 2);

    // H2: reset user2's password
    let user2_id = users.iter().find(|u| u["username"] == "user2").unwrap()["user_id"]
        .as_i64()
        .unwrap();

    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{user2_id}/reset_password"),
        &serde_json::json!({"new_password": "resetpwd123"}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // H3: user2 login with new password
    let resp = login(&app, "user2", "resetpwd123", "device1").await;
    assert_eq!(resp.status(), StatusCode::OK);

    // ================================================================
    //  I. Error Handling
    // ================================================================
    section!("I. Error Handling");

    // I1: no auth token
    let resp = make_request(&app, Method::GET, "/api/v1/books", Body::empty(), None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // I2: non-existent book
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/99999",
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // I3: content with invalid position
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book1_id}/content?position=999999"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // I4: user token for admin endpoint
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/users",
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // I5: invalid file format (上传真正的二进制文件，非 txt/epub)
    let exe_bytes: &[u8] = &[0x4d, 0x5a, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00]; // MZ = Windows EXE
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "bad file"), ("is_public", "true")],
        Some(("file", "test.exe", exe_bytes)),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // I6: change password with wrong old password
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/change_password",
        &serde_json::json!({"old_password": "wrongpassword", "new_password": "newpwd"}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("Incorrect old password")
    );

    println!("\n===== All integration tests passed! =====");
}

#[tokio::test]
async fn test_bookmarks_and_annotations() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "reader", "password1").await;

    // 上传一本书作为测试目标
    let book = upload_test_book(&app, &token, "笔记测试书", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    // ================================================================
    //  书签 CRUD
    // ================================================================

    // 创建书签
    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/bookmarks"),
        &serde_json::json!({"locator": "100", "percent": 0.1, "label": "第一章末尾"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let bookmark_id = json["data"]["id"].as_i64().unwrap();
    assert_eq!(json["data"]["locator"], "100");
    assert_eq!(json["data"]["percent"], 0.1);

    // 列出书签
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/bookmarks"),
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["bookmarks"].as_array().unwrap().len(), 1);

    // 删除书签
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book_id}/bookmarks/{bookmark_id}"),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 删除不存在书签应 404
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book_id}/bookmarks/99999"),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // ================================================================
    //  批注 CRUD
    // ================================================================

    // 创建笔记
    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &serde_json::json!({
            "kind": "note",
            "start_locator": "50",
            "start_percent": 0.05,
            "content": "这段写得好"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let note_id = json["data"]["id"].as_i64().unwrap();

    // 创建高亮：必须带 end_locator + end_percent > start_percent
    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &serde_json::json!({
            "kind": "highlight",
            "start_locator": "200",
            "end_locator": "250",
            "start_percent": 0.2,
            "end_percent": 0.25,
            "excerpt": "..."
        }),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let highlight_id = json["data"]["id"].as_i64().unwrap();

    // 高亮校验：end_position 缺失应失败
    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &serde_json::json!({"kind": "highlight", "start_locator": "10", "start_percent": 0.01}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 列出批注
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/annotations"),
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["annotations"].as_array().unwrap().len(), 2);

    // 更新批注内容
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}/annotations/{note_id}"),
        &serde_json::json!({"content": "更新后的笔记"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 删除批注
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/v1/books/{book_id}/annotations/{highlight_id}"),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 列表应只剩一条
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/annotations"),
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["annotations"].as_array().unwrap().len(), 1);

    println!("\n===== Bookmarks/Annotations tests passed! =====");
}

#[tokio::test]
async fn test_book_search() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "searcher", "password1").await;

    upload_test_book(&app, &token, "三体", "刘慈欣\n第一章 科学边界\n...", false).await;
    upload_test_book(&app, &token, "球状闪电", "刘慈欣\n序章\n...", false).await;
    upload_test_book(&app, &token, "小王子", "圣埃克苏佩里\n...", false).await;

    // 按 title 搜
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/search?q=%E4%B8%89", // "三"
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 1);
    assert_eq!(json["data"]["books"][0]["title"], "三体");

    // 按 author 搜（刘慈欣匹配两本）
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/search?q=%E5%88%98%E6%85%88%E6%AC%A3", // "刘慈欣"
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 2);

    // 按作者搜（圣埃克苏佩里 → 小王子）
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/search?q=%E5%9C%A3%E5%9F%83%E5%85%8B", // "圣埃"
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 1);
    assert_eq!(json["data"]["books"][0]["title"], "小王子");

    // 空查询应 400
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/search?q=",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 非法 scope 应 400
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/search?q=x&scope=invalid",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    println!("\n===== Book search tests passed! =====");
}

#[tokio::test]
async fn test_user_ban_and_delete_account() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;

    // 关闭邀请码，方便注册
    let _ = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/admin/settings",
        &serde_json::json!({"invite_code_required": false}),
        Some(&admin_token),
    )
    .await;

    let token_user = register_and_login(&app, "ban_me", "pass1234").await;

    // user 上传一本书
    let _book = upload_test_book(&app, &token_user, "测试", TEST_BOOK_CONTENT, false).await;

    // 列出用户拿 user_id
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/users",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    let json = response_json(resp).await;
    let user_id = json["data"]["users"][0]["user_id"].as_i64().unwrap();
    assert_eq!(json["data"]["users"][0]["status"], "active");

    // ================================================================
    //  封禁 / 解封
    // ================================================================

    // 封禁
    let resp = make_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/ban"),
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 用户原 token 应失效（被封禁）
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&token_user),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 列表 status=banned
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/users",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["users"][0]["status"], "banned");

    // 解封
    let resp = make_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/unban"),
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 但 token_version 已变更，旧 token 仍失效；重新登录才能用
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({"username": "ban_me", "password": "pass1234"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let new_token = json["data"]["token"].as_str().unwrap().to_string();

    // ================================================================
    //  自助注销
    // ================================================================

    // 错密码应失败
    let resp = make_json_request(
        &app,
        Method::DELETE,
        "/api/v1/auth/account",
        &serde_json::json!({"password": "wrong"}),
        Some(&new_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 正确密码注销
    let resp = make_json_request(
        &app,
        Method::DELETE,
        "/api/v1/auth/account",
        &serde_json::json!({"password": "pass1234"}),
        Some(&new_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 再登录应失败（账户已删）
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({"username": "ban_me", "password": "pass1234"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    println!("\n===== Ban / Delete account tests passed! =====");
}

#[tokio::test]
async fn test_etag_caching() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "cacher", "password1").await;

    // 第一次获取设置：应该返回 200 + ETag
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let etag = resp
        .headers()
        .get("etag")
        .expect("ETag should be set")
        .to_str()
        .unwrap()
        .to_string();

    // 第二次带 If-None-Match：应该返回 304
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        Body::empty(),
        Some(&token),
    )
    .await;
    // make_request 不支持自定义 header，重发同样请求验证 ETag 仍然稳定
    let _ = resp;
    // 直接重新发带 If-None-Match 的请求
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/v1/reading/settings")
        .header("Authorization", format!("Bearer {token}"))
        .header("If-None-Match", &etag)
        .header("X-Forwarded-For", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);

    println!("\n===== ETag caching tests passed! =====");
}

async fn login_as_admin(app: &Router) -> Response {
    make_json_request(
        app,
        Method::POST,
        "/api/v1/auth/admin/login",
        &serde_json::json!({"username": "admin", "password": "admin123"}),
        None,
    )
    .await
}

// ----------------------------------------------------------------
//  WebSocket 阅读会话端到端测试
// ----------------------------------------------------------------

/// 已连接的 ws 客户端类型别名（避免到处写长类型）
type WsClient =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 启动一个真实 TCP 服务（提供 `connect_info` 给限流层），返回监听地址
async fn spawn_server(app: Router) -> std::net::SocketAddr {
    use std::net::SocketAddr;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    addr
}

async fn ws_connect(addr: std::net::SocketAddr, token: &str) -> WsClient {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let url = format!("ws://{addr}/api/v1/reading/ws?token={token}");
    let mut req = url.into_client_request().unwrap();
    // 注入 X-Forwarded-For 以满足 SmartIpKeyExtractor
    req.headers_mut()
        .insert("X-Forwarded-For", "127.0.0.1".parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws
}

async fn send_json(ws: &mut WsClient, msg: &serde_json::Value) {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();
}

/// 接收一条消息，跳过 Ping/Pong，将 Text 帧解析为 JSON
async fn recv_json(ws: &mut WsClient) -> serde_json::Value {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    loop {
        match ws.next().await.unwrap().unwrap() {
            Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
            Message::Close(_) => panic!("connection closed unexpectedly"),
            // Ping/Pong/Binary/Frame 由底层处理或忽略
            _ => {},
        }
    }
}

#[tokio::test]
async fn test_ws_reading_session() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "wsuser", "password").await;
    let book = upload_test_book(&app, &token, "wsbook", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    let addr = spawn_server(app).await;

    // 设备 1 建连、订阅、首次心跳
    let mut ws_a = ws_connect(addr, &token).await;
    send_json(
        &mut ws_a,
        &serde_json::json!({"type":"subscribe","book_id":book_id,"device_id":"device1"}),
    )
    .await;
    let v = recv_json(&mut ws_a).await;
    assert_eq!(v["type"], "subscribed");

    send_json(
        &mut ws_a,
        &serde_json::json!({"type":"heartbeat","book_id":book_id,"locator":"100","percent":0.05}),
    )
    .await;
    let v = recv_json(&mut ws_a).await;
    assert_eq!(v["type"], "heartbeat_ack");
    assert_eq!(v["synced"], true);
    assert_eq!(v["locator"], "100");
    assert_eq!(v["percent"], 0.05);
    assert_eq!(v["reading_time"], 0); // 首次心跳不累加

    // 设备 2 建连、订阅；心跳时检测到 last_device=device1 → synced=false
    let mut ws_b = ws_connect(addr, &token).await;
    send_json(
        &mut ws_b,
        &serde_json::json!({"type":"subscribe","book_id":book_id,"device_id":"device2"}),
    )
    .await;
    let v = recv_json(&mut ws_b).await;
    assert_eq!(v["type"], "subscribed");

    send_json(
        &mut ws_b,
        &serde_json::json!({"type":"heartbeat","book_id":book_id,"locator":"999","percent":0.99}),
    )
    .await;
    let v = recv_json(&mut ws_b).await;
    assert_eq!(v["type"], "heartbeat_ack");
    assert_eq!(v["synced"], false);
    assert_eq!(v["locator"], "100"); // 服务端进度（来自 device1）

    // 设备 1 再次心跳：广播 progress_update 给设备 2
    send_json(
        &mut ws_a,
        &serde_json::json!({"type":"heartbeat","book_id":book_id,"locator":"150","percent":0.08}),
    )
    .await;
    let v = recv_json(&mut ws_a).await;
    assert_eq!(v["type"], "heartbeat_ack");
    assert_eq!(v["locator"], "150");

    let v = recv_json(&mut ws_b).await;
    assert_eq!(v["type"], "progress_update");
    assert_eq!(v["locator"], "150");
    assert_eq!(v["device_id"], "device1");

    // unsubscribe
    send_json(
        &mut ws_b,
        &serde_json::json!({"type":"unsubscribe","book_id":book_id}),
    )
    .await;
    let v = recv_json(&mut ws_b).await;
    assert_eq!(v["type"], "unsubscribed");

    // 退订后再心跳应得到 error（设备 2）
    send_json(
        &mut ws_b,
        &serde_json::json!({"type":"heartbeat","book_id":book_id,"locator":"1","percent":0.0}),
    )
    .await;
    let v = recv_json(&mut ws_b).await;
    assert_eq!(v["type"], "error");

    let _ = ws_a.close(None).await;
    let _ = ws_b.close(None).await;
}

#[tokio::test]
async fn test_ws_unauthenticated_rejected() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let (app, _pool) = setup_test_app().await.unwrap();
    let addr = spawn_server(app).await;

    // 无 token：connect 应被服务端拒绝（axum 在升级阶段返回错误响应）
    let url = format!("ws://{addr}/api/v1/reading/ws");
    let mut req = url.into_client_request().unwrap();
    req.headers_mut()
        .insert("X-Forwarded-For", "127.0.0.1".parse().unwrap());
    let result = tokio_tungstenite::connect_async(req).await;
    assert!(result.is_err(), "ws without token should be rejected");
}

#[tokio::test]
async fn test_ws_annotation_bookmark_sync() {
    // 验证：通过 HTTP 接口增删书签 / 批注时，订阅了同 (user, book) 的其它
    // ws 连接会收到 BookmarkChanged / AnnotationChanged 通知
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "syncuser", "password1").await;
    let book = upload_test_book(&app, &token, "syncbook", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    // 复用同一个 Router clone：HTTP oneshot 与 TCP ws 共享 AppState
    let addr = spawn_server(app.clone()).await;

    // 设备 A：ws 连接 + 订阅
    let mut ws_a = ws_connect(addr, &token).await;
    send_json(
        &mut ws_a,
        &serde_json::json!({"type":"subscribe","book_id":book_id,"device_id":"deviceA"}),
    )
    .await;
    let _ = recv_json(&mut ws_a).await; // subscribed

    // 设备 B：ws 连接 + 订阅（带 X-Device-Id，便于后续 HTTP 调用排除自身）
    let mut ws_b = ws_connect(addr, &token).await;
    send_json(
        &mut ws_b,
        &serde_json::json!({"type":"subscribe","book_id":book_id,"device_id":"deviceB"}),
    )
    .await;
    let _ = recv_json(&mut ws_b).await; // subscribed

    // 设备 B 通过 HTTP 创建书签（带 X-Device-Id: deviceB）
    // 期望：设备 A 收到 bookmark_changed，设备 B 自己被排除不收到
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/books/{book_id}/bookmarks"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("X-Device-Id", "deviceB")
        .header("X-Forwarded-For", "127.0.0.1")
        .body(Body::from(
            serde_json::to_vec(
                &serde_json::json!({"locator": "50", "percent": 0.05, "label": "标记"}),
            )
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bookmark_json: serde_json::Value = response_json(resp).await;
    let created_id = bookmark_json["data"]["id"].as_i64().unwrap();

    // 设备 A 应收到 bookmark_changed
    let v = recv_json(&mut ws_a).await;
    assert_eq!(v["type"], "bookmark_changed");
    assert_eq!(v["book_id"], book_id);
    assert_eq!(v["action"], "created");
    assert_eq!(v["bookmark_id"], created_id);

    // 设备 A 通过 HTTP 创建批注（无 X-Device-Id，所以设备 B 也应收到，包括设备 A
    // 自己）
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/books/{book_id}/annotations"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("X-Forwarded-For", "127.0.0.1")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "kind": "note",
                "start_locator": "10",
                "start_percent": 0.01,
                "content": "笔记"
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let aj: serde_json::Value = response_json(resp).await;
    let annotation_id = aj["data"]["id"].as_i64().unwrap();

    // 设备 A 自己也订阅了，所以会收到（HTTP 调用方未声明 device_id）
    let v = recv_json(&mut ws_a).await;
    assert_eq!(v["type"], "annotation_changed");
    assert_eq!(v["action"], "created");
    assert_eq!(v["annotation_id"], annotation_id);

    // 设备 B 也收到
    let v = recv_json(&mut ws_b).await;
    assert_eq!(v["type"], "annotation_changed");
    assert_eq!(v["action"], "created");
    assert_eq!(v["annotation_id"], annotation_id);

    // 设备 B 通过 HTTP 删除书签（带 X-Device-Id: deviceB）
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/api/v1/books/{book_id}/bookmarks/{created_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Device-Id", "deviceB")
        .header("X-Forwarded-For", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let v = recv_json(&mut ws_a).await;
    assert_eq!(v["type"], "bookmark_changed");
    assert_eq!(v["action"], "deleted");
    assert_eq!(v["bookmark_id"], created_id);

    let _ = ws_a.close(None).await;
    let _ = ws_b.close(None).await;
}

// ----------------------------------------------------------------
//  多格式书籍上传 / 下载 / 元数据
// ----------------------------------------------------------------

/// 构造一个最小合法的 PDF（1 页空白页），用于上传测试
///
/// 使用 lopdf 直接生成，确保结构合法；手工拼装 PDF 字节流的话 xref
/// 偏移很难对齐，lopdf 会拒绝。
fn minimal_pdf_bytes() -> Vec<u8> {
    use lopdf::{Dictionary, Document, Object, StringFormat, dictionary};

    let mut doc = Document::with_version("1.4");

    // Page tree：先创建空的 Pages 字典，再创建 Page，最后用 Reference 互联
    let page_tree_id = doc.add_object(Dictionary::from_iter([
        ("Type", "Pages".into()),
        ("Count", 1u32.into()),
    ]));
    let page_id = doc.add_object(Dictionary::from_iter([
        ("Type", "Page".into()),
        ("Parent", Object::Reference(page_tree_id)),
        (
            "MediaBox",
            vec![0.into(), 0.into(), 200.into(), 200.into()].into(),
        ),
    ]));
    doc.get_object_mut(page_tree_id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Kids", vec![Object::Reference(page_id)]);

    let catalog_id = doc.add_object(Dictionary::from_iter([
        ("Type", "Catalog".into()),
        ("Pages", Object::Reference(page_tree_id)),
    ]));
    doc.trailer.set("Root", Object::Reference(catalog_id));

    // Info 字典：标题/作者
    let mut info = Dictionary::new();
    info.set(
        "Title",
        Object::String(b"Test PDF".to_vec(), StringFormat::Literal),
    );
    info.set(
        "Author",
        Object::String(b"Tester".to_vec(), StringFormat::Literal),
    );
    let info_id = doc.add_object(info);
    doc.trailer.set("Info", Object::Reference(info_id));

    // 避免未使用 import 警告
    let _ = dictionary! {};

    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("failed to serialize PDF");
    buf
}

#[tokio::test]
async fn test_pdf_upload_and_download() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "pdfuser", "password1").await;

    let pdf = minimal_pdf_bytes();

    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "PDF测试"), ("is_public", "false")],
        Some(("file", "test.pdf", &pdf)),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "PDF upload failed");
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0, "upload error: {json:?}");
    assert_eq!(json["data"]["format"], "pdf");
    assert!(
        json["data"]["total_pages"].as_i64().unwrap_or(0) >= 1,
        "PDF should have at least 1 page"
    );
    assert_eq!(json["data"]["total_chars"], 0); // pdf 不计算字符数
    let book_id = json["data"]["book_id"].as_i64().unwrap();

    // 详情接口应反映 pdf 格式与 total_pages
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}"),
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["format"], "pdf");
    assert!(json["data"]["total_pages"].as_i64().unwrap() >= 1);

    // 下载原文件：应返回 application/pdf + ETag
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/file"),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let etag = resp
        .headers()
        .get("etag")
        .expect("ETag should be set")
        .to_str()
        .unwrap()
        .to_string();
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(content_type.starts_with("application/pdf"));

    // 第二次带 If-None-Match：应返回 304
    let req = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/v1/books/{book_id}/file"))
        .header("Authorization", format!("Bearer {token}"))
        .header("If-None-Match", &etag)
        .header("X-Forwarded-For", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);

    // /content 接口对 pdf 应拒绝（400）
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/content?position=0"),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    println!("\n===== PDF upload/download tests passed! =====");
}

#[tokio::test]
async fn test_txt_download_endpoint() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "txtuser", "password1").await;

    let book = upload_test_book(&app, &token, "下载测试", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    // txt 也支持 /file 下载
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/file"),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(content_type.starts_with("text/plain"));

    println!("\n===== TXT download endpoint tests passed! =====");
}

#[tokio::test]
async fn test_percent_validation_in_heartbeat() {
    let (app, pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "valid", "password1").await;
    let book = upload_test_book(&app, &token, "校验", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'valid'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // percent 越界：1.5 应失败
    let result = crate::routes::reading::process_heartbeat_logic(
        &pool, user_id, book_id, "0", 1.5, "device1",
    )
    .await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Percent"), "unexpected error: {msg}");

    // locator 为空应失败
    let result = crate::routes::reading::process_heartbeat_logic(
        &pool, user_id, book_id, "", 0.0, "device1",
    )
    .await;
    assert!(result.is_err());

    // 合法值应通过
    let result = crate::routes::reading::process_heartbeat_logic(
        &pool, user_id, book_id, "0", 0.0, "device1",
    )
    .await
    .unwrap();
    assert!(result.synced);
    assert!(result.percent.abs() < f64::EPSILON);

    println!("\n===== Percent validation tests passed! =====");
}
