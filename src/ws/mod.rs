//! 阅读会话的 WebSocket 通道：替代旧的每秒轮询心跳接口，降低 QPS。
//!
//! - [`protocol`]：客户端/服务端消息协议
//! - [`manager`]：连接注册表与广播
//! - [`handler`]：ws 升级握手与连接主循环

pub mod handler;
pub mod manager;
pub mod protocol;
