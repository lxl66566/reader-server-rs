//! WebSocket 连接管理器
//!
//! 维护 `(user_id, book_id, device_id) -> mpsc 发送端` 的映射，
//! 提供 O(1) 订阅/退订与"按 `(user, book)` 维度广播"能力。
//! 广播用于多设备进度同步：当某设备心跳更新进度成功时，
//! 通知同账号下订阅了同本书的其他设备拉取/跳转进度。

use std::{collections::HashMap, sync::Arc};

use tokio::sync::{RwLock, mpsc};
use tracing::warn;

use super::protocol::ServerMessage;

/// 每个连接的出站队列容量。
///
/// 取舍：正常客户端的出站消息只有心跳响应与低频广播（进度同步/书签批注变更），
/// 64 条缓冲绰绰有余；单条消息为百字节级 JSON，64 条约几十 KB/连接，
/// 即便大量慢客户端堆积也不会撑大内存。宁可断开只收不读的客户端，
/// 也不让单连接队列无界增长。
pub const OUTBOUND_CAPACITY: usize = 64;

/// 单条待发送消息的通道；接收方在连接主循环里消费并发往 ws socket。
/// `Clone` 后多端共享同一队列，因此注册到 registry 时直接 clone 即可。
pub type ClientTx = mpsc::Sender<ServerMessage>;

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
    ///
    /// 队列满（客户端只收不读）或已断开的订阅会被驱逐出注册表，之后不再收到广播；
    /// 对端连接会在其下一次回包失败或 idle 超时时被断开（见 handler）。
    pub async fn broadcast_progress(
        &self,
        user_id: i64,
        book_id: i64,
        exclude_device: Option<&str>,
        msg: ServerMessage,
    ) {
        let mut failed = Vec::new();
        {
            let guard = self.conns.read().await;
            for ((u, b, d), tx) in guard.iter() {
                if *u == user_id && *b == book_id && (exclude_device != Some(d.as_str())) {
                    match tx.try_send(msg.clone()) {
                        Ok(()) => {},
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            warn!(user_id, book_id, device_id = %d, "evict slow ws client: outbound queue full");
                            failed.push((user_id, book_id, d.clone()));
                        },
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            failed.push((user_id, book_id, d.clone()));
                        },
                    }
                }
            }
        }
        if !failed.is_empty() {
            let mut guard = self.conns.write().await;
            for key in failed {
                guard.remove(&key);
            }
        }
    }

    /// 当前活跃连接数（主要用于测试与监控）
    #[cfg(test)]
    pub async fn len(&self) -> usize {
        self.conns.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg() -> ServerMessage {
        ServerMessage::error("test")
    }

    #[tokio::test]
    async fn broadcast_delivers_only_to_matching_book() {
        let registry = ConnectionRegistry::new();
        let (tx1, mut rx1) = mpsc::channel(OUTBOUND_CAPACITY);
        let (tx2, mut rx2) = mpsc::channel(OUTBOUND_CAPACITY);
        registry.subscribe((1, 1, "a".into()), tx1).await;
        registry.subscribe((1, 2, "b".into()), tx2).await;

        registry.broadcast_progress(1, 1, None, msg()).await;

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_err());
    }

    #[tokio::test]
    async fn broadcast_excludes_source_device() {
        let registry = ConnectionRegistry::new();
        let (tx1, mut rx1) = mpsc::channel(OUTBOUND_CAPACITY);
        let (tx2, mut rx2) = mpsc::channel(OUTBOUND_CAPACITY);
        registry.subscribe((1, 1, "src".into()), tx1).await;
        registry.subscribe((1, 1, "other".into()), tx2).await;

        registry.broadcast_progress(1, 1, Some("src"), msg()).await;

        assert!(rx1.try_recv().is_err());
        assert!(rx2.try_recv().is_ok());
    }

    /// 队列满（客户端只收不读）的订阅会被驱逐出注册表，之后不再收到任何广播
    #[tokio::test]
    async fn broadcast_evicts_slow_client() {
        let registry = ConnectionRegistry::new();
        let (tx_slow, _rx_slow) = mpsc::channel(OUTBOUND_CAPACITY);
        // 塞满队列但不消费，模拟只收不读的慢客户端
        for _ in 0..OUTBOUND_CAPACITY {
            tx_slow.try_send(msg()).unwrap();
        }
        registry.subscribe((1, 1, "slow".into()), tx_slow).await;

        registry.broadcast_progress(1, 1, None, msg()).await;

        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn broadcast_evicts_closed_client() {
        let registry = ConnectionRegistry::new();
        let (tx, rx) = mpsc::channel::<ServerMessage>(OUTBOUND_CAPACITY);
        registry.subscribe((1, 1, "gone".into()), tx).await;
        drop(rx);

        registry.broadcast_progress(1, 1, None, msg()).await;

        assert_eq!(registry.len().await, 0);
    }
}
