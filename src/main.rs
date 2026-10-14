mod access;
mod app;
mod auth;
mod config;
mod db;
mod error;
mod frontend;
mod models;
mod parser;
mod routes;
mod user_data;
mod utils;
mod ws;

// 因为是 bin target，所以集成测试必须放在 src 里
#[cfg(test)]
mod tests;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Result;
use clap::Parser;
use tokio::{fs, net::TcpListener, signal};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// 命令行参数：--version/-V 与 -h/--help 由 clap 自动生成，about 取 Cargo.toml description
#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"))]
struct Args {
    /// Path to the config file
    #[arg(short, long, default_value = "config.json")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化错误处理
    color_eyre::install().map_err(|e| anyhow::anyhow!("Failed to install color_eyre: {e}"))?;

    // 解析命令行参数，加载配置（先于日志初始化，因为日志路径来自配置）
    let args = Args::parse();
    let config = config::load_config(&args.config).await?;

    // 初始化日志：同时输出到滚动文件与 stderr
    // _guard 必须存活到程序结束，否则后台 flush 任务会被 drop，丢日志
    let _log_guard = init_tracing(&config)?;

    // 确保目录存在
    ensure_directories(&config).await?;

    // 初始化数据库
    let db_pool = db::init_db_pool(&config).await?;

    // 运行数据库迁移
    db::run_migrations(&db_pool).await?;

    // 构建应用
    let app = app::create_app(db_pool, &config);

    // 监听地址取自配置 server.host + server.port（host 支持 IP 或主机名），
    // 打印实际绑定地址，避免配置与生效行为不一致
    let listener = TcpListener::bind((config.server.host.as_str(), config.server.port)).await?;
    let addr = listener.local_addr()?;
    tracing::info!("server listening on http://{addr}");
    // debug 构建不内嵌前端，提示走 vite dev server；release 构建访问同一地址即可
    #[cfg(debug_assertions)]
    tracing::info!("debug build: frontend not embedded, run `pnpm dev` in frontend/ and visit http://localhost:5173");
    #[cfg(not(debug_assertions))]
    tracing::info!("frontend embedded, visit http://{addr}");

    // 启动服务器（必须携带 ConnectInfo，否则限流中间件无法提取客户端 IP）
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

/// 等待关闭信号：Ctrl+C 或 Unix SIGTERM（容器/ systemd 停机默认发 SIGTERM）
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received, starting graceful shutdown");
}

/// 初始化 tracing：根据配置选择滚动策略，同时输出到文件和 stderr
/// 返回的 `WorkerGuard` 必须由调用方持有到程序退出，确保异步 writer 刷盘
fn init_tracing(
    config: &config::Config,
) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    // 注意：bin target 名为 reader-server，日志过滤的 crate 名应为 reader_server，
    // 否则本 crate（含启动日志、ws 日志）会被默认级别 ERROR 静默吞掉
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "reader_server=debug,tower_http=debug,axum=debug".into());

    let rotation = config.log.rotation.trim().to_ascii_lowercase();
    if rotation == "never" {
        // 仅 stderr
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
        tracing::info!("日志仅输出到 stderr（rotation=never）");
        return Ok(None);
    }

    // 确保日志目录存在（同步创建，因为日志初始化早于 async runtime 上下文）
    std::fs::create_dir_all(&config.log.dir)?;

    let file_appender = match rotation.as_str() {
        "hourly" => tracing_appender::rolling::hourly(&config.log.dir, "reader-server.log"),
        "daily" | "" => tracing_appender::rolling::daily(&config.log.dir, "reader-server.log"),
        other => {
            anyhow::bail!("unknown log rotation '{other}', expected hourly/daily/never")
        },
    };
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // 文件 layer 用 non-blocking writer；stderr layer 直接走标准 fmt
    let file_layer = tracing_subscriber::fmt::layer().with_writer(non_blocking);
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .init();

    tracing::info!(
        dir = %config.log.dir.display(),
        rotation = %rotation,
        "日志文件已启用滚动"
    );
    Ok(Some(guard))
}

async fn ensure_directories(config: &config::Config) -> Result<()> {
    // 确保书籍目录存在
    let book_dir = Path::new(&config.storage.book_dir);
    if !book_dir.exists() {
        fs::create_dir_all(book_dir).await?;
    }

    Ok(())
}
