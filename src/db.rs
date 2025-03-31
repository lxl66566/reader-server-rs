use std::path::Path;

use anyhow::Result;
use sqlx::{sqlite::SqliteConnectOptions, Executor, Pool, Sqlite, SqlitePool};
use tokio::fs;

use crate::config::Config;

pub async fn init_db_pool(config: &Config) -> Result<Pool<Sqlite>> {
    // 确保数据库文件的目录存在
    if let Some(db_path) = Path::new(&config.db.url).parent() {
        if !db_path.exists() {
            fs::create_dir_all(db_path).await?;
        }
    }

    let options = SqliteConnectOptions::new()
        .filename(&config.db.url)
        .create_if_missing(true);

    // 创建连接池
    let pool = SqlitePool::connect_with(options).await?;
    pool.execute("PRAGMA time_zone = 'UTC';").await?;

    Ok(pool)
}

pub async fn run_migrations(pool: &Pool<Sqlite>) -> Result<()> {
    // 使用 schema 模块中定义的 SQL 语句
    let sql = include_str!("../schema.sql");
    let statements = sql.split(';').filter(|s| !s.trim().is_empty());

    for statement in statements {
        let query = format!("{};", statement);
        sqlx::query(&query).execute(pool).await?;
    }

    Ok(())
}

// 为测试创建内存数据库连接池
#[cfg(test)]
pub async fn create_test_pool() -> Result<Pool<Sqlite>> {
    let pool = SqlitePool::connect("sqlite::memory:").await?;

    // 使用 schema 模块中定义的 SQL 语句创建表
    let sql = include_str!("../schema.sql");
    let statements = sql.split(';').filter(|s| !s.trim().is_empty());

    for statement in statements {
        let query = format!("{};", statement);
        sqlx::query(&query).execute(&pool).await?;
    }

    Ok(pool)
}
