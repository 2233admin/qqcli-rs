//! NapCat OneBot11 WebSocket client
//!
//! Connects to NapCat via WebSocket (ws://127.0.0.1:18301) and exposes
//! the core OneBot11 API: friend list, group list, message history, and send.

pub mod ipc_client;
pub mod models;

use crate::napcat::models::{
    FriendInfo, GroupInfo, JsonRpcRequest, LoginInfo, MessageEvent, MessageInfo, SendResult,
    VersionInfo,
};
use anyhow::{anyhow, bail, Result};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};

static NEXT_ECHO: AtomicU64 = AtomicU64::new(1);

fn next_echo() -> u64 {
    NEXT_ECHO.fetch_add(1, Ordering::SeqCst)
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value>>>>>;

// ─── Outbound command ─────────────────────────────────────────

enum Outgoing {
    /// RPC call — reply is delivered via the shared PendingMap
    Rpc { payload: String },
}

// ─── Client ──────────────────────────────────────────────────

pub struct NapcatClient {
    cmd_tx: mpsc::Sender<Outgoing>,
    _pending: PendingMap,
}

impl NapcatClient {
    /// Connect to a NapCat WebSocket endpoint.
    /// Example: ws://127.0.0.1:18301
    pub async fn connect(url: &str, access_token: Option<&str>) -> Result<Self> {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        let parsed_url = if let Some(token) = access_token {
            format!("{}?access_token={}", url.trim_end_matches('/'), token)
        } else {
            url.to_string()
        };

        eprintln!("[NapCat] connecting to {} ...", parsed_url);

        // Handshake with 10s timeout
        let ws_stream = tokio::time::timeout(std::time::Duration::from_secs(10), connect_async(&parsed_url))
            .await
            .map_err(|_| anyhow!("连接超时 (10s)，请确认 NapCat WebSocket 已启动且端口可访问"))?
            .map_err(|e| anyhow!("WebSocket 连接失败: {}", e))?
            .0;

        eprintln!("[NapCat] 已连接!");

        let (mut write, mut read) = ws_stream.split();
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<Outgoing>(64);
        let pending_reader = pending.clone();

        // Reader loop — routes RPC responses via echo and logs async events
        let read_task = tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(t)) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                            if let Some(echo_val) = v.get("echo") {
                                if let Some(echo) = echo_val.as_u64() {
                                    let mut map = pending_reader.lock().await;
                                    if let Some(tx) = map.remove(&echo) {
                                        let _ = tx.send(Ok(v));
                                        continue;
                                    }
                                }
                            }
                            // Async event push
                            if let Ok(evt) = serde_json::from_value::<MessageEvent>(v) {
                                if evt.post_type == "message" {
                                    eprintln!(
                                        "[NapCat event] type={:?} from={:?} msg={:?}",
                                        evt.message_type,
                                        evt.user_id,
                                        evt.raw_message.as_ref().map(|s| s.as_str())
                                    );
                                }
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        // tungstenite auto-handles ping/pong
                        let _ = data;
                    }
                    Ok(Message::Close(..)) => break,
                    Err(e) => {
                        eprintln!("[NapCat] read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            eprintln!("[NapCat] reader loop ended");
        });

        // Writer loop — sends RPC calls and heartbeats
        let _write_task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    Some(out) = cmd_rx.recv() => {
                        let Outgoing::Rpc { payload } = out;
                        if write.send(Message::Text(payload.into())).await.is_err() {
                            eprintln!("[NapCat] write error");
                            break;
                        }
                    }
                    _ = ticker.tick() => {
                        let ping = r#"{"action":".ping","params":{}}"#;
                        if write.send(Message::Text(ping.into())).await.is_err() {
                            eprintln!("[NapCat] heartbeat failed");
                            break;
                        }
                    }
                }
            }
            eprintln!("[NapCat] writer loop ended");
            read_task.abort();
        });

        Ok(Self { cmd_tx, _pending: pending })
    }

    /// Issue an RPC call and wait for its response.
    async fn rpc(&self, action: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let echo = next_echo();
        let req = JsonRpcRequest::new(action, params).with_echo(serde_json::json!(echo));
        let payload = serde_json::to_string(&req)?;

        let (tx, rx) = oneshot::channel();
        {
            let mut map = self._pending.lock().await;
            map.insert(echo, tx);
        }

        self.cmd_tx
            .send(Outgoing::Rpc { payload })
            .await
            .map_err(|_| anyhow!("writer loop ended"))?;

        rx.await.map_err(|_| anyhow!("response channel dropped"))?
    }

    /// Get self login info
    pub async fn get_login_info(&self) -> Result<LoginInfo> {
        let resp: serde_json::Value = self.rpc("get_login_info", serde_json::json!({})).await?;
        let data = resp.get("data").ok_or_else(|| anyhow!("no data in login_info response"))?;
        serde_json::from_value(data.clone())
            .map_err(|e| anyhow!("parse login_info failed: {} — data={:?}", e, data))
    }

    /// Get NapCat version info
    pub async fn get_version_info(&self) -> Result<VersionInfo> {
        let resp: serde_json::Value = self.rpc("get_version_info", serde_json::json!({})).await?;
        let data = resp.get("data").ok_or_else(|| anyhow!("no data in version_info response"))?;
        serde_json::from_value(data.clone())
            .map_err(|e| anyhow!("parse version_info failed: {} — data={:?}", e, data))
    }

    /// Get friend list
    pub async fn get_friend_list(&self) -> Result<Vec<FriendInfo>> {
        let resp = self.rpc("get_friend_list", serde_json::json!({})).await?;
        let data = resp.get("data").ok_or_else(|| anyhow!("no data in friend_list response"))?;
        let list = data
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("get_friend_list did not return an array: {:?}", data))?;
        list.into_iter()
            .map(|v| serde_json::from_value(v).map_err(|e| anyhow!("parse friend: {}", e)))
            .collect()
    }

    /// Get group list
    pub async fn get_group_list(&self) -> Result<Vec<GroupInfo>> {
        let resp = self.rpc("get_group_list", serde_json::json!({})).await?;
        let data = resp.get("data").ok_or_else(|| anyhow!("no data in group_list response"))?;
        let list = data
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("get_group_list did not return an array: {:?}", data))?;
        list.into_iter()
            .map(|v| serde_json::from_value(v).map_err(|e| anyhow!("parse group: {}", e)))
            .collect()
    }

    /// Get chat history via get_msg_history
    ///
    /// `msg_type`: "private" | "group"
    /// `user_id`: QQ号 (私聊) 或 群号 (群聊)
    pub async fn get_msg_history(
        &self,
        msg_type: &str,
        user_id: i64,
        group_id: Option<i64>,
        count: usize,
        last_msg_id: Option<i64>,
    ) -> Result<Vec<MessageInfo>> {
        let mut params = serde_json::json!({
            "message_type": msg_type,
            "user_id": user_id,
            "count": count,
        });
        if let Some(gid) = group_id {
            params["group_id"] = serde_json::json!(gid);
        }
        if let Some(lmid) = last_msg_id {
            params["last_msg_id"] = serde_json::json!(lmid);
        }

        let resp = self.rpc("get_msg_history", params).await?;
        let data = resp.get("data").ok_or_else(|| anyhow!("no data in get_msg_history response"))?;
        let arr = data
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("get_msg_history did not return an array: {:?}", data))?;
        arr.into_iter()
            .map(|v| serde_json::from_value(v).map_err(|e| anyhow!("parse message: {}", e)))
            .collect()
    }

    /// Send a private message
    pub async fn send_private_msg(&self, user_id: i64, message: &str) -> Result<SendResult> {
        let params = serde_json::json!({
            "user_id": user_id,
            "message": message,
        });
        let resp = self.rpc("send_private_msg", params).await?;
        let data = resp.get("data").ok_or_else(|| anyhow!("no data in send_private_msg response"))?;
        serde_json::from_value(data.clone())
            .map_err(|e| anyhow!("parse send_private_msg result: {} — data={:?}", e, data))
    }

    /// Send a group message
    pub async fn send_group_msg(&self, group_id: i64, message: &str) -> Result<SendResult> {
        let params = serde_json::json!({
            "group_id": group_id,
            "message": message,
        });
        let resp = self.rpc("send_group_msg", params).await?;
        let data = resp.get("data").ok_or_else(|| anyhow!("no data in send_group_msg response"))?;
        serde_json::from_value(data.clone())
            .map_err(|e| anyhow!("parse send_group_msg result: {} — data={:?}", e, data))
    }
}

