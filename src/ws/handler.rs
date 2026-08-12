//! WebSocket 升级 handler 与连接生命周期管理
//!
//! 认证：浏览器原生 WebSocket API 无法设置自定义请求头，
//! 故 token 通过 query 参数 `?token=xxx` 传递；非浏览器客户端
//! 也可继续用 `Authorization: Bearer ...` 头。
//!
//! 连接生命周期：
//! 1. 升级握手 + 鉴权
//! 2. 主循环 select! 四路事件：客户端消息、待发送队列、定时 ping、idle 超时
//! 3. 任一端断开 → 退出循环 → 清理注册表中的订阅

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{
    app::AppState,
    auth::verify_token,
    error::AppError,
    routes::reading::process_heartbeat_logic,
    ws::protocol::{ClientMessage, ServerMessage},
};

/// 连接保活：超过该时长收不到任何客户端消息即判定为僵尸连接
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// 服务端主动 ping 间隔（应明显小于 [`IDLE_TIMEOUT`]）
const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Deserialize, Default)]
pub struct WsAuthQuery {
    pub token: Option<String>,
}

pub async fn ws_handler(
    State(state): State<AppState>,
    auth_header: Option<TypedHeader<Authorization<Bearer>>>,
    Query(query): Query<WsAuthQuery>,
    ws: axum::extract::WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    // 优先 Authorization 头（非浏览器/测试场景），其次 query token（浏览器）
    let token = auth_header
        .map(|TypedHeader(Authorization(bearer))| bearer.token().to_string())
        .or(query.token)
        .ok_or(AppError::TokenNotProvided)?;

    let claims = verify_token(&token, &state.config)?;
    if claims.role != "user" {
        return Err(AppError::InvalidTokenRole);
    }
    let user_id: i64 = claims.sub.parse().map_err(|_| AppError::InvalidUserId)?;

    // 复用 AuthUser 的鉴权逻辑：验证用户存在、token 版本号、封禁状态
    let user = sqlx::query!(
        "SELECT token_version, status FROM users WHERE id = ?",
        user_id
    )
    .fetch_optional(&state.db)
    .await?;

    match user {
        None => Err(AppError::UserNotFound),
        Some(row) if row.token_version != claims.ver => Err(AppError::TokenExpiredOrInvalid),
        Some(row) if row.status == "banned" => Err(AppError::UserBanned),
        Some(_) => Ok(ws.on_upgrade(move |socket| handle_socket(socket, user_id, state))),
    }
}

/// 单个连接的会话状态：本连接持有的所有订阅 key
///
/// 一个连接可订阅多本书（前端切书）；同一连接订阅新书前会先退订旧书，
/// 但如果前端希望同标签页切换书籍、保留旧订阅也可以——只要 `device_id` 一致。
struct Session {
    user_id: i64,
    /// `book_id` -> `device_id`（本连接订阅 book 时声明的 `device_id`）
    subs: HashMap<i64, String>,
}

impl Session {
    fn new(user_id: i64) -> Self {
        Self {
            user_id,
            subs: HashMap::new(),
        }
    }

    /// 构造 [`ConnKey`] 用于注册/注销
    fn key(&self, book_id: i64) -> Option<(i64, i64, String)> {
        self.subs
            .get(&book_id)
            .map(|d| (self.user_id, book_id, d.clone()))
    }
}

