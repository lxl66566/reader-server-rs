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

    // 夹具修复回归：四行章节标题（首行自动成章 + 第一/二/三章）必须全部提取
    let titles: Vec<&str> = book1["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        [
            "测试小说",
            "第一章 测试章节",
            "第二章 另一个章节",
            "第三章 最终章"
        ],
        "chapter extraction must cover every heading in the fixture"
    );

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

    // E3: random public books excludes requester's own public books
    // (此刻唯一的公开书属于 user1，故 user1 自己请求应返回空)
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/random_public?count=10",
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["books"].as_array().unwrap().len(), 0);

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
    assert_eq!(json["data"]["chinese_conversion"], "none");

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
            "chinese_conversion": "traditional"
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
    assert_eq!(json["data"]["chinese_conversion"], "traditional");

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

    // 超大 page 不应 panic（(page-1)*limit 的 u32 乘法溢出回归），返回空页
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books/search?q=%E4%B8%89&page=4294967295&limit=50",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 1);
    assert_eq!(json["data"]["books"].as_array().unwrap().len(), 0);

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
    assert_eq!(v["updated"], true);
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
    assert_eq!(v["updated"], false); // 让位：未写入，locator 为服务端现值
    assert_eq!(v["locator"], "100"); // 服务端进度（来自 device1）

    // 注意：设备接管语义已变更——设备 2 上一次心跳虽 synced=false，但服务端
    // 已将 last_device_id 让位给 device2，因此设备 2 的下一次心跳即可正常同步；
    // 旧设备（device1）若继续心跳会反过来被判定为“其他设备”而收到 synced=false。
    // 设备 2 再次心跳：已接管 → synced=true，广播 progress_update 给设备 1
    send_json(
        &mut ws_b,
        &serde_json::json!({"type":"heartbeat","book_id":book_id,"locator":"999","percent":0.99}),
    )
    .await;
    let v = recv_json(&mut ws_b).await;
    assert_eq!(v["type"], "heartbeat_ack");
    assert_eq!(v["synced"], true);
    assert_eq!(v["locator"], "999");

    let v = recv_json(&mut ws_a).await;
    assert_eq!(v["type"], "progress_update");
    assert_eq!(v["locator"], "999");
    assert_eq!(v["device_id"], "device2");

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

/// 并发首心跳：无进度记录时两设备同时创建，败者应收到 synced=false
/// 且以 DB 现值为准（不谎称已保存、不广播未持久化的进度）
#[tokio::test]
async fn test_concurrent_first_heartbeat() {
    let (app, pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "hb_race", "password").await;
    let book = upload_test_book(&app, &token, "racebook", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'hb_race'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // upload 会预创建 locator='' 的进度行；删掉以构造"无进度记录"场景，
    // 两条并发首心跳将同时进入 INSERT OR IGNORE 创建分支
    sqlx::query("DELETE FROM reading_progress WHERE user_id = ? AND book_id = ?")
        .bind(user_id)
        .bind(book_id)
        .execute(&pool)
        .await
        .unwrap();

    let (r1, r2) = tokio::join!(
        crate::routes::reading::process_heartbeat_logic(
            &pool, user_id, book_id, "100", 0.1, "dev1",
        ),
        crate::routes::reading::process_heartbeat_logic(
            &pool, user_id, book_id, "200", 0.2, "dev2",
        ),
    );
    let (r1, r2) = (r1.unwrap(), r2.unwrap());

    // 恰好一个胜者真正写入；败者让位（synced=false）且拿到 DB 现值
    let (winner, loser) = if r1.synced {
        (r1, r2)
    } else {
        (r2, r1)
    };
    assert!(winner.synced);
    assert!(winner.updated);
    assert!(!loser.synced);
    assert!(!loser.updated);
    assert_eq!(loser.locator, winner.locator);

    // DB 中只存在胜者的进度
    let db_loc: String = sqlx::query_scalar(
        "SELECT locator FROM reading_progress WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(db_loc, winner.locator);
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

/// subscribe 的访问控制：不存在的 book_id 与他人私有书均回 error 帧
/// （文案与对应 HTTP 错误一致），且连接保持、合法订阅可继续
#[tokio::test]
async fn test_ws_subscribe_access_control() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "subowner", "password1").await;
    let intruder_token = register_and_login(&app, "subintruder", "password1").await;
    let book = upload_test_book(&app, &token, "subbook", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    let addr = spawn_server(app).await;

    let mut ws = ws_connect(addr, &token).await;
    let mut ws_other = ws_connect(addr, &intruder_token).await;

    // 不存在的 book_id → error 帧（404 语义）
    send_json(
        &mut ws,
        &serde_json::json!({"type":"subscribe","book_id":999_999,"device_id":"d1"}),
    )
    .await;
    let v = recv_json(&mut ws).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "Book not found");

    // 他人私有书 → error 帧（403 语义）
    send_json(
        &mut ws_other,
        &serde_json::json!({"type":"subscribe","book_id":book_id,"device_id":"d2"}),
    )
    .await;
    let v = recv_json(&mut ws_other).await;
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "Access denied to this book");

    // error 后连接保持：owner 自身的合法订阅不受影响
    send_json(
        &mut ws,
        &serde_json::json!({"type":"subscribe","book_id":book_id,"device_id":"d1"}),
    )
    .await;
    let v = recv_json(&mut ws).await;
    assert_eq!(v["type"], "subscribed");

    let _ = ws.close(None).await;
    let _ = ws_other.close(None).await;
}

/// 每连接订阅数上限：第 MAX+1 本新书的 subscribe 被拒（error 帧），
/// 已订阅书的重订阅（换 device_id）不受上限影响
#[tokio::test]
async fn test_ws_subscribe_limit() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "sublimit", "password1").await;

    // 上传"上限+1"本可访问的书
    let book_count = crate::ws::handler::MAX_SUBS_PER_CONN + 1;
    let mut book_ids = Vec::with_capacity(book_count);
    for i in 0..book_count {
        let book = upload_test_book(
            &app,
            &token,
            &format!("sublimit_book{i}"),
            TEST_BOOK_CONTENT,
            false,
        )
        .await;
        book_ids.push(book["book_id"].as_i64().unwrap());
    }

    let addr = spawn_server(app).await;
    let mut ws = ws_connect(addr, &token).await;

    for id in &book_ids[..book_count - 1] {
        send_json(
            &mut ws,
            &serde_json::json!({"type":"subscribe","book_id":id,"device_id":"d"}),
        )
        .await;
        let v = recv_json(&mut ws).await;
        assert_eq!(v["type"], "subscribed");
    }

    // 超限的新订阅 → error 帧
    send_json(
        &mut ws,
        &serde_json::json!({"type":"subscribe","book_id":book_ids[book_count - 1],"device_id":"d"}),
    )
    .await;
    let v = recv_json(&mut ws).await;
    assert_eq!(v["type"], "error");

    // 已订阅书的重订阅不受上限影响
    send_json(
        &mut ws,
        &serde_json::json!({"type":"subscribe","book_id":book_ids[0],"device_id":"d2"}),
    )
    .await;
    let v = recv_json(&mut ws).await;
    assert_eq!(v["type"], "subscribed");

    let _ = ws.close(None).await;
}

