//! QQ SQLite 数据库访问层
//!
//! 表结构见 `schema` 模块

use crate::cache;
use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn default_db_path() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Tencent Files")
        .join("1497479966")
        .join("nt_qq")
        .join("nt_db")
        .join("nt_msg.db")
}

// ─── 数据模型 ──────────────────────────────────────────────

pub fn default_decrypted_db_path() -> Option<PathBuf> {
    // 优先使用 .archive 中的解密 DB（更大，包含更多历史消息）
    if let Some(downloads) = dirs::download_dir() {
        let archived = downloads
            .join("voile")
            .join(".archive")
            .join("nt_msg_decrypted.db");
        if archived.exists() {
            return Some(archived);
        }
    }
    dirs::download_dir().map(|downloads| downloads.join("voile").join("nt_msg_decrypted.db"))
}

pub fn detect_db_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("QQCLI_DB_PATH") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!("QQCLI_DB_PATH 指向的 DB 不存在: {}", path.display());
    }

    if let Some(decrypted) = default_decrypted_db_path() {
        if decrypted.exists() {
            return Ok(decrypted);
        }
    }

    let default = default_db_path();
    if default.exists() {
        return Ok(default);
    }

    if let Some(docs) = dirs::document_dir() {
        let base = docs.join("Tencent Files");
        if base.exists() {
            if let Ok(entries) = walkdir(&base) {
                for entry in entries {
                    if entry.ends_with("nt_msg.db") {
                        return Ok(entry);
                    }
                }
            }
        }
    }

    anyhow::bail!(
        "找不到 nt_msg.db，请确认 QQ NT 已运行过\n默认路径: {}",
        default.display()
    );
}

fn walkdir(base: &Path) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    let mut stack = vec![base.to_path_buf()];

    while let Some(curr) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&curr) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    results.push(path);
                }
            }
        }
    }

    Ok(results)
}

fn open_conn(path: &Path) -> Result<Connection> {
    // rusqlite::Connection is not Sync; keep connections request-scoped instead
    // of hiding one behind a global singleton.
    Connection::open(path).with_context(|| format!("无法打开 DB: {}", path.display()))
}

/// Open a rusqlite connection (public, used by db_index)
pub fn open_db(path: &Path) -> Result<Connection> {
    open_conn(path)
}

// UID 映射缓存: 加密 UID -> 真实 QQ号
static UID_CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, i64>>> =
    std::sync::OnceLock::new();
static DB_PATH_CACHE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn get_uid_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, i64>> {
    UID_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 从 nt_uid_mapping_table 加载所有映射
pub fn load_uid_mapping(path: &Path) -> Result<()> {
    // 缓存 DB 路径
    let _ = DB_PATH_CACHE.set(path.to_path_buf());

    let conn = open_conn(path)?;
    let mut stmt = conn.prepare(
        "SELECT schema::UID_MAPPING_ENC, schema::UID_MAPPING_QQ FROM nt_uid_mapping_table",
    )?;
    let mut rows = stmt.query([])?;
    let cache = get_uid_cache();
    let mut guard = cache.lock().map_err(|_| anyhow::anyhow!("lock poisoned"))?;

    let mut count = 0;
    while let Some(row) = rows.next()? {
        let uid: String = row.get(0)?;
        let qq: i64 = row.get(1)?;
        guard.insert(uid, qq);
        count += 1;
    }
    eprintln!("[qqcli] 加载 {} 个 UID 映射", count);
    Ok(())
}

// ─── 数据模型 ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub chat_type: String,
    pub is_group: bool,
    pub last_sender: String,
    pub last_content: String,
    pub last_type: String,
    pub timestamp: i64,
    pub unread: i64,
}

/// 内部消息格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub sender_id: i64,
    pub sender_name: String,
    pub content: String,
    pub msg_type: String,
    pub is_mine: bool,
    pub timestamp: i64,
    pub time_str: String,
}

impl Message {
    /// 转换为与 qq-data-exporter 兼容的 NormalizedMessage
    pub fn to_normalized(&self, chat_id: &str) -> NormalizedMessage {
        let ts = self.timestamp;
        let is_group = chat_id.parse::<i64>().is_err() || chat_id.starts_with("group:");

        NormalizedMessage {
            chat_type: if is_group {
                "group".to_string()
            } else {
                "private".to_string()
            },
            chat_id: chat_id.to_string(),
            group_id: if is_group {
                Some(chat_id.to_string())
            } else {
                None
            },
            peer_id: if !is_group {
                Some(chat_id.to_string())
            } else {
                None
            },
            chat_name: None,
            sender_id: self.sender_id.to_string(),
            sender_name: self.sender_name.clone(),
            sender_card: None,
            message_id: Some(self.id.to_string()),
            message_seq: None,
            timestamp_ms: ts * 1000,
            timestamp_iso: self.time_str.clone(),
            content: self.content.clone(),
            text_content: self.content.clone(),
            image_file_names: extract_image_names(&self.content),
            uploaded_file_names: vec![],
            emoji_tokens: extract_emoji_tokens(&self.content),
            segments: vec![NormalizedSegment {
                seg_type: self.msg_type.clone(),
                token: None,
                text: Some(self.content.clone()),
                file_name: None,
                path: None,
                md5: None,
            }],
            reply_to: None,
        }
    }
}