async fn handle_socket(socket: axum::extract::ws::WebSocket, user_id: i64, state: AppState) {
    let (mut sink, mut stream) = socket.split();
    // 每个连接一个无界通道：广播方 send，主循环 recv 后通过 sink 发出。
    // UnboundedSender: Clone 后共享队列，因此注册到 registry 时 clone 即可。
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerMessage>();

    let mut session = Session::new(user_id);
    let mut ping_interval = tokio::time::interval(PING_INTERVAL);
    // interval 首次 tick 立即完成，跳过避免连接刚建立就发 ping
    ping_interval.tick().await;

    info!(user_id, "ws connected");

    loop {
        tokio::select! {
            // 接收客户端消息
            msg = stream.next() => match msg {
                Some(Ok(m)) => match m {
                    axum::extract::ws::Message::Text(text) => {
                        handle_text(text.as_str(), &state, &out_tx, &mut session).await;
                    }
                    axum::extract::ws::Message::Binary(b) => {
                        // 兼容二进制承载的 JSON
                        match std::str::from_utf8(&b) {
                            Ok(s) => handle_text(s, &state, &out_tx, &mut session).await,
                            Err(_) => {
                                let _ = out_tx.send(ServerMessage::error(
                                    "binary payload must be valid UTF-8 JSON",
                                ));
                            }
                        }
                    }
                    axum::extract::ws::Message::Close(_) => break,
                    // Ping/Pong 由底层自动处理
                    axum::extract::ws::Message::Ping(_) | axum::extract::ws::Message::Pong(_) => {}
                },
                Some(Err(e)) => {
                    warn!(user_id, error = %e, "ws recv error");
                    break;
                }
                None => break,
            },

            // 待发送消息 → 推给 socket
            msg = out_rx.recv() => match msg {
                Some(server_msg) => {
                    let json = match serde_json::to_string(&server_msg) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(user_id, error = %e, "serialize server message failed");
                            continue;
                        }
                    };
                    if sink.send(axum::extract::ws::Message::text(json)).await.is_err() {
                        break;
                    }
                }
                None => break,
            },

            // 定时 ping 保活
            _ = ping_interval.tick() => {
                if sink
                    .send(axum::extract::ws::Message::Ping(bytes::Bytes::new()))
                    .await
                    .is_err()
                {
                    break;
                }
            }

            // 长时间无任何上行消息（无心跳、无 pong）→ 视为僵尸连接
            () = tokio::time::sleep(IDLE_TIMEOUT) => {
                warn!(user_id, "ws idle timeout, closing");
                break;
            }
        }
    }

    // 清理所有订阅
    for book_id in session.subs.keys().copied().collect::<Vec<_>>() {
        if let Some(key) = session.key(book_id) {
            state.ws.unsubscribe(&key).await;
        }
    }
    info!(user_id, "ws disconnected");
}

/// 处理一条文本消息：反序列化 → 派发到对应业务
async fn handle_text(
    text: &str,
    state: &AppState,
    out_tx: &mpsc::UnboundedSender<ServerMessage>,
    session: &mut Session,
) {
    let msg: ClientMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            let _ = out_tx.send(ServerMessage::error(format!("invalid message: {e}")));
            return;
        },
    };

    match msg {
        ClientMessage::Subscribe { book_id, device_id } => {
            if device_id.trim().is_empty() {
                let _ = out_tx.send(ServerMessage::error("device_id must not be empty"));
                return;
            }
            // 已订阅同书 → 视为重复订阅，先退订再用新 device_id 注册
            // （通常不会发生，但防御性处理）
            if let Some(old_key) = session.key(book_id) {
                state.ws.unsubscribe(&old_key).await;
            }
            let key = (session.user_id, book_id, device_id.clone());
            // clone 出一个 sender 给 registry：广播方通过它把消息塞进本连接的接收队列
            state.ws.subscribe(key, out_tx.clone()).await;
            session.subs.insert(book_id, device_id);
            let _ = out_tx.send(ServerMessage::Subscribed { book_id });
        },
        ClientMessage::Heartbeat {
            book_id,
            locator,
            percent,
        } => {
            let Some(device_id) = session.subs.get(&book_id).cloned() else {
                let _ = out_tx.send(ServerMessage::error(
                    "not subscribed to this book; send subscribe first",
                ));
                return;
            };
            match process_heartbeat_logic(
                &state.db,
                session.user_id,
                book_id,
                &locator,
                percent,
                &device_id,
            )
            .await
            {
                Ok(result) => {
                    let _ = out_tx.send(ServerMessage::HeartbeatAck {
                        book_id,
                        synced: result.synced,
                        locator: result.locator.clone(),
                        percent: result.percent,
                        reading_time: result.reading_time,
                    });
                    // 仅当进度真正被更新时才广播给其他设备
                    if result.updated {
                        state
                            .ws
                            .broadcast_progress(
                                session.user_id,
                                book_id,
                                Some(&device_id),
                                ServerMessage::ProgressUpdate {
                                    book_id,
                                    locator: result.locator,
                                    percent: result.percent,
                                    reading_time: result.reading_time,
                                    device_id: device_id.clone(),
                                },
                            )
                            .await;
                    }
                },
                Err(e) => {
                    let _ = out_tx.send(ServerMessage::error(e.to_string()));
                },
            }
        },
        ClientMessage::Unsubscribe { book_id } => {
            if let Some(key) = session.key(book_id) {
                state.ws.unsubscribe(&key).await;
            }
            session.subs.remove(&book_id);
            let _ = out_tx.send(ServerMessage::Unsubscribed { book_id });
        },
    }
}
