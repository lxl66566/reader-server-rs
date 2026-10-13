use std::time::Duration;

use axum::{
    Router,
    extract::{DefaultBodyLimit, FromRef},
    routing::get,
};
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
    frontend,
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

// 请求体上限在配置文件大小之上额外允许的 multipart 边界/字段开销
const MULTIPART_BODY_OVERHEAD: usize = 1024 * 1024;

// 创建应用实例
pub fn create_app(db: Pool<Sqlite>, config: &Config) -> Router {
    // 创建共享状态
    let state = AppState {
        db,
        config: config.clone(),
        ws: ConnectionRegistry::new(),
    };

    // 请求体大小上限：配置的文件大小上限 + multipart 开销。
    // axum 默认仅 2MB，会卡死正常书籍上传（DefaultBodyLimit 只影响缓冲 body 的
    // 提取器与 Multipart，不包装 body 类型，与其它 layer 顺序无关）。
    let body_limit = state.config.storage.max_file_size + MULTIPART_BODY_OVERHEAD;

    // 创建CORS中间件
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 创建限流配置：用 SmartIpKeyExtractor 支持 X-Forwarded-For / x-real-ip /
    // forwarded 部署时务必保证反代正确设置这些 header（否则会被伪造绕过）
    // 注意 tower_governor 的 per_second(n) 语义是"每 n 秒补充 1 个 token"，
    // 与直觉相反，这里按"每秒 per_second 个请求"自行换算补充间隔
    let replenish =
        Duration::from_nanos(1_000_000_000 / u64::from(state.config.rate_limit.per_second.get()));
    let governor_config = GovernorConfigBuilder::default()
        .key_extractor(SmartIpKeyExtractor)
        .period(replenish)
        .burst_size(state.config.rate_limit.burst_size.get())
        .use_headers()
        .finish()
        .expect("validated rate limit config always builds a quota");

    // 构建路由
    Router::new()
        // 健康检查（不受限流影响，方便监控探活）
        .route("/health", get(|| async { "ok" }))
        // 前端静态资源兜底（release 嵌入，debug 提示 pnpm dev）
        .fallback(frontend::fallback)
        // 认证路由
        .nest("/api/v1/auth", auth::routes())
        // 书籍路由
        .nest("/api/v1/books", books::routes())
        // 阅读路由
        .nest("/api/v1/reading", reading::routes())
        // 管理员路由
        .nest("/api/v1/admin", admin::routes())
        // 请求体大小上限（见上方注释）
        .layer(DefaultBodyLimit::max(body_limit))
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
