//! NTQQ message BLOB normalization.
//!
//! QQ NT stores message bodies as a protobuf-style binary BLOB
//! (see `db.rs::extract_text_from_blob` for the existing
//! `[0x82][0x16][varint-len][text]` pattern used by the text-only
//! fallback path). This module turns those BLOBs (and a few related
//! payload shapes) into a structured `Vec<Segment>` view.
//!
//! Five entry pathways, tried in order, exactly one wins per call:
//!
//! 1. **Text** — when a clean UTF-8 string is extractable from the
//!    BLOB. We also run the media sweep over the same bytes so a
//!    plain-text message that contains an embedded URL / file path
//!    gets a second `Image` / `File` segment appended
//!    (PY rule #4: do not flatten, preserve segment order).
//! 2. **JSON elementType dispatch** — when the bytes parse as JSON
//!    and look like a QQ NT element
//!    (`{"elementType": N, "element": {...}}` or
//!    `{"msgType": N, "msgBody": {...}}`). Handles elementType
//!    taxonomy 1/2/3/4/6/7/8/11/16; unknown ids fall through to
//!    `Segment::Unknown` with `reason="unhandled elementType N"`.
//! 3. **Media sweep** — regex/byte scan for image / audio / file
//!    extension hints or HTTP URLs. Used as a fallback when neither
//!    text nor JSON can be extracted.
//! 4. **Unknown** — last resort. Stores a 128-byte preview and a
//!    `reason` string for forensics.
//! 5. **OneBot 11 segment array** — exposed via
//!    [`normalize_onebot_message`] and [`normalize_onebot_array`] for
//!    callers that already have a OneBot 11 segment array payload
//!    (NapCat HTTP responses, manual JSON exports, etc.). Shapes
//!    `{"type":"text","data":{...}}` / `{"message":[...]} /
//!    `{"message":"plain text"}` are all accepted.
//!
//! Forward recursion is **one level** for now: nested `forward`
//! elements are kept as `Forward` segments with the embedded
//! `multiForwardMsgElement.xmlContent` preview parsed, but
//! `messages` lists may contain synthetic `Unknown` placeholders
//! that task #3 will recursively expand.

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

/// Public entry point for OneBot 11 segment array payloads. Accepts
/// the full message dict (`{"message": [...]}` or
/// `{"message": "plain string"}`); returns a `MessageWithSegments`.
///
/// See module-level docs, pathway 5, for the supported shapes.
#[allow(dead_code)] // wired into forward recursion (task #3) and NapCat client (task #5)
pub fn normalize_onebot_message(message: &Value) -> MessageWithSegments {
    let segments = normalize_onebot_array(message);
    MessageWithSegments::from_segments(message.to_string().len(), segments)
}

/// Variant that returns just the segment list (useful for forward
/// recursion in task #3 where we already track raw length separately).
#[allow(dead_code)] // wired into forward recursion (task #3) and NapCat client (task #5)
pub fn normalize_onebot_array(message: &Value) -> Vec<Segment> {
    let payload = message.get("message");
    match payload {
        Some(Value::Array(arr)) => dispatch_onebot_segments(arr),
        Some(Value::String(s)) if !s.is_empty() => vec![Segment::Text { text: s.clone() }],
        // `raw_message` shape (QQ exporter passthrough) — surface as
        // Unknown so the caller can see what shape it actually had.
        _ => vec![Segment::Unknown {
            raw_json: preview_json(message, 128),
            reason: "onebot: missing message array / string".to_string(),
        }],
    }
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
        Some(8) => at_from_json(v),
        Some(11) => mface_from_json(v),
        Some(16) => Some(forward_from_json(v)),
        Some(n) => Some(unknown_element_type(n, v)),
        _ => None,
    }
}