/// 从内容中提取图片文件名
fn extract_image_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in content.lines() {
        if let Some(start) = line.find("[图片]") {
            if let Some(name) = line
                .get(start + 4..)
                .and_then(|s| s.split_whitespace().next())
            {
                if name.ends_with(".jpg") || name.ends_with(".png") || name.ends_with(".gif") {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

/// 从内容中提取表情 token
fn extract_emoji_tokens(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for line in content.lines() {
        if line.contains("[表情") && line.contains("]") {
            if let Some(start) = line.find("[表情") {
                if let Some(end) = line[start..].find(']') {
                    let token = &line[start + 1..start + end];
                    tokens.push(token.to_string());
                }
            }
        }
    }
    tokens
}

/// 与 qq-data-exporter 兼容的消息格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMessage {
    pub chat_type: String, // "group" | "private"
    pub chat_id: String,
    #[serde(rename = "group_id", skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(rename = "peer_id", skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    pub chat_name: Option<String>,
    pub sender_id: String,
    pub sender_name: String,
    #[serde(rename = "sender_card", skip_serializing_if = "Option::is_none")]
    pub sender_card: Option<String>,
    #[serde(rename = "message_id", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(rename = "message_seq", skip_serializing_if = "Option::is_none")]
    pub message_seq: Option<String>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: i64,
    pub timestamp_iso: String,
    pub content: String,
    #[serde(rename = "text_content")]
    pub text_content: String,
    #[serde(rename = "image_file_names", default)]
    pub image_file_names: Vec<String>,
    #[serde(rename = "uploaded_file_names", default)]
    pub uploaded_file_names: Vec<String>,
    #[serde(rename = "emoji_tokens", default)]
    pub emoji_tokens: Vec<String>,
    #[serde(default)]
    pub segments: Vec<NormalizedSegment>,
    #[serde(rename = "reply_to", skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedSegment {
    #[serde(rename = "type")]
    pub seg_type: String,
    pub token: Option<String>,
    pub text: Option<String>,
    pub file_name: Option<String>,
    pub path: Option<String>,
    pub md5: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyRef {
    #[serde(rename = "referenced_message_id")]
    pub referenced_message_id: Option<String>,
    #[serde(rename = "referenced_sender_id")]
    pub referenced_sender_id: Option<String>,
    #[serde(rename = "referenced_timestamp")]
    pub referenced_timestamp: Option<String>,
    pub preview_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    pub uid: String,
    pub name: String,
    pub card: String,
}

// ─── 公开 API ─────────────────────────────────────────────

pub fn init_db() -> Result<PathBuf> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;

    let has_c2c = conn
        .query_row("SELECT COUNT(*) FROM c2c_msg_table", [], |_| Ok(()))
        .is_ok();

    if !has_c2c {
        anyhow::bail!("DB 格式不对，未找到 c2c_msg_table");
    }

    let c2c_count: i64 = conn.query_row("SELECT COUNT(*) FROM c2c_msg_table", [], |r| r.get(0))?;
    let group_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM dataline_msg_table", [], |r| r.get(0))?;

    println!("数据目录: {}", path.display());
    println!("私聊消息: {}", c2c_count);
    println!("群聊消息: {}", group_count);

    // 加载 UID 映射
    let _ = load_uid_mapping(&path);

    Ok(path)
}

pub fn list_sessions(limit: usize) -> Result<Vec<Session>> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;

    let mut sessions = Vec::new();

    // 私聊会话
    let mut stmt = conn.prepare(
        "SELECT schema::C2C_PEER_ID, schema::GROUP_NAME, MAX(schema::TIMESTAMP)
         FROM c2c_msg_table
         WHERE schema::TIMESTAMP > 0
         GROUP BY schema::C2C_PEER_ID, schema::GROUP_NAME
         ORDER BY MAX(schema::TIMESTAMP) DESC
         LIMIT ?",
    )?;

    let rows = stmt.query_map([limit as i64], |row| {
        let peer_id: i64 = row.get(0)?;
        let peer_id_str = peer_id.to_string();
        let name = cache::resolve_nickname(peer_id).unwrap_or_else(|| format!("uid_{}", peer_id));
        Ok(Session {
            id: peer_id_str,
            name,
            chat_type: "private".to_string(),
            is_group: false,
            last_sender: String::new(),
            last_content: String::new(),
            last_type: "文本".to_string(),
            timestamp: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            unread: 0,
        })
    })?;

    for r in rows.flatten() {
        sessions.push(r);
    }

    // 群聊会话
    let mut stmt = conn.prepare(
        "SELECT schema::GROUP_NAME, MAX(schema::TIMESTAMP)
         FROM dataline_msg_table
         WHERE schema::TIMESTAMP > 0
         GROUP BY schema::GROUP_NAME
         ORDER BY MAX(schema::TIMESTAMP) DESC
         LIMIT ?",
    )?;

    let rows = stmt.query_map([limit as i64], |row| {
        let group_name = row
            .get::<_, Option<String>>(0)?
            .unwrap_or_else(|| "未知群聊".to_string());
        Ok(Session {
            id: format!("group:{}", group_name),
            name: group_name,
            chat_type: "group".to_string(),
            is_group: true,
            last_sender: String::new(),
            last_content: String::new(),
            last_type: "文本".to_string(),
            timestamp: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            unread: 0,
        })
    })?;

    for r in rows.flatten() {
        sessions.push(r);
    }

    sessions.sort_by_key(|s| s.timestamp);
    sessions.reverse();
    sessions.truncate(limit);
    Ok(sessions)
}

pub fn get_messages(
    chat: &str,
    limit: usize,
    offset: usize,
    since_ts: Option<i64>,
    until_ts: Option<i64>,
    _msg_type: Option<&str>,
) -> Result<Vec<Message>> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;

    let ts_where = build_ts_where(since_ts, until_ts);
    let is_numeric = chat.chars().all(|c| c.is_ascii_digit()) && chat.len() > 5;

    let mut messages = Vec::new();

    if let Some(group_name) = chat.strip_prefix("group:") {
        // 群聊 (dataline)
        // schema::CONTENT 列存 GBK 编码字节，CAST AS BLOB 强制读取原始字节
        let sql = format!(
            "SELECT schema::MSG_ID,schema::GROUP_SENDER_ID,schema::C2C_SENDER_NAME,schema::CONTENT,schema::TIMESTAMP,schema::IS_SENDER_ME
             FROM dataline_msg_table
             WHERE schema::GROUP_NAME = ? {} AND schema::CONTENT IS NOT NULL
             ORDER BY schema::TIMESTAMP DESC
             LIMIT ? OFFSET ?",
            ts_where
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![group_name, limit as i64, offset as i64])?;

        while let Some(row) = rows.next()? {
            // 用 CAST AS BLOB 强制读取原始字节，避免 TEXT 列的 UTF-8 解码
            let content_ref = row.get_ref(3)?;
            let content_raw: Vec<u8> = match content_ref {
                rusqlite::types::ValueRef::Blob(bytes) => bytes.to_vec(),
                rusqlite::types::ValueRef::Text(bytes) => bytes.to_vec(),
                _ => vec![],
            };
            let sender_id: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            let ts: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let is_mine: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);

            let sender_name = cache::resolve_or_fallback(sender_id, format!("uid_{}", sender_id));

            messages.push(Message {
                id: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                sender_id,
                sender_name,
                content: extract_text(&content_raw),
                msg_type: detect_type(&content_raw),
                is_mine: is_mine == 1,
                timestamp: ts,
                time_str: fmt_ts(ts),
            });
        }
    } else {
        // 私聊 (c2c)
        let sql;
        let mut stmt;
        let mut rows;
        let name_pattern;

        if is_numeric {
            let peer_id = chat
                .parse::<i64>()
                .with_context(|| format!("会话 ID 不是合法数字: {}", chat))?;
            sql = format!(
                "SELECT schema::MSG_ID,schema::C2C_SENDER_ID,schema::C2C_SENDER_NAME,schema::CONTENT,schema::TIMESTAMP,schema::IS_SENDER_ME
                 FROM c2c_msg_table
                 WHERE schema::C2C_PEER_ID = ? {} AND schema::CONTENT IS NOT NULL
                 ORDER BY schema::TIMESTAMP DESC
                 LIMIT ? OFFSET ?",
                ts_where
            );
            stmt = conn.prepare(&sql)?;
            rows = stmt.query(params![peer_id, limit as i64, offset as i64])?;
        } else {
            name_pattern = format!("%{}%", chat);
            sql = format!(
                "SELECT schema::MSG_ID,schema::C2C_SENDER_ID,schema::C2C_SENDER_NAME,schema::CONTENT,schema::TIMESTAMP,schema::IS_SENDER_ME
                 FROM c2c_msg_table
                 WHERE schema::GROUP_NAME LIKE ? {} AND schema::CONTENT IS NOT NULL
                 ORDER BY schema::TIMESTAMP DESC
                 LIMIT ? OFFSET ?",
                ts_where
            );
            stmt = conn.prepare(&sql)?;
            rows = stmt.query(params![name_pattern, limit as i64, offset as i64])?;
        }

        while let Some(row) = rows.next()? {
            // 用 get_ref 获取原始字节（避免 UTF-8 解码破坏二进制数据）
            let content_ref = row.get_ref(3)?;
            let content_raw: Vec<u8> = match content_ref {
                rusqlite::types::ValueRef::Text(bytes) => bytes.to_vec(),
                rusqlite::types::ValueRef::Blob(bytes) => bytes.to_vec(),
                _ => vec![],
            };
            let sender_id: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            let ts: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
            let is_mine: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);

            let sender_name = cache::resolve_or_fallback(sender_id, format!("uid_{}", sender_id));

            messages.push(Message {
                id: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                sender_id,
                sender_name,
                content: extract_text(&content_raw),
                msg_type: detect_type(&content_raw),
                is_mine: is_mine == 1,
                timestamp: ts,
                time_str: fmt_ts(ts),
            });
        }
    }

    Ok(messages)
}

