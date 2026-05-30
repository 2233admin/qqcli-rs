//! QQ NT 数据库解密模块
//!
//! 策略:
//!  1. 优先使用 sqlcipher.exe 本地解密 (sqlcipher_bin 配置)
//!  2. 备选: 调用 windows_ntqq_get_key.ps1 从进程内存提取密钥，再调用 sqlcipher
//!  3. 如果密钥已缓存，直接解密
//!  4. 输出解密后的 DB 路径

use anyhow::{bail, Context, Result};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 加密 DB 检测结果
#[derive(Debug)]
pub enum DbStatus {
    /// DB 未加密，可直接使用
    Plaintext(PathBuf),
    /// DB 已加密，需要解密
    Encrypted { raw_db: PathBuf, key: Option<String> },
    /// DB 文件不存在
    NotFound(PathBuf),
}

impl std::fmt::Display for DbStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbStatus::Plaintext(p) => write!(f, "明文 DB: {}", p.display()),
            DbStatus::Encrypted { raw_db, key } => {
                write!(f, "加密 DB: {}", raw_db.display())?;
                if key.is_some() {
                    write!(f, " (已有密钥)")
                } else {
                    write!(f, " (需要提取密钥)")
                }
            }
            DbStatus::NotFound(p) => write!(f, "DB 不存在: {}", p.display()),
        }
    }
}

/// 检测 DB 加密状态
pub fn detect_db_status() -> DbStatus {
    // 1. 检查解密后的 DB (优先)
    if let Some(decrypted) = crate::db::default_decrypted_db_path() {
        if decrypted.exists() {
            // 快速验证是否真的能打开
            if rusqlite::Connection::open(&decrypted).is_ok() {
                return DbStatus::Plaintext(decrypted);
            }
        }
    }

    // 2. 检查原始加密 DB
    let raw_db = crate::db::default_db_path();
    if !raw_db.exists() {
        return DbStatus::NotFound(raw_db);
    }

    // 3. 尝试用已知密钥解密检测 (如果已缓存)
    let key = crate::config::get_config()
        .ok()
        .and_then(|c| c.db_key);

    DbStatus::Encrypted { raw_db, key }
}

/// 默认 sqlcipher.exe 路径
fn default_sqlcipher_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("Downloads").join("voile").join("sqlcipher.exe"))
        .unwrap_or_else(|| PathBuf::from("sqlcipher.exe"))
}

/// 默认密钥提取 PS1 脚本路径
fn default_ps1_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("Downloads").join("qq-win-db-key").join("windows_ntqq_get_key.ps1"))
        .unwrap_or_else(|| PathBuf::from("windows_ntqq_get_key.ps1"))
}

/// 尝试用 sqlcipher.exe 解密 DB
pub fn decrypt_with_sqlcipher(raw_db: &Path, output: &Path, key: &str) -> Result<()> {
    // 先 strip 1024-byte header
    let clean_path = output.with_file_name("nt_msg_clean.db");

    strip_sqlcipher_header(raw_db, &clean_path)?;

    // SQLCipher 解密
    let sqlcipher_bin = crate::config::get_config()
        .ok()
        .and_then(|c| c.sqlcipher_bin.clone())
        .unwrap_or_else(default_sqlcipher_path);

    if !sqlcipher_bin.exists() {
        bail!(
            "sqlcipher.exe 不存在: {}\n请从 https://github.com/willur enter/sqlcipher-windows 下载",
            sqlcipher_bin.display()
        );
    }

    let sql = format!(
        r#"PRAGMA key = "{}";
PRAGMA cipher_page_size = 4096;
PRAGMA kdf_iter = 4000;
PRAGMA cipher_hmac_algorithm = HMAC_SHA1;
PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;
ATTACH DATABASE '{}' AS plaintext KEY '';
SELECT sqlcipher_export('plaintext');
DETACH DATABASE plaintext;"#,
        key.replace('"', "\\\""),
        output.display().to_string().replace('\\', "\\\\")
    );

    let child = Command::new(&sqlcipher_bin)
        .arg(clean_path.display().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("执行 sqlcipher 失败: {}", sqlcipher_bin.display()))?;

    let mut child = child;
    use std::io::Write;
    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(sql.as_bytes())?;
    }
    drop(child.stdin.take());

    let output_str = child
        .wait_with_output()
        .with_context(|| "sqlcipher 等待输出失败")?;

    if !output_str.status.success() {
        let stderr = String::from_utf8_lossy(&output_str.stderr);
        bail!("sqlcipher 解密失败:\n{}", stderr);
    }

    // 验证解密后的 DB
    if !output.exists() {
        bail!("sqlcipher 执行成功但输出文件不存在: {}", output.display());
    }

    // 用 rusqlite 验证
    if rusqlite::Connection::open(output).is_err() {
        bail!("解密后的 DB 无法打开，可能密钥错误");
    }

    println!("解密完成: {}", output.display());
    Ok(())
}

