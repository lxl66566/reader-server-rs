use argon2::{Argon2, PasswordHasher as _, password_hash::SaltString};
use rand::RngExt;
use uuid::Uuid;

use crate::error::AppError;

pub mod utf8;

/// 简易 `ETag` 哈希：用 FNV-1a 64bit，足够短、足够快、无外部依赖
/// 返回带双引号的 `ETag` 字符串（符合 RFC 7232）
pub fn make_etag(input: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for &byte in input.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    // 加 W/ 前缀表示 weak ETag：内容语义相同即可，不需要严格 byte-equal
    format!("W/\"{hash:016x}\"")
}

pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// 计算数据的 SHA-256 hex（小写）
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    // hex(lower)
    let mut out = String::with_capacity(64);
    for b in result {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

// 生成随机邀请码
pub fn generate_invite_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    const CODE_LEN: usize = 8;

    let mut rng = rand::rng();
    (0..CODE_LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// 哈希密码
pub fn hash_password(password: &str) -> Result<String, AppError> {
    let mut rng = rand::rng();
    // 生成 16 字节的随机 salt（使用 thread-local ChaCha12 RNG）
    let salt_bytes: [u8; 16] = rng.random();

    // 将随机字节数组编码为 Base64 格式的 SaltString
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| AppError::Internal(format!("Salt encoding failed: {e}")))?;

    // 执行 Argon2 哈希（默认使用推荐的 Argon2id 变体）
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::Internal(format!("Password hashing failed: {e}")))
}

// 验证密码
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHash, PasswordVerifier},
    };

    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(format!("Failed to parse hash: {e}")))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}
