//! QQCLI 命令实现

use crate::cache;
use crate::db::{self, Message};
use crate::db_index;
use crate::decrypt;
use crate::napcat::ipc_client::NapcatIpcClient;
use crate::output::YamlWriter;
use anyhow::{anyhow, Context, Result};
use rusqlite::params;
use serde::Serialize;
use std::path::Path;

use crate::schema::{
    C2C_PEER_ID, C2C_SENDER_ID, C2C_SENDER_NAME, CONTENT, IS_SENDER_ME, MSG_ID, TIMESTAMP,
};

#[derive(Debug, Serialize)]
struct InitCandidate {
    account: Option<String>,
    path: String,
}

#[derive(Debug, Serialize)]
struct InitReport {
    status: &'static str,
    message: String,
    db_path: Option<String>,
    account: Option<String>,
    summary: Option<db::DbSummary>,
    candidates: Vec<InitCandidate>,
    next_command: String,
}

fn print_init_report(report: &InitReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("状态: {}", report.message);
    if let Some(account) = &report.account {
        println!("QQ 账号: {}", account);
    }
    if let Some(path) = &report.db_path {
        println!("数据库: {}", path);
    }
    if report.summary.is_some() {
        println!("数据库校验: 通过");
    }
    if !report.candidates.is_empty() {
        println!("可用账号:");
        for candidate in &report.candidates {
            match &candidate.account {
                Some(account) => println!("  - {}  {}", account, candidate.path),
                None => println!("  - {}", candidate.path),
            }
        }
    }
    println!("下一步: {}", report.next_command);
    Ok(())
}

/// 初始化数据库。默认只检测；只有显式传 --decrypt 才会提取密钥或启动解密。
pub fn init(
    account: Option<&str>,
    db_path: Option<&Path>,
    decrypt_requested: bool,
    json: bool,
) -> Result<()> {
    let selected = match db::select_raw_db_path(account, db_path) {
        Ok(path) => path,
        Err(err) => {
            let candidates: Vec<InitCandidate> = db::raw_db_candidates()
                .into_iter()
                .map(|path| InitCandidate {
                    account: db::account_from_db_path(&path),
                    path: path.display().to_string(),
                })
                .collect();
            // 兼容旧版仅保留了解密缓存、没有原始数据库的用户。
            if candidates.is_empty() {
                if let decrypt::DbStatus::Plaintext(path) = decrypt::detect_db_status() {
                    let summary = db::validate_db(&path)?;
                    let report = InitReport {
                        status: "ready_legacy_cache",
                        message:
                            "已使用旧版解密缓存；建议重新运行 QQ NT 后执行 qq init 以绑定账号。"
                                .to_string(),
                        db_path: Some(path.display().to_string()),
                        account: None,
                        summary: Some(summary),
                        candidates: vec![],
                        next_command: "qq sessions --limit 20".to_string(),
                    };
                    return print_init_report(&report, json);
                }
            }
            let report = InitReport {
                status: if candidates.len() > 1 {
                    "selection_required"
                } else {
                    "database_not_found"
                },
                message: err.to_string(),
                db_path: None,
                account: None,
                summary: None,
                candidates,
                next_command: "qq init --account <QQ号>  或  qq init --db-path <nt_msg.db 路径>"
                    .to_string(),
            };
            print_init_report(&report, json)?;
            return Err(err.context("初始化未完成"));
        }
    };

    crate::config::save_db_path(&selected)?;
    let account = db::account_from_db_path(&selected);

    match decrypt::detect_db_status() {
        decrypt::DbStatus::Plaintext(path) => {
            let summary = db::validate_db(&path)?;
            let report = InitReport {
                status: "ready",
                message: "数据库已就绪".to_string(),
                db_path: Some(path.display().to_string()),
                account,
                summary: Some(summary),
                candidates: vec![],
                next_command: "qq sessions --limit 20".to_string(),
            };
            print_init_report(&report, json)
        }
        decrypt::DbStatus::Encrypted { raw_db, key } if !decrypt_requested => {
            let prerequisites = decrypt::decrypt_prerequisites();
            let dependencies_ready =
                prerequisites.sqlcipher_ready && (key.is_some() || prerequisites.key_script_ready);
            let report = InitReport {
                status: "decrypt_required",
                message: if key.is_some() {
                    "数据库已加密，已找到受保护密钥。不会自动解密。".to_string()
                } else {
                    "数据库已加密，需要提取密钥。不会自动启动 QQ 或解密。".to_string()
                },
                db_path: Some(raw_db.display().to_string()),
                account,
                summary: None,
                candidates: vec![],
                next_command: if dependencies_ready {
                    "qq init --decrypt".to_string()
                } else {
                    "qq doctor".to_string()
                },
            };
            print_init_report(&report, json)?;
            anyhow::bail!("初始化需要显式解密")
        }
        decrypt::DbStatus::Encrypted { .. } => {
            let decrypted = decrypt::ensure_decrypted()
                .context("解密失败。请确认 QQ NT 已登录，并检查 sqlcipher 与密钥提取工具配置。")?;
            let summary = db::validate_db(&decrypted)?;
            let report = InitReport {
                status: "ready",
                message: "解密完成，数据库已就绪".to_string(),
                db_path: Some(decrypted.display().to_string()),
                account,
                summary: Some(summary),
                candidates: vec![],
                next_command: "qq sessions --limit 20".to_string(),
            };
            print_init_report(&report, json)
        }
        decrypt::DbStatus::NotFound(path) => {
            let report = InitReport {
                status: "database_not_found",
                message: "未找到 QQ 数据库".to_string(),
                db_path: Some(path.display().to_string()),
                account,
                summary: None,
                candidates: vec![],
                next_command:
                    "运行并登录 QQ NT 后重试；自定义目录请使用 qq init --db-path <nt_msg.db 路径>"
                        .to_string(),
            };
            print_init_report(&report, json)?;
            anyhow::bail!("初始化未完成")
        }
    }
}

