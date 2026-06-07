//! QQ NT 数据库 Schema 定义
//!
//! 字段 ID 来源: QQ NT 本地数据库 (nt_msg.db) 反向工程
//! 版本兼容性: 基于 QQ NT 3.x - 4.x 测试
//!
//! 表结构:
//!   - c2c_msg_table: 私聊消息
//!   - dataline_msg_table: 群聊消息
//!   - c2c_msg_flow_table: 消息流/未读状态
//!   - nt_uid_mapping_table: UID ↔ 真实QQ号映射

#![allow(dead_code)]

// ─── 通用字段 ───────────────────────────────────────────────

/// 消息 ID (全局唯一)
pub const MSG_ID: &str = "[40001]";
/// Unix 时间戳 (秒)
pub const TIMESTAMP: &str = "[40050]";
/// 是否自己发送 (0/1)
pub const IS_SENDER_ME: &str = "[40009]";
/// 消息内容载荷 (BLOB/TEXT，含 fallback)
pub const CONTENT: &str = "[40800]";
/// 内容载荷原始 (某些版本)
pub const CONTENT_ORIG: &str = "[40090]";
/// 内容载荷备用 (某些版本)
pub const CONTENT_ALT: &str = "[40093]";
/// 附加载荷1
pub const PAYLOAD_1: &str = "[40900]";
/// 附加载荷2
pub const PAYLOAD_2: &str = "[40600]";

// ─── C2C 私聊表 (c2c_msg_table) ────────────────────────────

/// 发送者 ID (数字 QQ 号)
pub const C2C_SENDER_ID: &str = "[40030]";
/// 接收方 ID (peer_id, 数字 QQ 号)
pub const C2C_PEER_ID: &str = "[40033]";
/// 发送者昵称
pub const C2C_SENDER_NAME: &str = "[40021]";
/// 加密 UID (已弃用，用 nt_uid_mapping_table)
pub const C2C_UID: &str = "[40020]";

// ─── 群聊表 (dataline_msg_table) ───────────────────────────

/// 群号/群名
pub const GROUP_NAME: &str = "[40020]";
/// 发送者 ID (数字 QQ 号)
pub const GROUP_SENDER_ID: &str = "[40005]";
/// 成员 UID (群消息)
pub const GROUP_MEMBER_UID: &str = "[40006]";
/// 发送者昵称
pub const GROUP_SENDER_NAME: &str = "[40021]";

// ─── 未读状态表 (c2c_msg_flow_table) ────────────────────────

/// 未读标记 (0=未读)
pub const FLOW_UNREAD: &str = "[40026]";

// ─── UID 映射表 (nt_uid_mapping_table) ──────────────────────

/// 加密 UID
pub const UID_MAPPING_ENC: &str = "[48902]";
/// UID 映射条目 ID (auto-increment primary key)
pub const UID_MAPPING_ID: &str = "[48901]";
/// 真实姓名 / 昵称
pub const UID_MAPPING_NAME: &str = "[48912]";
/// 真实 QQ 号
pub const UID_MAPPING_QQ: &str = "[1002]";
