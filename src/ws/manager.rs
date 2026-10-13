//! WebSocket 连接管理器
//!
//! 维护 `(user_id, book_id, device_id) -> (mpsc 发送端, 订阅令牌)` 的映射，
//! 提供 O(1) 订阅/退订与"按 `(user, book)` 维度广播"能力。
//! 广播用于多设备进度同步：当某设备心跳更新进度成功时，
//! 通知同账号下订阅了同本书的其他设备拉取/跳转进度。

use std::{collections::HashMap, sync::Arc};

use tokio::sync::{RwLock, mpsc};
use tracing::warn;
use uuid::Uuid;

use super::protocol::ServerMessage;

/// 每个连接的出站队列容量。
///
/// 取舍：正常客户端的出站消息只有心跳响应与低频广播（进度同步/书签批注变更），
/// 64 条缓冲绰绰有余；单条消息为百字节级 JSON，64 条约几十 KB/连接，
/// 即便大量慢客户端堆积也不会撑大内存。宁可断开只收不读的客户端，
/// 也不让单连接队列无界增长。
pub const OUTBOUND_CAPACITY: usize = 64;

/// 每用户 ws 连接数上限。
///
/// 订阅数上限只约束单连接内的订阅，恶意客户端仍可无界开连接占住
/// 注册表与内存；正常使用（多设备 + 若干标签页）远不会触及该上限。
/// 超限时新连接在升级后立即被服务端关闭。
pub const MAX_CONNS_PER_USER: usize = 8;

/// 单条待发送消息的通道；接收方在连接主循环里消费并发往 ws socket。
/// `Clone` 后多端共享同一队列，因此注册到 registry 时直接 clone 即可。
pub type ClientTx = mpsc::Sender<ServerMessage>;

/// 连接的唯一标识：`(user_id, book_id, device_id)`
///
/// 设计说明：一个 ws 连接在生命周期内可订阅多本书（前端切书场景），
/// 但对同一 `(user, book)` 不允许同 `device_id` 重复订阅——
/// 前端生成 `device_id` 时应保证不同标签页/窗口各异。
pub type ConnKey = (i64, i64, String);

/// 订阅令牌：每个 ws 连接在生命周期内唯一。
///
/// 同一 key（含同 `device_id` 双开）可能先后被多个连接注册，
/// 覆盖/驱逐/退订仅在令牌匹配（即注册项仍属于该连接）时生效，
/// 防止旧连接的清理误伤已接管该 key 的新连接。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnToken(Uuid);

impl ConnToken {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConnToken {
    fn default() -> Self {
        Self::new()
    }
}

/// 单条注册项：出站发送端 + 订阅令牌（属主标记）
#[derive(Clone)]
struct Registration {
    tx: ClientTx,
    token: ConnToken,
}

#[derive(Default)]
pub struct ConnectionRegistry {
    conns: RwLock<HashMap<ConnKey, Registration>>,
    /// user_id → 活跃连接数。std Mutex：临界区极短且不含 await，
    /// 连接结束的释放走 Drop，无法用异步锁
    conn_counts: std::sync::Mutex<HashMap<i64, usize>>,
}

impl ConnectionRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 注册一个连接；若同 key 已存在（旧连接未清理），覆盖之。
    /// 同令牌重复注册（心跳自愈，见 handler）为幂等操作，不写回
    pub async fn subscribe(&self, key: ConnKey, token: ConnToken, tx: ClientTx) {
        let mut guard = self.conns.write().await;
        if let Some(reg) = guard.get(&key)
            && reg.token == token
        {
            return;
        }
        guard.insert(key, Registration { tx, token });
    }

    /// 注销一个连接；仅当注册项仍属于该令牌时删除，
    /// 避免误删接管了同 key 的新连接
    pub async fn unsubscribe(&self, key: &ConnKey, token: &ConnToken) {
        let mut guard = self.conns.write().await;
        if guard.get(key).is_some_and(|reg| &reg.token == token) {
            guard.remove(key);
        }
    }

