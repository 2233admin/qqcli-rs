//! qqcli 持久化配置 (TOML)
//!
//! 存储路径: ~/.config/qqcli/config.toml (Linux/macOS)
//!            %APPDATA%/qqcli/config.toml   (Windows)
//!
//! 字段:
//!   db_key       - 最近一次解密的密钥 (16-char)
//!   db_uin       - QQ 号
//!   sqlcipher_bin - sqlcipher.exe 路径

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CONFIG_DIR: &str = "qqcli";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QqcliConfig {
    /// 旧版明文密钥，仅用于一次性迁移；保存配置时会被清除。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_key: Option<String>,
    /// Windows DPAPI 加密后的密钥，仅当前 Windows 用户可解密。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_key_protected: Option<String>,
    /// 用户明确选择的原始 QQ 数据库路径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_path: Option<PathBuf>,
    pub db_uin: Option<String>,
    pub sqlcipher_bin: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_script: Option<PathBuf>,
}

/// 获取配置目录 (~/.config/qqcli/)
pub fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir().ok_or_else(|| anyhow::anyhow!("无法获取配置目录"))?;
    let dir = dir.join(CONFIG_DIR);
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("创建配置目录失败: {}", dir.display()))?;
    }
    Ok(dir)
}

pub fn config_path() -> PathBuf {
    config_dir()
        .map(|d| d.join(CONFIG_FILE))
        .unwrap_or_else(|_| PathBuf::from("qqcli_config.toml"))
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

fn write_config(cfg: &QqcliConfig) -> Result<()> {
    let path = config_path();
    let text = toml::to_string_pretty(cfg).context("序列化配置失败")?;
    std::fs::write(&path, text).with_context(|| format!("写入配置文件失败: {}", path.display()))
}

fn account_from_db_path(path: &Path) -> Option<String> {
    path.ancestors()
        .nth(3)
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .filter(|uin| uin.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

/// 保存用户选择的数据库路径，后续命令不需要重复传环境变量。
pub fn save_db_path(path: &Path) -> Result<()> {
    save_db_path_for_account(path, account_from_db_path(path))
}

pub fn save_db_path_for_account(path: &Path, account: Option<String>) -> Result<()> {
    let mut cfg = get_config().unwrap_or_default();
    cfg.db_path = Some(path.to_path_buf());
    cfg.db_uin = account.or_else(|| account_from_db_path(path));
    write_config(&cfg)
}

pub fn clear_db_path() -> Result<()> {
    let mut cfg = get_config().unwrap_or_default();
    cfg.db_path = None;
    cfg.db_uin = None;
    write_config(&cfg)
}

pub fn save_sqlcipher_bin(path: &Path) -> Result<()> {
    let mut cfg = get_config().unwrap_or_default();
    cfg.sqlcipher_bin = Some(path.to_path_buf());
    write_config(&cfg)
}

pub fn save_key_script(path: &Path) -> Result<()> {
    let mut cfg = get_config().unwrap_or_default();
    cfg.key_script = Some(path.to_path_buf());
    write_config(&cfg)
}

/// 保存解密密钥。Windows 上使用当前用户的 DPAPI 加密，密钥不会写入终端或 TOML 明文。
pub fn save_key(key: &str) -> Result<()> {
    let mut cfg = get_config().unwrap_or_default();
    cfg.db_key = None;
    cfg.db_key_protected = Some(protect_for_current_user(key)?);
    write_config(&cfg)?;
    eprintln!(
        "密钥已使用 Windows 凭据保护保存到: {}",
        config_path().display()
    );
    Ok(())
}

/// 返回供本次解密使用的密钥。Agent 可通过 QQCLI_DB_KEY 注入，不会落盘。
pub fn get_key() -> Result<Option<String>> {
    if let Ok(key) = std::env::var("QQCLI_DB_KEY") {
        return Ok(Some(key));
    }

    let cfg = get_config()?;
    if let Some(protected) = cfg.db_key_protected {
        return unprotect_for_current_user(&protected).map(Some);
    }

    if let Some(legacy_key) = cfg.db_key {
        // 无缝迁移旧版配置，并立即从 TOML 移除明文。
        save_key(&legacy_key)?;
        eprintln!("已将旧版明文密钥迁移为 Windows 凭据保护格式。");
        return Ok(Some(legacy_key));
    }

    Ok(None)
}

#[cfg(windows)]
fn protect_for_current_user(key: &str) -> Result<String> {
    dpapi(key.as_bytes()).map(|bytes| BASE64.encode(bytes))
}

#[cfg(not(windows))]
fn protect_for_current_user(_: &str) -> Result<String> {
    bail!("仅 Windows 支持将密钥保存到 DPAPI；请通过 QQCLI_DB_KEY 注入密钥")
}

#[cfg(windows)]
fn unprotect_for_current_user(protected: &str) -> Result<String> {
    let bytes = BASE64.decode(protected).context("受保护密钥格式无效")?;
    let plaintext = dpapi_unprotect(&bytes)?;
    String::from_utf8(plaintext).context("受保护密钥不是有效 UTF-8")
}

#[cfg(not(windows))]
fn unprotect_for_current_user(_: &str) -> Result<String> {
    bail!("仅 Windows 支持读取 DPAPI 保护的密钥；请通过 QQCLI_DB_KEY 注入密钥")
}

#[cfg(windows)]
fn dpapi(input: &[u8]) -> Result<Vec<u8>> {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input_copy = input.to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input_copy.len()).context("密钥过长")?,
        pbData: input_copy.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB::default();
    let success = unsafe {
        CryptProtectData(
            &input_blob,
            null(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output_blob,
        )
    };
    if success == 0 {
        bail!(
            "Windows DPAPI 加密失败: {}",
            std::io::Error::last_os_error()
        );
    }

    let output = unsafe {
        let bytes = std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize);
        let copy = bytes.to_vec();
        LocalFree(output_blob.pbData.cast());
        copy
    };
    Ok(output)
}

#[cfg(windows)]
fn dpapi_unprotect(input: &[u8]) -> Result<Vec<u8>> {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input_copy = input.to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input_copy.len()).context("受保护密钥过长")?,
        pbData: input_copy.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB::default();
    let success = unsafe {
        CryptUnprotectData(
            &input_blob,
            std::ptr::null_mut(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output_blob,
        )
    };
    if success == 0 {
        bail!(
            "Windows DPAPI 解密失败: {}",
            std::io::Error::last_os_error()
        );
    }

    let output = unsafe {
        let bytes = std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize);
        let copy = bytes.to_vec();
        LocalFree(output_blob.pbData.cast());
        copy
    };
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::account_from_db_path;
    use std::path::Path;

    #[test]
    fn derives_account_from_standard_nt_path() {
        let path = Path::new(r"C:\Users\test\Documents\Tencent Files\123456\nt_qq\nt_db\nt_msg.db");
        assert_eq!(account_from_db_path(path).as_deref(), Some("123456"));
    }

    #[test]
    fn ignores_non_standard_paths() {
        assert_eq!(account_from_db_path(Path::new(r"C:\data\nt_msg.db")), None);
    }

    #[test]
    #[cfg(windows)]
    fn dpapi_round_trip_keeps_key_out_of_plaintext() {
        let key = "example-key-1234";
        let protected = super::protect_for_current_user(key).expect("protect key");
        assert_ne!(protected, key);
        let serialized = toml::to_string(&super::QqcliConfig {
            db_key_protected: Some(protected.clone()),
            ..Default::default()
        })
        .expect("serialize protected key");
        assert!(!serialized.contains(key));
        assert_eq!(
            super::unprotect_for_current_user(&protected).expect("unprotect key"),
            key
        );
    }
}
