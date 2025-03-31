mod app;
mod auth;
mod config;
mod db;
mod error;
mod models;
mod routes;
mod utils;

// 因为是 bin target，所以集成测试必须放在 src 里
#[cfg(test)]
mod tests;

use std::{net::SocketAddr, path::Path};

use anyhow::Result;
use tokio::{fs, net::TcpListener};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化错误处理
    color_eyre::install().map_err(|e| anyhow::anyhow!("Failed to install color_eyre: {}", e))?;

    // 初始化日志
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "reader_server_rs=debug,tower_http=debug,axum=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 加载配置
    let config = config::load_config().await?;

    // 确保目录存在
    ensure_directories(&config).await?;

    // 初始化数据库
    let db_pool = db::init_db_pool(&config).await?;

    // 运行数据库迁移
    db::run_migrations(&db_pool).await?;

    // 构建应用
    let app = app::create_app(db_pool, config.clone()).await?;

    // 获取服务地址
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));

    tracing::info!("服务器启动在 http://{}", addr);

    // 启动服务器
    axum::serve(TcpListener::bind(addr).await?, app).await?;

    Ok(())
}

async fn ensure_directories(config: &config::Config) -> Result<()> {
    // 确保书籍目录存在
    let book_dir = Path::new(&config.storage.book_dir);
    if !book_dir.exists() {
        fs::create_dir_all(book_dir).await?;
    }

    Ok(())
}
