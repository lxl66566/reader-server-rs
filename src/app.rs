use anyhow::Result;
use axum::{extract::FromRef, Router};
use sqlx::{Pool, Sqlite};
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    config::Config,
    routes::{admin, auth, books, reading},
};

// 应用状态
#[derive(Clone)]
pub struct AppState {
    pub db: Pool<Sqlite>,
    pub config: Config,
}

// 为状态实现FromRef trait，允许从状态中提取数据库连接和配置
impl FromRef<AppState> for Pool<Sqlite> {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl FromRef<AppState> for Config {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

// 创建应用实例
pub async fn create_app(db: Pool<Sqlite>, config: Config) -> Result<Router> {
    // 创建共享状态
    let state = AppState { db, config };

    // 创建CORS中间件
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 构建路由
    let app = Router::new()
        // 认证路由
        .nest("/api/auth", auth::routes())
        // 书籍路由
        .nest("/api/books", books::routes())
        // 阅读路由
        .nest("/api/reading", reading::routes())
        // 管理员路由
        .nest("/api/admin", admin::routes())
        // 中间件
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors),
        )
        .with_state(state);

    Ok(app)
}
