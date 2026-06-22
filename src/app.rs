use axum::{Router, extract::FromRef, routing::get};
use sqlx::{Pool, Sqlite};
use tower::ServiceBuilder;
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    config::Config,
    routes::{admin, auth, books, reading},
    ws::manager::ConnectionRegistry,
};

// 应用状态
#[derive(Clone)]
pub struct AppState {
    pub db: Pool<Sqlite>,
    pub config: Config,
    pub ws: std::sync::Arc<ConnectionRegistry>,
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

impl FromRef<AppState> for std::sync::Arc<ConnectionRegistry> {
    fn from_ref(state: &AppState) -> Self {
        state.ws.clone()
    }
}

// 创建应用实例
pub fn create_app(db: Pool<Sqlite>, config: &Config) -> Router {
    // 创建共享状态
    let state = AppState {
        db,
        config: config.clone(),
        ws: ConnectionRegistry::new(),
    };

    // 创建CORS中间件
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 创建限流配置：用 SmartIpKeyExtractor 支持 X-Forwarded-For / x-real-ip /
    // forwarded 部署时务必保证反代正确设置这些 header（否则会被伪造绕过）
    let governor_config = GovernorConfigBuilder::default()
        .key_extractor(SmartIpKeyExtractor)
        .per_second(state.config.rate_limit.per_second)
        .burst_size(state.config.rate_limit.burst_size)
        .use_headers()
        .finish()
        .expect("governor config with positive per_second/burst_size always builds");

    // 构建路由
    Router::new()
        // 健康检查（不受限流影响，方便监控探活）
        .route("/health", get(|| async { "ok" }))
        // 认证路由
        .nest("/api/v1/auth", auth::routes())
        // 书籍路由
        .nest("/api/v1/books", books::routes())
        // 阅读路由
        .nest("/api/v1/reading", reading::routes())
        // 管理员路由
        .nest("/api/v1/admin", admin::routes())
        // 限流：内层最先执行；放外层避免被其它 layer 包装导致 body 类型不兼容
        .layer(GovernorLayer::new(governor_config))
        // 其它中间件
        .layer(
            ServiceBuilder::new()
                .layer(CompressionLayer::new())
                .layer(TraceLayer::new_for_http())
                .layer(cors),
        )
        .with_state(state)
}
