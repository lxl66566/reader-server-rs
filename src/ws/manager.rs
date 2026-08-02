//! WebSocket 连接管理器
//!
//! 维护 `(user_id, book_id, device_id) -> mpsc 发送端` 的映射，
//! 提供 O(1) 订阅/退订与"按 `(user, book)` 维度广播"能力。
//! 广播用于多设备进度同步：当某设备心跳更新进度成功时，
//! 通知同账号下订阅了同本书的其他设备拉取/跳转进度。

use std::{collections::HashMap, sync::Arc};

use tokio::sync::{RwLock, mpsc};

use super::protocol::ServerMessage;

/// 单条待发送消息的通道；接收方在连接主循环里消费并发往 ws socket。
/// `Clone` 后多端共享同一队列，因此注册到 registry 时直接 clone 即可。
pub type ClientTx = mpsc::UnboundedSender<ServerMessage>;

/// 连接的唯一标识：`(user_id, book_id, device_id)`
///
/// 设计说明：一个 ws 连接在生命周期内可订阅多本书（前端切书场景），
/// 但对同一 `(user, book)` 不允许同 `device_id` 重复订阅——
/// 前端生成 `device_id` 时应保证不同标签页/窗口各异。
pub type ConnKey = (i64, i64, String);

#[derive(Default)]
pub struct ConnectionRegistry {
    conns: RwLock<HashMap<ConnKey, ClientTx>>,
}

impl ConnectionRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 注册一个连接；若同 key 已存在（旧连接未清理），覆盖之
    pub async fn subscribe(&self, key: ConnKey, tx: ClientTx) {
        let mut guard = self.conns.write().await;
        guard.insert(key, tx);
    }

    /// 注销一个连接
    pub async fn unsubscribe(&self, key: &ConnKey) {
        let mut guard = self.conns.write().await;
        guard.remove(key);
    }

    /// 广播进度更新给同 `(user, book)` 下的所有设备（可选排除某个 device，
    /// 通常排除发起心跳的设备本身）。
    pub async fn broadcast_progress(
        &self,
        user_id: i64,
        book_id: i64,
        exclude_device: Option<&str>,
        msg: ServerMessage,
    ) {
        let guard = self.conns.read().await;
        for ((u, b, d), tx) in guard.iter() {
            if *u == user_id
                && *b == book_id
                && exclude_device.map_or(true, |ex| d.as_str() != ex)
            {
                let _ = tx.send(msg.clone());
            }
        }
    }

    /// 当前活跃连接数（主要用于测试与监控）
    #[cfg(test)]
    pub async fn len(&self) -> usize {
        self.conns.read().await.len()
    }
}
