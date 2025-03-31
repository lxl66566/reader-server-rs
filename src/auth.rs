use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

use crate::{config::Config, error::AppError, models::User};

// JWT 声明结构
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // 用户ID
    pub exp: usize,   // 过期时间
    pub iat: usize,   // 颁发时间
    pub role: String, // 角色: "user" 或 "admin"
}

// 为Claims实现方法
impl Claims {
    // 创建用户JWT声明
    pub fn new_user(user_id: i64, config: &Config) -> Self {
        let now = Utc::now();
        let expiry = now + Duration::seconds(config.jwt.expiration as i64);
        Self {
            sub: user_id.to_string(),
            iat: now.timestamp() as usize,
            exp: expiry.timestamp() as usize,
            role: "user".to_string(),
        }
    }

    // 创建管理员JWT声明
    pub fn new_admin(admin_id: i64, config: &Config) -> Self {
        let now = Utc::now();
        let expiry = now + Duration::seconds(config.jwt.admin_expiration as i64);
        Self {
            sub: admin_id.to_string(),
            iat: now.timestamp() as usize,
            exp: expiry.timestamp() as usize,
            role: "admin".to_string(),
        }
    }
}

// 创建JWT令牌
pub fn create_token(claims: &Claims, config: &Config) -> Result<String, AppError> {
    let encoding_key = EncodingKey::from_secret(config.jwt.secret.as_bytes());
    encode(&Header::default(), claims, &encoding_key).map_err(|e| AppError::Jwt(e))
}

// 验证JWT令牌
pub fn verify_token(token: &str, config: &Config) -> Result<Claims, AppError> {
    let decoding_key = DecodingKey::from_secret(config.jwt.secret.as_bytes());
    let validation = Validation::default();

    decode::<Claims>(token, &decoding_key, &validation)
        .map(|data| data.claims)
        .map_err(|e| AppError::Jwt(e))
}

// 提取用户的认证中间件
pub struct AuthUser {
    pub user_id: i64,
    pub created_at: DateTime<Utc>,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Config: FromRef<S>,
    Pool<Sqlite>: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 从状态获取配置
        let config = Config::from_ref(state);

        // 从请求头获取Authorization
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AppError::Auth("未提供授权令牌".to_string()))?;

        // 验证令牌
        let claims = verify_token(bearer.token(), &config)?;

        // 验证角色
        if claims.role != "user" {
            return Err(AppError::Auth("令牌角色无效".to_string()));
        }

        // 获取用户ID
        let user_id: i64 = claims
            .sub
            .parse()
            .map_err(|_| AppError::Auth("无效的用户ID".to_string()))?;

        // 获取数据库连接
        let pool = Pool::<Sqlite>::from_ref(state);

        // 验证用户是否存在
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&pool)
            .await
            .map_err(AppError::Database)?
            .ok_or_else(|| AppError::NotFound("用户不存在".to_string()))?;

        Ok(AuthUser {
            user_id,
            created_at: user
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| panic!("无法将其 parse 为 UTC 时间: {}", user.created_at)),
        })
    }
}

// 提取管理员的认证中间件
pub struct AuthAdmin {
    pub admin_id: i64,
}

impl<S> FromRequestParts<S> for AuthAdmin
where
    S: Send + Sync,
    Config: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 从状态获取配置
        let config = Config::from_ref(state);

        // 从请求头获取Authorization
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AppError::Auth("未提供管理员授权令牌".to_string()))?;

        // 验证令牌
        let claims = verify_token(bearer.token(), &config)?;

        // 验证角色
        if claims.role != "admin" {
            return Err(AppError::Forbidden("需要管理员权限".to_string()));
        }

        // 获取管理员ID
        let admin_id: i64 = claims
            .sub
            .parse()
            .map_err(|_| AppError::Auth("无效的管理员ID".to_string()))?;

        Ok(AuthAdmin { admin_id })
    }
}
