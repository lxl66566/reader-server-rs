// EPUB 懒加载资源端点（/books/{id}/epub/{token}/{+entry}）集成测试：
// token 鉴权、条目读取与清洗、ETag 304、权限与格式约束

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::tests::{
    make_multipart_request, make_request, register_and_login, response_json, setup_test_app,
};

/// 最小可用 EPUB：1 个 spine 章节；xhtml 含自闭合 script、css 含外链 @font-face，
/// 用于同时验证条目读取与输出清洗
fn build_minimal_epub() -> Vec<u8> {
    use std::io::Write as _;

    let mut buf: Vec<u8> = Vec::new();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));

    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // EPUB 规范要求 mimetype 为首个条目且不压缩
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();

    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
            .as_bytes(),
    )
    .unwrap();

    zip.start_file("OEBPS/content.opf", deflated).unwrap();
    zip.write_all(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:test-epub-entry</dc:identifier>
    <dc:title>条目测试</dc:title>
    <dc:creator>tester</dc:creator>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
  </spine>
</package>"#
            .as_bytes(),
    )
    .unwrap();

    zip.start_file("OEBPS/nav.xhtml", deflated).unwrap();
    zip.write_all(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body><nav epub:type="toc"><ol><li><a href="c1.xhtml">第一章</a></li></ol></nav></body>
</html>"#
            .as_bytes(),
    )
    .unwrap();

    zip.start_file("OEBPS/style.css", deflated).unwrap();
    zip.write_all(
        b"@font-face { font-family: ext; src: url(file:///x.woff); }\n\
          @font-face { font-family: inline; src: url(f.woff); }\n\
          p { color: #000 !important; }\n",
    )
    .unwrap();

    zip.start_file("OEBPS/c1.xhtml", deflated).unwrap();
    zip.write_all(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <head><title>第一章</title><link rel="stylesheet" href="style.css"/></head>
  <body><script src="a.js"/><p>正文内容</p></body>
</html>"#
            .as_bytes(),
    )
    .unwrap();

    zip.finish().unwrap();
    buf
}

async fn upload_epub(app: &Router, token: &str, title: &str, is_public: bool) -> i64 {
    let resp = make_multipart_request(
        app,
        "/api/v1/books/upload",
        vec![
            ("title", title),
            (
                "is_public",
                if is_public {
                    "true"
                } else {
                    "false"
                },
            ),
        ],
        Some(("file", "b.epub", &build_minimal_epub())),
        Some(token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "upload failed");
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0, "upload error: {json:?}");
    json["data"]["book_id"].as_i64().unwrap()
}

fn entry_uri(book_id: i64, token: &str, entry: &str) -> String {
    format!("/api/v1/books/{book_id}/epub/{token}/{entry}")
}

async fn body_bytes(resp: Response) -> Vec<u8> {
    resp.into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn test_epub_entry_read_sanitize_and_etag() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;
    let token = register_and_login(&app, "epub_res_owner", "password1").await;
    let book_id = upload_epub(&app, &token, "懒加载书", false).await;

    // container.xml：200 + text/xml + rootfile 路径可见
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(book_id, &token, "META-INF/container.xml"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/xml; charset=utf-8");
    let body = String::from_utf8(body_bytes(resp).await).unwrap();
    assert!(body.contains("OEBPS/content.opf"), "missing package path");

    // xhtml：自闭合 script 展开为空标签对
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(book_id, &token, "OEBPS/c1.xhtml"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "application/xhtml+xml");
    let body = String::from_utf8(body_bytes(resp).await).unwrap();
    assert!(
        body.contains(r#"<script src="a.js"></script>"#),
        "self-closed script not expanded"
    );

    // css：外链 @font-face 剥离，相对 url 保留
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(book_id, &token, "OEBPS/style.css"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/css; charset=utf-8");
    let body = String::from_utf8(body_bytes(resp).await).unwrap();
    assert!(!body.contains("file:"), "external font-face not stripped");
    assert!(body.contains("url(f.woff)"), "relative font-face removed");

    // ETag：If-None-Match 命中 304
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(book_id, &token, "OEBPS/c1.xhtml"),
        Body::empty(),
        None,
    )
    .await;
    let etag = resp
        .headers()
        .get("etag")
        .expect("etag header")
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(resp.status(), StatusCode::OK);
    let req = Request::builder()
        .method(Method::GET)
        .uri(entry_uri(book_id, &token, "OEBPS/c1.xhtml"))
        .header("If-None-Match", &etag)
        .header("X-Forwarded-For", "127.0.0.1")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);

    // 不存在的条目：404
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(book_id, &token, "OEBPS/missing.xhtml"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    Ok(())
}

#[tokio::test]
async fn test_epub_entry_auth_and_access() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;
    let token = register_and_login(&app, "epub_res_a", "password1").await;
    let other = register_and_login(&app, "epub_res_b", "password1").await;

    // 私有书：非 owner 403
    let book_id = upload_epub(&app, &token, "私有书", false).await;
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(book_id, &other, "META-INF/container.xml"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 无效 token：401
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(book_id, "not.a.jwt", "META-INF/container.xml"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 公开书：任意登录用户 200
    let pub_id = upload_epub(&app, &token, "公开书", true).await;
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(pub_id, &other, "META-INF/container.xml"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // txt 书：400（无 zip 条目语义）
    let txt_resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "纯文本"), ("is_public", "false")],
        Some(("file", "a.txt", "第一章\n内容".as_bytes())),
        Some(&token),
    )
    .await;
    assert_eq!(txt_resp.status(), StatusCode::OK);
    let json = response_json(txt_resp).await;
    let txt_id = json["data"]["book_id"].as_i64().unwrap();
    let resp = make_request(
        &app,
        Method::GET,
        &entry_uri(txt_id, &token, "META-INF/container.xml"),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    Ok(())
}
