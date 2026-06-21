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
async fn test_full_integration() {
    let (app, _pool) = setup_test_app().await.unwrap();

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
    let resp = super::make_json_request(
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
    let resp = super::make_json_request(
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
    let resp = super::make_json_request(
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
    let resp = super::make_json_request(
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
    let _resp = super::make_json_request(
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
    let _resp = super::make_json_request(
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
    let books = json["data"]["books"].as_array().unwrap();
    assert_eq!(books.len(), 2);

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
    assert!(!json["data"]["content"].as_str().unwrap().is_empty());
    assert!(json["data"]["next_position"].as_i64().unwrap() > 0);

    // D6: update book (change title + is_public=false)
    let resp = super::make_json_request(
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
    let resp = super::make_json_request(
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
    let resp = super::make_json_request(
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

    // G1: first heartbeat (creates progress)
    let resp = super::make_json_request(
        &app,
        Method::POST,
        "/api/v1/reading/heartbeat",
        &serde_json::json!({"book_id": book1_id, "position": 100, "device_id": "device1"}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["synced"], true);
    assert_eq!(json["data"]["position"], 100);

    // G2: second heartbeat (same device, progress updates)
    let resp = super::make_json_request(
        &app,
        Method::POST,
        "/api/v1/reading/heartbeat",
        &serde_json::json!({"book_id": book1_id, "position": 200, "device_id": "device1"}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["synced"], true);
    assert_eq!(json["data"]["position"], 200);

    // G3: heartbeat from different device (should return server progress)
    let resp = super::make_json_request(
        &app,
        Method::POST,
        "/api/v1/reading/heartbeat",
        &serde_json::json!({"book_id": book1_id, "position": 999, "device_id": "device2"}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["synced"], false);
    assert_eq!(json["data"]["position"], 200); // server's saved progress

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

    let resp = super::make_json_request(
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
    let exe_bytes: &[u8] = &[0x4D, 0x5A, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00]; // MZ = Windows EXE
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
    let resp = super::make_json_request(
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
    let resp = super::make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/bookmarks"),
        &serde_json::json!({"position": 100, "label": "第一章末尾"}),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let bookmark_id = json["data"]["id"].as_i64().unwrap();
    assert_eq!(json["data"]["position"], 100);

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
    let resp = super::make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &serde_json::json!({
            "kind": "note",
            "start_position": 50,
            "content": "这段写得好"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let note_id = json["data"]["id"].as_i64().unwrap();

    // 创建高亮：必须带 end_position > start_position
    let resp = super::make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &serde_json::json!({
            "kind": "highlight",
            "start_position": 200,
            "end_position": 250,
            "excerpt": "..."
        }),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let highlight_id = json["data"]["id"].as_i64().unwrap();

    // 高亮校验：end_position 缺失应失败
    let resp = super::make_json_request(
        &app,
        Method::POST,
        &format!("/api/v1/books/{book_id}/annotations"),
        &serde_json::json!({"kind": "highlight", "start_position": 10}),
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
    let resp = super::make_json_request(
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
    upload_test_book(
        &app,
        &token,
        "球状闪电",
        "刘慈欣\n序章\n...",
        false,
    )
    .await;
    upload_test_book(
        &app,
        &token,
        "小王子",
        "圣埃克苏佩里\n...",
        false,
    )
    .await;

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
    let (_admin_token, _) = setup_admin(&app).await;

    // 关闭邀请码，方便注册
    let _ = super::make_json_request(
        &app,
        Method::PUT,
        "/api/v1/admin/settings",
        &serde_json::json!({"invite_code_required": false}),
        Some(&_admin_token),
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
        Some(&_admin_token),
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
        Some(&_admin_token),
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
        Some(&_admin_token),
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
        Some(&_admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 但 token_version 已变更，旧 token 仍失效；重新登录才能用
    let resp = super::make_json_request(
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
    let resp = super::make_json_request(
        &app,
        Method::DELETE,
        "/api/v1/auth/account",
        &serde_json::json!({"password": "wrong"}),
        Some(&new_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 正确密码注销
    let resp = super::make_json_request(
        &app,
        Method::DELETE,
        "/api/v1/auth/account",
        &serde_json::json!({"password": "pass1234"}),
        Some(&new_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 再登录应失败（账户已删）
    let resp = super::make_json_request(
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

async fn login_as_admin(app: &axum::Router) -> axum::response::Response {
    super::make_json_request(
        app,
        Method::POST,
        "/api/v1/auth/admin/login",
        &serde_json::json!({"username": "admin", "password": "admin123"}),
        None,
    )
    .await
}
