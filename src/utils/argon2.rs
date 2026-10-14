//! 密码哈希：argon2 计算固定派发到单一专用 OS 线程执行。
//!
//! `Argon2::default()`（Argon2id，m_cost=19MiB）每次计算分配约 19MB；
//! glibc 会为不同线程维护独立 arena 且空闲页不归还 OS，计算落到多个
//! worker 线程时会各自滞留一份（实测 3 次登录后 +38MB RSS 常驻）。
//! 固定到单线程后仅滞留一份；登录/注册属低频操作，串行化无可感知影响。
use std::sync::{OnceLock, mpsc};

use argon2::{
    Argon2, PasswordHasher as _, PasswordVerifier as _, password_hash::phc::PasswordHash,
};

use crate::error::AppError;

type Job = Box<dyn FnOnce() + Send + 'static>;

static JOB_TX: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

/// 懒初始化专用线程：随进程常驻，channel 关闭（发送端全部 drop）时线程自然退出
fn job_sender() -> &'static mpsc::Sender<Job> {
    JOB_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("argon2".to_owned())
            .spawn(move || {
                for job in rx {
                    job();
                }
            })
            .expect("failed to spawn argon2 worker thread");
        tx
    })
}

/// 派发闭包到 argon2 专用线程执行，异步等待结果（不占用 tokio worker）
async fn run_on_argon2_thread<T, F>(f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    let job: Job = Box::new(move || {
        // 等待方已取消时忽略发送失败
        let _ = tx.send(f());
    });
    job_sender()
        .send(job)
        .map_err(|_| AppError::Internal("argon2 worker thread unavailable".into()))?;
    rx.await
        .map_err(|_| AppError::Internal("argon2 worker thread dropped the job".into()))
}

// 哈希密码：随机 salt 由库内部生成，输出 PHC 格式字符串
pub async fn hash_password(password: &str) -> Result<String, AppError> {
    let password = password.to_owned();
    run_on_argon2_thread(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
            .map_err(|e| AppError::Internal(format!("Password hashing failed: {e}")))
    })
    .await?
}

// 验证密码
pub async fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let password = password.to_owned();
    let hash = hash.to_owned();
    run_on_argon2_thread(move || {
        let parsed_hash = PasswordHash::new(&hash)
            .map_err(|e| AppError::Internal(format!("Failed to parse hash: {e}")))?;

        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    })
    .await?
}