pub fn search_messages(
    keyword: &str,
    chat_filter: Option<&str>,
    limit: usize,
    since_ts: Option<i64>,
    until_ts: Option<i64>,
) -> Result<Vec<Message>> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;
    let ts_where = build_ts_where(since_ts, until_ts);

    let mut messages = Vec::new();
    let keyword_lower = keyword.to_lowercase();

    // 私聊
    let sql = format!(
        "SELECT schema::MSG_ID,schema::C2C_SENDER_ID,schema::C2C_SENDER_NAME,schema::CONTENT,schema::TIMESTAMP,schema::IS_SENDER_ME,schema::C2C_PEER_ID
         FROM c2c_msg_table
         WHERE schema::CONTENT IS NOT NULL {}
         ORDER BY schema::TIMESTAMP DESC",
        ts_where
    );

    if let Ok(mut stmt) = conn.prepare(&sql) {
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let content_ref = row.get_ref(3)?;
            let content_raw: Vec<u8> = match content_ref {
                rusqlite::types::ValueRef::Text(bytes) => bytes.to_vec(),
                rusqlite::types::ValueRef::Blob(bytes) => bytes.to_vec(),
                _ => vec![],
            };

            // 解析消息内容
            let content = extract_text(&content_raw);
            let content_lower = content.to_lowercase();

            // 用解析后的内容匹配 keyword
            if !content_lower.contains(&keyword_lower) {
                continue;
            }

            let peer_id: String = row
                .get::<_, Option<i64>>(6)?
                .map(|n| n.to_string())
                .unwrap_or_default();

            if chat_filter.map(|c| peer_id.contains(c)).unwrap_or(true) {
                let sender_id: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
                let ts: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
                let is_mine: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);
                let sender_name =
                    cache::resolve_or_fallback(sender_id, format!("uid_{}", sender_id));
                messages.push(Message {
                    id: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    sender_id,
                    sender_name,
                    content,
                    msg_type: detect_type(&content_raw),
                    is_mine: is_mine == 1,
                    timestamp: ts,
                    time_str: fmt_ts(ts),
                });
            }

            if messages.len() >= limit * 2 {
                break;
            }
        }
    }

    // 群聊
    let sql2 = format!(
        "SELECT schema::MSG_ID,schema::GROUP_SENDER_ID,schema::C2C_SENDER_NAME,schema::CONTENT,schema::TIMESTAMP,schema::IS_SENDER_ME,schema::GROUP_NAME
         FROM dataline_msg_table
         WHERE schema::CONTENT IS NOT NULL {}
         ORDER BY schema::TIMESTAMP DESC",
        ts_where
    );

    if let Ok(mut stmt) = conn.prepare(&sql2) {
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let content_ref = row.get_ref(3)?;
            let content_raw: Vec<u8> = match content_ref {
                rusqlite::types::ValueRef::Text(bytes) => bytes.to_vec(),
                rusqlite::types::ValueRef::Blob(bytes) => bytes.to_vec(),
                _ => vec![],
            };

            let content = extract_text(&content_raw);
            let content_lower = content.to_lowercase();

            if !content_lower.contains(&keyword_lower) {
                continue;
            }

            let group_id: String = row.get::<_, Option<String>>(6)?.unwrap_or_default();
            if chat_filter.map(|c| group_id.contains(c)).unwrap_or(true) {
                let sender_id: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
                let ts: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
                let is_mine: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);
                let sender_name =
                    cache::resolve_or_fallback(sender_id, format!("uid_{}", sender_id));
                messages.push(Message {
                    id: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    sender_id,
                    sender_name,
                    content,
                    msg_type: detect_type(&content_raw),
                    is_mine: is_mine == 1,
                    timestamp: ts,
                    time_str: fmt_ts(ts),
                });
            }

            if messages.len() >= limit * 2 {
                break;
            }
        }
    }

    messages.sort_by_key(|m| m.timestamp);
    messages.reverse();
    messages.truncate(limit);
    Ok(messages)
}

