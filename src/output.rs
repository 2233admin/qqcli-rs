//! 输出格式化

use crate::db::{Contact, GroupMember, Message, Session};
use anyhow::Result;

pub struct YamlWriter;

impl YamlWriter {
    pub fn write_sessions(sessions: &[Session]) -> Result<()> {
        if sessions.is_empty() {
            println!("(无会话)");
            return Ok(());
        }

        for s in sessions {
            let unread_tag = if s.unread > 0 {
                format!(" [未读 {}]", s.unread)
            } else {
                String::new()
            };

            let chat_type_tag = if s.is_group { "[群]" } else { "[私]" };

            println!("- {}{} {}", chat_type_tag, s.name, unread_tag);
            println!("  id: {}", s.id);
            println!("  time: {}", ts_to_display(s.timestamp));
            if !s.last_content.is_empty() {
                println!("  last: {}", truncate(&s.last_content, 60));
            }
            println!();
        }

        Ok(())
    }

    pub fn write_messages(messages: &[Message]) -> Result<()> {
        if messages.is_empty() {
            println!("(无消息)");
            return Ok(());
        }

        let mut current_date = String::new();

        for m in messages {
            let date_str = &m.time_str[..10];
            if date_str != current_date {
                println!("\n── {} ──", date_str);
                current_date = date_str.to_string();
            }

            let sender = if m.is_mine { "→" } else { "←" };
            let name = if m.is_mine { "我" } else { &m.sender_name };

            println!("{} {} {}: {}", m.time_str, sender, name, m.content);
        }

        Ok(())
    }

    pub fn write_contacts(contacts: &[Contact]) -> Result<()> {
        if contacts.is_empty() {
            println!("(无联系人)");
            return Ok(());
        }

        let friends: Vec<&Contact> = contacts.iter().filter(|c| c.kind == "friend").collect();
        let groups: Vec<&Contact> = contacts.iter().filter(|c| c.kind == "group").collect();

        if !friends.is_empty() {
            println!("=== 好友 ({}个) ===", friends.len());
            for c in &friends {
                println!("- {} ({})", c.name, c.id);
            }
            println!();
        }

        if !groups.is_empty() {
            println!("=== 群聊 ({}个) ===", groups.len());
            for c in &groups {
                println!("- {} ({})", c.name, c.id);
            }
        }

        Ok(())
    }

    pub fn write_members(members: &[GroupMember], group_id: &str) -> Result<()> {
        println!("=== 群 {} 成员 ===", group_id);
        if members.is_empty() {
            println!("(无成员数据)");
            return Ok(());
        }

        for m in members {
            let card = if !m.card.is_empty() { &m.card } else { &m.name };
            println!("- {} ({})", card, m.uid);
        }
        println!("\n共 {} 人", members.len());

        Ok(())
    }
}

// ─── 辅助 ──────────────────────────────────────────────────

fn ts_to_display(ts: i64) -> String {
    use chrono::DateTime;
    if ts == 0 {
        return "未知".to_string();
    }
    if let Some(dt) = DateTime::from_timestamp(ts, 0) {
        dt.format("%Y-%m-%d %H:%M").to_string()
    } else {
        ts.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 3).collect::<String>() + "..."
    }
}
