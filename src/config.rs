use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub db: DbConfig,
    pub storage: StorageConfig,
    pub jwt: JwtConfig,
    pub log: LogConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub book_dir: PathBuf,
    /// 上传文件大小上限（字节）
    pub max_file_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    pub secret: String,
    pub expiration: u64, // 过期时间，单位为秒
    pub admin_expiration: u64,
}

/// 日志轮转配置
/// rotation: "hourly" | "daily" | "never"（never 时只输出到 stderr）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    pub dir: PathBuf,
    pub rotation: String,
}

/// 全局 IP 限流配置
/// per_second + burst_size 共同决定 token bucket；按客户端 IP 维度计数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub per_second: u64,
    pub burst_size: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
            },
            db: DbConfig {
                url: std::env::var("DATABASE_URL").unwrap_or("sqlite:reader.db".to_string()),
                max_connections: 10,
            },
            storage: StorageConfig {
                book_dir: PathBuf::from("books"),
                max_file_size: 10 * 1024 * 1024, // 10MB
            },
            jwt: JwtConfig {
                secret: String::new(),              // load_config 首次运行时会生成随机值
                expiration: 60 * 60 * 24 * 30,      // 30天
                admin_expiration: 60 * 60 * 24 * 7, // 7天
            },
            log: LogConfig {
                dir: PathBuf::from("logs"),
                rotation: "daily".to_string(),
            },
            rate_limit: RateLimitConfig {
                per_second: 10,
                burst_size: 30,
            },
        }
    }
}

/// 旧版硬编码的默认 secret，用于检测未修改的配置
const LEGACY_DEFAULT_SECRET: &str = "super_secret_key_change_me_in_production";

pub async fn load_config() -> Result<Config> {
    let config_path = PathBuf::from("config.json");

    if config_path.exists() {
        let config_str = fs::read_to_string(&config_path).await?;
        let config: Config = serde_json::from_str(&config_str)?;

        // 强制要求用户设置非空、非默认值的 JWT secret
        if config.jwt.secret.is_empty() || config.jwt.secret == LEGACY_DEFAULT_SECRET {
            anyhow::bail!(
                "JWT secret in config.json must be a non-empty, non-default value. \
                 Please generate a random secret and update config.json."
            );
        }

        Ok(config)
    } else {
        // 首次运行：生成随机 secret 并写入 config.json
        let mut config = Config::default();
        config.jwt.secret = Uuid::new_v4().to_string();

        let config_str = serde_json::to_string_pretty(&config)?;
        fs::write(&config_path, config_str).await?;

        tracing::info!("Generated config.json with a random JWT secret");
        Ok(config)
    }
}