/// 剥离 SQLCipher 1024-byte 头
fn strip_sqlcipher_header(raw: &Path, clean: &Path) -> Result<()> {
    let mut file = std::fs::File::open(raw)
        .with_context(|| format!("无法打开: {}", raw.display()))?;

    // 读取前 16 字节检测是否是 SQLCipher 格式
    let mut header = [0u8; 16];
    file.read_exact(&mut header)?;

    let file_size = file.metadata()?.len();

    file.seek(std::io::SeekFrom::Start(0))?;
    let mut all_data = Vec::with_capacity(file_size as usize);
    file.read_to_end(&mut all_data)?;

    if all_data.starts_with(b"SQLite format 3") {
        // 没有 header，直接复制
        std::fs::write(clean, &all_data)
            .with_context(|| format!("写入 clean DB 失败: {}", clean.display()))?;
        println!("  [无 header] 直接复制");
        return Ok(());
    }

    // 有 header，剥离前 1024 字节
    if all_data.len() > 1024 {
        let clean_data = &all_data[1024..];
        if clean_data.starts_with(b"SQLite format 3") {
            std::fs::write(clean, clean_data)
                .with_context(|| format!("写入 clean DB 失败: {}", clean.display()))?;
            println!("  [剥离 1024-byte header] OK");
            return Ok(());
        }
    }

    bail!("无法识别的 DB 格式，前 16 字节: {:02X?}", &header);
}

/// 从 QQ 进程内存提取密钥 (调用 PowerShell 脚本)
pub fn extract_key_from_process(ps1_path: Option<&Path>) -> Result<String> {
    let script: PathBuf = if let Some(p) = ps1_path {
        p.to_path_buf()
    } else {
        default_ps1_path()
    };

    if !script.exists() {
        bail!(
            "密钥提取脚本不存在: {}\n请下载 https://github.com/yourusername/qq-nt-decrypt",
            script.display()
        );
    }

    println!("正在提取密钥，请确保 QQ NT 已登录...");
    println!("(会启动新的 QQ 窗口，请在新窗口中登录目标账号)");

    let output = Command::new("powershell")
        .args([
            "-ExecutionPolicy", "Bypass",
            "-NoProfile",
            "-File", &script.display().to_string(),
        ])
        .output()
        .with_context(|| format!("执行 PS 脚本失败: {}", script.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!("stderr: {}", stderr);
        bail!("密钥提取脚本执行失败 (exit={})", output.status);
    }

    // 从 stdout 解析密钥
    let key = parse_key_from_output(&stdout);
    match key {
        Some(k) => {
            println!("密钥提取成功: {}", k);
            Ok(k)
        }
        None => {
            eprintln!("stdout:\n{}", stdout);
            bail!("无法从输出中解析密钥，请手动提取")
        }
    }
}

/// 从 PS 脚本 stdout 提取 Key 字段
fn parse_key_from_output(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        // 匹配 "加密密钥:      <key>" 或 "Key = <key>" 或 JSON 风格 "Key": "<key>"
        if line.contains("密钥") || line.contains("Key") {
            if let Some(eq_pos) = line.find(':') {
                let val = line[eq_pos + 1..].trim().trim_matches(|c| c == ',' || c == '"' || c == ' ');
                if !val.is_empty() && val.len() == 16 && val.chars().all(|c| c.is_ascii_graphic()) {
                    return Some(val.to_string());
                }
            }
            // JSON style
            if line.contains("Key") && line.contains(':') {
                let after_colon = line.split(':').nth(1)?.trim().trim_matches(',').trim_matches('"');
                if after_colon.len() == 16 && after_colon.chars().all(|c| c.is_ascii_graphic()) {
                    return Some(after_colon.to_string());
                }
            }
        }
    }
    None
}

/// 完整解密流程: 检测 -> 提取密钥(需要时) -> 解密 -> 保存密钥
pub fn ensure_decrypted(force: bool) -> Result<PathBuf> {
    let status = detect_db_status();

    match status {
        DbStatus::Plaintext(p) => {
            println!("DB 状态: 明文 OK");
            println!("DB 路径: {}", p.display());
            Ok(p)
        }
        DbStatus::NotFound(raw) => {
            bail!(
                "DB 文件不存在: {}\n请确认 QQ NT 已运行过",
                raw.display()
            );
        }
        DbStatus::Encrypted { raw_db, key } => {
            let key = if let Some(k) = key {
                println!("DB 状态: 加密 (已缓存密钥)");
                k
            } else {
                if force {
                    bail!("使用 --force 时必须有缓存密钥，请先运行 qq init 不带 --force");
                }
                println!("DB 状态: 加密，需要提取密钥");
                extract_key_from_process(None)?
            };

            // 解密
            let out_path = crate::db::default_decrypted_db_path()
                .unwrap_or_else(|| PathBuf::from("nt_msg_decrypted.db"));

            println!("正在解密...");
            decrypt_with_sqlcipher(&raw_db, &out_path, &key)?;

            // 保存密钥到配置
            crate::config::save_key(&key)?;

            Ok(out_path)
        }
    }
}
