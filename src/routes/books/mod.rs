mod content;
mod detail;
mod helpers;
mod list;
mod search;
mod tags;
mod trash;
mod upload;

use axum::{
    Router,
    routing::{delete, get, post, put},
};
pub use helpers::PaginationParams;
pub(crate) use helpers::{
    apply_book_field_updates, clean_tags, delete_book_cascade, escape_like, map_multipart_err,
    remove_book_file,
};

// 书籍路由
pub fn routes() -> Router<crate::app::AppState> {
    // 注意：所有 /{book_id}/... 路径必须放在同一个 Router 中，否则 axum 0.8
    // 的路径树在某些动态+静态混合情况下会返回 422（路由匹配失败）。
    Router::new()
        .route("/upload", post(upload::upload_book))
        .route("/", get(list::list_books))
        .route("/search", get(search::search_books))
        .route("/public", get(list::list_public_books))
        .route("/random_public", get(list::get_random_public_books))
        // 回收站列表（仅本人的已软删书籍）
        .route("/trash", get(trash::list_trash))
        .route(
            "/{book_id}",
            get(detail::get_book_detail)
                .put(detail::update_book)
                .delete(trash::delete_book),
        )
        // 从回收站恢复
        .route("/{book_id}/restore", post(trash::restore_book))
        // 彻底删除（复用级联删除 + 文件清理，不可恢复）
        .route("/{book_id}/purge", delete(trash::purge_book))
        // 原文件下载：epub / pdf 客户端渲染时使用；txt 也可用
        .route("/{book_id}/file", get(content::download_book_file))
        // 内容切片：仅 txt 有意义；epub/pdf 走 /file
        .route("/{book_id}/content", get(content::get_book_content))
        .route("/{book_id}/jump_to_chapter", get(content::jump_to_chapter))
        // 书内全文搜索：仅 txt 有意义；epub/pdf 沿用 content 的错误语义
        .route("/{book_id}/search", get(search::search_in_book))
        .route(
            "/{book_id}/bookmarks",
            get(crate::routes::annotations::list_bookmarks)
                .post(crate::routes::annotations::create_bookmark),
        )
        .route(
            "/{book_id}/bookmarks/{bookmark_id}",
            delete(crate::routes::annotations::delete_bookmark),
        )
        .route(
            "/{book_id}/annotations",
            get(crate::routes::annotations::list_annotations)
                .post(crate::routes::annotations::create_annotation),
        )
        .route(
            "/{book_id}/annotations/{annotation_id}",
            put(crate::routes::annotations::update_annotation)
                .delete(crate::routes::annotations::delete_annotation),
        )
        .route("/{book_id}/tags", get(tags::get_book_tags).put(tags::set_book_tags))
}
