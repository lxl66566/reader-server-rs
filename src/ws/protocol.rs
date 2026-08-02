use serde::{Deserialize, Serialize};

/// 客户端 → 服务端消息
///
/// 约定：建立连接后必须先 `subscribe` 携带 `device_id` 进入某本书的阅读会话，
/// 之后才能发 `heartbeat`。切换书籍时再次 `subscribe` 即可（自动退订旧书）。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// 订阅某本书的进度更新；同时声明本连接的 `device_id`
    Subscribe { book_id: i64, device_id: String },
    /// 心跳：上报当前阅读位置
    Heartbeat { book_id: i64, position: i64 },
    /// 退订（离开阅读页）
    Unsubscribe { book_id: i64 },
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
        position: i64,
        reading_time: i64,
    },
    /// 进度更新广播：同账号下另一台设备刚刚更新了进度，
    /// 本设备收到后应跳转到 `position` 以保持多端同步
    ProgressUpdate {
        book_id: i64,
        position: i64,
        reading_time: i64,
        device_id: String,
    },
    /// 退订成功
    Unsubscribed { book_id: i64 },
    /// 书签变更通知：客户端收到后重新拉取书签列表
    ///（HTTP 接口不感知发起方设备，发起方可通过 X-Device-Id 头被排除）
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
