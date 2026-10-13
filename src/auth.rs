use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

use crate::{config::Config, error::AppError, models::UserRole};

// JWT 声明结构
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // 用户ID
    pub exp: u64,     // 过期时间 (Unix 时间戳)
    pub iat: u64,     // 颁发时间 (Unix 时间戳)
    pub role: String, // 角色: "user" 或 "admin"
    pub ver: i64,     // token 版本号（用于吊销）
}

// 为Claims实现方法
impl Claims {
    // 创建用户JWT声明
    pub fn new_user(user_id: i64, token_version: i64, config: &Config) -> Self {
        let now = Utc::now();
        let expiration =
            i64::try_from(config.jwt.expiration).expect("JWT expiration must fit in i64");
        let expiry = now + Duration::seconds(expiration);
        Self {
            sub: user_id.to_string(),
            iat: timestamp_u64(&now),
            exp: timestamp_u64(&expiry),
            role: "user".to_string(),
            ver: token_version,
        }
    }

    // 创建管理员JWT声明（携带 admins.token_version，改密后旧 token 立即失效）
    pub fn new_admin(admin_id: i64, token_version: i64, config: &Config) -> Self {
        let now = Utc::now();
        let expiration = i64::try_from(config.jwt.admin_expiration)
            .expect("JWT admin expiration must fit in i64");
        let expiry = now + Duration::seconds(expiration);
        Self {
            sub: admin_id.to_string(),
            iat: timestamp_u64(&now),
            exp: timestamp_u64(&expiry),
            role: "admin".to_string(),
            ver: token_version,
        }
    }
}

/// 将 `DateTime` 的 timestamp 从 i64 安全转为 u64。
/// 在 1970 年之后的正常业务场景下不会失败。
fn timestamp_u64(dt: &DateTime<Utc>) -> u64 {
    u64::try_from(dt.timestamp()).expect("timestamp must be non-negative")
}

// 创建JWT令牌
pub fn create_token(claims: &Claims, config: &Config) -> Result<String, AppError> {
    let encoding_key = EncodingKey::from_secret(config.jwt.secret.as_bytes());
    encode(&Header::default(), claims, &encoding_key).map_err(AppError::Jwt)
}

// 验证JWT令牌
pub fn verify_token(token: &str, config: &Config) -> Result<Claims, AppError> {
    let decoding_key = DecodingKey::from_secret(config.jwt.secret.as_bytes());
    let validation = Validation::default();

    decode::<Claims>(token, &decoding_key, &validation)
        .map(|data| data.claims)
        // decode 失败均源自客户端提供的 token 内容（结构垃圾/伪造签名/过期/算法不匹配）；
        // HS256 的 DecodingKey 构造不可失败，此路径不存在服务端内部错误，统一按 401 处理
        .map_err(|_| AppError::TokenExpiredOrInvalid)
}

// 提取用户的认证中间件
pub struct AuthUser {
    pub user_id: i64,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Config: FromRef<S>,
    Pool<Sqlite>: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let config = Config::from_ref(state);
        let pool = Pool::<Sqlite>::from_ref(state);

        // 从请求头获取 Authorization
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AppError::TokenNotProvided)?;

        let claims = verify_token(bearer.token(), &config)?;

        if claims.role != UserRole::User.as_str() {
            return Err(AppError::InvalidTokenRole);
        }

        let user_id: i64 = claims.sub.parse().map_err(|_| AppError::InvalidUserId)?;

        // 验证用户存在、token 版本号匹配、账户未被封禁
        crate::access::check_user_token(&pool, user_id, claims.ver).await?;

        Ok(AuthUser { user_id })
    }
}

// 提取管理员的认证中间件
pub struct AuthAdmin {
    pub admin_id: i64,
    pub username: String,
}

impl<S> FromRequestParts<S> for AuthAdmin
where
    S: Send + Sync,
    Config: FromRef<S>,
    Pool<Sqlite>: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let config = Config::from_ref(state);
        let pool = Pool::<Sqlite>::from_ref(state);

        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AppError::AdminTokenNotProvided)?;

        let claims = verify_token(bearer.token(), &config)?;

        if claims.role != UserRole::Admin.as_str() {
            return Err(AppError::AdminPermissionRequired);
        }

        let admin_id: i64 = claims.sub.parse().map_err(|_| AppError::InvalidAdminId)?;

        // 查询管理员用户名与 token 版本号（同时验证存在性）；版本号不匹配视为已吊销
        let admin = sqlx::query!(
            "SELECT username, token_version FROM admin WHERE id = ?",
            admin_id
        )
        .fetch_optional(&pool)
        .await
        .map_err(AppError::Database)?;

        match admin {
            None => Err(AppError::AdminNotFound),
            Some(row) if row.token_version != claims.ver => Err(AppError::TokenExpiredOrInvalid),
            Some(row) => Ok(AuthAdmin {
                admin_id,
                username: row.username,
            }),
        }
    }
}
