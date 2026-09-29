use std::str::FromStr;

use anyhow::Result;
use sqlx::{Pool, Sqlite, sqlite::SqliteConnectOptions};
use tokio::fs;

use crate::config::Config;

pub async fn init_db_pool(config: &Config) -> Result<Pool<Sqlite>> {
    // db.url 是 sqlx 连接 URL（如 `sqlite://target/sqlx_schema.db`），
    // 必须通过 FromStr 解析剥离 scheme 与查询参数，不能直接当作文件路径
    let options = SqliteConnectOptions::from_str(&config.db.url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    // 确保数据库文件的目录存在
    if let Some(db_dir) = options.get_filename().parent()
        && !db_dir.exists()
    {
        fs::create_dir_all(db_dir).await?;
    }

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(config.db.max_connections)
        .connect_with(options)
        .await?;

    Ok(pool)
}

pub async fn run_migrations(pool: &Pool<Sqlite>) -> Result<()> {
    // 项目目前未投入生产，不需要考虑 db 兼容性，schema.sql 即唯一真实源
    let sql = include_str!("../schema.sql");
    sqlx::raw_sql(sql).execute(pool).await?;
    Ok(())
}

// 为测试创建内存数据库连接池
#[cfg(test)]
pub async fn create_test_pool() -> Result<Pool<Sqlite>> {
    use uuid::Uuid;

    // 每个测试使用独立的内存数据库，避免并行测试互相干扰
    let db_name = format!("file:test_{}?mode=memory&cache=private", Uuid::new_v4());
    let options = SqliteConnectOptions::new()
        .filename(&db_name)
        .foreign_keys(true);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let sql = include_str!("../schema.sql");
    sqlx::raw_sql(sql).execute(&pool).await?;
    // 测试环境默认放开邀请码：生产逻辑在 settings 行缺失时按需要邀请码处理
    // （安全兜底），这里预置一行 invite_code_required=0，保持既有测试免邀请码注册的语义
    sqlx::query("INSERT INTO settings (id, invite_code_required) VALUES (1, 0)")
        .execute(&pool)
        .await?;
    Ok(pool)
}
