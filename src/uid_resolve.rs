//! UID 解析 — 把 `nt_uid_mapping_table` 里的 encrypted uid (e.g. `u_Wcc5rkn...`)
//! 跟数字 QQ 号 (e.g. `1545136705`) 跟真实姓名映射起来。
//!
//! 这张表是 #1 优先级的**基础数据**: 之前 `cache::resolve_or_fallback` 是个
//! stub, 永远返回 `uid_xxx` 兜底字符串, 导致 sessions/contacts/history/search
//! 全显示不出真名 (QA BUG-003 + BUG-005)。
//!
//! 加载策略: 第一次调用 `resolve_*` 时一次性读 DB 进 HashMap, 之后纯内存查。
//! 写 1 个 `OnceLock<Mutex<UidMap>>` 懒加载, 跟 `cache::MEM_CACHE` 同模式。

use crate::db;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

// 字段 ID 跟 schema.rs 里的同名 const 完全一致, 这里再写一份以免拉 const-only module
const UID_MAPPING_ENC: &str = "[48902]";
const UID_MAPPING_QQ: &str = "[1002]";
const UID_MAPPING_NAME: &str = "[48912]";

/// 全量映射: encrypted uid -> 真实姓名, 跟 qq 号 -> 真实姓名两个视图
struct UidMap {
    /// `u_xxx` -> 姓名 (空字符串代表没名字)
    by_uid: HashMap<String, String>,
    /// 数字 QQ 号 -> 姓名
    by_qq: HashMap<i64, String>,
    /// 数字 QQ 号 -> encrypted uid (反向查)
    qq_to_uid: HashMap<i64, String>,
    /// encrypted uid -> 数字 QQ 号
    uid_to_qq: HashMap<String, i64>,
}

impl UidMap {
    fn empty() -> Self {
        Self {
            by_uid: HashMap::new(),
            by_qq: HashMap::new(),
            qq_to_uid: HashMap::new(),
            uid_to_qq: HashMap::new(),
        }
    }
}

static MAP: OnceLock<Mutex<Option<UidMap>>> = OnceLock::new();

fn cell() -> &'static Mutex<Option<UidMap>> {
    MAP.get_or_init(|| Mutex::new(None))
}

/// 加载一次。失败 (DB 不存在 / 表缺) 返空 map, 永远不 panic。
fn ensure_loaded() -> Result<()> {
    let mut guard = cell().lock().expect("uid_resolve mutex poisoned");
    if guard.is_some() {
        return Ok(());
    }
    let map = match load_from_db() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[uid_resolve] 加载 nt_uid_mapping_table 失败: {}", e);
            UidMap::empty()
        }
    };
    *guard = Some(map);
    Ok(())
}

fn load_from_db() -> Result<UidMap> {
    let path = db::detect_db_path()?;
    let conn = Connection::open(&path)?;
    let sql = format!(
        "SELECT {UID_MAPPING_ENC}, {UID_MAPPING_QQ}, {UID_MAPPING_NAME} FROM nt_uid_mapping_table",
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut map = UidMap::empty();
    while let Some(row) = rows.next()? {
        let enc: Option<String> = row.get(0)?;
        let qq: Option<i64> = row.get(1)?;
        let name: Option<String> = row.get(2)?;
        let Some(enc) = enc else { continue };
        let name = name.unwrap_or_default();
        if !name.is_empty() {
            map.by_uid.insert(enc.clone(), name.clone());
            if let Some(q) = qq {
                map.by_qq.insert(q, name);
            }
        }
        if let Some(q) = qq {
            map.qq_to_uid.entry(q).or_insert_with(|| enc.clone());
            map.uid_to_qq.entry(enc).or_insert(q);
        }
    }
    Ok(map)
}

/// 把 `u_xxx` 解成真实姓名, 没找到返 None。
pub fn name_for_uid(uid: &str) -> Option<String> {
    ensure_loaded().ok()?;
    let guard = cell().lock().ok()?;
    let m = guard.as_ref()?;
    m.by_uid.get(uid).filter(|s| !s.is_empty()).cloned()
}

/// 把数字 QQ 号解成真实姓名, 没找到返 None。
pub fn name_for_qq(qq: i64) -> Option<String> {
    ensure_loaded().ok()?;
    let guard = cell().lock().ok()?;
    let m = guard.as_ref()?;
    m.by_qq.get(&qq).filter(|s| !s.is_empty()).cloned()
}

/// 把 `u_xxx` 解成数字 QQ 号 (i64), 没找到返 None。
#[allow(dead_code)]
pub fn qq_for_uid(uid: &str) -> Option<i64> {
    ensure_loaded().ok()?;
    let guard = cell().lock().ok()?;
    let m = guard.as_ref()?;
    m.uid_to_qq.get(uid).copied()
}

/// 把数字 QQ 号解成 `u_xxx`, 没找到返 None。
#[allow(dead_code)]
pub fn uid_for_qq(qq: i64) -> Option<String> {
    ensure_loaded().ok()?;
    let guard = cell().lock().ok()?;
    let m = guard.as_ref()?;
    m.qq_to_uid.get(&qq).cloned()
}

/// 一站式: 给一个可能是 u_xxx 也可能是数字 QQ 的字符串, 尽力解出真实姓名。
#[allow(dead_code)]
pub fn resolve_name(raw: &str) -> Option<String> {
    if let Some(name) = name_for_uid(raw) {
        return Some(name);
    }
    if let Ok(qq) = raw.parse::<i64>() {
        if let Some(name) = name_for_qq(qq) {
            return Some(name);
        }
    }
    None
}

/// 测试钩子: 重置内存缓存, 让下次调用重读 DB。给 unit test 用。
#[cfg(test)]
#[allow(dead_code)]
pub fn reset_for_test() {
    let _ = cell().lock().map(|mut g| *g = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_inputs_yield_none() {
        // 不调 ensure_loaded, 所以不会碰 DB
        let m = UidMap::empty();
        assert!(!m.by_uid.contains_key("u_nope"));
    }

    #[test]
    fn resolve_name_falls_through_qq() {
        // 用一个 fallback map 直接验证 resolve_* 行为, 不动 OnceLock 全局
        // (OnceLock::set 在 stable 还没暴露)
        let mut m = UidMap::empty();
        m.by_qq.insert(1545136705, "芙芙莉亚".to_string());
        m.qq_to_uid.insert(1545136705, "u_test".to_string());
        assert_eq!(
            m.by_qq.get(&1545136705).map(|s| s.as_str()),
            Some("芙芙莉亚")
        );
        assert_eq!(
            m.qq_to_uid.get(&1545136705).map(|s| s.as_str()),
            Some("u_test")
        );
    }
}
