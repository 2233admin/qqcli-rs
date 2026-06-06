//! Message Segment types
//!
//! 9-variant taxonomy aligned with OneBot 11 message segments.
//! Each variant captures the minimum fields needed for downstream
//! analysis (image / record / file md5, sticker id, reply preview,
//! forwarded node list, etc.) without flattening into a single
//! inline string.
//!
//! See `D:/projects/_tools/qq-data-exporter/src/qq_data_core/normalize.py`
//! for the reference Python implementation and full field semantics.

use serde::{Deserialize, Serialize};

/// One parsed QQ message segment. `tag = "type"` so JSON looks like
/// `{"type":"text","text":"hi"}` (OneBot 11 style).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Segment {
    /// Plain UTF-8 text.
    Text { text: String },

    /// Image. At least one of `url` / `fileid` / `local_path` is usually populated.
    Image {
        url: Option<String>,
        fileid: Option<String>,
        md5: Option<String>,
        size: Option<u64>,
        local_path: Option<String>,
    },

    /// Voice / "ptt" / silk record.
    Record {
        url: Option<String>,
        fileid: Option<String>,
        md5: Option<String>,
        duration: Option<u32>,
    },

    /// Uploaded file (non-image binary).
    File {
        name: String,
        url: Option<String>,
        fileid: Option<String>,
        size: Option<u64>,
        local_path: Option<String>,
    },

    /// Built-in face (text-style emoji like `[表情xxx]`).
    Face { id: String, name: Option<String> },

    /// Market / sticker / animated face (large GIF).
    Mface {
        id: String,
        url: Option<String>,
        name: Option<String>,
    },

    /// Reply / quote reference to a previous message.
    Reply {
        sender_id: String,
        sender_name: String,
        original_msg_id: String,
        original_content_preview: String,
    },

    /// @mention of a peer.
    At {
        target_id: String,
        target_name: Option<String>,
    },

    /// Merged-forwarded chat bundle. The contained `Vec<ForwardNode>`
    /// is heap-allocated, so the recursion stays at a finite compile-
    /// time size even when forwards nest inside forwards.
    Forward {
        sender_id: Option<String>,
        sender_name: Option<String>,
        messages: Vec<ForwardNode>,
    },

    /// Fallback for elements we failed to classify. `raw_json` keeps a
    /// compact debug snapshot; `reason` is a short string explaining
    /// why we fell through.
    Unknown { raw_json: String, reason: String },
}

/// One node inside a merged-forward bundle. `segments` may itself
/// contain nested `Forward` (recursion through `Vec<Segment>` is fine
/// because the segment list is heap-allocated).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForwardNode {
    pub sender_id: String,
    pub sender_name: String,
    pub timestamp: i64,
    pub segments: Vec<Segment>,
}

/// Convenience string used by `content_inline` when rendering media
/// placeholders, e.g. `"[image:foo.jpg]"`. Mirrors the PY exporter's
/// inline token convention.
pub fn inline_token(seg: &Segment) -> Option<String> {
    match seg {
        Segment::Text { text } => Some(text.clone()),
        Segment::Image { local_path, .. } => {
            let name = local_path
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("image");
            Some(format!("[image:{}]", name))
        }
        Segment::Record { .. } => Some("[speech audio]".to_string()),
        Segment::File { name, .. } => Some(format!("[uploaded_file_name:{}]", name)),
        Segment::Face { id, name } => Some(format!(
            "[emoji:id={}]",
            name.as_deref().unwrap_or(id.as_str())
        )),
        Segment::Mface { name, id, .. } => Some(format!(
            "[sticker:summary={},emoji_id={}]",
            name.as_deref().unwrap_or("sticker"),
            id
        )),
        Segment::Reply {
            original_content_preview,
            ..
        } => Some(format!("[reply:{}]", original_content_preview)),
        Segment::At {
            target_name, target_id, ..
        } => Some(format!(
            "@{}",
            target_name.as_deref().unwrap_or(target_id.as_str())
        )),
        Segment::Forward { sender_name, .. } => Some(format!(
            "[forward message{}]",
            sender_name
                .as_deref()
                .map(|n| format!(" from {}", n))
                .unwrap_or_default()
        )),
        Segment::Unknown { reason, .. } => Some(format!("[unsupported:{}]", reason)),
    }
}

