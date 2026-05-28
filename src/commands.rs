//! QQCLI 命令实现

use crate::db::{self, Message};
use crate::decrypt;
use crate::output::YamlWriter;
use anyhow::Result;

/// 检测/初始化 DB: 查找 DB，检测加密状态，必要时自动解密
pub fn init(force: bool) -> Result<()> {
    let status = decrypt::detect_db_status();

    match &status {
        decrypt::DbStatus::Plaintext(p) => {
            println!("DB 状态: 明文 OK");
            println!("DB 路径: {}", p.display());
            // 快速验证
            if let Err(e) = db::init_db() {
                eprintln!("警告: DB 打开异常: {}", e);
            }
        }
        decrypt::DbStatus::NotFound(raw) => {
            println!("DB 状态: 未找到");
            println!("原始路径: {}", raw.display());
            println!();
            println!("提示: 请先运行 QQ NT，然后重新执行 qq init");
        }
        decrypt::DbStatus::Encrypted { raw_db, key } => {
            println!("DB 状态: 加密");
            println!("加密 DB: {}", raw_db.display());
            if key.is_some() {
                println!("密钥: 已缓存");
            } else {
                println!("密钥: 未缓存 (需要提取)");
            }
            println!();

            if key.is_some() && !force {
                // 有密钥，直接解密
                println!("正在解密...");
                match decrypt::ensure_decrypted(false) {
                    Ok(p) => {
                        println!("解密成功: {}", p.display());
                    }
                    Err(e) => {
                        eprintln!("解密失败: {}", e);
                        eprintln!("提示: 请确认 QQ NT 进程正在运行，然后重试");
                    }
                }
            } else if key.is_none() {
                println!("正在从 QQ 进程提取密钥 (会启动 QQ 窗口，请登录)...");
                match decrypt::ensure_decrypted(false) {
                    Ok(p) => {
                        println!("解密成功: {}", p.display());
                    }
                    Err(e) => {
                        eprintln!("密钥提取失败: {}", e);
                        eprintln!();
                        eprintln!("手动解密步骤:");
                        eprintln!("  1. 下载 https://github.com/yourusername/qq-nt-decrypt");
                        eprintln!("  2. 运行 windows_ntqq_get_key.ps1 获取密钥");
                        eprintln!("  3. 将密钥保存到: {}", crate::config::config_path().display());
                    }
                }
            } else {
                println!("使用 --force 跳过自动解密");
            }
        }
    }

    Ok(())
}

pub fn debug_tables() -> Result<()> {
    db::debug_tables()?;
    Ok(())
}

pub fn debug_probe() -> Result<()> {
    db::debug_probe()?;
    Ok(())
}

pub fn sessions(limit: usize, json_flag: bool) -> Result<()> {
    let sessions = db::list_sessions(limit)?;
    if json_flag {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
    } else {
        YamlWriter::write_sessions(&sessions)?;
    }
    Ok(())
}

pub fn history(
    chat: &str,
    limit: usize,
    offset: usize,
    since: Option<&str>,
    until: Option<&str>,
    msg_type: Option<&str>,
    json_flag: bool,
) -> Result<()> {
    let since_ts = since.and_then(|s| db::parse_ts(s).ok());
    let until_ts = until.and_then(|s| db::parse_ts(s).ok());

    let messages = db::get_messages(chat, limit, offset, since_ts, until_ts, msg_type)?;

    if json_flag {
        println!("{}", serde_json::to_string_pretty(&messages)?);
    } else {
        YamlWriter::write_messages(&messages)?;
    }
    Ok(())
}

pub fn search(
    keyword: &str,
    chat: Option<&str>,
    limit: usize,
    since: Option<&str>,
    until: Option<&str>,
    json_flag: bool,
) -> Result<()> {
    let since_ts = since.and_then(|s| db::parse_ts(s).ok());
    let until_ts = until.and_then(|s| db::parse_ts(s).ok());

    let messages = db::search_messages(keyword, chat, limit, since_ts, until_ts)?;

    if json_flag {
        println!("{}", serde_json::to_string_pretty(&messages)?);
    } else {
        YamlWriter::write_messages(&messages)?;
    }
    Ok(())
}

pub fn contacts(query: Option<&str>, limit: usize, kind: &str, json_flag: bool) -> Result<()> {
    let contacts = db::list_contacts(query, limit, kind)?;
    if json_flag {
        println!("{}", serde_json::to_string_pretty(&contacts)?);
    } else {
        YamlWriter::write_contacts(&contacts)?;
    }
    Ok(())
}