fn unknown_element_type(element_type: i64, v: &Value) -> Segment {
    Segment::Unknown {
        raw_json: preview_json(v, 128),
        reason: format!("unhandled elementType {}", element_type),
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
    let fileid = extract_id(pic, &["fileUuid", "fileid"]);
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
    let fileid = extract_id(f, &["fileUuid", "fileid"]);
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
    let fileid = extract_id(ptt, &["fileUuid", "fileid"]);
    let md5 = ptt
        .get("md5HexStr")
        .or_else(|| ptt.get("fileMd5"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    // PY reads `fileTime` first; fall back to `duration`. QQ NT
    // measures `fileTime` in seconds; some SDK builds emit
    // milliseconds, which `normalise_ptt_duration` corrects.
    let duration = ptt
        .get("fileTime")
        .or_else(|| ptt.get("duration"))
        .and_then(|x| x.as_u64())
        .map(normalise_ptt_duration);
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

/// QQ NT historically reports `fileTime` in seconds, but a handful of
/// SDK builds report it in milliseconds. We use the threshold of
/// 10 minutes (600 s) as the discriminator: a real ptt blob longer
/// than 10 minutes is vanishingly rare, so anything > 600 is
/// almost certainly ms-scale and gets divided by 1000.
fn normalise_ptt_duration(raw: u64) -> u32 {
    if raw > 600 {
        (raw / 1000).min(u32::MAX as u64) as u32
    } else {
        raw.min(u32::MAX as u64) as u32
    }
}

fn face_from_json(v: &Value) -> Option<Segment> {
    let face = v.get("faceElement").unwrap_or(v);
    let id = extract_id(face, &["faceIndex", "id"])?;
    let name = face
        .get("faceName")
        .or_else(|| face.get("faceText"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    Some(Segment::Face { id, name })
}

fn mface_from_json(v: &Value) -> Option<Segment> {
    let m = v.get("marketFaceElement").unwrap_or(v);
    let id = extract_id(m, &["emojiId"]).unwrap_or_default();
    let name = m
        .get("faceName")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let url = m
        .get("url")
        .or_else(|| m.get("faceUrl"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    if id.is_empty() && name.is_none() && url.is_none() {
        return None;
    }
    Some(Segment::Mface { id, url, name })
}

fn reply_from_json(v: &Value) -> Option<Segment> {
    let r = v.get("replyElement").unwrap_or(v);
    // senderUid / senderUinStr / senderUin — prefer the str variant
    // when both exist (PY _normalize_onebot_segments → reply branch).
    let sender_id = extract_id(r, &["senderUinStr", "senderUidStr"])
        .or_else(|| extract_id(r, &["senderUid"]))
        .or_else(|| extract_id(r, &["senderUin"]))
        .unwrap_or_default();
    let sender_name = r
        .get("senderName")
        .or_else(|| r.get("senderNickName"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let original_msg_id = extract_id(r, &["replyMsgId", "replayMsgId", "sourceMsgId"])
        .unwrap_or_default();
    // `sourceMsgContent` in NTQQ often exceeds 200 chars. Truncate to
    // 200 to keep `Reply.original_content_preview` bounded for
    // downstream rendering.
    let original_content_preview = {
        let raw = r
            .get("sourceMsgContent")
            .or_else(|| r.get("content"))
            .or_else(|| r.get("summary"))
            .or_else(|| r.get("preview"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        truncate_chars(raw, 200)
    };
    Some(Segment::Reply {
        sender_id,
        sender_name,
        original_msg_id,
        original_content_preview,
    })
}

/// Pick the first present key (in order) and return its string
/// representation, accepting both JSON strings and JSON numbers.
fn extract_id(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(x) = v.get(*k) {
            if let Some(s) = x.as_str() {
                return Some(s.to_string());
            }
            if let Some(n) = x.as_i64() {
                return Some(n.to_string());
            }
        }
    }
    None
}

fn at_from_json(v: &Value) -> Option<Segment> {
    let a = v.get("atElement").unwrap_or(v);
    // atType: 0 = @-single, 1 = @-all. When 1, the protocol often
    // omits atUid; we fill in a fixed "全体成员" marker so downstream
    // rendering still works.
    let at_type = a.get("atType").and_then(|x| x.as_i64()).unwrap_or(0);
    let (target_id, target_name) = if at_type == 1 {
        ("0".to_string(), Some("全体成员".to_string()))
    } else {
        let id = extract_id(a, &["atUid", "atUin"]).unwrap_or_default();
        let name = a
            .get("atNickName")
            .or_else(|| a.get("atName"))
            .and_then(|x| x.as_str())
            .map(str::to_string);
        (id, name)
    };
    Some(Segment::At {
        target_id,
        target_name,
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
        .or_else(|| fwd.get("forwardSourceName"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    // Parse the embedded XML preview (if any) so the Forward segment
    // carries title / summary / source_name / forwarded_count instead
    // of just an opaque BLOB. The preview is parsed but currently
    // not threaded into ForwardNode metadata; task #3 will pick it
    // up via the recursion layer.
    let xml_content = fwd.get("xmlContent").and_then(|x| x.as_str());
    let _preview = parse_forward_preview_xml(xml_content.unwrap_or(""));

    let mut nodes: Vec<ForwardNode> = Vec::new();
    if let Some(arr) = fwd.get("messages").and_then(|x| x.as_array()) {
        for node in arr {
            if let Some(obj) = node.as_object() {
                let obj_val = Value::Object(obj.clone());
                let sender_id =
                    extract_id(&obj_val, &["sender_id", "user_id"]).unwrap_or_default();
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

// ─── Pathway 2b: XML forward preview parser ─────────────────
//
// `multiForwardMsgElement.xmlContent` is the only piece of forward
// metadata QQ NT stores in the message BLOB itself. The full message
// list is fetched separately via the resId file. We parse the XML
// with regex (no quick-xml dep) to extract:
//
//   - tSum        → total message count
//   - source      → source / chat name
//   - brief       → title (often `[聊天记录]`)
//   - m_fileName  → internal resource name
//   - title text inside <item><title>...</title></item>

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ForwardPreview {
    /// Decoded title (e.g. `[聊天记录]`, or first item title).
    pub title: Option<String>,
    /// Source name from `<source name="...">`.
    pub source_name: Option<String>,
    /// Total message count from `tSum` attribute.
    pub forwarded_count: Option<u32>,
    /// Internal resource file name (rarely useful outside the
    /// QQ resId fetch path).
    pub file_name: Option<String>,
    /// First body line (joined `<item><title>` text) for inline
    /// preview rendering.
    pub preview_line: Option<String>,
}

pub fn parse_forward_preview_xml(xml: &str) -> ForwardPreview {
    let mut out = ForwardPreview::default();
    if xml.trim().is_empty() {
        return out;
    }
    out.title = xml_attr(xml, "brief");
    out.forwarded_count = xml_attr(xml, "tSum").and_then(|s| s.parse::<u32>().ok());
    out.file_name = xml_attr(xml, "m_fileName");
    // `<source name="...">text</source>` — prefer the `name`
    // attribute, fall back to element text.
    if let Some(source) = extract_source_element(xml) {
        out.source_name = Some(source);
    }
    // First <item><title>...</title></item> text → preview line.
    if let Some(line) = extract_first_item_title(xml) {
        out.preview_line = Some(line);
    }
    out
}

fn xml_attr(xml: &str, attr: &str) -> Option<String> {
    // Match `attr="..."` or `attr='...'`. We do not support the
    // unquoted form — QQ always uses double-quoted attribute values.
    let pat = format!(r#"(?s)\b{}\s*=\s*"([^"]*)""#, regex_lite::escape(attr));
    let re = Regex::new(&pat).ok()?;
    re.captures(xml)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

fn extract_source_element(xml: &str) -> Option<String> {
    // `<source name="X">Y</source>` — prefer X. If no name attr, use Y.
    let re = Regex::new(r#"(?s)<source\b([^>]*)>(.*?)</source>"#).ok()?;
    let caps = re.captures(xml)?;
    let attrs = caps.get(1)?.as_str();
    if let Some(name) = xml_attr(&format!("<source{}", attrs), "name") {
        if !name.is_empty() {
            return Some(name);
        }
    }
    let body = caps.get(2)?.as_str().trim().to_string();
    if body.is_empty() {
        None
    } else {
        Some(strip_xml_tags_inline(&body))
    }
}

fn extract_first_item_title(xml: &str) -> Option<String> {
    // Match the first <item ...><title>...</title>...</item> region
    // and pull the title text. We do not worry about nested items
    // since QQ's preview schema is flat.
    let re = Regex::new(r#"(?s)<item\b[^>]*>\s*<title\b[^>]*>(.*?)</title>"#).ok()?;
    let caps = re.captures(xml)?;
    let body = caps.get(1)?.as_str();
    let cleaned = strip_xml_tags_inline(body);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn strip_xml_tags_inline(s: &str) -> String {
    let re = Regex::new(r"<[^>]+>").ok();
    let stripped = match re {
        Some(r) => r.replace_all(s, " ").into_owned(),
        None => s.to_string(),
    };
    collapse_whitespace(stripped.trim())
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut end = s.len();
    for (count, (idx, _)) in s.char_indices().enumerate() {
        if count == max {
            end = idx;
            break;
        }
    }
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..end])
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

// ─── Pathway 5: OneBot 11 segment array ─────────────────────
//
// OneBot 11 sends messages as an array of `{"type": "...", "data":
// {...}}` segments. NapCat-side payloads and any manual JSON
// export pass through this pathway. The accepted shapes are:
//
//   - `{"message": [<seg>, <seg>, ...]}`
//   - `{"message": "plain string"}`
//   - `{"message": {"type": "text", "data": {...}}}`  (single seg)
//
// Type strings follow OneBot 11 spec; the `text/image/record/file/
// at/reply/forward/face/mface/json/xml/video/onlinefile/node` set
// covers everything NapCat produces.

#[allow(dead_code)] // wired into forward recursion (task #3) and NapCat client (task #5)
fn dispatch_onebot_segments(arr: &[Value]) -> Vec<Segment> {
    let mut out = Vec::with_capacity(arr.len());
    for raw in arr {
        if let Some(seg) = raw.as_object() {
            let seg_type = seg
                .get("type")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_lowercase();
            let data = seg.get("data").cloned().unwrap_or(Value::Null);
            if let Some(s) = onebot_segment_to_segment(&seg_type, &data) {
                out.push(s);
                continue;
            }
        }
        out.push(Segment::Unknown {
            raw_json: preview_json(raw, 128),
            reason: "onebot: unparsable segment shape".to_string(),
        });
    }
    out
}

#[allow(dead_code)] // wired into forward recursion (task #3) and NapCat client (task #5)
fn onebot_segment_to_segment(seg_type: &str, data: &Value) -> Option<Segment> {
    let d = || {
        if data.is_object() {
            data.clone()
        } else {
            Value::Object(Default::default())
        }
    };
    match seg_type {
        "text" => {
            let s = d()
                .get("text")
                .and_then(|x| x.as_str())
                .map(str::to_string)?;
            if s.is_empty() {
                None
            } else {
                Some(Segment::Text { text: s })
            }
        }
        "image" => {
            let data = d();
            let url = data
                .get("url")
                .or_else(|| data.get("file"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let local_path = data
                .get("path")
                .or_else(|| data.get("file"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let fileid = extract_id(&data, &["file_id"]);
            let md5 = data.get("md5").and_then(|x| x.as_str()).map(str::to_string);
            if url.is_none() && local_path.is_none() && fileid.is_none() && md5.is_none() {
                return None;
            }
            Some(Segment::Image {
                url,
                fileid,
                md5,
                size: None,
                local_path,
            })
        }
        "record" | "voice" => {
            let data = d();
            let url = data
                .get("url")
                .or_else(|| data.get("file"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let local_path = data
                .get("path")
                .or_else(|| data.get("file"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let fileid = extract_id(&data, &["file_id"]);
            let md5 = data.get("md5").and_then(|x| x.as_str()).map(str::to_string);
            // NapCat `record.magic` is the silk codec marker; the
            // duration is rarely provided by NapCat so we leave None.
            Some(Segment::Record {
                url: url.or_else(|| local_path.clone()),
                fileid,
                md5,
                duration: None,
            })
        }
        "file" | "onlinefile" => {
            let data = d();
            let name = file_name_from_data(&data).unwrap_or_else(|| "uploaded_file".to_string());
            let url = data
                .get("url")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let fileid = extract_id(&data, &["file_id"]);
            let size = data
                .get("size")
                .or_else(|| data.get("fileSize"))
                .and_then(|x| x.as_u64());
            let local_path = data
                .get("path")
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
        "at" => {
            let data = d();
            // OneBot 11: `qq` is the user id, `name` is the display
            // name. If `qq` is `"all"` we treat as @-all.
            if let Some(qq) = extract_id(&data, &["qq"]) {
                if qq == "all" {
                    return Some(Segment::At {
                        target_id: "0".to_string(),
                        target_name: Some("全体成员".to_string()),
                    });
                }
                let name = data.get("name").and_then(|x| x.as_str()).map(str::to_string);
                return Some(Segment::At {
                    target_id: qq,
                    target_name: name,
                });
            }
            None
        }
        "reply" => {
            let data = d();
            let sender_id = extract_id(&data, &["user_id", "qq"]).unwrap_or_default();
            let sender_name = data
                .get("nickname")
                .or_else(|| data.get("name"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let original_msg_id = extract_id(&data, &["id", "seq"]).unwrap_or_default();
            // OneBot reply only carries the id; we leave preview empty
            // rather than fabricate one.
            Some(Segment::Reply {
                sender_id,
                sender_name,
                original_msg_id,
                original_content_preview: String::new(),
            })
        }
        "face" => {
            let data = d();
            let id = extract_id(&data, &["id"])?;
            let name = data.get("raw").and_then(|x| x.as_str()).map(str::to_string);
            Some(Segment::Face { id, name })
        }
        "mface" => {
            let data = d();
            let id = extract_id(&data, &["emoji_id"]).unwrap_or_default();
            let name = data.get("summary").and_then(|x| x.as_str()).map(str::to_string);
            let url = data
                .get("url")
                .or_else(|| data.get("remote_url"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            if id.is_empty() && name.is_none() && url.is_none() {
                None
            } else {
                Some(Segment::Mface { id, url, name })
            }
        }
        "video" => {
            let data = d();
            // OneBot 11 does not have a dedicated video segment; we
            // map it to File with the original name kept.
            let name = data
                .get("name")
                .and_then(|x| x.as_str())
                .map(str::to_string)
                .or_else(|| basename_from_data_field(&data, "file"))
                .unwrap_or_else(|| "video.mp4".to_string());
            let url = data
                .get("url")
                .or_else(|| data.get("file"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let local_path = data
                .get("path")
                .or_else(|| data.get("file"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            Some(Segment::File {
                name,
                url,
                fileid: None,
                size: None,
                local_path,
            })
        }
        "forward" | "node" => {
            let data = d();
            // Both "node" and "forward" map to Forward. Recursion
            // through `data.content` is intentionally not done here —
            // task #3 will do that. For now we just hold the surface
            // metadata.
            let sender_id = extract_id(&data, &["user_id", "uin"]);
            let sender_name = data
                .get("nickname")
                .or_else(|| data.get("name"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let messages = match data.get("content") {
                Some(Value::Array(arr)) => forward_nodes_from_onebot(arr),
                _ => Vec::new(),
            };
            Some(Segment::Forward {
                sender_id,
                sender_name,
                messages,
            })
        }
        "json" | "xml" | "share" => {
            // Share / json card / xml card — surface as Unknown with
            // the raw blob inlined so downstream tooling can render
            // it via the QQ card path. We do **not** try to parse the
            // inner JSON / XML here; the Share variant lives in the
            // bigger normalize refactor and is intentionally out of
            // scope for this module.
            let preview = preview_json(data, 128);
            let reason = match seg_type {
                "json" | "xml" => format!("onebot: {} card (rendered externally)", seg_type),
                _ => "onebot: share card (rendered externally)".to_string(),
            };
            Some(Segment::Unknown {
                raw_json: preview,
                reason,
            })
        }
        _ => None,
    }
}

#[allow(dead_code)] // wired into forward recursion (task #3) and NapCat client (task #5)
fn forward_nodes_from_onebot(arr: &[Value]) -> Vec<ForwardNode> {
    let mut out = Vec::new();
    for raw in arr {
        if let Some(obj) = raw.as_object() {
            // `node` shape: `{"type":"node","data":{"user_id":...,
            // "nickname":...,"message":[...]}}` — we extract the
            // sender but keep segments empty (recursion is task #3).
            let data = obj.get("data").cloned().unwrap_or(Value::Null);
            let data_obj = data.as_object();
            let sender_id = data_obj
                .map(|d| {
                    extract_id(
                        &Value::Object(d.clone()),
                        &["user_id", "sender_id", "uin"],
                    )
                })
                .unwrap_or_default()
                .unwrap_or_default();
            let sender_name = data_obj
                .and_then(|d| {
                    d.get("nickname")
                        .or_else(|| d.get("name"))
                        .or_else(|| d.get("sender_name"))
                })
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let timestamp = data_obj
                .and_then(|d| d.get("time").or_else(|| d.get("timestamp")))
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            // If `data.message` is a non-empty list, we leave a
            // synthetic Unknown so the caller can tell the data was
            // there but not yet expanded.
            let has_inner_message = data_obj
                .and_then(|d| d.get("message").or_else(|| d.get("content")))
                .map(|v| v.is_array() || v.is_string())
                .unwrap_or(false);
            let segments = if has_inner_message {
                vec![Segment::Unknown {
                    raw_json: preview_json(&data, 128),
                    reason: "nested forward (onebot) — not yet expanded".to_string(),
                }]
            } else {
                Vec::new()
            };
            out.push(ForwardNode {
                sender_id,
                sender_name,
                timestamp,
                segments,
            });
        }
    }
    out
}

// ─── helpers shared between the two pathways ────────────────

/// Resolve a onebot file `name` from the candidate fields, in order:
/// `name` / `fileName` / `file_path_basename(path)` /
/// `file_path_basename(file)`. Returns `None` if none of them yield
/// a usable name.
#[allow(dead_code)] // wired into forward recursion (task #3) and NapCat client (task #5)
fn file_name_from_data(data: &Value) -> Option<String> {
    if let Some(s) = data.get("name").and_then(|x| x.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    if let Some(s) = data.get("fileName").and_then(|x| x.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    basename_from_data_field(data, "path").or_else(|| basename_from_data_field(data, "file"))
}

#[allow(dead_code)] // wired into forward recursion (task #3) and NapCat client (task #5)
fn basename_from_data_field(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(|x| x.as_str())
        .and_then(|p| std::path::Path::new(p).file_name())
        .and_then(|n| n.to_str())
        .map(str::to_string)
}

// ─── generic helpers ────────────────────────────────────────

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

    // ── New tests for pathway 2 enrichment ──

    #[test]
    fn pic_element_full_fields() {
        let raw = br#"{
            "elementType": 2,
            "picElement": {
                "fileName": "shot.jpg",
                "sourcePath": "C:/Pic/shot.jpg",
                "md5HexStr": "deadbeefdeadbeefdeadbeefdeadbeef",
                "fileUuid": "file-uuid-123",
                "originImageUrl": "https://gchat.qpic.cn/shot.jpg",
                "picWidth": 1920,
                "picHeight": 1080,
                "fileSize": 524288
            }
        }"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::Image { url, fileid, md5, size, local_path } => {
                assert_eq!(url.as_deref(), Some("https://gchat.qpic.cn/shot.jpg"));
                assert_eq!(fileid.as_deref(), Some("file-uuid-123"));
                assert_eq!(md5.as_deref(), Some("deadbeefdeadbeefdeadbeefdeadbeef"));
                assert_eq!(*size, Some(524288));
                assert_eq!(local_path.as_deref(), Some("C:/Pic/shot.jpg"));
            }
            other => panic!("expected Image, got {:?}", other),
        }
    }

    #[test]
    fn ptt_element_with_duration() {
        let raw = br#"{
            "elementType": 4,
            "pttElement": {
                "fileName": "voice.silk",
                "filePath": "C:/Rec/voice.silk",
                "fileMd5": "abcdabcdabcdabcdabcdabcdabcdabcd",
                "fileUuid": "ptt-uuid-1",
                "fileTime": 12
            }
        }"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::Record { fileid, md5, duration, .. } => {
                assert_eq!(fileid.as_deref(), Some("ptt-uuid-1"));
                assert_eq!(md5.as_deref(), Some("abcdabcdabcdabcdabcdabcdabcdabcd"));
                assert_eq!(*duration, Some(12), "12s voice should stay 12s, not 12000");
            }
            other => panic!("expected Record, got {:?}", other),
        }
    }

    #[test]
    fn ptt_element_ms_scale_normalised() {
        // fileTime > 600 must be treated as ms and divided by 1000.
        let raw = br#"{
            "elementType": 4,
            "pttElement": {"fileTime": 12500}
        }"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::Record { duration, .. } => assert_eq!(*duration, Some(12)),
            other => panic!("expected Record, got {:?}", other),
        }
    }

    #[test]
    fn reply_element_with_preview() {
        let raw = br#"{
            "elementType": 7,
            "replyElement": {
                "replyMsgId": "msg-100",
                "senderUinStr": "12345",
                "senderName": "Alice",
                "sourceMsgContent": "hello world, this is the original message that should be truncated to 200 characters when necessary. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum."
            }
        }"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::Reply { sender_id, sender_name, original_msg_id, original_content_preview } => {
                assert_eq!(sender_id, "12345");
                assert_eq!(sender_name, "Alice");
                assert_eq!(original_msg_id, "msg-100");
                assert!(original_content_preview.ends_with('…'), "should be truncated with ellipsis");
                let trimmed = original_content_preview.trim_end_matches('…');
                assert!(trimmed.chars().count() <= 200, "preview must be <= 200 chars");
            }
            other => panic!("expected Reply, got {:?}", other),
        }
    }

    #[test]
    fn at_element_atall() {
        let raw = br#"{
            "elementType": 8,
            "atElement": {"atType": 1, "atUid": 0}
        }"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::At { target_id, target_name } => {
                assert_eq!(target_id, "0");
                assert_eq!(target_name.as_deref(), Some("全体成员"));
            }
            other => panic!("expected At, got {:?}", other),
        }
    }

    #[test]
    fn at_element_single() {
        let raw = br#"{
            "elementType": 8,
            "atElement": {"atType": 0, "atUid": "12345", "atNickName": "Bob"}
        }"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::At { target_id, target_name } => {
                assert_eq!(target_id, "12345");
                assert_eq!(target_name.as_deref(), Some("Bob"));
            }
            other => panic!("expected At, got {:?}", other),
        }
    }

    #[test]
    fn forward_with_xml_preview() {
        let xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><msg brief=\"[聊天记录]\" m_fileName=\"abc123.forward\" tSum=\"3\" source=\"某群聊\"><item><title>Alice: hello</title><title>Bob: yo</title><summary>截图.jpg</summary></item><source name=\"某群聊\"></source></msg>";
        let preview = parse_forward_preview_xml(xml);
        assert_eq!(preview.title.as_deref(), Some("[聊天记录]"));
        assert_eq!(preview.source_name.as_deref(), Some("某群聊"));
        assert_eq!(preview.forwarded_count, Some(3));
        assert_eq!(preview.file_name.as_deref(), Some("abc123.forward"));
        assert_eq!(preview.preview_line.as_deref(), Some("Alice: hello"));
        // And the normalize pathway still produces a Forward segment
        // for a multiForwardMsgElement that carries the XML.
        let raw = r#"{
            "elementType": 16,
            "multiForwardMsgElement": {
                "sender_id": "u0",
                "xmlContent": "<?xml version=\"1.0\" encoding=\"utf-8\"?><msg brief=\"[聊天记录]\" m_fileName=\"abc123.forward\" tSum=\"3\" source=\"某群聊\"><item><title>Alice: hello</title><title>Bob: yo</title><summary>截图.jpg</summary></item><source name=\"某群聊\"></source></msg>"
            }
        }"#;
        let mws = normalize_blob_to_segments(raw.as_bytes());
        assert!(matches!(&mws.segments[0], Segment::Forward { .. }));
    }

    #[test]
    fn onebot_segment_array_text_image() {
        let msg = serde_json::json!({
            "message": [
                {"type": "text", "data": {"text": "look at this:"}},
                {"type": "image", "data": {"file": "shot.jpg", "url": "https://x/y.jpg"}},
                {"type": "at", "data": {"qq": "all"}}
            ]
        });
        let mws = normalize_onebot_message(&msg);
        assert_eq!(mws.segments.len(), 3);
        assert!(matches!(&mws.segments[0], Segment::Text { text } if text == "look at this:"));
        match &mws.segments[1] {
            Segment::Image { url, local_path, .. } => {
                assert_eq!(url.as_deref(), Some("https://x/y.jpg"));
                assert_eq!(local_path.as_deref(), Some("shot.jpg"));
            }
            other => panic!("expected Image, got {:?}", other),
        }
        match &mws.segments[2] {
            Segment::At { target_id, target_name } => {
                assert_eq!(target_id, "0");
                assert_eq!(target_name.as_deref(), Some("全体成员"));
            }
            other => panic!("expected At, got {:?}", other),
        }
    }

    #[test]
    fn onebot_segment_array_plain_string() {
        let msg = serde_json::json!({"message": "hi from napcat"});
        let mws = normalize_onebot_message(&msg);
        assert_eq!(mws.segments.len(), 1);
        assert!(matches!(&mws.segments[0], Segment::Text { text } if text == "hi from napcat"));
    }

    #[test]
    fn onebot_unknown_type_does_not_panic() {
        let msg = serde_json::json!({
            "message": [
                {"type": "future_type", "data": {"foo": "bar"}}
            ]
        });
        let mws = normalize_onebot_message(&msg);
        assert_eq!(mws.segments.len(), 1);
        assert!(
            matches!(&mws.segments[0], Segment::Unknown { .. }),
            "expected Unknown for unhandled type, got {:?}",
            mws.segments[0]
        );
    }

    #[test]
    fn qq_elementtype_unhandled_to_unknown() {
        // elementType=42 is not in our table; we should still emit a
        // Unknown segment with a reason rather than dropping the BLOB
        // or panicking.
        let raw = br#"{"elementType": 42, "element": {"foo": "bar"}}"#;
        let mws = normalize_blob_to_segments(raw);
        match &mws.segments[0] {
            Segment::Unknown { reason, .. } => {
                assert!(reason.contains("42"), "reason should mention the elementType: {}", reason);
            }
            other => panic!("expected Unknown, got {:?}", other),
        }
    }
}
