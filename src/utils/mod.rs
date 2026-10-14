use rand::RngExt;
use uuid::Uuid;

pub mod argon2;
pub mod utf8;
pub mod zip;

// 密码哈希固定在专用线程执行（见 argon2.rs），此处重导出保持调用点写法不变
pub use argon2::{hash_password, verify_password};

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
