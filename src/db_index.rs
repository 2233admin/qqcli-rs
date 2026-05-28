//! DuckDB FTS 索引 — 将 QQ 消息批量导入 DuckDB，支持全文搜索

use crate::cache::{self, ContactCache};
use crate::db;
use anyhow::{Context, Result};
use duckdb::{params, Connection};
use std::path::PathBuf;

const DB_NAME: &str = "messages.duckdb";

/// 返回 DuckDB 文件路径
pub fn get_path() -> Result<PathBuf> {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("qqcli");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("创建目录失败: {}", dir.display()))?;
    Ok(dir.join(DB_NAME))
}

/// 初始化 DuckDB 表结构
pub fn init_db(path: &PathBuf) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        CREATE SEQUENCE IF NOT EXISTS msg_id_seq;

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY DEFAULT nextval('msg_id_seq'),
            msg_id BIGINT,
            chat_id TEXT,
            chat_type TEXT,
            sender_id BIGINT,
            sender_name TEXT,
            content TEXT,
            timestamp BIGINT,
            time_str TEXT,
            msg_type TEXT,
            is_mine BOOLEAN DEFAULT FALSE
        );
        "#,
    )
    .context("创建 DuckDB 表失败")?;
    Ok(())
}

/// 批量导入所有私聊和群聊消息到 DuckDB
/// 返回导入的消息总数
pub fn import_all(sqlite_path: &PathBuf, _cache: &ContactCache) -> Result<usize> {
    let duckdb_path = get_path()?;
    init_db(&duckdb_path)?;

    let src_conn = db::open_db(sqlite_path)?;
    let duck_conn = Connection::open(&duckdb_path)?;

    // 清空旧数据（幂等重跑）
    duck_conn.execute("DELETE FROM messages", [])?;

    let mut count = 0;

    // ── 私聊 ──
    println!("导入私聊消息...");
    {
        let sql_c2c = "SELECT [40001], [40033], [40050], [40800] FROM c2c_msg_table WHERE [40800] IS NOT NULL";
        let mut stmt = src_conn.prepare(sql_c2c)?;
        let mut rows = stmt.raw_query();

        let tx = duck_conn.unchecked_transaction()?;
        let mut insert = tx.prepare_cached(
            "INSERT INTO messages (msg_id, chat_id, chat_type, sender_id, sender_name, content, timestamp, time_str, msg_type, is_mine) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )?;

        while let Some(row) = rows.next()? {
            let msg_id: i64 = row.get(0)?;
            let peer_id: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            let timestamp: i64 = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
            let content_raw: Vec<u8> = db::get_either_blob_or_text(row, 3);

            let content = db::extract_text(&content_raw);
            let sender_name = cache::resolve_or_fallback(peer_id, format!("uid_{}", peer_id));
            let time_str = db::fmt_ts(timestamp);

            insert.execute(params![
                msg_id,
                peer_id.to_string(),
                "private",
                peer_id,
                sender_name,
                content,
                timestamp,
                time_str,
                "text",
                false
            ])?;
            count += 1;

            if count % 10000 == 0 {
                println!("  已导入 {} 条...", count);
            }
        }

        drop(insert);
        tx.commit()?;
    }

    // ── 群聊 ──
    println!("导入群聊消息...");
    {
        let sql_group = "SELECT [40001], [40020], [40050], [40800] FROM dataline_msg_table WHERE [40800] IS NOT NULL";
        let mut stmt = src_conn.prepare(sql_group)?;
        let mut rows = stmt.raw_query();

        let tx = duck_conn.unchecked_transaction()?;
        let mut insert = tx.prepare_cached(
            "INSERT INTO messages (msg_id, chat_id, chat_type, sender_id, sender_name, content, timestamp, time_str, msg_type, is_mine) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )?;

        while let Some(row) = rows.next()? {
            let msg_id: i64 = row.get(0)?;
            let group_name_raw: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            let timestamp: i64 = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
            let content_raw: Vec<u8> = db::get_either_blob_or_text(row, 3);

            let content = db::extract_text(&content_raw);
            let time_str = db::fmt_ts(timestamp);

            insert.execute(params![
                msg_id,
                group_name_raw.clone(),
                "group",
                0i64,
                group_name_raw,
                content,
                timestamp,
                time_str,
                "text",
                false
            ])?;
            count += 1;

            if count % 10000 == 0 {
                println!("  已导入 {} 条...", count);
            }
        }

        drop(insert);
        tx.commit()?;
    }

    // ── 去重 + 建索引 ──
    println!("去重并建立索引...");
    duck_conn.execute_batch(
        r#"
        CREATE SEQUENCE IF NOT EXISTS msg_dedup_seq;
        CREATE TABLE IF NOT EXISTS messages_dedup AS
            SELECT DISTINCT msg_id, chat_id, chat_type, sender_id, sender_name, content, timestamp, time_str, msg_type, is_mine
            FROM messages;
        DROP TABLE messages;
        ALTER TABLE messages_dedup RENAME TO messages;
        ALTER TABLE messages ADD COLUMN id INTEGER DEFAULT nextval('msg_dedup_seq');
        ALTER TABLE messages ALTER COLUMN id SET NOT NULL;
        ALTER TABLE messages ALTER COLUMN id SET DEFAULT nextval('msg_dedup_seq');
        CREATE UNIQUE INDEX IF NOT EXISTS idx_msg_id ON messages(msg_id);
        CREATE INDEX IF NOT EXISTS idx_timestamp ON messages(timestamp);
        CREATE INDEX IF NOT EXISTS idx_chat_id ON messages(chat_id);
        "#,
    )?;

    println!("导入完成: {} 条消息 (去重后)", count);
    Ok(count)
}

/// DuckDB 全文搜索
pub fn search(query: &str, chat_id: Option<&str>, limit: usize) -> Result<Vec<SearchResult>> {
    let path = get_path()?;
    if !path.exists() {
        anyhow::bail!("请先运行 qq index");
    }

    let conn = Connection::open(&path)?;

    let sql = if chat_id.is_some() {
        "SELECT msg_id, chat_id, chat_type, sender_id, sender_name, content, timestamp, time_str
         FROM messages
         WHERE content LIKE ? AND chat_id = ?
         ORDER BY timestamp DESC
         LIMIT ?"
    } else {
        "SELECT msg_id, chat_id, chat_type, sender_id, sender_name, content, timestamp, time_str
         FROM messages
         WHERE content LIKE ?
         ORDER BY timestamp DESC
         LIMIT ?"
    };

    let pattern = format!("%{}%", query);
    let mut results = Vec::new();

    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(cid) = chat_id {
        stmt.query(params![pattern, cid, limit as i64])?
    } else {
        stmt.query(params![pattern, limit as i64])?
    };

    let mut rows = rows;
    while let Some(row) = rows.next()? {
        results.push(SearchResult {
            msg_id: row.get(0)?,
            chat_id: row.get(1)?,
            chat_type: row.get(2)?,
            sender_id: row.get(3)?,
            sender_name: row.get(4)?,
            content: row.get(5)?,
            timestamp: row.get(6)?,
            time_str: row.get(7)?,
        });
    }

    Ok(results)
}

#[derive(Debug)]
pub struct SearchResult {
    pub msg_id: i64,
    pub chat_id: String,
    pub chat_type: String,
    pub sender_id: i64,
    pub sender_name: String,
    pub content: String,
    pub timestamp: i64,
    pub time_str: String,
}