/// 每用户连接数上限：第 MAX+1 个连接升级后即被服务端关闭；
/// 断开一个释放名额后可重新连接
#[tokio::test]
async fn test_ws_connection_limit() {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "connlimit", "password1").await;
    let addr = spawn_server(app).await;

    let mut conns = Vec::with_capacity(crate::ws::manager::MAX_CONNS_PER_USER);
    for _ in 0..crate::ws::manager::MAX_CONNS_PER_USER {
        conns.push(ws_connect(addr, &token).await);
    }

    // 超限连接：升级后立即被关闭（None / Close / 错误均视为关闭）
    let mut extra = ws_connect(addr, &token).await;
    let closed = tokio::time::timeout(std::time::Duration::from_secs(5), extra.next())
        .await
        .expect("over-limit connection should close");
    assert!(matches!(
        closed,
        None | Some(Ok(Message::Close(_)) | Err(_))
    ));

    // 断开一个连接释放名额后可重连（服务端清理计数存在微小延迟，重试等待）
    let _ = conns.pop().unwrap().close(None).await;
    let mut reconnected = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let Ok(mut ws) =
            tokio::time::timeout(std::time::Duration::from_secs(2), ws_connect(addr, &token)).await
        else {
            continue;
        };
        reconnected = true;
        let _ = ws.close(None).await;
        break;
    }
    assert!(reconnected, "slot should be reusable after a disconnect");

    for mut ws in conns {
        let _ = ws.close(None).await;
    }
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

#[tokio::test]
async fn test_multi_device_heartbeat_takeover() {
    // 回归测试（🔴-2）：设备 A 停止心跳后，设备 B 第一次心跳返回 synced=false
    // 且不写进度，但服务端会让位 last_device_id；B 的第二次心跳必须能
    // synced=true 接管并把进度/时长落库（修复前的行为是 B 永远无法接管）
    let (app, pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "takeover", "password1").await;
    let book = upload_test_book(&app, &token, "接管", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'takeover'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // 设备 A 首次心跳：创建进度记录
    let r = crate::routes::reading::process_heartbeat_logic(
        &pool, user_id, book_id, "100", 0.1, "deviceA",
    )
    .await
    .unwrap();
    assert!(r.synced);
    assert!(r.updated);

    // 设备 B 第一次心跳：synced=false，返回服务端进度，不覆盖进度/时长
    let r = crate::routes::reading::process_heartbeat_logic(
        &pool, user_id, book_id, "500", 0.5, "deviceB",
    )
    .await
    .unwrap();
    assert!(!r.synced);
    assert!(!r.updated);
    assert_eq!(r.locator, "100"); // 服务端保存的旧进度

    // 间隔超过 1 秒，确保接管后的心跳产生可断言的阅读时长增量
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    // 设备 B 第二次心跳：已接管 → synced=true，进度与时长正常更新
    let r = crate::routes::reading::process_heartbeat_logic(
        &pool, user_id, book_id, "600", 0.6, "deviceB",
    )
    .await
    .unwrap();
    assert!(r.synced);
    assert!(r.updated);
    assert_eq!(r.locator, "600");
    assert!(r.reading_time >= 1);

    // 进度与阅读时长确实落库
    let (locator, reading_time): (String, i64) = sqlx::query_as(
        "SELECT locator, reading_time FROM reading_progress WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(locator, "600");
    assert!(reading_time >= 1);

    println!("\n===== Multi-device heartbeat takeover tests passed! =====");
}

#[tokio::test]
async fn test_pagination_params_clamped() {
    // 回归测试：page=0 曾导致 `(page - 1) * limit` u32 下溢（debug panic / release 垃圾
    // offset）；修复后 page<1 视为第一页，超大 limit 被钳制上限而不是全表扫描
    let (app, _pool) = setup_test_app().await.unwrap();
    // 注意：setup_admin 会开启 invite_code_required，须先注册普通用户
    let token = register_and_login(&app, "paged_user", "password1").await;
    upload_test_book(&app, &token, "分页书", TEST_BOOK_CONTENT, true).await;
    let (admin_token, _) = setup_admin(&app).await;

    // page=0 与超大 limit 都应正常返回第一页数据
    for uri in [
        "/api/v1/books?page=0&limit=10",
        "/api/v1/books?page=1&limit=4294967295",
        "/api/v1/books/public?page=0&limit=999999",
    ] {
        let resp = make_request(&app, Method::GET, uri, Body::empty(), Some(&token)).await;
        assert_eq!(resp.status(), StatusCode::OK, "{uri} must not panic");
        let json = response_json(resp).await;
        assert_eq!(json["code"], 0, "{uri}: {json:?}");
        assert_eq!(json["data"]["total"], 1, "{uri}: {json:?}");
        assert_eq!(
            json["data"]["books"].as_array().unwrap().len(),
            1,
            "{uri}: {json:?}"
        );
    }

    // 管理端审计日志：page=0 / limit=0 同样不得下溢（limit 被钳制到最小 1）
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/audit_logs?page=0&limit=0",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0, "{json:?}");
    assert!(json["data"]["logs"].is_array(), "{json:?}");

    println!("\n===== Pagination clamp tests passed! =====");
}

#[tokio::test]
async fn test_backup_global_contains_db_and_books() {
    // 回归测试：备份 zip 必须包含数据库快照（修复前因 db 路径前缀剥离顺序错误，
    // zip 里根本没有 db 文件且只 warn 不报错）以及已上传的书籍文件
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "backup_user", "password1").await;
    upload_test_book(&app, &token, "备份书", TEST_BOOK_CONTENT, false).await;
    let (admin_token, _) = setup_admin(&app).await;

    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/backup",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n == "reader.db"),
        "backup must contain db snapshot: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("books/")),
        "backup must contain book files: {names:?}"
    );

    // db 快照必须是一个完整可用的 sqlite 数据库文件（VACUUM INTO 一致性快照）
    let mut db_bytes = Vec::new();
    {
        let mut db_file = zip.by_name("reader.db").unwrap();
        std::io::Read::read_to_end(&mut db_file, &mut db_bytes).unwrap();
    }
    assert_eq!(&db_bytes[..16], b"SQLite format 3\0");
    assert!(db_bytes.len() > 16);

    println!("\n===== Backup tests passed! =====");
}

