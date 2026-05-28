//! qqcli 持久化配置 (TOML)
//!
//! 存储路径: ~/.config/qqcli/config.toml (Linux/macOS)
//!            %APPDATA%/qqcli/config.toml   (Windows)
//!
//! 字段:
//!   db_key       - 最近一次解密的密钥 (16-char)
//!   db_uin       - QQ 号
//!   sqlcipher_bin - sqlcipher.exe 路径

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_DIR: &str = "qqcli";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QqcliConfig {
    pub db_key: Option<String>,
    pub db_uin: Option<String>,
    pub sqlcipher_bin: Option<PathBuf>,
}

/// 获取配置目录 (~/.config/qqcli/)
fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("无法获取配置目录"))?;
    let dir = dir.join(CONFIG_DIR);
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("创建配置目录失败: {}", dir.display()))?;
    }
    Ok(dir)
}

pub fn config_path() -> PathBuf {
    config_dir().map(|d| d.join(CONFIG_FILE)).unwrap_or_else(|_| PathBuf::from("qqcli_config.toml"))
}

/// 加载配置 (不报错，只返回默认值)
pub fn get_config() -> Result<QqcliConfig> {
    let path = config_path();
    if !path.exists() {
        return Ok(QqcliConfig::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
    toml::from_str(&text).context("解析配置文件失败")
}

/// 保存解密密钥
pub fn save_key(key: &str) -> Result<()> {
    let mut cfg = get_config().unwrap_or_default();
    cfg.db_key = Some(key.to_string());

    let path = config_path();
    let text = toml::to_string_pretty(&cfg)
        .context("序列化配置失败")?;
    std::fs::write(&path, text)
        .with_context(|| format!("写入配置文件失败: {}", path.display()))?;
    println!("密钥已保存到: {}", path.display());
    Ok(())
}