pub fn list_contacts(query: Option<&str>, limit: usize, kind: &str) -> Result<Vec<Contact>> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;
    let mut contacts = Vec::new();

    // 确保 UID 映射已加载
    let _ = load_uid_mapping(&path);

    // 加载联系人缓存用于昵称解析
    let cache = crate::cache::load_cache();

    if kind == "all" || kind == "friend" {
        // schema::C2C_PEER_ID = peer_id (数字 QQ), schema::GROUP_NAME = encrypted UID
        let sql = "SELECT DISTINCT schema::C2C_PEER_ID, schema::GROUP_NAME FROM c2c_msg_table";
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            // schema::C2C_PEER_ID 是真实 QQ 号
            let peer_id: i64 = row.get(0).unwrap_or(0);
            let encrypted_uid: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();

            // 用 schema::C2C_PEER_ID 作为 ID（真实 QQ）
            let id = peer_id.to_string();

            // 跳过自己
            if id == "1497479966" || peer_id == 0 {
                continue;
            }

            // 尝试用缓存解析昵称
            let name = if cache.is_some() {
                cache::resolve_or_fallback(peer_id, encrypted_uid.clone())
            } else {
                encrypted_uid
            };

            // 过滤查询
            if let Some(q) = query {
                if !id.contains(q) && !name.to_lowercase().contains(&q.to_lowercase()) {
                    continue;
                }
            }

            contacts.push(Contact {
                id,
                name,
                kind: "friend".to_string(),
            });

            if contacts.len() >= limit {
                break;
            }
        }
    }

    if kind == "all" || kind == "group" {
        if let Some(q) = query {
            let pattern = format!("%{}%", q);
            let sql = "SELECT DISTINCT schema::GROUP_NAME
                 FROM dataline_msg_table
                 WHERE schema::GROUP_NAME LIKE ?
                 LIMIT ?";
            let mut stmt = conn.prepare(sql)?;
            let mut rows = stmt.query(params![pattern, limit as i64])?;
            while let Some(row) = rows.next()? {
                let name: String = row
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "未知群聊".to_string());
                contacts.push(Contact {
                    id: format!("group:{}", name),
                    name,
                    kind: "group".to_string(),
                });
            }
        } else {
            let sql = "SELECT DISTINCT schema::GROUP_NAME FROM dataline_msg_table LIMIT ?";
            let mut stmt = conn.prepare(sql)?;
            let mut rows = stmt.query([limit as i64])?;
            while let Some(row) = rows.next()? {
                let name: String = row
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "未知群聊".to_string());
                contacts.push(Contact {
                    id: format!("group:{}", name),
                    name,
                    kind: "group".to_string(),
                });
            }
        }
    }

    contacts.truncate(limit);
    Ok(contacts)
}

