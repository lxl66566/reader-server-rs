use std::{num::NonZeroU32, path::PathBuf};

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

impl JwtConfig {
    /// 启动期校验：过期时间上限 365 天。
    /// 超大 u64 会在签发路径触发 i64 转换 panic，这里提前 bail 给出明确报错
    fn validate(&self) -> Result<()> {
        const MAX_EXPIRATION_SECS: u64 = 365 * 24 * 60 * 60;
        for (field, value) in [
            ("jwt.expiration", self.expiration),
            ("jwt.admin_expiration", self.admin_expiration),
        ] {
            anyhow::ensure!(
                value <= MAX_EXPIRATION_SECS,
                "{field} must be <= {MAX_EXPIRATION_SECS} seconds (365 days), got {value}"
            );
        }
        Ok(())
    }
}

/// 日志轮转配置
/// rotation: "hourly" | "daily" | "never"（never 时只输出到 stderr）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    pub dir: PathBuf,
    pub rotation: String,
}

/// 全局 IP 限流配置
/// `per_second` + `burst_size` 共同决定 token bucket；按客户端 IP 维度计数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// 每 IP 每秒放行的请求数
    pub per_second: NonZeroU32,
    /// 令牌桶最大突发容量
    pub burst_size: NonZeroU32,
    /// 是否信任反代设置的 X-Forwarded-For / X-Real-IP / Forwarded 头。
    /// 默认 false（直连部署）：限流 key 取连接 peer addr，客户端伪造这些头无效；
    /// 仅当部署在正确改写下这些头的反向代理之后时才应开启。
    /// serde(default)：旧 config.json 无此字段时按 false 处理，无需手动迁移
    #[serde(default)]
    pub trust_proxy_headers: bool,
}

impl RateLimitConfig {
    /// 启动期校验：补充间隔 = 1s / per_second，governor 内部以 u64 纳秒
    /// 承载 GCRA 参数且要求间隔为正；上限 10^6 保证间隔 >= 1µs，
    /// 对任意 u32 burst 均远离溢出边界（tower_governor 自身不校验，
    /// 溢出时 debug panic、release 静默回绕导致限流失效）
    fn validate(&self) -> Result<()> {
        const MAX_PER_SECOND: u32 = 1_000_000;
        anyhow::ensure!(
            self.per_second.get() <= MAX_PER_SECOND,
            "rate_limit.per_second must be <= {MAX_PER_SECOND}, got {}",
            self.per_second.get()
        );
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 3000,
            },
            db: DbConfig {
                url: std::env::var("DATABASE_URL").unwrap_or("sqlite:reader.db".to_string()),
                max_connections: 10,
            },
            storage: StorageConfig {
                book_dir: PathBuf::from("books"),
                // 50MB：覆盖常见 txt / epub / pdf；
                // 在线上传超时与内存压力可控
                max_file_size: 50 * 1024 * 1024,
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
                per_second: NonZeroU32::new(20).unwrap(),
                burst_size: NonZeroU32::new(70).unwrap(),
                trust_proxy_headers: false,
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
                "JWT secret in config.json must be a non-empty, non-default value. Please \
                 generate a random secret and update config.json."
            );
        }

        config.rate_limit.validate()?;
        config.jwt.validate()?;

        Ok(config)
    } else {
        // 首次运行：生成随机 secret 并写入 config.json
        let mut config = Config::default();
        config.jwt.secret = Uuid::new_v4().to_string();

        let config_str = serde_json::to_string_pretty(&config)?;
        fs::write(&config_path, config_str).await?;
        // 文件含 JWT secret：立即收紧权限为仅属主可读写
        restrict_config_permissions(&config_path).await?;

        tracing::info!("Generated config.json with a random JWT secret");
        Ok(config)
    }
}

/// 把 config.json 权限收紧为 0600（仅 Unix；其他平台为空操作）
async fn restrict_config_permissions(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).await?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归：新增配置字段必须带 serde(default)，旧 config.json 免迁移可直接启动
    #[test]
    fn rate_limit_config_parses_without_trust_proxy_headers() {
        let legacy = r#"{"per_second": 20, "burst_size": 70}"#;
        let config: RateLimitConfig = serde_json::from_str(legacy).unwrap();
        assert!(!config.trust_proxy_headers);
    }
}
