use serde::{Deserialize, Serialize};

/// 输入长度/字符集限制（协议层统一校验，防止超长输入撑大注册表与 DB）
pub mod limits {
    /// `device_id` 最大长度（字符数，字符集限定 ASCII 故等于字节数）
    pub const DEVICE_ID_MAX_CHARS: usize = 64;
    /// `locator` 最大长度（字节数，4KB 足够覆盖 EpubCFI 等最长格式）
    pub const LOCATOR_MAX_BYTES: usize = 4 * 1024;
}

/// 客户端 → 服务端消息
///
/// 约定：建立连接后必须先 `subscribe` 携带 `device_id` 进入某本书的阅读会话，
/// 之后才能发 `heartbeat`。订阅新书不会自动退订旧书；旧订阅通过显式
/// `unsubscribe` 或断开连接清理。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// 订阅某本书的进度更新；同时声明本连接的 `device_id`
    Subscribe { book_id: i64, device_id: String },
    /// 心跳：上报当前阅读位置（locator + percent）
    ///
    /// `locator`：与书籍 format 相关的位置字符串
    ///   - txt：字符偏移数字字符串（如 "1234"）
    ///   - epub：EpubCFI 字符串
    ///   - pdf：页码字符串（如 "page:42"，从 0 起）
    ///
    /// `percent`：0.0-1.0，前端计算的精确阅读进度，用于服务端列表展示
    Heartbeat {
        book_id: i64,
        locator: String,
        percent: f64,
    },
    /// 退订（离开阅读页）
    Unsubscribe { book_id: i64 },
}

impl ClientMessage {
    /// 协议层输入校验；失败返回用于 error 帧的描述
    pub fn validate(&self) -> Result<(), String> {
        use limits::*;
        match self {
            Self::Subscribe { device_id, .. } => {
                if device_id.is_empty() || device_id.len() > DEVICE_ID_MAX_CHARS {
                    return Err(format!("device_id must be 1-{DEVICE_ID_MAX_CHARS} chars"));
                }
                if !device_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    return Err("device_id may only contain [A-Za-z0-9_-]".to_string());
                }
            },
            Self::Heartbeat { locator, .. } => {
                if locator.is_empty() {
                    return Err("locator must not be empty".to_string());
                }
                if locator.len() > LOCATOR_MAX_BYTES {
                    return Err(format!("locator exceeds {LOCATOR_MAX_BYTES} bytes"));
                }
            },
            Self::Unsubscribe { .. } => {},
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscribe(device_id: &str) -> ClientMessage {
        ClientMessage::Subscribe {
            book_id: 1,
            device_id: device_id.to_string(),
        }
    }

    fn heartbeat(locator: &str) -> ClientMessage {
        ClientMessage::Heartbeat {
            book_id: 1,
            locator: locator.to_string(),
            percent: 0.1,
        }
    }

    #[test]
    fn validate_device_id() {
        assert!(subscribe("device-1_aZ9").validate().is_ok());
        assert!(subscribe("").validate().is_err());
        assert!(subscribe(" ").validate().is_err()); // 空白不在字符集白名单内
        assert!(subscribe("含中文").validate().is_err());
        assert!(subscribe(&"x".repeat(65)).validate().is_err());
        assert!(subscribe(&"x".repeat(64)).validate().is_ok());
    }

    #[test]
    fn validate_locator() {
        assert!(heartbeat("100").validate().is_ok());
        assert!(heartbeat("").validate().is_err());
        assert!(heartbeat(&"x".repeat(4 * 1024)).validate().is_ok());
        assert!(heartbeat(&"x".repeat(4 * 1024 + 1)).validate().is_err());
    }
}

/// 服务端 → 客户端消息
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// 订阅成功
    Subscribed { book_id: i64 },
    /// 心跳响应：客户端据此判断是否需要同步到服务端进度
    HeartbeatAck {
        book_id: i64,
        synced: bool,
        /// 本次心跳是否真正写入了服务端进度。
        /// false 时（并发竞争失败/首跳让位）`locator`/`percent` 是服务端现值，
        /// 客户端可据此区分「已保存」与「被丢弃」
        updated: bool,
        locator: String,
        percent: f64,
        reading_time: i64,
    },
    /// 进度更新广播：同账号下另一台设备刚刚更新了进度，
    /// 本设备收到后应跳转到 `locator` 以保持多端同步
    ProgressUpdate {
        book_id: i64,
        locator: String,
        percent: f64,
        reading_time: i64,
        device_id: String,
    },
    /// 退订成功
    Unsubscribed { book_id: i64 },
    /// 书签变更通知：客户端收到后重新拉取书签列表
    /// （HTTP 接口不感知发起方设备，发起方可通过 X-Device-Id 头被排除）
    BookmarkChanged {
        book_id: i64,
        /// `created` / `deleted`（更新目前走 delete+recreate 也可）
        action: String,
        bookmark_id: i64,
    },
    /// 批注变更通知：客户端收到后重新拉取批注列表
    AnnotationChanged {
        book_id: i64,
        /// `created` / `updated` / `deleted`
        action: String,
        annotation_id: i64,
    },
    /// 业务/协议错误（连接不会关闭，客户端可继续发消息）
    Error { message: String },
}

impl ServerMessage {
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}