pub fn get_unread_sessions(limit: usize) -> Result<Vec<Session>> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;

    let sql = "SELECT schema::C2C_PEER_ID, schema::GROUP_NAME, MAX(schema::TIMESTAMP)
               FROM c2c_msg_flow_table
               WHERE schema::FLOW_UNREAD = 0
               GROUP BY schema::C2C_PEER_ID, schema::GROUP_NAME
               ORDER BY MAX(schema::TIMESTAMP) DESC
               LIMIT ?";

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([limit as i64], |row| {
        let peer_id: i64 = row.get(0).unwrap_or(0);
        let peer_id_str = peer_id.to_string();
        let name = cache::resolve_nickname(peer_id).unwrap_or_else(|| format!("uid_{}", peer_id));
        Ok(Session {
            id: peer_id_str,
            name,
            chat_type: "private".to_string(),
            is_group: false,
            last_sender: String::new(),
            last_content: String::new(),
            last_type: "文本".to_string(),
            timestamp: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            unread: 0,
        })
    })?;

    let mut sessions = Vec::new();
    for r in rows.flatten() {
        sessions.push(r);
    }
    Ok(sessions)
}

pub fn get_group_members(group_id: &str) -> Result<Vec<GroupMember>> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;
    let group_name = group_id.strip_prefix("group:").unwrap_or(group_id);

    let sql = "SELECT DISTINCT schema::GROUP_MEMBER_UID, schema::C2C_SENDER_NAME
         FROM dataline_msg_table
         WHERE schema::GROUP_NAME = ? AND schema::GROUP_MEMBER_UID IS NOT NULL AND schema::GROUP_MEMBER_UID != 0";

    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params![group_name])?;
    let mut members = Vec::new();

    while let Some(row) = rows.next()? {
        let uid: i64 = row.get(0).unwrap_or(0);
        let uid = uid.to_string();
        let name: String = row
            .get::<_, Option<String>>(1)?
            .unwrap_or_else(|| uid.clone());
        members.push(GroupMember {
            uid: uid.trim_start_matches("u_").to_string(),
            name,
            card: String::new(),
        });
    }

    Ok(members)
}

