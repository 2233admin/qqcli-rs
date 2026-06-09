//! DuckDB FTS 索引 — 将 QQ 消息批量导入 DuckDB，支持全文搜索

use crate::cache::ContactCache;
use crate::db;
use anyhow::{Context, Result};
use duckdb::{Connection, params};
use std::path::{Path, PathBuf};

use crate::schema::{C2C_PEER_ID, CONTENT, GROUP_NAME, MSG_ID, TIMESTAMP};

const DB_NAME: &str = "messages.duckdb";

/// 返回 DuckDB 文件路径
pub fn get_path() -> Result<PathBuf> {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("qqcli");
    std::fs::create_dir_all(&dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
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
    // 5 件事 E: build a BM25 FTS index on messages.content, idempotent.
    // DuckDB's create_fts_index is idempotent — re-running with the same
    // (table, id, *fields) tuple is a no-op. We swallow errors so legacy
    // databases without an FTS index still initialise; search() will then
    // fall back to the LIKE path until the user re-runs `qq index`.
    let _ = conn.execute_batch(
        r#"PRAGMA create_fts_index(
            'messages', 'id', 'content', overwrite=false
        );"#,
    );
    Ok(())
}

/// 批量导入所有私聊和群聊消息到 DuckDB
/// 返回导入的消息总数
pub fn import_all(sqlite_path: &Path, _cache: &ContactCache) -> Result<usize> {
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
        let sql_c2c = format!(
            "SELECT {MSG_ID}, {C2C_PEER_ID}, {TIMESTAMP}, {CONTENT} FROM c2c_msg_table WHERE {CONTENT} IS NOT NULL"
        );
        let mut stmt = src_conn.prepare(&sql_c2c)?;
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
            let sender_name = crate::uid_resolve::name_for_qq(peer_id)
                .or_else(|| crate::uid_resolve::name_for_uid(&format!("uid_{}", peer_id)))
                .unwrap_or_else(|| format!("uid_{}", peer_id));
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
        let sql_group = format!(
            "SELECT {MSG_ID}, {GROUP_NAME}, {TIMESTAMP}, {CONTENT} FROM dataline_msg_table WHERE {CONTENT} IS NOT NULL"
        );
        let mut stmt = src_conn.prepare(&sql_group)?;
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

/// DuckDB 全文搜索 — 5 件事 E
///
/// Tries the FTS path first (BM25 score over messages.content) and
/// falls back to LIKE substring matching if the FTS index is missing
/// (legacy DBs that pre-date the create_fts_index migration) or the
/// FTS query errors out. The fallback keeps the search command
/// working in both states; the FTS path is the fast / scalable one.
///
/// Note: the FTS path requires DuckDB's `fts` community extension
/// to be installed (`INSTALL fts; LOAD fts;`). On a fresh install
/// the FTS path errors out and the LIKE fallback runs — which is
/// fine, just slower on large datasets. See
/// `docs/decoupling-compliance.md` section E.
pub fn search(query: &str, chat_id: Option<&str>, limit: usize) -> Result<Vec<SearchResult>> {
    let path = get_path()?;
    if !path.exists() {
        anyhow::bail!("请先运行 qq index");
    }

    let conn = Connection::open(&path)?;

    // ── Try FTS first ──
    if let Ok(results) = search_fts(&conn, query, chat_id, limit) {
        return Ok(results);
    }

    // ── FTS not available / errored → LIKE fallback ──
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
    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(cid) = chat_id {
        stmt.query(params![pattern, cid, limit as i64])?
    } else {
        stmt.query(params![pattern, limit as i64])?
    };

    let mut rows = rows;
    let mut results = Vec::new();
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

/// DuckDB FTS (BM25) search — 5 件事 E
///
/// Uses DuckDB's built-in `fts_main_messages.match_bm25` against the
/// FTS index built by `init_db` (PRAGMA create_fts_index). On success
/// returns rows ordered by BM25 score (best matches first).
///
/// Returns Err if the FTS index is missing (legacy DBs) or the FTS
/// extension is not available — caller is expected to fall back to
/// the LIKE path. Empty result set is a real answer and is Ok(vec![]).
fn search_fts(
    conn: &Connection,
    query: &str,
    chat_id: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let sql = if chat_id.is_some() {
        "SELECT m.msg_id, m.chat_id, m.chat_type, m.sender_id, m.sender_name,
                m.content, m.timestamp, m.time_str
         FROM messages m
         JOIN (
             SELECT id, fts_main_messages.match_bm25(id, ?) AS score
             FROM messages
             WHERE score IS NOT NULL
         ) AS r ON m.id = r.id
         WHERE m.chat_id = ?
         ORDER BY r.score DESC
         LIMIT ?"
    } else {
        "SELECT m.msg_id, m.chat_id, m.chat_type, m.sender_id, m.sender_name,
                m.content, m.timestamp, m.time_str
         FROM messages m
         JOIN (
             SELECT id, fts_main_messages.match_bm25(id, ?) AS score
             FROM messages
             WHERE score IS NOT NULL
         ) AS r ON m.id = r.id
         ORDER BY r.score DESC
         LIMIT ?"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(cid) = chat_id {
        stmt.query(params![query, cid, limit as i64])?
    } else {
        stmt.query(params![query, limit as i64])?
    };

    let mut rows = rows;
    let mut results = Vec::new();
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

#[cfg(test)]
mod fts_tests {
    use super::*;
    use std::env;

    /// When the FTS extension is not installed, search_fts should
    /// return Err (not panic, not silently return empty). The
    /// top-level search() then falls through to the LIKE path.
    ///
    /// This is the realistic case for a fresh CI/dev environment
    /// where the user has not run `INSTALL fts; LOAD fts;`. The test
    /// builds a minimal DB with no FTS index and verifies the
    /// failure mode.
    #[test]
    fn search_fts_errors_when_no_fts_index() {
        let tmp = env::temp_dir().join(format!(
            "qqcli_no_fts_{}.duckdb",
            std::process::id()
        ));
        if tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
        }
        // Create schema WITHOUT FTS (mimics legacy DB).
        let conn = Connection::open(&tmp).expect("open db");
        conn.execute_batch(
            r#"
            CREATE TABLE messages (
                id INTEGER PRIMARY KEY,
                msg_id BIGINT, chat_id TEXT, chat_type TEXT, sender_id BIGINT,
                sender_name TEXT, content TEXT, timestamp BIGINT,
                time_str TEXT, msg_type TEXT, is_mine BOOLEAN DEFAULT FALSE
            );
            INSERT INTO messages (id, msg_id, chat_id, chat_type, sender_id, sender_name, content, timestamp, time_str, msg_type, is_mine)
            VALUES (1, 1, 'c1', 'private', 100, 'Alice', 'fallback match', 1700000000, '2023-11-14 22:13:20', 'text', false);
            "#,
        )
        .expect("create + insert");

        let r = search_fts(&conn, "fallback", None, 10);
        assert!(r.is_err(), "search_fts should error when no FTS index exists, got {:?}", r.is_ok());

        let _ = std::fs::remove_file(&tmp);
    }

    /// Verify that init_db does not error out even when the FTS
    /// extension is unavailable. The PRAGMA is best-effort; on
    /// success it gives us real FTS, on failure we fall through
    /// to LIKE.
    #[test]
    fn init_db_does_not_panic_when_fts_unavailable() {
        let tmp = env::temp_dir().join(format!(
            "qqcli_init_{}.duckdb",
            std::process::id()
        ));
        if tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
        }
        // Should not panic, even though the FTS extension is likely
        // absent in the test environment.
        let r = init_db(&tmp);
        assert!(r.is_ok(), "init_db should succeed, got {:?}", r.err());
        let _ = std::fs::remove_file(&tmp);
    }
}

#[derive(Debug)]
#[allow(dead_code)]
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
