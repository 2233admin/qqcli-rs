//! NTQQ message BLOB normalization.
//!
//! QQ NT stores message bodies as a protobuf-style binary BLOB
//! (see `db.rs::extract_text_from_blob` for the existing
//! `[0x82][0x16][varint-len][text]` pattern used by the text-only
//! fallback path).
//!
//! Full protobuf decoding of every element type is **out of scope**
//! for this first pass; instead we walk the BLOB, surface any text
//! we can find, and synthesise a single `Segment` per message:
//!
//! 1. try the existing text extractor; if we get a clean UTF-8 string
//!    that looks like real user content, emit a `Text` segment
//! 2. try JSON decoding; if it parses, dispatch to `_from_json` which
//!    handles elementType taxonomy (1=text, 2=image, 3=file,
//!    4=record, 6=face, 11=mface, 16=forward)
//! 3. otherwise scan for an embedded image / record / file path /
//!    md5 / fileid / URL using regex and produce a best-effort
//!    `Image` or `File` segment
//! 4. last resort: `Unknown` with the reason recorded for forensics
//!
//! Forward recursion is **one level** for now: nested `forward`
//! elements are kept as `Forward` segments with empty `messages`
//! lists, marked `reason="nested forward — not yet expanded"`. The
//! recursive expansion lands in task #3.

use crate::segment::{ForwardNode, MessageWithSegments, Segment};
use regex_lite::Regex;
use serde_json::Value;

/// Public entry point. Accepts a raw NTQQ BLOB (or plain UTF-8 bytes)
/// and returns a fully-populated `MessageWithSegments`.
pub fn normalize_blob_to_segments(raw: &[u8]) -> MessageWithSegments {
    if raw.is_empty() {
        return MessageWithSegments::from_segments(0, vec![]);
    }

    // Pathway 1: real text payload. When a text segment is found, also
    // run the media sweep over the same bytes so a plain-text message
    // that contains an embedded URL / file path gets a second
    // `Image` / `File` segment appended (PY rule #4: do not flatten,
    // preserve segment order).
    if let Some(text_seg) = extract_text_segment(raw) {
        let mut segments = vec![text_seg];
        if let Some(media) = extract_media_segment(raw) {
            segments.push(media);
        }
        return MessageWithSegments::from_segments(raw.len(), segments);
    }

    // Pathway 2: JSON / elementType dispatch.
    if let Some(json_seg) = extract_json_segment(raw) {
        return MessageWithSegments::from_segments(raw.len(), vec![json_seg]);
    }

    // Pathway 3: regex sweep for image / file / record hints.
    if let Some(media_seg) = extract_media_segment(raw) {
        return MessageWithSegments::from_segments(raw.len(), vec![media_seg]);
    }

    // Pathway 4: Unknown fallback.
    MessageWithSegments::from_segments(
        raw.len(),
        vec![Segment::Unknown {
            raw_json: preview_bytes(raw, 128),
            reason: "no recognised payload pattern".to_string(),
        }],
    )
}

// ─── Pathway 1: text ────────────────────────────────────────

fn extract_text_segment(raw: &[u8]) -> Option<Segment> {
    // JSON-shaped payloads are handled by pathway 2; skip them here
    // so the whole literal does not become a single Text segment.
    {
        let s = String::from_utf8_lossy(raw);
        if s.trim_start().starts_with('{') {
            return None;
        }
    }
    // Reuse the existing text extractor without re-implementing the
    // [0x82][0x16] varint walk. We do **not** import it directly to
    // avoid widening `db.rs` public surface; instead mirror the
    // minimal scan inline.
    let text = scan_for_text(raw);
    if text.is_empty() {
        return None;
    }
    // Heuristic: skip anything that doesn't look like a real user
    // message. Bytes with non-printable noise stay in the Unknown
    // pathway where the regex sweep can still try.
    if text.chars().any(|c| c == '\u{FFFD}') {
        return None;
    }
    if text.chars().all(|c| c.is_whitespace()) {
        return None;
    }
    // Binary-only payloads (every byte >= 0x80) cannot be a real user
    // text message; the GBK fallback would otherwise happily decode
    // them into a single weird CJK char. Hand them off to Unknown.
    if raw.iter().all(|b| *b >= 0x80) {
        return None;
    }
    Some(Segment::Text { text })
}

