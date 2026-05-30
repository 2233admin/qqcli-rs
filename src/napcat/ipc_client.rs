//! NapCat IPC Plugin client for direct wrapper access
//! Alternative to WebSocket - lower overhead, direct API access

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

const IPC_PORT: u16 = 9334;
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct NapcatIpcClient {
    port: u16,
    stream: Mutex<Option<TcpStream>>,
    request_id: Mutex<u64>,
}

impl NapcatIpcClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            port: IPC_PORT,
            stream: Mutex::new(None),
            request_id: Mutex::new(0),
        })
    }

    pub fn with_port(port: u16) -> Result<Self> {
        Ok(Self {
            port,
            stream: Mutex::new(None),
            request_id: Mutex::new(0),
        })
    }

    fn connect(&self) -> Result<()> {
        let mut stream_guard = self.stream.lock().unwrap();
        if stream_guard.is_none() {
            let addr = format!("127.0.0.1:{}", self.port);
            let stream = TcpStream::connect_timeout(&addr.parse().context("Invalid IP")?, TIMEOUT)
                .context(format!("Failed to connect to IPC server at {}", addr))?;
            stream.set_read_timeout(Some(TIMEOUT))?;
            stream.set_write_timeout(Some(TIMEOUT))?;
            *stream_guard = Some(stream);
        }
        Ok(())
    }

    fn send_request(&self, action: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        self.connect()?;

        let mut id = self.request_id.lock().unwrap();
        *id += 1;
        let req_id = *id;
        drop(id);

        let request = serde_json::json!({
            "action": action,
            "id": req_id,
            "params": params
        });

        let request_str = serde_json::to_string(&request).context("Serialize request")?;
        let mut stream_guard = self.stream.lock().unwrap();
        let stream = stream_guard.as_mut().context("Not connected")?;

        stream.write_all(request_str.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()?;

        // Read response
        let mut response_buf = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).context("Read response")?;
            if n == 0 {
                break;
            }
            response_buf.extend_from_slice(&buf[..n]);
            if let Ok(response) = serde_json::from_slice::<serde_json::Value>(&response_buf) {
                return Ok(response);
            }
        }

        anyhow::bail!("Invalid response from IPC server")
    }

    pub fn ping(&self) -> Result<bool> {
        let resp = self.send_request("ping", serde_json::json!({}))?;
        let success = resp
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Ok(success)
    }

    #[allow(dead_code)]
    pub fn get_login_info(&self) -> Result<serde_json::Value> {
        self.send_request("get_login_info", serde_json::json!({}))
    }

    #[allow(dead_code)]
    pub fn get_login_list(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self.send_request("get_login_list", serde_json::json!({}))?;
        let result = resp.get("result").context("No result")?;
        Ok(result.as_array().cloned().unwrap_or_default())
    }

    pub fn send_private_msg(&self, uin: &str, message: &str) -> Result<serde_json::Value> {
        self.send_request(
            "send_private_msg",
            serde_json::json!({
                "uin": uin,
                "message": message
            }),
        )
    }

    pub fn send_group_msg(&self, group_id: &str, message: &str) -> Result<serde_json::Value> {
        self.send_request(
            "send_group_msg",
            serde_json::json!({
                "groupId": group_id,
                "message": message
            }),
        )
    }

    pub fn get_friend_list(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self.send_request("get_friend_list", serde_json::json!({}))?;
        let result = resp.get("result").context("No result")?;
        Ok(result.as_array().cloned().unwrap_or_default())
    }

    pub fn get_group_list(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self.send_request("get_group_list", serde_json::json!({}))?;
        let result = resp.get("result").context("No result")?;
        Ok(result.as_array().cloned().unwrap_or_default())
    }

    pub fn get_group_members(&self, group_id: &str) -> Result<Vec<serde_json::Value>> {
        let resp = self.send_request(
            "get_group_members",
            serde_json::json!({
                "groupId": group_id
            }),
        )?;
        let result = resp.get("result").context("No result")?;
        Ok(result.as_array().cloned().unwrap_or_default())
    }

    pub fn get_recent_chats(&self) -> Result<Vec<serde_json::Value>> {
        let resp = self.send_request("get_recent_chats", serde_json::json!({}))?;
        let result = resp.get("result").context("No result")?;
        Ok(result.as_array().cloned().unwrap_or_default())
    }
}

impl Default for NapcatIpcClient {
    fn default() -> Self {
        Self::new().expect("Failed to create IPC client")
    }
}