/// Primary segment type, used by `db.rs::detect_type` compatibility
/// layer. The old Chinese label is kept so old CLI output stays stable.
pub fn primary_label(seg: &Segment) -> &'static str {
    match seg {
        Segment::Text { .. } => "文本",
        Segment::Image { .. } => "图片",
        Segment::Record { .. } => "语音",
        Segment::File { .. } => "文件",
        Segment::Face { .. } | Segment::Mface { .. } => "表情",
        Segment::Reply { .. } => "回复",
        Segment::At { .. } => "@",
        Segment::Forward { .. } => "转发",
        Segment::Unknown { .. } => "未知",
    }
}

/// A full message BLOB plus its parsed segment view. The first segment
/// that is not `Unknown` is treated as the primary type; if all are
/// `Unknown`, primary_type is `"未知"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageWithSegments {
    /// Original BLOB length, kept for debugging / forensic.
    #[serde(default)]
    pub raw_bytes_len: usize,
    /// Ordered segment list (not flattened, per PY rule #4).
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// Inline view: text segments joined; media segments replaced by
    /// placeholders. Stable for downstream analysis.
    #[serde(default)]
    pub content_inline: String,
    /// Primary type for backward compat with `db.rs::detect_type`.
    #[serde(default)]
    pub primary_type: String,
}

impl MessageWithSegments {
    /// Build a `MessageWithSegments` from a finalised segment list.
    /// Order is preserved; primary_type is the first non-Unknown.
    pub fn from_segments(raw_len: usize, segments: Vec<Segment>) -> Self {
        let content_inline = segments
            .iter()
            .filter_map(inline_token)
            .collect::<Vec<_>>()
            .join(" ");
        let primary_type = segments
            .iter()
            .find(|s| !matches!(s, Segment::Unknown { .. }))
            .map(primary_label)
            .unwrap_or("未知")
            .to_string();
        Self {
            raw_bytes_len: raw_len,
            segments,
            content_inline,
            primary_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip_json() {
        let seg = Segment::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_string(&seg).unwrap();
        assert_eq!(json, r#"{"type":"text","text":"hello"}"#);
        let back: Segment = serde_json::from_str(&json).unwrap();
        assert_eq!(back, seg);
    }

    #[test]
    fn image_snake_case_field_names() {
        let seg = Segment::Image {
            url: Some("https://x".into()),
            fileid: Some("abc".into()),
            md5: Some("0".into()),
            size: Some(123),
            local_path: Some("C:/foo.jpg".into()),
        };
        let v: serde_json::Value = serde_json::to_value(&seg).unwrap();
        assert_eq!(v["type"], "image");
        assert_eq!(v["local_path"], "C:/foo.jpg");
        assert_eq!(v["fileid"], "abc");
    }

    #[test]
    fn forward_nested_segments() {
        let node = ForwardNode {
            sender_id: "u1".into(),
            sender_name: "Alice".into(),
            timestamp: 0,
            segments: vec![Segment::Text {
                text: "nested".into(),
            }],
        };
        let fwd = Segment::Forward {
            sender_id: Some("u0".into()),
            sender_name: Some("Bob".into()),
            messages: vec![node],
        };
        let v: serde_json::Value = serde_json::to_value(&fwd).unwrap();
        assert_eq!(v["type"], "forward");
        assert_eq!(v["messages"][0]["sender_name"], "Alice");
    }

    #[test]
    fn primary_label_and_inline_token() {
        let msgs = vec![
            Segment::Text {
                text: "hi".into(),
            },
            Segment::Image {
                url: None,
                fileid: None,
                md5: None,
                size: None,
                local_path: Some("C:/Pic/foo.jpg".into()),
            },
        ];
        let mws = MessageWithSegments::from_segments(0, msgs);
        assert_eq!(mws.primary_type, "文本");
        assert!(mws.content_inline.contains("[image:foo.jpg]"));
        assert!(mws.content_inline.contains("hi"));
    }
}