fn scan_for_text(raw: &[u8]) -> String {
    // 1. Try the [0x82][0x16][varint-len][utf8] pattern.
    if let Some(t) = try_0x82_16_pattern(raw) {
        if is_meaningful_text(&t) {
            return t;
        }
    }

    // 2. Try GBK.
    let (decoded, _, _) = encoding_rs::GBK.decode(raw);
    let decoded = decoded.trim();
    if is_meaningful_text(decoded) {
        return decoded.to_string();
    }

    // 3. Try UTF-8 with replacement; only accept if it looks clean.
    let utf8 = String::from_utf8_lossy(raw).trim().to_string();
    if is_meaningful_text(&utf8) {
        return utf8;
    }

    String::new()
}

fn try_0x82_16_pattern(data: &[u8]) -> Option<String> {
    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] == 0x82 && data[i + 1] == 0x16 {
            // read varint
            let mut len: u64 = 0;
            let mut shift = 0u32;
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
            if len > 0 && len < 4096 && j + (len as usize) <= data.len() {
                let slice = &data[j..j + (len as usize)];
                let s = String::from_utf8_lossy(slice).trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        i += 1;
    }
    None
}

fn is_meaningful_text(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let trimmed = s.trim();
    if trimmed.len() < 2 {
        return false;
    }
    let has_chinese = trimmed
        .chars()
        .any(|c| matches!(c as u32, 0x4E00..=0x9FFF | 0x3000..=0x303F | 0xFF00..=0xFFEF));
    let printable_ratio = trimmed
        .chars()
        .filter(|c| c.is_ascii_graphic() || !c.is_ascii())
        .count() as f64
        / trimmed.chars().count() as f64;
    has_chinese || printable_ratio > 0.9
}

// ─── Pathway 2: JSON / elementType ─────────────────────────

fn extract_json_segment(raw: &[u8]) -> Option<Segment> {
    // Only treat the input as JSON if it starts with `{`.
    let text = String::from_utf8_lossy(raw);
    let trimmed = text.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }

    // Try parsing directly first.
    let value: Value = serde_json::from_str(trimmed).ok()?;
    dispatch_json_value(&value)
}

fn dispatch_json_value(v: &Value) -> Option<Segment> {
    // elementType / msgType / type field scan
    let mut element_type_id: Option<i64> = None;
    for key in &["elementType", "msgType", "type"] {
        if let Some(n) = v.get(*key).and_then(|x| x.as_i64()) {
            element_type_id = Some(n);
            break;
        }
    }

    // If the whole payload is just text
    if let Some(text) = v.get("text").and_then(|x| x.as_str()) {
        if text.chars().count() >= 2 {
            return Some(Segment::Text {
                text: text.to_string(),
            });
        }
    }

    match element_type_id {
        Some(1) => text_from_json(v),
        Some(2) => image_from_json(v),
        Some(3) => file_from_json(v),
        Some(4) => record_from_json(v),
        Some(6) => face_from_json(v),
        Some(7) => reply_from_json(v),
        Some(11) => mface_from_json(v),
        Some(16) => Some(forward_from_json(v)),
        _ => None,
    }
}

fn text_from_json(v: &Value) -> Option<Segment> {
    let text = v
        .get("textElement")
        .and_then(|t| t.get("content"))
        .and_then(|t| t.as_str())
        .or_else(|| v.get("content").and_then(|t| t.as_str()))
        .or_else(|| v.get("text").and_then(|t| t.as_str()))?;
    Some(Segment::Text {
        text: text.to_string(),
    })
}

