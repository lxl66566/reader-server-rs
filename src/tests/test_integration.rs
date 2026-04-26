use axum::body::Body;
use axum::http::{Method, StatusCode};

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
    let resp = make_request(&app, Method::GET, "/api/admin/check_setup", Body::empty(), None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);
    assert_eq!(json["data"]["is_setup"], false);

    // A2: setup admin
    let (admin_token, _) = setup_admin(&app).await;

    // A3: check_setup 已初始化
    let resp = make_request(&app, Method::GET, "/api/admin/check_setup", Body::empty(), None).await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["is_setup"], true);

    // A4: 重复设置管理员 (error 1006)
    let resp = super::make_json_request(
        &app,
        Method::POST,
        "/api/auth/admin/setup",
        &serde_json::json!({"password": "another"}),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 1006);

    // A5: admin login
    let resp = login_as_admin(&app).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // A6: update settings (关闭邀请码)
    let resp = super::make_json_request(
        &app,
        Method::PUT,
        "/api/admin/settings",
        &serde_json::json!({"invite_code_required": false}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // A7: get settings
    let resp = make_request(&app, Method::GET, "/api/admin/settings", Body::empty(), Some(&admin_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["invite_code_required"], false);

    // A8: create invite code
    let resp = super::make_json_request(
        &app,
        Method::POST,
        "/api/admin/invite_code",
        &serde_json::json!({"limit_times": 1, "description": "test invite"}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);
    let invite_code = json["data"]["invite_code"].as_str().unwrap().to_string();

    // A9: list invite codes
    let resp = make_request(&app, Method::GET, "/api/admin/invite_codes", Body::empty(), Some(&admin_token)).await;
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
    assert_eq!(json["code"], 1001);

    // B5: get user_info
    let resp = make_request(&app, Method::GET, "/api/auth/user_info", Body::empty(), Some(&token1)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["username"], "user1");
    assert_eq!(json["data"]["book_count"], 0);

    // B6: duplicate username
    let resp = register_user(&app, "user1", "password1", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 1003);

    // B7: change password
    let resp = super::make_json_request(
        &app,
        Method::POST,
        "/api/auth/change_password",
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
        "/api/admin/settings",
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
    assert_eq!(json["code"], 1002);

    // C4: register without invite code (required)
    let resp = register_user(&app, "user5", "password5", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 1002);

    // C5: disable invite_code_required
    let _resp = super::make_json_request(
        &app,
        Method::PUT,
        "/api/admin/settings",
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
    let resp = make_request(&app, Method::GET, "/api/books", Body::empty(), Some(&token1)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["total"], 2);
    let books = json["data"]["books"].as_array().unwrap();
    assert_eq!(books.len(), 2);

    // D4: get book detail
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/books/{}", book1_id),
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
        &format!("/api/books/{}/content?position=0&length=100", book1_id),
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
        &format!("/api/books/{}", book1_id),
        &serde_json::json!({"title": "修改后的标题", "is_public": false}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // D7: verify update
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/books/{}", book1_id),
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
        &format!("/api/books/{}", book1_id),
        Body::empty(),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // D9: user2 tries to delete user1's book (should fail)
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/books/{}", book1_id),
        Body::empty(),
        Some(&token2),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // D10: user1 deletes book2
    let resp = make_request(
        &app,
        Method::DELETE,
        &format!("/api/books/{}", book2_id),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // D11: user1 lists books (should have 1)
    let resp = make_request(&app, Method::GET, "/api/books", Body::empty(), Some(&token1)).await;
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
    let resp = make_request(&app, Method::GET, "/api/books/public", Body::empty(), Some(&token2)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert!(json["data"]["total"].as_i64().unwrap() >= 1);

    // E2: random public books
    let resp = make_request(
        &app,
        Method::GET,
        "/api/books/random_public?count=1",
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
    let resp = make_request(&app, Method::GET, "/api/reading/settings", Body::empty(), Some(&token1)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 18);
    assert_eq!(json["data"]["background_color"], "#F5F5DC");
    assert_eq!(json["data"]["simplified_chinese"], true);

    // F2: update all fields
    let resp = super::make_json_request(
        &app,
        Method::PUT,
        "/api/reading/settings",
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
    let resp = make_request(&app, Method::GET, "/api/reading/settings", Body::empty(), Some(&token1)).await;
    let json = response_json(resp).await;
    assert_eq!(json["data"]["font_size"], 22);
    assert_eq!(json["data"]["background_color"], "#FFFFFF");
    assert_eq!(json["data"]["text_color"], "#333333");
    assert_eq!(json["data"]["simplified_chinese"], false);

    // F4: partial update (only font_size)
    let resp = super::make_json_request(
        &app,
        Method::PUT,
        "/api/reading/settings",
        &serde_json::json!({"font_size": 24}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // F5: verify only font_size changed
    let resp = make_request(&app, Method::GET, "/api/reading/settings", Body::empty(), Some(&token1)).await;
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
        "/api/reading/heartbeat",
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
        "/api/reading/heartbeat",
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
        "/api/reading/heartbeat",
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
        &format!("/api/books/{}", public_book_id),
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
            &format!("/api/books/{}/jump_to_chapter?chapter_id={}", public_book_id, chapter_id),
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
    let resp = make_request(&app, Method::GET, "/api/admin/users", Body::empty(), Some(&admin_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    let users = json["data"]["users"].as_array().unwrap();
    assert!(users.len() >= 2);

    // H2: reset user2's password
    let user2_id = users.iter().find(|u| u["username"] == "user2").unwrap()["user_id"].as_i64().unwrap();

    let resp = super::make_json_request(
        &app,
        Method::POST,
        &format!("/api/admin/users/{}/reset_password", user2_id),
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
    let resp = make_request(&app, Method::GET, "/api/books", Body::empty(), None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // I2: non-existent book
    let resp = make_request(&app, Method::GET, "/api/books/99999", Body::empty(), Some(&token1)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // I3: content with invalid position
    let resp = make_request(
        &app,
        Method::GET,
        &format!("/api/books/{}/content?position=999999", book1_id),
        Body::empty(),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // I4: user token for admin endpoint
    let resp = make_request(&app, Method::GET, "/api/admin/users", Body::empty(), Some(&token1)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // I5: invalid file format
    let resp = make_multipart_request(
        &app,
        "/api/books/upload",
        vec![("title", "bad file"), ("is_public", "true")],
        Some(("file", "test.pdf", b"not a real pdf".as_slice())),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // I6: change password with wrong old password
    let resp = super::make_json_request(
        &app,
        Method::POST,
        "/api/auth/change_password",
        &serde_json::json!({"old_password": "wrongpassword", "new_password": "newpwd"}),
        Some(&token1),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 1007);

    println!("\n===== All integration tests passed! =====");
}

async fn login_as_admin(app: &axum::Router) -> axum::response::Response {
    super::make_json_request(
        app,
        Method::POST,
        "/api/auth/admin/login",
        &serde_json::json!({"password": "admin123"}),
        None,
    )
    .await
}