// ─── CLI command wrappers ────────────────────────────────────

const DEFAULT_NAPCAT_URL: &str = "ws://127.0.0.1:18301";

/// Run a napcat subcommand: whoami | friends | groups | history | send
pub async fn run(sub: &str, url: &str, token: Option<&str>, args: &[&str]) -> Result<()> {
    let url = if url.is_empty() { DEFAULT_NAPCAT_URL } else { url };
    let client = NapcatClient::connect(url, token).await?;

    match sub {
        "whoami" => cmd_whoami(&client).await,
        "friends" => cmd_friends(&client).await,
        "groups" => cmd_groups(&client).await,
        "history" => cmd_history(&client, args).await,
        "send" => cmd_send(&client, args).await,
        _ => {
            eprintln!("未知子命令: {}", sub);
            eprintln!("可用: whoami | friends | groups | history | send");
            bail!("unknown subcommand");
        }
    }
}

async fn cmd_whoami(client: &NapcatClient) -> Result<()> {
    let info = client.get_login_info().await?;
    println!("登录账号: {} ({})", info.nickname, info.user_id);
    let ver = client.get_version_info().await?;
    println!("NapCat: {} | 协议: {}", ver.impl_name, ver.protocol_version);
    println!("App版本: {}", ver.app_version);
    Ok(())
}