fn image_from_json(v: &Value) -> Option<Segment> {
    let pic = v.get("picElement").unwrap_or(v);
    let url = pic
        .get("originImageUrl")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let fileid = pic
        .get("fileUuid")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let md5 = pic
        .get("md5HexStr")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let size = pic.get("fileSize").and_then(|x| x.as_u64());
    let local_path = pic
        .get("sourcePath")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    if url.is_none() && fileid.is_none() && md5.is_none() && local_path.is_none() {
        return None;
    }
    Some(Segment::Image {
        url,
        fileid,
        md5,
        size,
        local_path,
    })
}

fn file_from_json(v: &Value) -> Option<Segment> {
    let f = v.get("fileElement").unwrap_or(v);
    let name = f
        .get("fileName")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or_else(|| {
            f.get("filePath")
                .and_then(|x| x.as_str())
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "uploaded_file".to_string());
    let url = f
        .get("url")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let fileid = f
        .get("fileUuid")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let size = f.get("fileSize").and_then(|x| x.as_u64());
    let local_path = f
        .get("filePath")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    Some(Segment::File {
        name,
        url,
        fileid,
        size,
        local_path,
    })
}

fn record_from_json(v: &Value) -> Option<Segment> {
    let ptt = v.get("pttElement").unwrap_or(v);
    let url = ptt
        .get("url")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let fileid = ptt
        .get("fileUuid")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let md5 = ptt
        .get("md5HexStr")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let duration = ptt.get("duration").and_then(|x| x.as_u64()).map(|n| n as u32);
    if url.is_none() && fileid.is_none() && md5.is_none() && duration.is_none() {
        return None;
    }
    Some(Segment::Record {
        url,
        fileid,
        md5,
        duration,
    })
}

fn face_from_json(v: &Value) -> Option<Segment> {
    let face = v.get("faceElement").unwrap_or(v);
    let id = face
        .get("faceIndex")
        .and_then(|x| x.as_i64())
        .map(|n| n.to_string())
        .or_else(|| {
            face.get("id")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })?;
    let name = face
        .get("faceName")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    Some(Segment::Face { id, name })
}

fn mface_from_json(v: &Value) -> Option<Segment> {
    let m = v.get("marketFaceElement").unwrap_or(v);
    let id = m
        .get("emojiId")
        .and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_i64().map(|n| n.to_string())))
        .unwrap_or_default();
    let name = m
        .get("faceName")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let url = m
        .get("url")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    if id.is_empty() && name.is_none() && url.is_none() {
        return None;
    }
    Some(Segment::Mface { id, url, name })
}

