//! OneBot11 data models for NapCat

use serde::{Deserialize, Serialize};

// ─── Friend / Group List ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendInfo {
    pub user_id: i64,
    pub nickname: String,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfo {
    pub group_id: i64,
    pub group_name: String,
    pub member_count: Option<i64>,
    pub max_member_count: Option<i64>,
}

// ─── Chat History ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageInfo {
    pub message_id: i64,
    pub real_id: Option<i64>,
    pub message_type: String, // "private" | "group"
    pub sender: SenderInfo,
    pub time: i64,
    pub content: String,
    pub raw_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderInfo {
    pub user_id: Option<i64>,
    pub nickname: String,
    pub card: Option<String>,
}

impl MessageInfo {
    /// Extract plain text from the `content` field.
    /// Handles JSON segment format (OneBot 11) and CQ-code strings.
    pub fn text(&self) -> String {
        // Try JSON array form first (OneBot segment format)
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&self.content) {
            if let Some(segs) = v.as_array() {
                let mut out = String::new();
                for seg in segs {
                    if let Some(t) = seg.get("data").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                        out.push_str(t);
                    } else if let Some(t) = seg.as_str() {
                        out.push_str(t);
                    }
                }
                if !out.is_empty() {
                    return out;
                }
            }
        }
        // CQ code stripping fallback
        let mut s = self.content.replace('\u{200b}', " ");
        s = s.replace('\u{3000}', " ");
        for (pat, repl) in [
            (r"\[CQ:at,[^\]]+\]", ""),
            (r"\[CQ:image,[^\]]+\]", "[图片]"),
            (r"\[CQ:face,[^\]]+\]", ""),
            (r"\[CQ:reply,[^\]]+\]", ""),
            (r"\[CQ:[^,\]]+,[^\]]+\]", ""),
        ] {
            if let Ok(re) = regex_lite::Regex::new(pat) {
                s = re.replace_all(&s, repl).into_owned();
            }
        }
        s.trim().to_string()
    }
}

// ─── Send API ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResult {
    pub message_id: Option<i64>,
    pub message_seq: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    #[serde(rename = "app_name")]
    pub impl_name: String,
    pub protocol_version: String,
    #[serde(rename = "app_version")]
    pub app_version: String,
    #[serde(rename = "coolq_edition")]
    pub coolq_edition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginInfo {
    pub user_id: i64,
    pub nickname: String,
}

// ─── Event Payloads ────────────────────────────────────────

/// Incoming message event from NapCat (reverse ws push)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    pub post_type: String,
    #[serde(rename = "message_type")]
    pub message_type: Option<String>,
    pub sub_type: Option<String>,
    pub message_id: Option<i64>,
    pub user_id: Option<i64>,
    pub group_id: Option<i64>,
    pub message: Option<String>,
    pub raw_message: Option<String>,
    pub font: Option<i64>,
    pub sender: Option<SenderInfo>,
    pub time: Option<i64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ─── Internal Request ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub action: String,
    #[serde(rename = "params")]
    pub params: serde_json::Value,
    #[serde(rename = "echo")]
    pub echo: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(action: &str, params: serde_json::Value) -> Self {
        Self { action: action.to_string(), params, echo: None }
    }

    pub fn with_echo(mut self, echo: serde_json::Value) -> Self {
        self.echo = Some(echo);
        self
    }
}
