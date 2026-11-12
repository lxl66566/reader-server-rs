// EPUB locations 预生成端点（/books/{id}/locations）集成测试：
// 鉴权与权限、格式约束、数组格式与章序、ETag 304、sidecar 缓存

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use tower::ServiceExt;

use crate::tests::{
    make_multipart_request, make_request, register_and_login, response_json, setup_test_app,
};

/// 最小 EPUB：2 个 spine 章节（其一 linear=no），正文跨多个文本节点
fn build_epub() -> Vec<u8> {
    use std::io::Write as _;

    let mut buf: Vec<u8> = Vec::new();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(
        br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
    )
    .unwrap();
    zip.start_file("OEBPS/content.opf", deflated).unwrap();
    zip.write_all(
        br#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Loc</dc:title></metadata>
  <manifest>
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="c1"/><itemref idref="c2" linear="no"/>
  </spine>
</package>"#,
    )
    .unwrap();
    zip.start_file("OEBPS/c1.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>first paragraph</p><p>second paragraph</p></body></html>"#,
    )
    .unwrap();
    zip.start_file("OEBPS/c2.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>non linear chapter</p></body></html>"#,
    )
    .unwrap();
    zip.finish().unwrap();
    buf
}

async fn upload(app: &Router, token: &str, title: &str, is_public: bool, epub: Vec<u8>) -> i64 {
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
        Some(("file", "b.epub", &epub)),
        Some(token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = response_json(resp).await;
    assert_eq!(json["code"], 0, "upload error: {json:?}");
    json["data"]["book_id"].as_i64().unwrap()
}

fn locations_uri(book_id: i64) -> String {
    format!("/api/v1/books/{book_id}/locations")
}

#[tokio::test]
async fn test_locations_format_etag_and_cache() -> Result<()> {
    let (app, pool) = setup_test_app().await?;
    let token = register_and_login(&app, "locs_owner", "password1").await;
    let book_id = upload(&app, &token, "locations 书", false, build_epub()).await;

    let resp = make_request(
        &app,
        Method::GET,
        &locations_uri(book_id),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "application/json");
    assert_eq!(resp.headers()["cache-control"], "private, max-age=3600");
    let etag = resp.headers()["etag"].to_str().unwrap().to_string();

    let json = response_json(resp).await;
    assert_eq!(json["code"], 0);
    let data = json["data"].as_array().expect("data array");
    // c1 两段文本共 31 字符（< 1600）→ 单条 range；linear=no 的 c2 不产出
    assert_eq!(data.len(), 1, "locations: {data:?}");
    assert!(
        data[0].as_str().unwrap().starts_with("epubcfi(/6/2!"),
        "unexpected cfi: {}",
        data[0]
    );

    // If-None-Match 命中 304
    let req = Request::builder()
        .method(Method::GET)
        .uri(locations_uri(book_id))
        .header("X-Forwarded-For", "127.0.0.1")
        .header("Authorization", format!("Bearer {token}"))
        .header("If-None-Match", &etag)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);

    // sidecar 缓存文件已生成
    let file_path: String =
        sqlx::query_scalar!("SELECT file_path FROM books WHERE id = ?", book_id)
            .fetch_one(&pool)
            .await?;
    assert!(std::path::Path::new(&format!("{file_path}.locations.json")).exists());

    Ok(())
}

#[tokio::test]
async fn test_locations_auth_access_format() -> Result<()> {
    let (app, _pool) = setup_test_app().await?;
    let token = register_and_login(&app, "locs_a", "password1").await;
    let other = register_and_login(&app, "locs_b", "password1").await;

    // 无 token：401
    let book_id = upload(&app, &token, "私有", false, build_epub()).await;
    let resp = make_request(
        &app,
        Method::GET,
        &locations_uri(book_id),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 他人私有书：403
    let resp = make_request(
        &app,
        Method::GET,
        &locations_uri(book_id),
        Body::empty(),
        Some(&other),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 公开书：任意登录用户 200
    let pub_id = upload(&app, &token, "公开", true, build_epub()).await;
    let resp = make_request(
        &app,
        Method::GET,
        &locations_uri(pub_id),
        Body::empty(),
        Some(&other),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // 不存在的书：404
    let resp = make_request(
        &app,
        Method::GET,
        &locations_uri(999_999),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // txt 书：400
    let txt_resp = make_multipart_request(
        &app,
        "/api/v1/books/upload",
        vec![("title", "纯文本"), ("is_public", "false")],
        Some(("file", "a.txt", "第一章\n内容".as_bytes())),
        Some(&token),
    )
    .await;
    let json = response_json(txt_resp).await;
    let txt_id = json["data"]["book_id"].as_i64().unwrap();
    let resp = make_request(
        &app,
        Method::GET,
        &locations_uri(txt_id),
        Body::empty(),
        Some(&token),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    Ok(())
}