fn reply_from_json(v: &Value) -> Option<Segment> {
    let r = v.get("replyElement").unwrap_or(v);
    let sender_id = r
        .get("senderUid")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let sender_name = r
        .get("senderName")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let original_msg_id = r
        .get("replayMsgId")
        .or_else(|| r.get("replyMsgId"))
        .or_else(|| r.get("sourceMsgId"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let original_content_preview = r
        .get("content")
        .or_else(|| r.get("summary"))
        .or_else(|| r.get("preview"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Some(Segment::Reply {
        sender_id,
        sender_name,
        original_msg_id,
        original_content_preview,
    })
}

fn forward_from_json(v: &Value) -> Segment {
    // One-level expansion only: we extract the `multiForwardMsgElement`
    // metadata but do **not** recurse into `messages` yet (task #3).
    let fwd = v.get("multiForwardMsgElement").unwrap_or(v);
    let sender_id = fwd
        .get("sender_id")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let sender_name = fwd
        .get("sender_name")
        .or_else(|| fwd.get("senderName"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let mut nodes: Vec<ForwardNode> = Vec::new();
    if let Some(arr) = fwd.get("messages").and_then(|x| x.as_array()) {
        for node in arr {
            if let Some(obj) = node.as_object() {
                let sender_id = obj
                    .get("sender_id")
                    .or_else(|| obj.get("user_id"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let sender_name = obj
                    .get("sender_name")
                    .or_else(|| obj.get("nickname"))
                    .or_else(|| obj.get("name"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = obj
                    .get("timestamp")
                    .or_else(|| obj.get("time"))
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                let segments = if obj.contains_key("forward") || obj.contains_key("messages") {
                    // Nested forward present — record an empty segments
                    // list and mark it via a synthetic Unknown in the
                    // node. The recursive walker in task #3 will fill
                    // this in.
                    vec![Segment::Unknown {
                        raw_json: preview_json(&Value::Object(obj.clone()), 128),
                        reason: "nested forward — not yet expanded".to_string(),
                    }]
                } else {
                    // No nested forward; copy any plain text we find.
                    obj.get("text")
                        .and_then(|x| x.as_str())
                        .map(|t| {
                            vec![Segment::Text {
                                text: t.to_string(),
                            }]
                        })
                        .unwrap_or_default()
                };
                nodes.push(ForwardNode {
                    sender_id,
                    sender_name,
                    timestamp,
                    segments,
                });
            }
        }
    }
    Segment::Forward {
        sender_id,
        sender_name,
        messages: nodes,
    }
}

// ─── Pathway 3: media sweep ─────────────────────────────────

fn extract_media_segment(raw: &[u8]) -> Option<Segment> {
    let text = String::from_utf8_lossy(raw);
    // url: schema detected image url (manual scan, avoids regex-lite
    // quoting issues with the `"` character class).
    let url = find_first_url(&text);
    let md5_re = Regex::new(r"\b[0-9a-fA-F]{32}\b").ok()?;
    let img_ext_re = Regex::new(r"(?i)\.(jpg|jpeg|png|gif|webp|bmp)(\b|/)").ok()?;
    let audio_ext_re = Regex::new(r"(?i)\.(amr|silk|mp3|wav|m4a|ogg)(\b|/)").ok()?;
    let fileid_re = Regex::new(r#"(?i)(file[_]?id|fileuuid)["'\s:=]+([A-Za-z0-9_\-]{8,})"#).ok()?;

    let md5 = md5_re
        .find(&text)
        .map(|m| m.as_str().to_lowercase());
    let fileid = fileid_re
        .captures(&text)
        .and_then(|c| c.get(2))
        .map(|m| m.as_str().to_string());

    if img_ext_re.is_match(&text) {
        return Some(Segment::Image {
            url,
            fileid,
            md5,
            size: None,
            local_path: None,
        });
    }
    if audio_ext_re.is_match(&text) {
        return Some(Segment::Record {
            url,
            fileid,
            md5,
            duration: None,
        });
    }
    if let Some(u) = url {
        // Generic URL-bearing payload: treat as file when no extension
        // hint present.
        return Some(Segment::File {
            name: "uploaded_file".to_string(),
            url: Some(u),
            fileid,
            size: None,
            local_path: None,
        });
    }
    None
}

fn find_first_url(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'h' && bytes.get(i + 1) == Some(&b't') && bytes.get(i + 2) == Some(&b't')
        {
            // http or https
            if (bytes.get(i + 3) == Some(&b'p') || bytes.get(i + 3) == Some(&b's'))
                && bytes.get(i + 4) == Some(&b':') && bytes.get(i + 5) == Some(&b'/')
                && bytes.get(i + 6) == Some(&b'/')
            {
                let start = i;
                let mut j = i + 7;
                while j < bytes.len() {
                    let c = bytes[j];
                    if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0
                        || c == b'"' || c == b'\'' || c == b'<' || c == b'>'
                    {
                        break;
                    }
                    j += 1;
                }
                if j > start + 7 {
                    return Some(String::from_utf8_lossy(&bytes[start..j]).to_string());
                }
            }
        }
        i += 1;
    }
    None
}

// ─── helpers ────────────────────────────────────────────────

fn preview_bytes(raw: &[u8], max: usize) -> String {
    let n = raw.len().min(max);
    let mut s = String::with_capacity(n * 2);
    for &b in &raw[..n] {
        if b.is_ascii_graphic() || b == b' ' {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{:02x}", b));
        }
    }
    s
}

fn preview_json(v: &Value, max: usize) -> String {
    let s = v.to_string();
    if s.len() <= max {
        s
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::inline_token;

    #[test]
    fn text_only() {
        // 0x82 0x16 0x05 hello
        let mut raw = vec![0x82, 0x16, 0x05];
        raw.extend_from_slice(b"hello");
        let mws = normalize_blob_to_segments(&raw);
        assert_eq!(mws.segments.len(), 1);
        match &mws.segments[0] {
            Segment::Text { text } => assert_eq!(text, "hello"),
            other => panic!("expected Text, got {:?}", other),
        }
        assert_eq!(mws.primary_type, "文本");
        assert_eq!(mws.content_inline, "hello");
    }

    #[test]
    fn image_with_text() {
        let raw = b"some caption text and then https://example.com/pic.jpg abc123def456abc123def456abc123def fileUuid AbCdEfGhIjKlMn1234";
        let mws = normalize_blob_to_segments(raw);
        assert!(!mws.segments.is_empty(), "expected at least one segment");
        // First pathway is the text extractor; if it picks up only the
        // URL tail that's still fine. We just need the image to be
        // recognised somewhere.
        let has_image = mws.segments.iter().any(|s| matches!(s, Segment::Image { .. }));
        assert!(has_image, "expected at least one Image segment, got {:?}", mws.segments);
    }

    #[test]
    fn forward_one_level() {
        let json = serde_json::json!({
            "elementType": 16,
            "multiForwardMsgElement": {
                "sender_id": "u0",
                "sender_name": "Bob",
                "messages": [
                    {"sender_id": "u1", "sender_name": "Alice", "timestamp": 123, "text": "hi"},
                    {"sender_id": "u2", "sender_name": "Carol", "timestamp": 456, "text": "yo"}
                ]
            }
        });
        let raw = serde_json::to_string(&json).unwrap();
        let mws = normalize_blob_to_segments(raw.as_bytes());
        assert_eq!(mws.segments.len(), 1);
        match &mws.segments[0] {
            Segment::Forward { sender_name, messages, .. } => {
                assert_eq!(sender_name.as_deref(), Some("Bob"));
                assert_eq!(messages.len(), 2);
                assert_eq!(messages[0].sender_name, "Alice");
                assert_eq!(inline_token(&messages[0].segments[0]).as_deref(), Some("hi"));
            }
            other => panic!("expected Forward, got {:?}", other),
        }
        assert_eq!(mws.primary_type, "转发");
    }

    #[test]
    fn unknown_fallback() {
        // 4 high bytes that are invalid UTF-8 and that don't match any
        // image / audio / file extension. The pipeline must give up
        // and emit an Unknown segment.
        let raw = [0xFFu8, 0xFE, 0xFD, 0xFC];
        let mws = normalize_blob_to_segments(&raw);
        assert!(matches!(&mws.segments[0], Segment::Unknown { .. }), "got {:?}", mws.segments);
        assert_eq!(mws.primary_type, "未知");
    }

    #[test]
    fn json_text_dispatch() {
        let raw = br#"{"elementType":1,"textElement":{"content":"hi"}}"#;
        let mws = normalize_blob_to_segments(raw);
        assert!(matches!(&mws.segments[0], Segment::Text { text } if text == "hi"));
    }

    #[test]
    fn json_image_dispatch() {
        let raw = br#"{"elementType":2,"picElement":{"fileUuid":"u1","md5HexStr":"abc","originImageUrl":"https://x/y.jpg","sourcePath":"C:/Pic/y.jpg"}}"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::Image { url, fileid, md5, local_path, .. } => {
                assert_eq!(url.as_deref(), Some("https://x/y.jpg"));
                assert_eq!(fileid.as_deref(), Some("u1"));
                assert_eq!(md5.as_deref(), Some("abc"));
                assert_eq!(local_path.as_deref(), Some("C:/Pic/y.jpg"));
            }
            other => panic!("expected Image, got {:?}", other),
        }
    }
}