pub fn get_stats(
    chat: Option<&str>,
    since_ts: Option<i64>,
    until_ts: Option<i64>,
) -> Result<serde_json::Value> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;
    let ts_where = build_ts_where(since_ts, until_ts);

    let (c2c_count, group_count, since_str, until_str) = if let Some(chat) = chat {
        let is_numeric = chat.chars().all(|c| c.is_ascii_digit()) && chat.len() > 5;
        if let Some(group_name) = chat.strip_prefix("group:") {
            let count_sql = format!(
                "SELECT COUNT(*) FROM dataline_msg_table WHERE schema::GROUP_NAME = ? {}",
                ts_where
            );
            let range_sql = format!(
                "SELECT MIN(schema::TIMESTAMP), MAX(schema::TIMESTAMP)
                 FROM dataline_msg_table
                 WHERE schema::GROUP_NAME = ? AND schema::TIMESTAMP > 0 {}",
                ts_where
            );
            let group_count = conn
                .query_row(&count_sql, params![group_name], |r| r.get(0))
                .unwrap_or(0);
            let (since_str, until_str) = conn
                .query_row(&range_sql, params![group_name], |r| {
                    Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
                })
                .unwrap_or((None, None));
            (0, group_count, since_str, until_str)
        } else if is_numeric {
            let peer_id = chat
                .parse::<i64>()
                .with_context(|| format!("会话 ID 不是合法数字: {}", chat))?;
            let count_sql = format!(
                "SELECT COUNT(*) FROM c2c_msg_table WHERE schema::C2C_PEER_ID = ? {}",
                ts_where
            );
            let range_sql = format!(
                "SELECT MIN(schema::TIMESTAMP), MAX(schema::TIMESTAMP)
                 FROM c2c_msg_table
                 WHERE schema::C2C_PEER_ID = ? AND schema::TIMESTAMP > 0 {}",
                ts_where
            );
            let c2c_count = conn
                .query_row(&count_sql, params![peer_id], |r| r.get(0))
                .unwrap_or(0);
            let (since_str, until_str) = conn
                .query_row(&range_sql, params![peer_id], |r| {
                    Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
                })
                .unwrap_or((None, None));
            (c2c_count, 0, since_str, until_str)
        } else {
            let pattern = format!("%{}%", chat);
            let count_sql = format!(
                "SELECT COUNT(*) FROM c2c_msg_table WHERE schema::GROUP_NAME LIKE ? {}",
                ts_where
            );
            let range_sql = format!(
                "SELECT MIN(schema::TIMESTAMP), MAX(schema::TIMESTAMP)
                 FROM c2c_msg_table
                 WHERE schema::GROUP_NAME LIKE ? AND schema::TIMESTAMP > 0 {}",
                ts_where
            );
            let c2c_count = conn
                .query_row(&count_sql, params![pattern], |r| r.get(0))
                .unwrap_or(0);
            let (since_str, until_str) = conn
                .query_row(&range_sql, params![pattern], |r| {
                    Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
                })
                .unwrap_or((None, None));
            (c2c_count, 0, since_str, until_str)
        }
    } else {
        let c2c_count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM c2c_msg_table WHERE 1=1 {}", ts_where),
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let group_count: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM dataline_msg_table WHERE 1=1 {}",
                    ts_where
                ),
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let range_sql = format!(
            "SELECT MIN(ts), MAX(ts)
             FROM (
                 SELECT schema::TIMESTAMP AS ts FROM c2c_msg_table WHERE schema::TIMESTAMP > 0 {}
                 UNION ALL
                 SELECT schema::TIMESTAMP AS ts FROM dataline_msg_table WHERE schema::TIMESTAMP > 0 {}
             )",
            ts_where, ts_where
        );
        let (since_str, until_str) = conn
            .query_row(&range_sql, [], |r| {
                Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?))
            })
            .unwrap_or((None, None));

        (c2c_count, group_count, since_str, until_str)
    };

    let date_range = match (since_str, until_str) {
        (Some(s), Some(u)) => serde_json::json!({ "since": fmt_ts(s), "until": fmt_ts(u) }),
        _ => serde_json::json!(null),
    };

    Ok(serde_json::json!({
        "c2c_count": c2c_count,
        "group_count": group_count,
        "total_messages": c2c_count + group_count,
        "date_range": date_range,
    }))
}

// ─── 内部辅助 ─────────────────────────────────────────────

fn build_ts_where(since_ts: Option<i64>, until_ts: Option<i64>) -> String {
    let mut parts = Vec::new();
    if let Some(s) = since_ts {
        parts.push(format!("AND schema::TIMESTAMP >= {}", s));
    }
    if let Some(u) = until_ts {
        parts.push(format!("AND schema::TIMESTAMP <= {}", u));
    }
    parts.join(" ")
}

/// 从 BLOB 中提取文本
/// 格式: [0x82] [0x16] [len:varint] [text:utf8]
pub fn extract_text_from_blob(data: &[u8]) -> String {
    if data.len() < 4 {
        return String::new();
    }

    // 扫描 [0x82][0x16] 模式
    let mut i = 0;
    while i < data.len() - 3 {
        if data[i] == 0x82 && data[i + 1] == 0x16 {
            // 读取 varint 长度
            let mut len: u64 = 0;
            let mut shift = 0;
            let mut j = i + 2;
            while j < data.len() && shift < 64 {
                let b = data[j];
                j += 1;
                len |= ((b & 0x7F) as u64) << shift;
                if (b & 0x80) == 0 {
                    break;
                }
                shift += 7;
            }

            // 读取文本
            if j + (len as usize) <= data.len() && len > 0 && len < 4096 {
                let text_bytes = &data[j..j + (len as usize)];

                // 尝试 UTF-8
                let text = String::from_utf8_lossy(text_bytes);
                let trimmed = text.trim();

                // 如果是可读文本（有中文字符或 ASCII）
                if !trimmed.is_empty() && trimmed.len() >= 2 {
                    let has_chinese = trimmed.chars().any(|c| {
                        let cp = c as u32;
                        matches!(cp, 0x4E00..=0x9FFF | 0x3000..=0x303F | 0xFF00..=0xFFEF)
                    });
                    let has_ascii_printable = trimmed
                        .chars()
                        .all(|c| c.is_ascii_graphic() || c as u32 > 0x9FFF);

                    if has_chinese || (has_ascii_printable && trimmed.len() >= 3) {
                        return trimmed.to_string();
                    }
                }
            }
        }
        i += 1;
    }

    String::new()
}

pub fn extract_text(raw: &[u8]) -> String {
    if raw.is_empty() {
        return String::new();
    }

    // 检测是否有 0x82 标记
    let has_0x82 = raw.contains(&0x82);

    if has_0x82 {
        let result = extract_text_from_blob_scanned(raw);
        if !result.is_empty() && result.len() > 3 {
            return result;
        }
    }

    // 尝试 GBK 解码
    let (decoded, _, _) = encoding_rs::GBK.decode(raw);
    let decoded = decoded.trim().to_string();
    if !decoded.is_empty() && decoded.len() > 3 {
        return decoded;
    }

    decoded
}

