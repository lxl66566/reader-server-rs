use std::path::Path;

use anyhow::Result;
use sqlx::{Executor, Pool, Sqlite, SqlitePool, sqlite::SqliteConnectOptions};
use tokio::fs;

use crate::config::Config;

pub async fn init_db_pool(config: &Config) -> Result<Pool<Sqlite>> {
    // 确保数据库文件的目录存在
    if let Some(db_path) = Path::new(&config.db.url).parent()
        && !db_path.exists()
    {
        fs::create_dir_all(db_path).await?;
    }

    let options = SqliteConnectOptions::new()
        .filename(&config.db.url)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePool::connect_with(options).await?;
    pool.execute("PRAGMA time_zone = 'UTC';").await?;

    Ok(pool)
}

pub async fn run_migrations(pool: &Pool<Sqlite>) -> Result<()> {
    let sql = include_str!("../schema.sql");
    sqlx::raw_sql(sql).execute(pool).await?;
    run_compat_migrations(pool).await?;
    Ok(())
}

/// 兼容性迁移：为旧版数据库（schema.sql 变更前创建）补充新增列。
/// 新建数据库的 CREATE TABLE 已包含这些列，ALTER TABLE 会失败（duplicate column），忽略即可。
async fn run_compat_migrations(pool: &Pool<Sqlite>) -> Result<()> {
    for stmt in [
        "ALTER TABLE admin ADD COLUMN username TEXT",
        "ALTER TABLE users ADD COLUMN token_version INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE users ADD COLUMN failed_login_attempts INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE users ADD COLUMN locked_until TEXT",
    ] {
        let _ = sqlx::query(stmt).execute(pool).await;
    }
    // 旧 admin 表没有 username，设默认值
    let _ = sqlx::query("UPDATE admin SET username = 'admin' WHERE username IS NULL")
        .execute(pool)
        .await;
    Ok(())
}

// 为测试创建内存数据库连接池
#[cfg(test)]
pub async fn create_test_pool() -> Result<Pool<Sqlite>> {
    let options = SqliteConnectOptions::new()
        .filename("file::memory:?cache=shared")
        .foreign_keys(true);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let sql = include_str!("../schema.sql");
    sqlx::raw_sql(sql).execute(&pool).await?;
    Ok(pool)
}
