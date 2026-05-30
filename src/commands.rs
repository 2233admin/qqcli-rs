//! QQCLI 命令实现

use crate::cache;
use crate::db::{self, Message};
use crate::db_index;
use crate::decrypt;
use crate::napcat::ipc_client::NapcatIpcClient;
use crate::output::YamlWriter;
use crate::schema;
use anyhow::{anyhow, Result};

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
    // 优先用 DuckDB 搜索
    if let Ok(results) = db_index::search(keyword, chat, limit) {
        for r in results {
            let content = if r.content.len() > 100 {
                format!("{}...", &r.content[..100])
            } else {
                r.content
            };
            println!("[{}] {} ({}): {}", r.time_str, r.sender_name, r.chat_id, content);
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

/// 打包聊天记录中的媒体文件
pub fn bundle_media(chat: &str, since: Option<&str>, until: Option<&str>, limit: usize, output: &str) -> Result<()> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use md5;

    let since_ts = since.and_then(|s| db::parse_ts(s).ok());
    let until_ts = until.and_then(|s| db::parse_ts(s).ok());

    let messages = db::get_messages(chat, limit, 0, since_ts, until_ts, None)?;

    // 提取所有图片 URL
    let mut image_urls: Vec<(String, String)> = Vec::new(); // (url, filename)
    for m in &messages {
        for url in extract_image_urls(&m.content) {
            let filename = url.split('/').last().unwrap_or("image.jpg").to_string();
            image_urls.push((url, filename));
        }
    }

    if image_urls.is_empty() {
        println!("未找到图片链接");
        return Ok(());
    }

    println!("找到 {} 张图片，开始下载...", image_urls.len());

    // 创建 zip 文件
    let file = std::fs::File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut downloaded = 0;
    let mut failed = 0;

    let total = image_urls.len();
    for (i, (url, filename)) in image_urls.iter().enumerate() {
        match client.get(url).send() {
            Ok(response) => {
                if let Ok(bytes) = response.bytes() {
                    let md5_hash = format!("{:x}", md5::compute(&bytes));
                    let ext = filename.rsplit('.').next().unwrap_or("jpg");
                    let unique_name = format!("{}_{}.{}", &md5_hash[..8], i, ext);

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
    println!("完成！下载 {} 张图片，失败 {} 张", downloaded, failed);
    println!("已保存到: {}", output);
    Ok(())
}

/// 从消息内容中提取图片 URL
fn extract_image_urls(content: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for line in content.lines() {
        // 匹配 QQ 图片 CDN URL
        if line.contains("multimedia.nt.qq.com.cn") && line.contains("fileid=") {
            if let Some(start) = line.find("fileid=") {
                if let Some(rest) = line.get(start..) {
                    let params = &rest[7..rest.len().min(200)];
                    if let Some(end) = params.find(|c: char| !c.is_alphanumeric() && c != '=' && c != '_' && c != '-' && c != '~') {
                        let fileid = &params[..end.min(100)];
                        let full_url = format!("https://multimedia.nt.qq.com.cn{}", params);
                        if !urls.iter().any(|u: &String| u.contains(fileid)) {
                            urls.push(full_url);
                        }
                    }
                }
            }
        }
    }
    urls
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
        "SELECT {}, {}, {}, {}, {}, {}, {}
         FROM c2c_msg_table
         WHERE {} >= {{}} AND {} IS NOT NULL
         ORDER BY {} DESC
         LIMIT ?",
        schema::MSG_ID,
        schema::C2C_SENDER_ID,
        schema::C2C_SENDER_NAME,
        schema::CONTENT,
        schema::TIMESTAMP,
        schema::IS_SENDER_ME,
        schema::C2C_PEER_ID,
        schema::TIMESTAMP,
        schema::CONTENT,
        schema::TIMESTAMP
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
        println!("同步完成: {} 个好友, {} 个群, 时间 {}", friends.len(), groups.len(), dt);
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
    println!("索引完成: {} 条消息 -> {}", count, db_index::get_path()?.display());
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
            let msg_type = args.first().ok_or_else(|| anyhow!("用法: plugin send <private|group> <target> <message...>"))?;
            let target = args.get(1).ok_or_else(|| anyhow!("用法: plugin send <private|group> <target> <message...>"))?;
            let message = args.get(2..).map(|a| a.join(" ")).ok_or_else(|| anyhow!("用法: plugin send <private|group> <target> <message...>"))?;

            if message.is_empty() {
                anyhow::bail!("消息内容不能为空");
            }

            let result = match *msg_type {
                "private" => client.send_private_msg(target, &message),
                "group" => client.send_group_msg(target, &message),
                _ => anyhow::bail!("msg_type 必须是 private 或 group"),
            }.map_err(|e| anyhow!("发送失败: {}", e))?;

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
                let uin = f.get("uin").or(f.get("uid")).and_then(|v| v.as_str()).unwrap_or("?");
                println!("- {} ({})", nick, uin);
            }
        }
        "groups" => {
            let groups = client.get_group_list().map_err(|e| anyhow!("{}", e))?;
            println!("=== 群列表 ({}个) ===", groups.len());
            for g in &groups {
                let name = g.get("groupName").or(g.get("name")).and_then(|v| v.as_str()).unwrap_or("?");
                let code = g.get("groupCode").or(g.get("code")).or(g.get("id")).and_then(|v| v.as_str()).unwrap_or("?");
                println!("- {} ({})", name, code);
            }
        }
        "members" => {
            let group_id = args.first().ok_or_else(|| anyhow!("用法: plugin members <group_id>"))?;
            let members = client.get_group_members(group_id).map_err(|e| anyhow!("{}", e))?;
            println!("=== 群成员 ({}个) ===", members.len());
            for m in &members {
                let nick = m.get("nick").or(m.get("nickname")).and_then(|v| v.as_str()).unwrap_or("?");
                let uin = m.get("uin").or(m.get("uid")).and_then(|v| v.as_str()).unwrap_or("?");
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
                let name = c.get("nickName").or(c.get("name")).and_then(|v| v.as_str()).unwrap_or("?");
                let id = c.get("peerUid").or(c.get("uid")).and_then(|v| v.as_str()).unwrap_or("?");
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
