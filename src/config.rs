use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub db: DbConfig,
    pub storage: StorageConfig,
    pub jwt: JwtConfig,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    pub secret: String,
    pub expiration: u64, // 过期时间，单位为秒
    pub admin_expiration: u64,
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
            },
            jwt: JwtConfig {
                secret: "super_secret_key_change_me_in_production".to_string(),
                expiration: 60 * 60 * 24 * 30,      // 30天
                admin_expiration: 60 * 60 * 24 * 7, // 7天
            },
        }
    }
}

pub async fn load_config() -> Result<Config> {
    // 配置文件路径
    let config_path = PathBuf::from("config.json");

    // 如果配置文件存在，从文件加载配置
    if config_path.exists() {
        let config_str = fs::read_to_string(config_path).await?;
        let config: Config = serde_json::from_str(&config_str)?;
        Ok(config)
    } else {
        // 否则使用默认配置，并写入配置文件
        let config = Config::default();
        let config_str = serde_json::to_string_pretty(&config)?;
        fs::write(config_path, config_str).await?;
        Ok(config)
    }
}