#[derive(Debug, Serialize)]
struct ConfigReport {
    db_path: Option<String>,
    db_uin: Option<String>,
    protected_key_saved: bool,
    legacy_plaintext_key_detected: bool,
    sqlcipher_bin: Option<String>,
    key_script: Option<String>,
}

pub fn config_show(json: bool) -> Result<()> {
    let cfg = crate::config::get_config()?;
    let report = ConfigReport {
        db_path: cfg.db_path.map(|path| path.display().to_string()),
        db_uin: cfg.db_uin,
        protected_key_saved: cfg.db_key_protected.is_some(),
        legacy_plaintext_key_detected: cfg.db_key.is_some(),
        sqlcipher_bin: cfg.sqlcipher_bin.map(|path| path.display().to_string()),
        key_script: cfg.key_script.map(|path| path.display().to_string()),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("配置文件: {}", crate::config::config_path().display());
        println!(
            "数据库: {}",
            report.db_path.as_deref().unwrap_or("未选择；运行 qq init")
        );
        println!("账号: {}", report.db_uin.as_deref().unwrap_or("未识别"));
        println!(
            "受保护密钥: {}",
            if report.protected_key_saved {
                "已保存"
            } else {
                "未保存"
            }
        );
        println!(
            "sqlcipher: {}",
            report.sqlcipher_bin.as_deref().unwrap_or("未配置")
        );
        println!(
            "密钥提取脚本: {}",
            report.key_script.as_deref().unwrap_or("未配置")
        );
        if report.legacy_plaintext_key_detected {
            println!("注意: 检测到旧版明文密钥；下次解密时会自动迁移。");
        }
    }
    Ok(())
}

pub fn config_set_db_path(path: &Path, json: bool) -> Result<()> {
    if !path.is_file() {
        anyhow::bail!("指定的数据库不存在或不是文件: {}", path.display());
    }
    crate::config::save_db_path(path)?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "saved",
                "db_path": path.display().to_string(),
                "next_command": "qq init"
            })
        );
    } else {
        println!("已保存数据库路径: {}", path.display());
        println!("下一步: qq init");
    }
    Ok(())
}