#[tokio::test]
async fn test_concurrent_first_progress_insert() {
    // 回归测试：public 书详情与首次心跳曾用裸 INSERT 创建进度，两个并发请求同时
    // 进入"无进度"分支时，后到的 INSERT 会撞 UNIQUE(user_id, book_id) 变 500；
    // 改 INSERT OR IGNORE 后并发/重复调用都必须成功且只留一行进度
    let (app, pool) = setup_test_app().await.unwrap();
    let owner = register_and_login(&app, "race_owner", "password1").await;
    let book = upload_test_book(&app, &owner, "并发公开书", TEST_BOOK_CONTENT, true).await;
    let book_id = book["book_id"].as_i64().unwrap();
    let reader_token = register_and_login(&app, "race_reader", "password1").await;
    let reader_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'race_reader'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // 并发首插心跳：全部成功，不得有 500/UNIQUE 冲突
    let mut handles = Vec::new();
    for i in 0..8 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            crate::routes::reading::process_heartbeat_logic(
                &pool,
                reader_id,
                book_id,
                &format!("loc{i}"),
                0.1,
                "dev",
            )
            .await
        }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }

    // 并发请求 public 书详情（GET 内部也会为读者建进度）：全部 200
    let mut handles = Vec::new();
    for _ in 0..8 {
        let app = app.clone();
        let token = reader_token.clone();
        handles.push(tokio::spawn(async move {
            make_request(
                &app,
                Method::GET,
                &format!("/api/v1/books/{book_id}"),
                Body::empty(),
                Some(&token),
            )
            .await
            .status()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }

    // 最终只存在一行进度
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM reading_progress WHERE user_id = ? AND book_id = ?",
    )
    .bind(reader_id)
    .bind(book_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    println!("\n===== Concurrent first progress insert tests passed! =====");
}

#[tokio::test]
async fn test_access_check_helpers() {
    use crate::{
        access::{check_book_access, check_user_token},
        error::AppError,
    };

    // 统一的权限校验助手：书籍读权限（所有者或公开书）与用户 token 状态
    // （存在 / 版本号 / 封禁），HTTP AuthUser 与 WS 握手/周期重验共用同一实现
    let (app, pool) = setup_test_app().await.unwrap();
    let owner = register_and_login(&app, "acc_owner", "password1").await;
    register_and_login(&app, "acc_other", "password1").await;

    let owner_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'acc_owner'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let other_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'acc_other'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let private_book = upload_test_book(&app, &owner, "私有书", TEST_BOOK_CONTENT, false).await;
    let private_id = private_book["book_id"].as_i64().unwrap();
    let public_book = upload_test_book(&app, &owner, "公开书", TEST_BOOK_CONTENT, true).await;
    let public_id = public_book["book_id"].as_i64().unwrap();

    // 书籍读权限：所有者恒可读；私有书对他人 403；公开书他人可读；不存在 404
    assert!(check_book_access(&pool, owner_id, private_id).await.is_ok());
    assert!(matches!(
        check_book_access(&pool, other_id, private_id).await,
        Err(AppError::BookAccessDenied)
    ));
    assert!(check_book_access(&pool, other_id, public_id).await.is_ok());
    assert!(matches!(
        check_book_access(&pool, other_id, i64::MAX).await,
        Err(AppError::BookNotFound)
    ));

    // 用户 token 状态：版本号不匹配 / banned / 用户不存在分别对应不同错误
    let ver: i64 = sqlx::query_scalar("SELECT token_version FROM users WHERE id = ?")
        .bind(other_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(check_user_token(&pool, other_id, ver).await.is_ok());
    assert!(matches!(
        check_user_token(&pool, other_id, ver + 1).await,
        Err(AppError::TokenExpiredOrInvalid)
    ));
    assert!(matches!(
        check_user_token(&pool, i64::MAX, ver).await,
        Err(AppError::AccountDeleted)
    ));
    sqlx::query("UPDATE users SET status = 'banned' WHERE id = ?")
        .bind(other_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        check_user_token(&pool, other_id, ver).await,
        Err(AppError::UserBanned)
    ));

    println!("\n===== Access check helper tests passed! =====");
}

#[tokio::test]
async fn test_admin_change_password_revokes_tokens() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;

    // 改密前 token 可用
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/settings",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 旧密码错误 → 400
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/admin/password",
        &serde_json::json!({"old_password": "wrong", "new_password": "newpass123"}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 改密成功
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/admin/password",
        &serde_json::json!({"old_password": "admin123", "new_password": "newpass123"}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 旧 admin token 立即失效（token_version 已递增）
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/settings",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 旧密码登录失败，新密码登录成功且新 token 可用
    let resp = login_as_admin(&app).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/admin/login",
        &serde_json::json!({"username": "admin", "password": "newpass123"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let new_token = json["data"]["admin_token"].as_str().unwrap().to_string();
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/settings",
        Body::empty(),
        Some(&new_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    println!("\n===== Admin change password revocation test passed! =====");
}

#[tokio::test]
async fn test_admin_login_lockout() {
    async fn wrong_login(app: &Router, password: &str) -> Response {
        make_json_request(
            app,
            Method::POST,
            "/api/v1/auth/admin/login",
            &serde_json::json!({"username": "admin", "password": password}),
            None,
        )
        .await
    }

    let (app, _pool) = setup_test_app().await.unwrap();
    let (_admin_token, _) = setup_admin(&app).await;

    // 前 4 次失败仅返回密码错误（401）
    for _ in 0..4 {
        let resp = wrong_login(&app, "wrong_password").await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let json = response_json(resp).await;
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("Incorrect admin password")
        );
    }

    // 第 5 次失败触发锁定，但仍返回密码错误
    let resp = wrong_login(&app, "wrong_password").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let json = response_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("Incorrect admin password")
    );

    // 锁定期间即使密码正确也返回锁定错误
    let resp = wrong_login(&app, "admin123").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let json = response_json(resp).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("temporarily locked")
    );

    println!("\n===== Admin login lockout test passed! =====");
}

#[tokio::test]
async fn test_audit_logs_record_login_events() {
    let (app, _pool) = setup_test_app().await.unwrap();

    // admin_setup 事件（setup 会顺带开启 invite_code_required，注册需邀请码，
    // 因此用户登录失败用不存在的用户名触发）
    let (admin_token, _) = setup_admin(&app).await;

    // 用户登录失败：用户名不存在
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({"username": "ghost_user", "password": "whatever"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 管理员登录失败 + 成功各一次
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/admin/login",
        &serde_json::json!({"username": "admin", "password": "wrong"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = login_as_admin(&app).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 审计日志应包含全部四类登录事件
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/audit_logs?limit=100",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let logs = json["data"]["logs"].as_array().unwrap();
    let actions: Vec<&str> = logs.iter().filter_map(|l| l["action"].as_str()).collect();
    for expected in [
        "admin_setup",
        "admin_login",
        "admin_login_failed",
        "user_login_failed",
    ] {
        assert!(
            actions.contains(&expected),
            "missing audit action {expected}: {actions:?}"
        );
    }
    // 认证前的失败事件没有操作管理员，admin_id 为 null
    let user_failed = logs
        .iter()
        .find(|l| l["action"] == "user_login_failed")
        .unwrap();
    assert!(user_failed["admin_id"].is_null(), "{user_failed:?}");

    println!("\n===== Audit login events test passed! =====");
}

// ================================================================
// 用户数据迁移：管理员导出 / 导入
// ================================================================

/// 解析导出包里的 metadata.json（用 zip crate 直接读条目）
fn read_export_metadata(zip_bytes: &[u8]) -> serde_json::Value {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    let mut entry = archive.by_name("metadata.json").unwrap();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
    serde_json::from_slice(&buf).unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_admin_export_and_import_user() {
    let (app, pool) = setup_test_app().await.unwrap();

    // 源用户先注册（setup_admin 会开启邀请码注册）
    let alice_token = register_and_login(&app, "alice", "pass1234").await;
    let book = upload_test_book(&app, &alice_token, "测试书", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    // 标签 / 书签 / 批注 / 设置
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}/tags"),
        &serde_json::json!({"tags": ["科幻", "经典"]}),
        Some(&alice_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/bookmarks"),
        &serde_json::json!({"locator": "10", "percent": 0.25, "label": "记号"}),
        Some(&alice_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &serde_json::json!({
            "kind": "highlight", "start_locator": "5", "end_locator": "20",
            "start_percent": 0.1, "end_percent": 0.4,
            "excerpt": "一段文字", "content": "感想"
        }),
        Some(&alice_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/reading/settings",
        &serde_json::json!({"font_size": 22, "timezone": "Asia/Shanghai", "theme": "sepia"}),
        Some(&alice_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 进度 / 时长 / 每日统计直接 SQL（进度写入路径在 ws 心跳里）
    let alice_id: i64 = sqlx::query_scalar!("SELECT id FROM users WHERE username = ?", "alice")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query!(
        "UPDATE reading_progress SET locator = '42', percent = 0.5, reading_time = 120 WHERE \
         user_id = ? AND book_id = ?",
        alice_id,
        book_id
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query!(
        "UPDATE users SET total_reading_time = 360 WHERE id = ?",
        alice_id
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO daily_reading_stats (user_id, book_id, date, reading_seconds) VALUES (?, ?, \
         '2024-01-01', 60), (?, ?, '2024-01-02', 90)",
        alice_id,
        book_id,
        alice_id,
        book_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // 管理员导出
    let (admin_token, _) = setup_admin(&app).await;
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/users/{alice_id}/export"),
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let export_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(!export_bytes.is_empty());

    // 迁移导出包含密码哈希与版本号
    let metadata = read_export_metadata(&export_bytes);
    assert_eq!(metadata["version"], 2);
    assert!(metadata["password_hash"].is_string());

    // 导入为新用户 bob（显式新密码）
    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![("username", "bob"), ("password", "newpass99")],
        Some(("file", "export.zip", &export_bytes)),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "import failed");
    let json = response_json(resp).await;
    assert_eq!(json["data"]["username"], "bob");
    assert_eq!(json["data"]["book_count"], 1);

    // 同一包再导入且不指定用户名 → 与 alice 冲突
    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![],
        Some(("file", "export.zip", &export_bytes)),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 新用户可登录且数据完整
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({"username": "bob", "password": "newpass99"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let bob_token = json["data"]["token"].as_str().unwrap().to_string();

    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/user_info",
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total_reading_time"], 360);
    assert_eq!(json["data"]["book_count"], 1);

    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books",
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 1);
    let books = json["data"]["books"].as_array().unwrap();
    assert_eq!(books[0]["title"], "测试书");
    assert_eq!(books[0]["locator"], "42");
    assert_eq!(books[0]["percent"], 0.5);
    assert_eq!(books[0]["reading_time"], 120);
    let new_book_id = books[0]["book_id"].as_i64().unwrap();

    // 书籍文件内容完整
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{new_book_id}/content?position=0&length=4"),
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    assert!(
        json["data"]["content"]
            .as_str()
            .unwrap()
            .starts_with("测试小说")
    );

    // 章节 / 标签 / 书签 / 批注
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{new_book_id}"),
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["chapters"].as_array().unwrap().len(), 4);

    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{new_book_id}/tags"),
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    let tags = json["data"]["tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);

    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{new_book_id}/bookmarks"),
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    let bookmarks = json["data"]["bookmarks"].as_array().unwrap();
    assert_eq!(bookmarks.len(), 1);
    assert_eq!(bookmarks[0]["label"], "记号");

    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{new_book_id}/annotations"),
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    let annotations = json["data"]["annotations"].as_array().unwrap();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["kind"], "highlight");
    assert_eq!(annotations[0]["excerpt"], "一段文字");

    // 设置与每日统计
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 22);
    assert_eq!(json["data"]["timezone"], "Asia/Shanghai");
    assert_eq!(json["data"]["theme"], "sepia");

    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/stats",
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total_reading_seconds"], 360);

    // 每日统计按书归属正确（导出/导入经 book_id 重映射）
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/stats/daily?start=2024-01-01&end=2024-01-02",
        Body::empty(),
        Some(&bob_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let days = json["data"]["days"].as_array().unwrap();
    assert_eq!(days.len(), 2);
    assert_eq!(days[0]["date"], "2024-01-01");
    assert_eq!(days[0]["book_id"], new_book_id);
    assert_eq!(days[0]["title"], "测试书");
    assert_eq!(days[0]["reading_seconds"], 60);

    // 非法区间被拒绝：start 晚于 end / 跨度过大 / 日期格式错误
    for query in [
        "start=2024-01-02&end=2024-01-01",
        "start=2020-01-01&end=2024-01-01",
        "start=bad&end=2024-01-01",
    ] {
        let resp = make_request(
            &app,
            Method::GET,
            &format!("/api/v1/reading/stats/daily?{query}"),
            Body::empty(),
            Some(&bob_token),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{query}");
    }

    // 源用户未受影响
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books",
        Body::empty(),
        Some(&alice_token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 1);

    // 审计记录
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/audit_logs?limit=100",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    let json = response_json(resp).await;
    let actions: Vec<&str> = json["data"]["logs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["action"].as_str())
        .collect();
    assert!(actions.contains(&"export_user"), "{actions:?}");
    assert!(actions.contains(&"import_user"), "{actions:?}");

    println!("\n===== Admin export/import user test passed! =====");
}

#[tokio::test]
async fn test_import_legacy_export_package() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;

    // 历史自助导出格式：无 version、books 用 id、无 file 字段、条目名 000_ 前缀
    let metadata = serde_json::json!({
        "user": {"id": 1, "username": "legacy_user", "created_at": "2020-01-01T00:00:00.000Z", "total_reading_time": 30},
        "books": [{
            "id": 7, "user_id": 1, "title": "旧书", "author": null, "file_path": "/old/path",
            "is_public": true, "cover_path": null, "total_chars": 100, "total_pages": 0,
            "description": null, "format": "txt", "content_hash": "xx", "created_at": "2020-01-01T00:00:00.000Z"
        }],
        "chapters": [{"chapter_id": 1, "book_id": 7, "title": "第一章", "position": 0}],
        "reading_progress": [{"book_id": 7, "locator": "8", "percent": 0.4, "reading_time": 30, "last_read_at": null}],
        "reading_settings": [{
            "id": 1, "user_id": 1, "font_size": 20, "background_color": "#FFFFFF",
            "text_color": "#000000", "line_height": 1.6, "letter_spacing": 0.1,
            "paragraph_spacing": 1.0, "reading_width": 40, "text_indent": 2.0,
            "chinese_conversion": "none", "timezone": "UTC", "font_family": "serif",
            "theme": "light", "pagination_mode": "scroll"
        }],
        "bookmarks": [{"id": 1, "user_id": 1, "book_id": 7, "locator": "3", "percent": 0.1, "label": "旧书签", "created_at": "2020-01-02T00:00:00.000Z"}],
        "annotations": [],
        "exported_at": "2020-01-02T00:00:00.000Z"
    });

    let mut buf = Vec::new();
    {
        use std::io::Write as _;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("metadata.json", opts).unwrap();
        w.write_all(&serde_json::to_vec_pretty(&metadata).unwrap())
            .unwrap();
        w.start_file("000_旧书.txt", opts).unwrap();
        w.write_all(TEST_BOOK_CONTENT.as_bytes()).unwrap();
        w.finish().unwrap();
    }

    // 不提供用户名 → 沿用包内 legacy_user；提供密码
    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![("password", "legacy123")],
        Some(("file", "legacy.zip", &buf)),
        Some(&admin_token),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "legacy import failed: {}",
        String::from_utf8_lossy(&resp.into_body().collect().await.unwrap().to_bytes())
    );
    let json = response_json(resp).await;
    assert_eq!(json["data"]["username"], "legacy_user");

    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({"username": "legacy_user", "password": "legacy123"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let token = json["data"]["token"].as_str().unwrap();

    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books",
        Body::empty(),
        Some(token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 1);
    let books = json["data"]["books"].as_array().unwrap();
    assert_eq!(books[0]["title"], "旧书");
    assert_eq!(books[0]["is_public"], true);
    assert_eq!(books[0]["percent"], 0.4);
    assert_eq!(books[0]["reading_time"], 30);
    let book_id = books[0]["book_id"].as_i64().unwrap();

    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/content?position=0&length=4"),
        Body::empty(),
        Some(token),
    )
    .await;
    let json = response_json(resp).await;
    assert!(
        json["data"]["content"]
            .as_str()
            .unwrap()
            .starts_with("测试小说")
    );

    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/bookmarks"),
        Body::empty(),
        Some(token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["bookmarks"].as_array().unwrap().len(), 1);

    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/reading/settings",
        Body::empty(),
        Some(token),
    )
    .await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 20);
    assert_eq!(json["data"]["line_height"], 1.6);

    println!("\n===== Legacy export import test passed! =====");
}

#[tokio::test]
async fn test_import_self_service_export_requires_password() {
    let (app, _pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "carol", "pass1234").await;
    upload_test_book(&app, &token, "卡罗尔的书", TEST_BOOK_CONTENT, false).await;

    // 自助导出不含密码哈希
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/export",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let export_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let metadata = read_export_metadata(&export_bytes);
    assert!(metadata.get("password_hash").is_none());

    let (admin_token, _) = setup_admin(&app).await;

    // 不带密码导入 → 400
    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![("username", "carol2")],
        Some(("file", "export.zip", &export_bytes)),
        Some(&admin_token),
    )
    .await;
    let status = resp.status();
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "import without password: {}",
        String::from_utf8_lossy(&resp.into_body().collect().await.unwrap().to_bytes())
    );

    // 提供密码即可导入并登录
    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![("username", "carol2"), ("password", "newpass77")],
        Some(("file", "export.zip", &export_bytes)),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({"username": "carol2", "password": "newpass77"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    println!("\n===== Self-service export import test passed! =====");
}

#[tokio::test]
async fn test_admin_user_export_access_control() {
    let (app, _pool) = setup_test_app().await.unwrap();
    // 先注册再 setup（setup 会开启邀请码注册）
    let token = register_and_login(&app, "dave", "pass1234").await;
    let (admin_token, _) = setup_admin(&app).await;

    // 用户 token 不能调管理员接口
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/users/999/export",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 不存在的用户
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/admin/users/999/export",
        Body::empty(),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 导入同样要求管理员（403）
    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![],
        None,
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    println!("\n===== Admin export access control test passed! =====");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_edit_reading_time() {
    // 手动编辑阅读时长：差值同步 total_reading_time、校验、权限、每日统计不动

    // 设置指定书阅读时长的请求封装，避免用例里重复拼 uri/body
    async fn set_time(app: &Router, book_id: i64, secs: i64, token: &str) -> Response {
        make_json_request(
            app,
            Method::PUT,
            &format!("/api/v1/reading/progress/{book_id}"),
            &serde_json::json!({ "reading_time": secs }),
            Some(token),
        )
        .await
    }

    let (app, pool) = setup_test_app().await.unwrap();
    let token1 = register_and_login(&app, "rt_user1", "password1").await;
    let token2 = register_and_login(&app, "rt_user2", "password2").await;

    let owner_book = upload_test_book(&app, &token1, "私有书", TEST_BOOK_CONTENT, false).await;
    let book_id = owner_book["book_id"].as_i64().unwrap();

    let user1_id: i64 = sqlx::query_scalar!("SELECT id FROM users WHERE username = ?", "rt_user1")
        .fetch_one(&pool)
        .await
        .unwrap();

    let total = || async {
        sqlx::query_scalar!(
            "SELECT total_reading_time FROM users WHERE id = ?",
            user1_id
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };

    // 设置新值 → 返回新值，total 累加差值（上传时建过进度行，旧值为 0）
    let resp = set_time(&app, book_id, 3600, &token1).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(response_json(resp).await["data"]["reading_time"], 3600);
    assert_eq!(total().await, 3600);

    // 再改 → 累加差值；下调 → 同步回退
    let resp = set_time(&app, book_id, 5400, &token1).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(total().await, 5400);
    let resp = set_time(&app, book_id, 60, &token1).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(total().await, 60);

    // 每日统计不受编辑影响
    sqlx::query!(
        "INSERT INTO daily_reading_stats (user_id, book_id, date, reading_seconds) VALUES (?, ?, \
         '2024-01-01', 100)",
        user1_id,
        book_id
    )
    .execute(&pool)
    .await
    .unwrap();
    let resp = set_time(&app, book_id, 600, &token1).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let daily: i64 = sqlx::query_scalar!(
        "SELECT reading_seconds FROM daily_reading_stats WHERE user_id = ? AND book_id = ?",
        user1_id,
        book_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(daily, 100);
    assert_eq!(total().await, 600);

    // 校验：负数 / 超上限 → 400
    for bad in [-1, 3650 * 24 * 60 * 60 + 1] {
        let resp = set_time(&app, book_id, bad, &token1).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // 权限：他人私有书 → 403；不存在的书 → 404
    let resp = set_time(&app, book_id, 10, &token2).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/reading/progress/999999",
        &serde_json::json!({ "reading_time": 10 }),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 无进度行（他人公开书）：非 0 建行并累加 total；0 不建行
    let pub_book1 = upload_test_book(&app, &token1, "公开书1", TEST_BOOK_CONTENT, true).await;
    let pub1_id = pub_book1["book_id"].as_i64().unwrap();
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/reading/progress/{pub1_id}"),
        &serde_json::json!({ "reading_time": 120 }),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let pub_book2 = upload_test_book(&app, &token1, "公开书2", TEST_BOOK_CONTENT, true).await;
    let pub2_id = pub_book2["book_id"].as_i64().unwrap();
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/reading/progress/{pub2_id}"),
        &serde_json::json!({ "reading_time": 0 }),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let user2_id: i64 = sqlx::query_scalar!("SELECT id FROM users WHERE username = ?", "rt_user2")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (t2_total, t2_rows): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT total_reading_time FROM users WHERE id = ?1), (SELECT COUNT(*) FROM \
         reading_progress WHERE user_id = ?1)",
    )
    .bind(user2_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(t2_total, 120);
    assert_eq!(t2_rows, 1); // 只为 120 秒那本建了行，0 不建

    // 管理员改任意用户；用户 token 调管理端 → 403；目标不存在 → 404
    let (admin_token, _) = setup_admin(&app).await;
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/admin/users/{user1_id}/reading_time"),
        &serde_json::json!({ "book_id": book_id, "reading_time": 7200 }),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(total().await, 7200);

    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/admin/users/{user1_id}/reading_time"),
        &serde_json::json!({ "book_id": book_id, "reading_time": 1 }),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let resp = make_json_request(
        &app,
        Method::PUT,
        "/api/v1/admin/users/999999/reading_time",
        &serde_json::json!({ "book_id": book_id, "reading_time": 1 }),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/admin/users/{user1_id}/reading_time"),
        &serde_json::json!({ "book_id": 999_999, "reading_time": 1 }),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 审计日志已记录
    let logged = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM audit_logs WHERE action = 'set_reading_time')"
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(logged, 1);

    println!("\n===== Edit reading time tests passed! =====");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_edit_daily_reading_stats() {
    // 每日阅读时长编辑：全量替换、0 秒不落库、差值同步累计与总时长（含钳制）、校验、权限

    async fn put_entries(
        app: &Router,
        book_id: i64,
        entries: serde_json::Value,
        token: &str,
    ) -> Response {
        make_json_request(
            app,
            Method::PUT,
            &format!("/api/v1/reading/stats/daily/{book_id}"),
            &serde_json::json!({ "entries": entries }),
            Some(token),
        )
        .await
    }

    // 读取累计时长与用户总时长，供差值断言
    async fn progress_total(pool: &Pool<Sqlite>, user_id: i64, book_id: i64) -> (i64, i64) {
        sqlx::query_as(
            "SELECT (SELECT reading_time FROM reading_progress WHERE user_id = ?1 AND book_id = \
             ?2), (SELECT total_reading_time FROM users WHERE id = ?1)",
        )
        .bind(user_id)
        .bind(book_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    let (app, pool) = setup_test_app().await.unwrap();
    let token1 = register_and_login(&app, "ds_user1", "password1").await;
    let token2 = register_and_login(&app, "ds_user2", "password2").await;

    let book = upload_test_book(&app, &token1, "每日统计书", TEST_BOOK_CONTENT, false).await;
    let book_id = book["book_id"].as_i64().unwrap();

    let user1_id: i64 = sqlx::query_scalar!("SELECT id FROM users WHERE username = ?", "ds_user1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // 预置：总时长 500、每日合计 300（不一致属合法历史状态，只按差值同步）

    for (date, secs) in [("2024-01-01", 100), ("2024-01-02", 200)] {
        sqlx::query!(
            "INSERT INTO daily_reading_stats (user_id, book_id, date, reading_seconds) VALUES (?, \
             ?, ?, ?)",
            user1_id,
            book_id,
            date,
            secs
        )
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query!(
        "UPDATE reading_progress SET reading_time = 500 WHERE user_id = ? AND book_id = ?",
        user1_id,
        book_id
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query!(
        "UPDATE users SET total_reading_time = 500 WHERE id = ?",
        user1_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // GET：date 升序返回全部记录
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/reading/stats/daily/{book_id}"),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let entries = json["data"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["date"], "2024-01-01");
    assert_eq!(entries[0]["reading_seconds"], 100);
    assert_eq!(entries[1]["date"], "2024-01-02");

    // PUT 全量替换：乱序入参被排序、0 秒条目剔除；差值 +50 同步累计与总时长
    let resp = put_entries(
        &app,
        book_id,
        serde_json::json!([
            { "date": "2024-01-03", "reading_seconds": 0 },
            { "date": "2024-01-02", "reading_seconds": 300 },
            { "date": "2024-01-01", "reading_seconds": 50 }
        ]),
        &token1,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let entries = json["data"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["date"], "2024-01-01");
    assert_eq!(entries[0]["reading_seconds"], 50);
    assert_eq!(entries[1]["reading_seconds"], 300);
    assert_eq!(progress_total(&pool, user1_id, book_id).await, (550, 550));

    let daily_zero: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM daily_reading_stats WHERE user_id = ? AND book_id = ? AND \
         reading_seconds = 0",
        user1_id,
        book_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(daily_zero, 0);

    // 差值为负且低于现值：累计与总时长均钳制到 0，每日记录清空
    sqlx::query!(
        "UPDATE reading_progress SET reading_time = 100 WHERE user_id = ? AND book_id = ?",
        user1_id,
        book_id
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query!(
        "UPDATE users SET total_reading_time = 100 WHERE id = ?",
        user1_id
    )
    .execute(&pool)
    .await
    .unwrap();
    let resp = put_entries(&app, book_id, serde_json::json!([]), &token1).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(progress_total(&pool, user1_id, book_id).await, (0, 0));
    let daily_left: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM daily_reading_stats WHERE user_id = ? AND book_id = ?",
        user1_id,
        book_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(daily_left, 0);

    // 校验：非法日期 / 越界秒数 / 重复日期 → 400，且不改动数据
    // （与区间接口的 parse_date 一致，chrono 对单位数月/日宽容并归一化）
    for bad in [
        serde_json::json!([{ "date": "2024-13-01", "reading_seconds": 10 }]),
        serde_json::json!([{ "date": "yesterday", "reading_seconds": 10 }]),
        serde_json::json!([{ "date": "2024-01-01", "reading_seconds": -1 }]),
        serde_json::json!([{ "date": "2024-01-01", "reading_seconds": 3650 * 24 * 60 * 60 + 1 }]),
        serde_json::json!([
            { "date": "2024-01-01", "reading_seconds": 10 },
            { "date": "2024-01-01", "reading_seconds": 20 }
        ]),
    ] {
        let resp = put_entries(&app, book_id, bad, &token1).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(progress_total(&pool, user1_id, book_id).await, (0, 0));

    // 无进度行：空列表不建行；非零和建行并同步总时长
    // （上传会建进度行，先删掉以构造无行场景）
    let own2 = upload_test_book(&app, &token2, "无进度书", TEST_BOOK_CONTENT, false).await;
    let own2_id = own2["book_id"].as_i64().unwrap();
    let user2_id: i64 = sqlx::query_scalar!("SELECT id FROM users WHERE username = ?", "ds_user2")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query!(
        "DELETE FROM reading_progress WHERE user_id = ? AND book_id = ?",
        user2_id,
        own2_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let resp = put_entries(&app, own2_id, serde_json::json!([]), &token2).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rows: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM reading_progress WHERE user_id = ?",
        user2_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rows, 0);

    let resp = put_entries(
        &app,
        own2_id,
        serde_json::json!([{ "date": "2024-05-05", "reading_seconds": 120 }]),
        &token2,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let created: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT reading_time FROM reading_progress WHERE book_id = ?1), (SELECT \
         total_reading_time FROM users WHERE id = ?2)",
    )
    .bind(own2_id)
    .bind(user2_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(created, (120, 120));

    // 权限：他人私有书 → 403；不存在的书 → 404
    for method in [Method::GET, Method::PUT] {
        let resp = if method == Method::GET {
            make_request(
                &app,
                method.clone(),
                &format!("/api/v1/reading/stats/daily/{book_id}"),
                Body::empty(),
                Some(&token2),
            )
            .await
        } else {
            put_entries(&app, book_id, serde_json::json!([]), &token2).await
        };
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let path = "/api/v1/reading/stats/daily/999999".to_string();
        let resp = if method == Method::GET {
            make_request(&app, method, &path, Body::empty(), Some(&token1)).await
        } else {
            put_entries(&app, 999_999, serde_json::json!([]), &token1).await
        };
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    println!("\n===== Edit daily reading stats tests passed! =====");
}

#[tokio::test]
async fn test_import_rejects_chapter_pattern_on_non_txt() {
    // 非 txt 书籍携带章节切分正则的导出包无效：与上传口约束一致
    let (app, _pool) = setup_test_app().await.unwrap();
    let (admin_token, _) = setup_admin(&app).await;

    let metadata = serde_json::json!({
        "user": {"id": 1, "username": "pattern_user", "created_at": "2024-01-01T00:00:00.000Z", "total_reading_time": 0},
        "books": [{
            "id": 7, "user_id": 1, "title": "epub 书", "author": null, "file_path": "/old/path",
            "is_public": false, "cover_path": null, "total_chars": 100, "total_pages": 0,
            "description": null, "format": "epub", "content_hash": "xx", "created_at": "2024-01-01T00:00:00.000Z",
            "chapter_pattern": "^Chapter"
        }],
        "chapters": [],
        "reading_progress": [],
        "bookmarks": [],
        "annotations": [],
        "exported_at": "2024-01-02T00:00:00.000Z"
    });

    let mut buf = Vec::new();
    {
        use std::io::Write as _;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("metadata.json", opts).unwrap();
        w.write_all(&serde_json::to_vec_pretty(&metadata).unwrap())
            .unwrap();
        w.finish().unwrap();
    }

    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![("password", "pass1234")],
        Some(("file", "bad-pattern.zip", &buf)),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    println!("\n===== Import chapter-pattern format check test passed! =====");
}

// ----------------------------------------------------------------

/// 获取书籍详情 JSON（断言 200）
async fn fetch_book_detail(app: &Router, token: &str, book_id: i64) -> serde_json::Value {
    let resp = make_request(
        app,
        Method::GET,
        &format!("/api/v1/books/{book_id}"),
        Body::empty(),
        Some(token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    response_json(resp).await
}

/// 详情中的章节标题列表
fn chapter_titles(detail: &serde_json::Value) -> Vec<&str> {
    detail["data"]["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect()
}

/// 每本书自定义章节切分规则：上传携带、PUT 重切、恢复默认、非法值与非 txt 拒绝
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_custom_chapter_pattern() {
    let (app, pool) = setup_test_app().await.unwrap();
    let token = register_and_login(&app, "patternuser", "pass1234").await;

    // 默认规则只切出首行；自定义 ^P\d 可切出三段
    let content = "引子\nP1 分段一\nP2 分段二\n中间过渡文字\nP3 分段三\n";

    // 1) 上传携带自定义 pattern：按自定义规则切分
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![
            ("title", "自定义切分"),
            ("is_public", "false"),
            ("chapter_pattern", r"^P\d"),
        ],
        Some(("file", "book.txt", content.as_bytes())),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "upload failed");
    let json = response_json(resp).await;
    assert_eq!(
        chapter_titles(&json),
        ["引子", "P1 分段一", "P2 分段二", "P3 分段三"],
        "custom pattern must split the fixture"
    );
    let book_id = json["data"]["book_id"].as_i64().unwrap();

    // 详情接口回显 pattern 与章节
    let detail = fetch_book_detail(&app, &token, book_id).await;
    assert_eq!(detail["data"]["chapter_pattern"], r"^P\d");
    assert_eq!(chapter_titles(&detail).len(), 4);

    // 2) PUT 修改 pattern：章节被重切，偏移仍指向原文（用 /content 校验）
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"chapter_pattern": r"^P\d 分段二"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let detail = fetch_book_detail(&app, &token, book_id).await;
    let titles = chapter_titles(&detail);
    assert_eq!(titles, ["引子", "P2 分段二"], "chapters must be re-split");
    let chapters = detail["data"]["chapters"].as_array().unwrap();
    let p2_position = chapters[1]["position"].as_i64().unwrap();
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/v1/books/{book_id}/content?position={p2_position}&length=6"),
        Body::empty(),
        Some(&token),
    )
    .await;
    let json = response_json(resp).await;
    // /content 的 length 有最小 100 钉制，只能断言前缀
    assert!(
        json["data"]["content"]
            .as_str()
            .unwrap()
            .starts_with("P2 分段二"),
        "offset must point at title"
    );

    // 3) 与现值相同的 pattern：无任何字段变化 → No updates，章节不动
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"chapter_pattern": r"^P\d 分段二"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["message"], "No updates");

    // 4) PUT null：恢复默认规则，pattern 回到 null
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"chapter_pattern": null}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let detail = fetch_book_detail(&app, &token, book_id).await;
    assert!(detail["data"]["chapter_pattern"].is_null());
    assert_eq!(chapter_titles(&detail), ["引子"]);

    // 5) 非法正则：上传与 PUT 均 400，且失败后原章节不被破坏
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![
            ("title", "坏正则"),
            ("is_public", "false"),
            ("chapter_pattern", "[unclosed"),
        ],
        Some(("file", "bad.txt", content.as_bytes())),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"chapter_pattern": "(?P<x>a)[unclosed"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let detail = fetch_book_detail(&app, &token, book_id).await;
    assert_eq!(
        chapter_titles(&detail),
        ["引子"],
        "failed update must not touch chapters"
    );

    // 6) 非 txt：上传携带非空 pattern 与 PUT 设置均 400
    let pdf = minimal_pdf_bytes();
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![
            ("title", "PDF切分"),
            ("is_public", "false"),
            ("chapter_pattern", r"^P\d"),
        ],
        Some(("file", "book.pdf", &pdf)),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "PDF书"), ("is_public", "false")],
        Some(("file", "book.pdf", &pdf)),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "pdf upload failed");
    let pdf_book_id = response_json(resp).await["data"]["book_id"]
        .as_i64()
        .unwrap();
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{pdf_book_id}"),
        &serde_json::json!({"chapter_pattern": r"^P\d"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 7) 导出 / 导入：包内携带 pattern，导入后章节按同一规则重提
    let resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![
            ("title", "导出书"),
            ("is_public", "false"),
            ("chapter_pattern", r"^P\d"),
        ],
        Some(("file", "exported.txt", content.as_bytes())),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let export_book_id = response_json(resp).await["data"]["book_id"]
        .as_i64()
        .unwrap();

    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/auth/export",
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let export_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let metadata = read_export_metadata(&export_bytes);
    let exported_book = metadata["books"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["book_id"] == export_book_id)
        .unwrap();
    assert_eq!(exported_book["chapter_pattern"], r"^P\d");

    let (admin_token, _) = setup_admin(&app).await;
    let resp = make_multipart_request(
        &app,
        "/api/v1/admin/users/import",
        vec![("username", "imported_pat"), ("password", "pass5678")],
        Some(("file", "export.zip", &export_bytes)),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "import failed");
    let json = response_json(resp).await;
    // 导出是用户级的：包含此前上传的全部 3 本书
    assert_eq!(json["data"]["book_count"], 3);
    let resp = make_json_request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({"username": "imported_pat", "password": "pass5678"}),
        None,
    )
    .await;
    let imported_token = response_json(resp).await["data"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = make_request(
        &app,
        Method::GET,
        "/api/v1/books",
        Body::empty(),
        Some(&imported_token),
    )
    .await;
    let json = response_json(resp).await;
    let imported_book_id = json["data"]["books"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["title"] == "导出书")
        .unwrap()["book_id"]
        .as_i64()
        .unwrap();
    let detail = fetch_book_detail(&app, &imported_token, imported_book_id).await;
    assert_eq!(detail["data"]["chapter_pattern"], r"^P\d");
    assert_eq!(
        chapter_titles(&detail).len(),
        4,
        "import must re-split with the pattern"
    );

    // 8) 原文件丢失：重切沿用下载路径的 404 语义
    let file_path: String =
        sqlx::query_scalar!("SELECT file_path FROM books WHERE id = ?", book_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let _ = tokio::fs::remove_file(&file_path).await;
    let resp = make_json_request(
        &app,
        Method::PUT,
        &format!("/api/v1/books/{book_id}"),
        &serde_json::json!({"chapter_pattern": r"^P\d"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    println!("\n===== Custom chapter pattern tests passed! =====");
}