async fn cmd_friends(client: &NapcatClient) -> Result<()> {
    let friends = client.get_friend_list().await?;
    if friends.is_empty() {
        println!("(无好友)");
        return Ok(());
    }
    println!("=== 好友列表 ({}个) ===", friends.len());
    for f in &friends {
        if let Some(tag) = &f.remark {
            if !tag.is_empty() {
                println!("- {} ({}) [{}]", f.nickname, f.user_id, tag);
                continue;
            }
        }
        println!("- {} ({})", f.nickname, f.user_id);
    }
    Ok(())
}

async fn cmd_groups(client: &NapcatClient) -> Result<()> {
    let groups = client.get_group_list().await?;
    if groups.is_empty() {
        println!("(无群)");
        return Ok(());
    }
    println!("=== 群列表 ({}个) ===", groups.len());
    for g in &groups {
        println!("- {} ({})", g.group_name, g.group_id);
    }
    Ok(())
}

async fn cmd_history(client: &NapcatClient, args: &[&str]) -> Result<()> {
    let msg_type = args
        .first()
        .ok_or_else(|| anyhow!("用法: napcat history <private|group> <target_id> [count=20]"))?;
    let target_id: i64 = args
        .get(1)
        .ok_or_else(|| anyhow!("用法: napcat history <private|group> <target_id> [count=20]"))?
        .parse()
        .map_err(|_| anyhow!("target_id 必须是数字"))?;
    let count: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);

    let group_id: Option<i64> = if *msg_type == "group" {
        Some(target_id)
    } else {
        None
    };

    let messages = client
        .get_msg_history(msg_type, target_id, group_id, count, None)
        .await?;

    if messages.is_empty() {
        println!("(无消息)");
        return Ok(());
    }

    for m in &messages {
        let ts = chrono::DateTime::from_timestamp(m.time, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| m.time.to_string());
        println!("[{}] {}: {}", ts, m.sender.nickname, m.text());
    }
    Ok(())
}

async fn cmd_send(client: &NapcatClient, args: &[&str]) -> Result<()> {
    let msg_type = args
        .first()
        .ok_or_else(|| anyhow!("用法: napcat send <private|group> <target_id> <message...>"))?;
    let target_id: i64 = args
        .get(1)
        .ok_or_else(|| anyhow!("用法: napcat send <private|group> <target_id> <message...>"))?
        .parse()
        .map_err(|_| anyhow!("target_id 必须是数字"))?;
    let message = args
        .get(2..)
        .ok_or_else(|| anyhow!("用法: napcat send <private|group> <target_id> <message...>"))?
        .join(" ");

    if message.is_empty() {
        bail!("消息内容不能为空");
    }

    let result = match *msg_type {
        "private" => client.send_private_msg(target_id, &message).await,
        "group" => client.send_group_msg(target_id, &message).await,
        _ => bail!("msg_type 必须是 private 或 group"),
    }?;

    println!("发送成功: message_id={:?}", result.message_id);
    Ok(())
}