pub fn config_set_sqlcipher(path: &Path, json: bool) -> Result<()> {
    if !path.is_file() {
        anyhow::bail!("sqlcipher.exe 不存在: {}", path.display());
    }
    crate::config::save_sqlcipher_bin(path)?;
    print_config_saved("sqlcipher", path, json)
}

pub fn config_set_key_script(path: &Path, json: bool) -> Result<()> {
    if !path.is_file() {
        anyhow::bail!("密钥提取脚本不存在: {}", path.display());
    }
    crate::config::save_key_script(path)?;
    print_config_saved("key_script", path, json)
}

fn print_config_saved(setting: &str, path: &Path, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "saved",
                "setting": setting,
                "path": path.display().to_string(),
                "next_command": "qq init --decrypt"
            })
        );
    } else {
        println!("已保存 {}: {}", setting, path.display());
        println!("下一步: qq init --decrypt");
    }
    Ok(())
}

pub fn config_clear_db_path(json: bool) -> Result<()> {
    crate::config::clear_db_path()?;
    if json {
        println!("{}", serde_json::json!({ "status": "cleared" }));
    } else {
        println!("已清除已保存的数据库路径。下一步: qq init");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    database_status: String,
    database_path: Option<String>,
    sqlcipher_path: String,
    sqlcipher_ready: bool,
    key_script_path: String,
    key_script_ready: bool,
    protected_key_saved: bool,
    next_command: String,
}

pub fn doctor(json: bool) -> Result<()> {
    let prerequisites = decrypt::decrypt_prerequisites();
    let protected_key_saved = crate::config::get_config()
        .map(|cfg| cfg.db_key_protected.is_some())
        .unwrap_or(false);
    let status = decrypt::detect_db_status();

    let (database_status, database_path, next_command) = match status {
        decrypt::DbStatus::Plaintext(path) => (
            "ready".to_string(),
            Some(path.display().to_string()),
            "qq sessions --limit 20".to_string(),
        ),
        decrypt::DbStatus::Encrypted { raw_db, key } => {
            let next = if !prerequisites.sqlcipher_ready {
                "qq config set-sqlcipher <sqlcipher.exe 路径>".to_string()
            } else if key.is_none() && !prerequisites.key_script_ready {
                "qq config set-key-script <windows_ntqq_get_key.ps1 路径>".to_string()
            } else {
                "qq init --decrypt".to_string()
            };
            (
                "decrypt_required".to_string(),
                Some(raw_db.display().to_string()),
                next,
            )
        }
        decrypt::DbStatus::NotFound(path) => (
            "database_not_found".to_string(),
            Some(path.display().to_string()),
            "qq init --account <QQ号>  或  qq init --db-path <nt_msg.db 路径>".to_string(),
        ),
    };

    let report = DoctorReport {
        database_status,
        database_path,
        sqlcipher_path: prerequisites.sqlcipher_path.display().to_string(),
        sqlcipher_ready: prerequisites.sqlcipher_ready,
        key_script_path: prerequisites.key_script_path.display().to_string(),
        key_script_ready: prerequisites.key_script_ready,
        protected_key_saved,
        next_command,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("数据库: {}", report.database_status);
        println!(
            "sqlcipher: {}",
            if report.sqlcipher_ready {
                "已就绪"
            } else {
                "未配置"
            }
        );
        println!(
            "密钥提取脚本: {}",
            if report.key_script_ready {
                "已就绪"
            } else {
                "未配置"
            }
        );
        println!(
            "受保护密钥: {}",
            if report.protected_key_saved {
                "已保存"
            } else {
                "未保存"
            }
        );
        println!("下一步: {}", report.next_command);
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
    // 优先用 DuckDB 搜索
    if let Ok(results) = db_index::search(keyword, chat, limit) {
        for r in results {
            let content = if r.content.len() > 100 {
                format!("{}...", &r.content[..100])
            } else {
                r.content
            };
            println!(
                "[{}] {} ({}): {}",
                r.time_str, r.sender_name, r.chat_id, content
            );
        }
        return Ok(());
    }

    // fallback 到 nt_msg.db
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
        "markdown" | "md" => {
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
        other => {
            anyhow::bail!(
                "未知导出格式: '{}'\n支持: markdown | md | txt | json | jsonl | yaml",
                other
            );
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

/// 打包聊天记录中的媒体文件
pub fn bundle_media(
    chat: &str,
    since: Option<&str>,
    until: Option<&str>,
    limit: usize,
    output: &str,
) -> Result<()> {
    use crate::segment::Segment;
    use md5;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let since_ts = since.and_then(|s| db::parse_ts(s).ok());
    let until_ts = until.and_then(|s| db::parse_ts(s).ok());

    let messages = db::get_messages(chat, limit, 0, since_ts, until_ts, None)?;

    // 走 Segment 列表 (解耦: 不再 regex 字符串) — 收 Image/Record/File/Mface 段
    let mut media_items: Vec<(String, String, String)> = Vec::new(); // (download_url, display_name, source_label)
    for m in &messages {
        for seg in &m.segments {
            match seg {
                Segment::Image {
                    url,
                    fileid,
                    local_path,
                    ..
                } => {
                    if let Some(u) = url {
                        let name = fileid.clone().unwrap_or_else(|| "image".to_string());
                        media_items.push((u.clone(), name, "image".to_string()));
                    } else if let Some(p) = local_path {
                        media_items.push((
                            p.clone(),
                            fileid.clone().unwrap_or_else(|| "image".to_string()),
                            "image-local".to_string(),
                        ));
                    }
                }
                Segment::Record {
                    url: Some(u),
                    fileid,
                    ..
                } => {
                    let name = fileid.clone().unwrap_or_else(|| "record".to_string());
                    media_items.push((u.clone(), name, "record".to_string()));
                }
                Segment::File {
                    url,
                    name,
                    fileid,
                    local_path,
                    ..
                } => {
                    if let Some(u) = url {
                        media_items.push((u.clone(), name.clone(), "file".to_string()));
                    } else if let Some(p) = local_path {
                        media_items.push((p.clone(), name.clone(), "file-local".to_string()));
                    } else if let Some(fid) = fileid {
                        media_items.push((fid.clone(), name.clone(), "file-id".to_string()));
                    }
                }
                Segment::Mface {
                    url: Some(u), id, ..
                } => {
                    media_items.push((u.clone(), id.clone(), "mface".to_string()));
                }
                _ => {}
            }
        }
    }

    if media_items.is_empty() {
        println!("未找到可打包的媒体 (Image/Record/File/Mface 段为空)");
        return Ok(());
    }

    println!("找到 {} 个媒体, 开始下载/打包...", media_items.len());

    // 创建 zip 文件
    let file = std::fs::File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut downloaded = 0;
    let mut failed = 0;
    let mut local_copied = 0;

    let total = media_items.len();
    for (i, (source, name, kind)) in media_items.iter().enumerate() {
        // local_path 类直接读文件, 不走 HTTP
        if kind.ends_with("-local") {
            match std::fs::read(source) {
                Ok(bytes) => {
                    let md5_hash = format!("{:x}", md5::compute(&bytes));
                    let ext = std::path::Path::new(name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("bin");
                    let unique_name = format!("{}_{}_{}.{}", kind, &md5_hash[..8], i, ext);
                    zip.start_file(&unique_name, options)?;
                    zip.write_all(&bytes)?;
                    local_copied += 1;
                }
                Err(_) => failed += 1,
            }
            continue;
        }

        match client.get(source).send() {
            Ok(response) => {
                if let Ok(bytes) = response.bytes() {
                    let md5_hash = format!("{:x}", md5::compute(&bytes));
                    let ext = std::path::Path::new(name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("bin");
                    let unique_name = format!("{}_{}_{}.{}", kind, &md5_hash[..8], i, ext);
                    zip.start_file(&unique_name, options)?;
                    zip.write_all(&bytes)?;
                    downloaded += 1;

                    if downloaded % 10 == 0 {
                        println!("已下载 {}/{}", downloaded, total);
                    }
                } else {
                    failed += 1;
                }
            }
            Err(_) => {
                failed += 1;
            }
        }
    }

    zip.finish()?;
    println!(
        "完成! 下载 {} 个, 拷贝本地 {} 个, 失败 {} 个",
        downloaded, local_copied, failed
    );
    println!("已保存到: {}", output);
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
    if !members.is_empty() {
        if json_flag {
            println!("{}", serde_json::to_string_pretty(&members)?);
        } else {
            YamlWriter::write_members(&members, chat)?;
        }
        return Ok(());
    }

    // 0 成员: 给清晰提示, 不要静默
    if json_flag {
        println!("[]");
    } else {
        println!("(无成员数据)");
        if chat.chars().all(|c| c.is_ascii_digit()) {
            eprintln!(
                "\n提示: '{}' 看起来是旧 groupCode (纯数字), NT 升级后群 ID 变成 'group:u_xxx' 形式。\n       用 `qq sessions` 查当前群里, 用 'group:u_xxx' 形式的 ID 重试。",
                chat
            );
        } else {
            eprintln!(
                "\n提示: 此群在 NT 升级后可能没有成员数据。用 `qq sessions` 确认群 ID 形式 (应是 'group:u_xxx')。"
            );
        }
    }
    Ok(())
}

pub fn new_messages(limit: usize, json_flag: bool) -> Result<()> {
    let _since_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 - 86400)
        .unwrap_or(0);

    let path = db::detect_db_path()?;
    let conn = rusqlite::Connection::open(&path)?;
    let mut messages: Vec<Message> = Vec::new();

    let sql = format!(
        "SELECT {MSG_ID}, {C2C_SENDER_ID}, {C2C_SENDER_NAME}, {CONTENT}, {TIMESTAMP}, {IS_SENDER_ME}, {C2C_PEER_ID}
         FROM c2c_msg_table
         WHERE {TIMESTAMP} >= ? AND {CONTENT} IS NOT NULL
         ORDER BY {TIMESTAMP} DESC
         LIMIT ?"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![_since_ts, limit as i64])?;

    while let Some(row) = rows.next()? {
        let content_raw: Vec<u8> = row.get(3).unwrap_or_default();
        let sender_id: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
        let ts: i64 = row.get::<_, Option<i64>>(4)?.unwrap_or(0);
        let is_mine: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);

        messages.push(crate::message::build_message(
            row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            sender_id,
            &content_raw,
            ts,
            is_mine,
        ));
    }

    messages.sort_by_key(|m| m.timestamp);
    messages.reverse();
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

/// 从 NapCat 同步联系人到本地缓存
pub async fn sync(url: &str, token: Option<&str>) -> Result<()> {
    use crate::napcat::NapcatClient;

    println!("正在连接 NapCat: {}", url);
    let client = NapcatClient::connect(url, token).await?;

    println!("正在获取好友列表...");
    let friends = client.get_friend_list().await?;
    println!("获取到 {} 个好友", friends.len());

    println!("正在获取群列表...");
    let groups = client.get_group_list().await?;
    println!("获取到 {} 个群", groups.len());

    cache::save_cache(&friends, &groups)?;

    let cache = cache::load_cache();
    if let Some(c) = cache {
        use chrono::DateTime;
        let dt = DateTime::from_timestamp(c.synced_at, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| c.synced_at.to_string());
        println!(
            "同步完成: {} 个好友, {} 个群, 时间 {}",
            friends.len(),
            groups.len(),
            dt
        );
    }

    Ok(())
}

/// 将 QQ 消息批量索引到 DuckDB FTS
pub fn index() -> Result<()> {
    let db_path = db::detect_db_path()?;
    let cache = cache::load_cache().unwrap_or_else(|| cache::ContactCache {
        synced_at: 0,
        friends: vec![],
        groups: vec![],
    });
    let count = db_index::import_all(&db_path, &cache)?;
    println!(
        "索引完成: {} 条消息 -> {}",
        count,
        db_index::get_path()?.display()
    );
    Ok(())
}

/// NapCat IPC 插件命令
pub fn plugin(sub: &str, port: u16, args: &[&str]) -> Result<()> {
    let client = NapcatIpcClient::with_port(port).map_err(|e| anyhow!("IPC 连接失败: {}", e))?;

    match sub {
        "ping" => {
            if client.ping().map_err(|e| anyhow!("{}", e))? {
                println!("[OK] NapCat IPC 连接正常");
            } else {
                anyhow::bail!("IPC ping 失败");
            }
        }
        "send" => {
            let msg_type = args.first().ok_or_else(|| {
                anyhow!("用法: plugin send <private|group> <target> <message...>")
            })?;
            let target = args.get(1).ok_or_else(|| {
                anyhow!("用法: plugin send <private|group> <target> <message...>")
            })?;
            let message = args.get(2..).map(|a| a.join(" ")).ok_or_else(|| {
                anyhow!("用法: plugin send <private|group> <target> <message...>")
            })?;

            if message.is_empty() {
                anyhow::bail!("消息内容不能为空");
            }

            let result = match *msg_type {
                "private" => client.send_private_msg(target, &message),
                "group" => client.send_group_msg(target, &message),
                _ => anyhow::bail!("msg_type 必须是 private 或 group"),
            }
            .map_err(|e| anyhow!("发送失败: {}", e))?;

            if let Some(success) = result.get("success").and_then(|v| v.as_bool()) {
                if success {
                    println!("发送成功: {:?}", result.get("msgId"));
                } else {
                    anyhow::bail!("发送失败: {:?}", result.get("error"));
                }
            }
        }
        "friends" => {
            let friends = client.get_friend_list().map_err(|e| anyhow!("{}", e))?;
            println!("=== 好友列表 ({}个) ===", friends.len());
            for f in &friends {
                let nick = f.get("nick").and_then(|v| v.as_str()).unwrap_or("?");
                let uin = f
                    .get("uin")
                    .or(f.get("uid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                println!("- {} ({})", nick, uin);
            }
        }
        "groups" => {
            let groups = client.get_group_list().map_err(|e| anyhow!("{}", e))?;
            println!("=== 群列表 ({}个) ===", groups.len());
            for g in &groups {
                let name = g
                    .get("groupName")
                    .or(g.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let code = g
                    .get("groupCode")
                    .or(g.get("code"))
                    .or(g.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                println!("- {} ({})", name, code);
            }
        }
        "members" => {
            let group_id = args
                .first()
                .ok_or_else(|| anyhow!("用法: plugin members <group_id>"))?;
            let members = client
                .get_group_members(group_id)
                .map_err(|e| anyhow!("{}", e))?;
            println!("=== 群成员 ({}个) ===", members.len());
            for m in &members {
                let nick = m
                    .get("nick")
                    .or(m.get("nickname"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let uin = m
                    .get("uin")
                    .or(m.get("uid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let card = m.get("cardName").or(m.get("card")).and_then(|v| v.as_str());
                if let Some(c) = card {
                    println!("- {} ({}) [{}]", c, uin, nick);
                } else {
                    println!("- {} ({})", nick, uin);
                }
            }
        }
        "chats" => {
            let chats = client.get_recent_chats().map_err(|e| anyhow!("{}", e))?;
            println!("=== 最近会话 ({}个) ===", chats.len());
            for c in &chats {
                let name = c
                    .get("nickName")
                    .or(c.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let id = c
                    .get("peerUid")
                    .or(c.get("uid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let chat_type = c.get("chatType").and_then(|v| v.as_i64()).unwrap_or(0);
                let type_str = match chat_type {
                    1 => "私聊",
                    2 => "群聊",
                    _ => "其他",
                };
                println!("- [{}] {} ({})", type_str, name, id);
            }
        }
        _ => {
            eprintln!("未知子命令: {}", sub);
            eprintln!("可用: ping | send | friends | groups | members | chats");
            anyhow::bail!("unknown subcommand");
        }
    }

    Ok(())
}
