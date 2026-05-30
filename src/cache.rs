//! 本地联系人缓存 (从 NapCat 同步)

use crate::napcat::models::{FriendInfo, GroupInfo};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// 单个好友在缓存中的结构
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct FriendCache {
    pub nickname: String,
    pub remark: Option<String>,
}

/// 联系人缓存根结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactCache {
    #[serde(rename = "synced_at")]
    pub synced_at: i64,

    #[serde(default)]
    pub friends: Vec<FriendCacheEntry>,

    #[serde(default)]
    pub groups: Vec<GroupCacheEntry>,
}

/// 内存缓存 (OnceLock 懒加载，只读一次磁盘)
static MEM_CACHE: std::sync::OnceLock<Mutex<Option<ContactCache>>> = std::sync::OnceLock::new();

fn mem_cache() -> &'static Mutex<Option<ContactCache>> {
    MEM_CACHE.get_or_init(|| Mutex::new(load_cache_from_disk()))
}

fn load_cache_from_disk() -> Option<ContactCache> {
    let path = cache_path();
    if !path.exists() {
        return None;
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[cache] 读取缓存失败: {}", e);
            return None;
        }
    };
    match serde_json::from_str::<ContactCache>(&text) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[cache] 解析缓存失败: {} — 文件: {}", e, path.display());
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FriendCacheEntry {
    #[serde(rename = "user_id")]
    pub user_id: i64,
    pub nickname: String,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GroupCacheEntry {
    #[serde(rename = "group_id")]
    pub group_id: i64,
    #[serde(rename = "group_name")]
    pub group_name: String,
}

/// 获取 cache 目录
fn cache_dir() -> Result<PathBuf> {
    let dir = crate::config::config_dir()?;
    let cache = dir.join("cache");
    if !cache.exists() {
        fs::create_dir_all(&cache)
            .with_context(|| format!("创建 cache 目录失败: {}", cache.display()))?;
    }
    Ok(cache)
}

/// 缓存文件路径
fn cache_path() -> PathBuf {
    cache_dir()
        .unwrap_or_else(|_| PathBuf::from("contacts.json"))
        .join("contacts.json")
}

/// 从 contacts.json 加载缓存（仅磁盘读取，内部使用 mem_cache）
pub fn load_cache() -> Option<ContactCache> {
    mem_cache().lock().ok()?.clone()
}

/// 保存联系人到缓存
pub fn save_cache(friends: &[FriendInfo], groups: &[GroupInfo]) -> Result<()> {
    let synced_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let cache = ContactCache {
        synced_at,
        friends: friends
            .iter()
            .map(|f| FriendCacheEntry {
                user_id: f.user_id,
                nickname: f.nickname.clone(),
                remark: f.remark.clone(),
            })
            .collect(),
        groups: groups
            .iter()
            .map(|g| GroupCacheEntry {
                group_id: g.group_id,
                group_name: g.group_name.clone(),
            })
            .collect(),
    };

    let path = cache_path();
    let text = serde_json::to_string_pretty(&cache).context("序列化联系人缓存失败")?;
    fs::write(&path, text).with_context(|| format!("写入缓存文件失败: {}", path.display()))?;
    println!(
        "缓存已保存: {} (好友 {} 个, 群 {} 个)",
        path.display(),
        friends.len(),
        groups.len()
    );
    Ok(())
}

/// 根据 user_id 解析昵称 (优先 remark，否则 nickname)
pub fn resolve_nickname(user_id: i64) -> Option<String> {
    let cache = load_cache()?;
    cache
        .friends
        .into_iter()
        .find(|f| f.user_id == user_id)
        .map(|f| f.remark.filter(|r| !r.is_empty()).unwrap_or(f.nickname))
}

/// 根据 group_id 解析群名
#[allow(dead_code)]
pub fn resolve_group_name(group_id: i64) -> Option<String> {
    let cache = load_cache()?;
    cache
        .groups
        .into_iter()
        .find(|g| g.group_id == group_id)
        .map(|g| g.group_name)
}

/// 解析 sender_id，fallback 规则：
/// - 昵称缓存命中 → 使用昵称
/// - 未命中 → 返回传入的 fallback 字符串
pub fn resolve_or_fallback(sender_id: i64, fallback: String) -> String {
    resolve_nickname(sender_id).unwrap_or(fallback)
}

/// 将 friend list 转换为 HashMap (用于快速查询)
#[allow(dead_code)]
pub fn friends_map() -> HashMap<i64, FriendCache> {
    load_cache()
        .map(|c| {
            c.friends
                .into_iter()
                .map(|f| {
                    (
                        f.user_id,
                        FriendCache {
                            nickname: f.nickname,
                            remark: f.remark,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 将 group list 转换为 HashMap (用于快速查询)
#[allow(dead_code)]
pub fn groups_map() -> HashMap<i64, String> {
    load_cache()
        .map(|c| {
            c.groups
                .into_iter()
                .map(|g| (g.group_id, g.group_name))
                .collect()
        })
        .unwrap_or_default()
}