    /// 连接计数 +1；超出每用户上限返回 false（调用方应立即断开连接）。
    /// 判定与自增在同一临界区内完成，无 check-then-act 竞争
    pub fn acquire_connection(&self, user_id: i64) -> bool {
        let mut counts = self.conn_counts.lock().expect("conn_counts poisoned");
        let count = counts.entry(user_id).or_insert(0);
        if *count >= MAX_CONNS_PER_USER {
            return false;
        }
        *count += 1;
        true
    }

    /// 连接结束计数 -1；归零时移除键，避免计数表随历史用户无界增长
    pub fn release_connection(&self, user_id: i64) {
        let mut counts = self.conn_counts.lock().expect("conn_counts poisoned");
        if let Some(count) = counts.get_mut(&user_id) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&user_id);
            }
        }
    }

    /// 广播进度更新给同 `(user, book)` 下的所有设备（可选排除某个 device，
    /// 通常排除发起心跳的设备本身）。
    ///
    /// 队列满（客户端只收不读）或已断开的订阅会被驱逐出注册表，暂停其广播。
    /// 被驱逐但连接仍存活的客户端会在下一次心跳时重注册（自愈，见 handler）；
    /// 若其队列持续满，连接会因心跳回包失败而断开。
    pub async fn broadcast_progress(
        &self,
        user_id: i64,
        book_id: i64,
        exclude_device: Option<&str>,
        msg: ServerMessage,
    ) {
        // (key, token)：删除时校验属主，防止扫描后注册项被新连接接管而误删
        let mut failed: Vec<(ConnKey, ConnToken)> = Vec::new();
        {
            let guard = self.conns.read().await;
            for (key, reg) in guard.iter() {
                let (u, b, d) = key;
                if *u == user_id && *b == book_id && (exclude_device != Some(d.as_str())) {
                    match reg.tx.try_send(msg.clone()) {
                        Ok(()) => {},
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            warn!(user_id, book_id, device_id = %d, "evict slow ws client: outbound queue full");
                            failed.push((key.clone(), reg.token.clone()));
                        },
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            failed.push((key.clone(), reg.token.clone()));
                        },
                    }
                }
            }
        }
        if !failed.is_empty() {
            let mut guard = self.conns.write().await;
            for (key, token) in failed {
                if guard.get(&key).is_some_and(|reg| reg.token == token) {
                    guard.remove(&key);
                }
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
        registry
            .subscribe((1, 1, "a".into()), ConnToken::new(), tx1)
            .await;
        registry
            .subscribe((1, 2, "b".into()), ConnToken::new(), tx2)
            .await;

        registry.broadcast_progress(1, 1, None, msg()).await;

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_err());
    }

    #[tokio::test]
    async fn broadcast_excludes_source_device() {
        let registry = ConnectionRegistry::new();
        let (tx1, mut rx1) = mpsc::channel(OUTBOUND_CAPACITY);
        let (tx2, mut rx2) = mpsc::channel(OUTBOUND_CAPACITY);
        registry
            .subscribe((1, 1, "src".into()), ConnToken::new(), tx1)
            .await;
        registry
            .subscribe((1, 1, "other".into()), ConnToken::new(), tx2)
            .await;

        registry.broadcast_progress(1, 1, Some("src"), msg()).await;

        assert!(rx1.try_recv().is_err());
        assert!(rx2.try_recv().is_ok());
    }

    /// 队列满（客户端只收不读）的订阅会被驱逐出注册表，直到重注册恢复
    #[tokio::test]
    async fn broadcast_evicts_slow_client() {
        let registry = ConnectionRegistry::new();
        let (tx_slow, _rx_slow) = mpsc::channel(OUTBOUND_CAPACITY);
        // 塞满队列但不消费，模拟只收不读的慢客户端
        for _ in 0..OUTBOUND_CAPACITY {
            tx_slow.try_send(msg()).unwrap();
        }
        registry
            .subscribe((1, 1, "slow".into()), ConnToken::new(), tx_slow)
            .await;

        registry.broadcast_progress(1, 1, None, msg()).await;

        assert_eq!(registry.len().await, 0);
    }

    /// 慢客户端被驱逐后，队列排空 + 心跳同令牌重注册 → 恢复接收广播（自愈）
    #[tokio::test]
    async fn resubscribe_restores_broadcast_after_eviction() {
        let registry = ConnectionRegistry::new();
        let (tx, mut rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let key = (1, 1, "dev".into());
        let token = ConnToken::new();
        registry
            .subscribe(key.clone(), token.clone(), tx.clone())
            .await;

        // 塞满队列不消费 → 广播时被驱逐
        for _ in 0..OUTBOUND_CAPACITY {
            tx.try_send(msg()).unwrap();
        }
        registry.broadcast_progress(1, 1, None, msg()).await;
        assert_eq!(registry.len().await, 0);

        // 客户端恢复（排空队列），下一次心跳重注册
        while rx.try_recv().is_ok() {}
        registry.subscribe(key, token, tx).await;
        registry.broadcast_progress(1, 1, None, msg()).await;
        assert!(rx.try_recv().is_ok());
    }

    /// 同 device_id 双连接：旧连接断开时的退订不得误删新连接的注册
    #[tokio::test]
    async fn unsubscribe_ignores_stale_token() {
        let registry = ConnectionRegistry::new();
        let (tx1, _rx1) = mpsc::channel(OUTBOUND_CAPACITY);
        let (tx2, mut rx2) = mpsc::channel(OUTBOUND_CAPACITY);
        let key = (1, 1, "dev".into());
        let token1 = ConnToken::new();
        registry.subscribe(key.clone(), token1.clone(), tx1).await;
        let token2 = ConnToken::new();
        registry.subscribe(key.clone(), token2, tx2).await;

        // 旧连接（token1）断开清理，此时 key 已被新连接接管
        registry.unsubscribe(&key, &token1).await;
        assert_eq!(registry.len().await, 1);

        registry.broadcast_progress(1, 1, None, msg()).await;
        assert!(rx2.try_recv().is_ok());
    }

    /// 同令牌重复注册（心跳自愈）幂等：不覆盖、不产生多余注册
    #[tokio::test]
    async fn resubscribe_with_same_token_is_idempotent() {
        let registry = ConnectionRegistry::new();
        let (tx, _rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let key = (1, 1, "dev".into());
        let token = ConnToken::new();
        registry
            .subscribe(key.clone(), token.clone(), tx.clone())
            .await;
        registry.subscribe(key, token, tx).await;
        assert_eq!(registry.len().await, 1);
    }

    /// 连接计数：上限内放行、超限拒绝、释放后名额恢复
    #[test]
    fn connection_limit_enforced_and_released() {
        let registry = ConnectionRegistry::new();
        for _ in 0..MAX_CONNS_PER_USER {
            assert!(registry.acquire_connection(7));
        }
        assert!(!registry.acquire_connection(7));
        // 其他用户不受影响
        assert!(registry.acquire_connection(8));
        registry.release_connection(8);
        // 释放一个名额后恢复放行
        registry.release_connection(7);
        assert!(registry.acquire_connection(7));
    }

    #[tokio::test]
    async fn broadcast_evicts_closed_client() {
        let registry = ConnectionRegistry::new();
        let (tx, rx) = mpsc::channel::<ServerMessage>(OUTBOUND_CAPACITY);
        registry
            .subscribe((1, 1, "gone".into()), ConnToken::new(), tx)
            .await;
        drop(rx);

        registry.broadcast_progress(1, 1, None, msg()).await;

        assert_eq!(registry.len().await, 0);
    }
}