/// 导出聊天记录，支持多种格式
pub fn export(
    chat: &str,
    since: Option<&str>,
    until: Option<&str>,
    limit: usize,
    format: &str,
    output: Option<&str>,
    json_flag: bool,
) -> Result<()> {
    let since_ts = since.and_then(|s| db::parse_ts(s).ok());
    let until_ts = until.and_then(|s| db::parse_ts(s).ok());

    let messages = db::get_messages(chat, limit, 0, since_ts, until_ts, None)?;

    let content = match format {
        // JSONL 格式（与 qq-data-exporter 兼容）
        "jsonl" => {
            let mut s = String::new();
            for m in &messages {
                let nm = db::Message::to_normalized(m, chat);
                s.push_str(&serde_json::to_string(&nm)?);
                s.push('\n');
            }
            s
        }
        "json" => serde_json::to_string_pretty(&messages)?,
        "yaml" => serde_yaml::to_string(&messages)?,
        "txt" => {
            let mut s = String::new();
            for m in &messages {
                s.push_str(&format!(
                    "[{}] {}: {}\n",
                    m.time_str, m.sender_name, m.content
                ));
            }
            s
        }
        _ => {
            // markdown
            let mut md = format!("# QQ 聊天记录: {}\n\n", chat);
            let mut current_date = String::new();
            for m in &messages {
                let date_str = &m.time_str[..10];
                if date_str != current_date {
                    md.push_str(&format!("\n## {}\n\n", date_str));
                    current_date = date_str.to_string();
                }
                let sender = if m.is_mine { "我" } else { &m.sender_name };
                md.push_str(&format!("**{}** [{}]: {}\n", m.time_str, sender, m.content));
            }
            md
        }
    };

    if let Some(path) = output {
        std::fs::write(path, &content)?;
        println!("已导出到: {}", path);
    } else {
        println!("{}", content);
    }

    let _ = json_flag;
    Ok(())
}

pub fn unread(limit: usize, json_flag: bool) -> Result<()> {
    match db::get_unread_sessions(limit) {
        Ok(sessions) if !sessions.is_empty() => {
            if json_flag {
                println!("{}", serde_json::to_string_pretty(&sessions)?);
            } else {
                YamlWriter::write_sessions(&sessions)?;
            }
        }
        _ => {
            println!("(QQ NT 未提供独立未读计数，显示最近会话)\n");
            sessions(limit, json_flag)?;
        }
    }
    Ok(())
}

pub fn members(chat: &str, json_flag: bool) -> Result<()> {
    let members = db::get_group_members(chat)?;
    if json_flag {
        println!("{}", serde_json::to_string_pretty(&members)?);
    } else {
        YamlWriter::write_members(&members, chat)?;
    }
    Ok(())
}

pub fn new_messages(limit: usize, json_flag: bool) -> Result<()> {
    let since_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 - 86400)
        .unwrap_or(0);

    let path = db::detect_db_path()?;
    let conn = rusqlite::Connection::open(&path)?;
    let mut messages: Vec<Message> = Vec::new();

    let sql = format!(
        "SELECT [40001],[40030],[40021],[40800],[40050],[40009],[40033]
         FROM c2c_msg_table
         WHERE [40050] >= {} AND [40800] IS NOT NULL
         ORDER BY [40050] DESC
         LIMIT ?",
        since_ts
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([limit as i64])?;

    while let Some(row) = rows.next()? {
        let content_raw: Vec<u8> = row.get(3).unwrap_or_default();
        let sender_id: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
        let ts: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
        let is_mine: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);

        messages.push(Message {
            id: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            sender_id,
            sender_name: row
                .get::<_, Option<String>>(2)?
                .unwrap_or_else(|| sender_id.to_string()),
            content: db::extract_text(&content_raw),
            msg_type: db::detect_type(&content_raw),
            is_mine: is_mine == 1,
            timestamp: ts,
            time_str: db::fmt_ts(ts),
        });
    }

    messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    messages.truncate(limit);

    if json_flag {
        println!("{}", serde_json::to_string_pretty(&messages)?);
    } else {
        YamlWriter::write_messages(&messages)?;
    }
    Ok(())
}

pub fn stats(
    chat: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    json_flag: bool,
) -> Result<()> {
    let since_ts = since.and_then(|s| db::parse_ts(s).ok());
    let until_ts = until.and_then(|s| db::parse_ts(s).ok());

    let stats = db::get_stats(chat, since_ts, until_ts)?;

    if json_flag {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!("=== QQ 数据统计 ===");
        println!(
            "私聊消息: {}",
            stats.get("c2c_count").and_then(|v| v.as_i64()).unwrap_or(0)
        );
        println!(
            "群聊消息: {}",
            stats
                .get("group_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        );
        println!(
            "总计: {}",
            stats
                .get("total_messages")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        );
        if let Some(range) = stats.get("date_range") {
            if let (Some(since_v), Some(until_v)) = (range.get("since"), range.get("until")) {
                println!("时间范围: {} ~ {}", since_v, until_v);
            }
        }
    }
    Ok(())
}

/// 从 NapCat 获取群列表（需要 NapCat 运行）
pub async fn groups(url: &str, token: Option<&str>) -> Result<()> {
    use crate::napcat::NapcatClient;

    let client = NapcatClient::connect(url, token).await?;
    let group_list = client.get_group_list().await?;

    if group_list.is_empty() {
        println!("(无群)");
        return Ok(());
    }

    println!("=== 群列表 ({}个) ===", group_list.len());
    for g in &group_list {
        println!("- {} ({})", g.group_name, g.group_id);
    }
    Ok(())
}
