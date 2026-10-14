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
    // sqlx 标准迁移机制：migrations/ 在编译期嵌入，release 二进制不依赖磁盘上的迁移目录。
    // 0001 为幂等建表（对存量库重放安全），后续迁移只做增量 ALTER。
    // schema.sql 仅作为全量 schema 的文档源，create_test_pool（测试内存库）继续使用它
    sqlx::migrate!("./migrations").run(pool).await?;
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
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// books 表是否已有 chapter_pattern 列（pragma 表值函数，免手写 PRAGMA 解析）
    async fn has_chapter_pattern(pool: &Pool<Sqlite>) -> bool {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('books') WHERE name = 'chapter_pattern'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        n == 1
    }

    /// 存量库升级路径：仅应用过 0001（本次改动前的 schema，无 _sqlx_migrations 表）的库
    /// 跑迁移应成功加列；重复执行幂等
    #[tokio::test]
    async fn migrations_apply_on_legacy_db() {
        use sqlx::{Connection, Executor, SqliteConnection, sqlite::SqlitePoolOptions};

        let dir = std::env::temp_dir().join("reader_test_migrations");
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join(format!("{}.db", uuid::Uuid::new_v4()));
        let url = format!("sqlite:{}?mode=rwc", db.display());

        let mut conn = SqliteConnection::connect(&url).await.unwrap();
        conn.execute(include_str!("../migrations/0001_init.sql"))
            .await
            .unwrap();

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        assert!(has_chapter_pattern(&pool).await);
        // 再次执行不应报错（已应用的迁移被跳过）
        run_migrations(&pool).await.unwrap();

        let _ = fs::remove_file(&db).await;
    }

    /// schema.sql 是全量 schema 的文档源，必须与迁移结果等价
    #[tokio::test]
    async fn test_pool_schema_matches_migrations() {
        let pool = create_test_pool().await.unwrap();
        assert!(has_chapter_pattern(&pool).await);
    }
}
