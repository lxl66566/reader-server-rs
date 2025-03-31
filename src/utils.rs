use std::str::FromStr;

use anyhow::Result;
use regex::Regex;
use regex_macro::regex;
use uuid::Uuid;

use crate::error::AppError;

// 中文数字映射
const CN_NUMS: [(&str, i64); 20] = [
    ("零", 0),
    ("〇", 0),
    ("一", 1),
    ("壹", 1),
    ("二", 2),
    ("贰", 2),
    ("两", 2),
    ("三", 3),
    ("叁", 3),
    ("四", 4),
    ("肆", 4),
    ("五", 5),
    ("伍", 5),
    ("六", 6),
    ("陆", 6),
    ("七", 7),
    ("柒", 7),
    ("八", 8),
    ("捌", 8),
    ("九", 9),
];

const CN_UNITS: [(&str, i64); 5] = [
    ("十", 10),
    ("拾", 10),
    ("百", 100),
    ("佰", 100),
    ("千", 1000),
];

// 生成唯一ID
pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

// 生成随机邀请码
pub fn generate_invite_code() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    const CODE_LEN: usize = 8;

    let mut rng = rand::rng();
    let code: String = (0..CODE_LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    code
}

// 哈希密码
pub fn hash_password(password: &str) -> Result<String, AppError> {
    use argon2::{
        password_hash::{PasswordHasher, SaltString},
        Argon2,
    };
    use password_hash::rand_core::OsRng;

    let salt = SaltString::generate(OsRng);
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::Internal(format!("密码哈希失败: {}", e)))
}

// 验证密码
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    use argon2::{
        password_hash::{PasswordHash, PasswordVerifier},
        Argon2,
    };

    let parsed_hash =
        PasswordHash::new(hash).map_err(|e| AppError::Internal(format!("解析哈希失败: {}", e)))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

// 解析中文数字章节
pub fn parse_chinese_chapter_number(text: &str) -> Option<i64> {
    let text = text.trim();
    let mut result = 0;
    let mut temp = 0;
    let mut has_digit = false;

    for c in text.chars() {
        let c_str = c.to_string();

        // 查找数字
        if let Some(&(_, value)) = CN_NUMS.iter().find(|&&(s, _)| s == c_str) {
            temp = value;
            has_digit = true;
            continue;
        }

        // 查找单位
        if let Some(&(_, unit)) = CN_UNITS.iter().find(|&&(s, _)| s == c_str) {
            // 如果前面有数字，则为该数字乘以单位
            if temp > 0 {
                result += temp * unit;
            } else {
                // 否则单位前视为1（如"十一"中的"十"）
                result += unit;
            }
            temp = 0;
            continue;
        }
    }

    // 处理没有单位的情况（如末尾的个位数）
    if temp > 0 {
        result += temp;
    }

    if has_digit || result > 0 {
        Some(result)
    } else {
        None
    }
}

// 从章节标题中提取章节号
pub fn extract_chapter_number(title: &str) -> Option<i64> {
    // 先查找数字形式（如"第1章"）
    if let Some(capture) = Regex::new(r"第\s*(\d+)\s*[章节卷集部篇]")
        .ok()?
        .captures(title)
    {
        if let Some(num_str) = capture.get(1) {
            return i64::from_str(num_str.as_str()).ok();
        }
    }

    // 再查找中文数字形式（如"第一章"）
    if let Some(capture) = Regex::new(
        r"第\s*([零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+)\s*[章节卷集部篇]",
    )
    .ok()?
    .captures(title)
    {
        if let Some(num_str) = capture.get(1) {
            return parse_chinese_chapter_number(num_str.as_str());
        }
    }

    None
}

// 从文本中提取章节
pub fn extract_chapters(content: &str) -> Vec<(String, usize)> {
    let mut chapters = Vec::new();
    let mut lines = content.lines().enumerate();

    // 提取首行可能的标题
    if let Some((pos, first_line)) = lines.next() {
        let first_line = first_line.trim();
        if !first_line.is_empty() {
            chapters.push((first_line.to_string(), pos));
        }
    }

    // 使用正则匹配章节标题
    for (line_num, line) in lines {
        let line = line.trim();
        // Regex::new(r"(?<=[
        // \s])(?:序章|序言|卷首语|扉页|楔子|正文(?!完|结)|终章|后记|尾声|番外|第?\s{0,
        // 4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+?\s{0,4}(?:
        // 章|节(?!课)|卷|集(?![合和])|部(?![分赛游])|篇(?!张))).{0,30}$").expect("
        // 无效的章节正则表达式")
        if regex!(r#"^[　\s]((?:序章|序言|卷首语|扉页|楔子|正文(完|结)|终章|后记|尾声|番外|第?\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+?\s{0,4}(?:章|节(课)|卷|集([合和])|部([分赛游])|篇(张))).{0,30})$"#).is_match(line) {
            chapters.push((line.to_string(), line_num));
        }
    }

    chapters
}