/// 扫描整个 BLOB 查找 [0x82][0x16][len][text] 二进制消息格式
fn extract_text_from_blob_scanned(data: &[u8]) -> String {
    if data.len() < 3 {
        return String::new();
    }
    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] == 0x82 && data[i + 1] == 0x16 {
            let result = extract_text_from_blob(&data[i..]);
            if !result.is_empty() {
                return result;
            }
        }
        i += 1;
    }
    String::new()
}

/// Try to read a column as BLOB; if it fails (TEXT stored in BLOB column),
/// fall back to reading as TEXT and converting to UTF-8 bytes.
pub(crate) fn get_either_blob_or_text(row: &rusqlite::Row<'_>, idx: usize) -> Vec<u8> {
    if let Ok(Some(v)) = row.get::<_, Option<Vec<u8>>>(idx) {
        return v;
    }
    if let Ok(Some(s)) = row.get::<_, Option<String>>(idx) {
        return s.into_bytes();
    }
    Vec::new()
}

pub fn detect_type(raw: &[u8]) -> String {
    if raw.is_empty() {
        return "未知".to_string();
    }
    // 如果是二进制 BLOB 格式开头
    if raw[0] == 0x82 && raw.get(1) == Some(&0x16) {
        // 检查是否有图片路径等特征
        let text = String::from_utf8_lossy(raw);
        if text.contains(".jpg") || text.contains(".png") || text.contains("Pic") {
            return "图片".to_string();
        }
        if text.contains(".amr") || text.contains(".silk") || text.contains(".mp3") {
            return "语音".to_string();
        }
        return "文本".to_string();
    }
    // 纯文本或 JSON
    let text = String::from_utf8_lossy(raw);
    if text.trim().starts_with('{') {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&text) {
            for type_field in &["elementType", "msgType", "type"] {
                if let Some(et) = j.get(type_field) {
                    if let Some(n) = et.as_i64() {
                        return match n {
                            1 => "文本".to_string(),
                            2 => "图片".to_string(),
                            3 => "语音".to_string(),
                            4 => "视频".to_string(),
                            5 => "表情".to_string(),
                            6 | 7 => "文件".to_string(),
                            8 => "位置".to_string(),
                            _ => format!("type-{}", n),
                        };
                    }
                }
            }
        }
    }
    "文本".to_string()
}

pub fn fmt_ts(ts: i64) -> String {
    if ts == 0 {
        return "未知".to_string();
    }
    if let Some(dt) = DateTime::from_timestamp(ts, 0) {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        ts.to_string()
    }
}

pub fn debug_tables() -> Result<()> {
    let path = detect_db_path()?;
    let conn = open_conn(&path)?;

    // 列出所有表
    let mut stmt =
        conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    println!("=== Tables ===");
    for t in &tables {
        println!("  {}", t);
    }

    // 检查 c2c_msg_table 结构
    if tables.iter().any(|t| t == "c2c_msg_table") {
        println!("\n=== c2c_msg_table columns ===");
        let mut stmt = conn.prepare("PRAGMA table_info(c2c_msg_table)")?;
        let cols: Vec<(i64, String, String, i64, Option<String>, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (cid, name, ctype, notnull, dflt, pk) in cols {
            println!(
                "  [{}] {} {} NOT_NULL={} DEFAULT={:?} PK={}",
                cid, name, ctype, notnull, dflt, pk
            );
        }

        // 查找有非空BLOB的记录
        println!("\n=== Find records with non-empty BLOBs ===");
        let mut stmt = conn.prepare(
            "SELECT schema::MSG_ID,schema::TIMESTAMP,schema::IS_SENDER_ME,schema::CONTENT,[40900],[40600] FROM c2c_msg_table WHERE schema::CONTENT IS NOT NULL LIMIT 10"
        )?;
        let mut rows = stmt.query([])?;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            let f40001: i64 = row.get(0)?;
            let f40050: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            let f40009: i64 = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
            // 40800 might be TEXT or BLOB; try blob first, fall back to text
            let f40800 = get_either_blob_or_text(row, 3);
            let f40900 = get_either_blob_or_text(row, 4);
            let f40600 = get_either_blob_or_text(row, 5);
            if !f40800.is_empty() || !f40900.is_empty() || !f40600.is_empty() {
                println!(
                    "Row {}: schema::MSG_ID={}, schema::TIMESTAMP={}, schema::IS_SENDER_ME={}",
                    count, f40001, f40050, f40009
                );
                println!(
                    "  schema::CONTENT len={} hex[0..20]={:?}",
                    f40800.len(),
                    &f40800[..f40800.len().min(20)]
                );
                println!(
                    "  [40900] len={} hex[0..20]={:?}",
                    f40900.len(),
                    &f40900[..f40900.len().min(20)]
                );
                println!(
                    "  [40600] len={} hex[0..20]={:?}",
                    f40600.len(),
                    &f40600[..f40600.len().min(20)]
                );
            }
            count += 1;
            if count >= 50 {
                break;
            }
        }
        println!("Checked {} rows", count);

        // 同时检查 schema::CONTENT 作为 TEXT 的非空内容
        println!("\n=== Check schema::CONTENT as TEXT with non-ASCII content ===");
        let mut stmt = conn.prepare(
            "SELECT schema::MSG_ID,schema::TIMESTAMP,schema::C2C_SENDER_NAME,schema::CONTENT FROM c2c_msg_table WHERE schema::CONTENT IS NOT NULL AND length(schema::CONTENT) > 0 LIMIT 5"
        )?;
        let mut rows = stmt.query([])?;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            let f40001: i64 = row.get(0)?;
            let f40050: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            let f40021: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            let f40800: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
            let has_non_ascii = f40800.chars().any(|c| !c.is_ascii() || c == '\u{FFFD}');
            println!(
                "Row {}: schema::MSG_ID={}, schema::TIMESTAMP={}, schema::C2C_SENDER_NAME='{}', schema::CONTENT len={}, has_replacement={}",
                count,
                f40001,
                f40050,
                f40021,
                f40800.len(),
                has_non_ascii
            );
            if has_non_ascii {
                println!(
                    "  First 50 chars: {:?}",
                    &f40800.chars().take(50).collect::<String>()
                );
            }
            count += 1;
        }
    }

    // 检查群聊表
    if tables.iter().any(|t| t == "dataline_msg_table") {
        println!("\n=== dataline_msg_table columns ===");
        let mut stmt = conn.prepare("PRAGMA table_info(dataline_msg_table)")?;
        let cols: Vec<(i64, String, String, i64, Option<String>, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (cid, name, ctype, notnull, dflt, pk) in cols {
            println!(
                "  [{}] {} {} NOT_NULL={} DEFAULT={:?} PK={}",
                cid, name, ctype, notnull, dflt, pk
            );
        }
    }

    Ok(())
}

pub fn debug_probe() -> Result<()> {
    use rusqlite::types::ValueRef;

    let path = detect_db_path()?;
    let conn = open_conn(&path)?;

    println!("=== Probing c2c_msg_table BLOB columns ===\n");

    // 找一个有内容的记录，探测所有列
    let mut stmt = conn.prepare("SELECT * FROM c2c_msg_table LIMIT 1")?;
    let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut rows = stmt.query([])?;

    if let Some(row) = rows.next()? {
        for (i, col_name) in col_names.iter().enumerate() {
            let val = row.get_ref(i)?;
            match val {
                ValueRef::Null => println!("[{}] {}: NULL", i, col_name),
                ValueRef::Integer(n) => println!("[{}] {}: INTEGER = {}", i, col_name, n),
                ValueRef::Real(n) => println!("[{}] {}: REAL = {}", i, col_name, n),
                ValueRef::Text(s) => {
                    let s = std::str::from_utf8(s).unwrap_or("(invalid utf8)");
                    if s.len() > 100 {
                        println!(
                            "[{}] {}: TEXT(len={}) = '{}...'",
                            i,
                            col_name,
                            s.len(),
                            &s[..100]
                        );
                    } else {
                        println!("[{}] {}: TEXT = '{}'", i, col_name, s);
                    }
                }
                ValueRef::Blob(b) => {
                    let hex: Vec<String> =
                        b.iter().take(30).map(|&x| format!("{:02x}", x)).collect();
                    println!(
                        "[{}] {}: BLOB(len={}) hex[0..30]=[{}]",
                        i,
                        col_name,
                        b.len(),
                        hex.join(" ")
                    );
                }
            }
        }
    }

    println!("\n=== Trying to find TEXT content ===");
    // 找包含非 ASCII 字符的记录
    let mut stmt = conn.prepare(
        "SELECT schema::MSG_ID,schema::TIMESTAMP,schema::C2C_SENDER_NAME,[40090],[40093],schema::CONTENT FROM c2c_msg_table LIMIT 10"
    )?;
    let mut rows = stmt.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let ts: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
        let name: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
        let f40090: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
        let f40093: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
        let f40800: String = row.get::<_, Option<String>>(5)?.unwrap_or_default();

        // 检查是否有可打印内容
        let has_content = !f40090.is_empty() || !f40093.is_empty() || !f40800.is_empty();
        let marker = if has_content { "***" } else { "" };

        println!("\n[{}] {} {} {}", marker, id, ts, name);
        println!(
            "  [40090] len={} '{}'",
            f40090.len(),
            &f40090[..f40090.len().min(80)]
        );
        println!(
            "  [40093] len={} '{}'",
            f40093.len(),
            &f40093[..f40093.len().min(80)]
        );
        println!("  schema::CONTENT len={}", f40800.len());

        count += 1;
        if count >= 10 {
            break;
        }
    }

    Ok(())
}

pub fn parse_ts(s: &str) -> Result<i64> {
    use chrono::{NaiveDate, Utc};

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp());
    }
    if let Ok(nd) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(dt) = Utc
            .from_local_datetime(&nd.and_hms_opt(0, 0, 0).unwrap())
            .single()
        {
            return Ok(dt.timestamp());
        }
    }
    if let Ok(ts) = s.parse::<i64>() {
        return Ok(ts);
    }
    anyhow::bail!("无法解析时间: {}", s)
}
