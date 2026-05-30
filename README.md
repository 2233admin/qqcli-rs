# qqcli

> QQ 聊天记录导出工具 | 本地数据库读取 | 消息搜索 | 终端命令行

本地 QQ 聊天记录命令行工具。不用开 QQ，直接在终端里查记录、搜消息、导数据。

**适用场景：** 导出微信/QQ聊天记录、分析聊天数据、备份聊天内容、搜索历史消息

![qqcli](docs/header.gif)

## 功能特性

### 聊天记录导出

| 功能 | 命令 | 说明 |
|------|------|------|
| 最近会话 | `qq sessions` | 查看所有聊天会话列表 |
| 聊天记录 | `qq history <QQ号>` | 查看指定好友/群的聊天内容 |
| 时间过滤 | `--since 2024-01-01` | 按日期范围筛选消息 |
| 群成员 | `qq members <群号>` | 查看群成员列表 |

### 消息搜索

| 功能 | 命令 | 说明 |
|------|------|------|
| 全文搜索 | `qq search "关键词"` | 在所有消息中搜索 |
| 会话内搜索 | `--chat <QQ号>` | 在指定会话中搜索 |
| 建立索引 | `qq index` | 用 DuckDB 建立搜索索引 |

### 数据导出

| 格式 | 命令 | 用途 |
|------|------|------|
| Markdown | `qq export <id> -o chat.md` | 阅读、存档 |
| JSONL | `--format jsonl` | 程序处理、数据迁移 |
| TXT | `--format txt` | 纯文本备份 |
| 图片打包 | `qq bundle <id> -o images.zip` | 导出聊天图片 |

### QQ Bot 集成

通过 [NapCat](https://github.com/NapNeko/NapCatQQ) 连接 QQ Bot：

| 功能 | 命令 |
|------|------|
| 发消息 | `qq plugin send private <QQ号> <内容>` |
| 查好友 | `qq plugin friends` |
| 查群列表 | `qq plugin groups` |
| 同步联系人 | `qq sync` |

## 安装

### 下载二进制（Windows / Linux / macOS）

👉 [Releases 页面下载](https://github.com/2233admin/qqcli-rs/releases)

```bash
# Windows
qqcli.exe --help

# Linux/macOS
chmod +x qqcli
./qqcli --help
```

### 编译安装

```bash
# 需要 Rust 环境
cargo install --git https://github.com/2233admin/qqcli-rs.git

# 或者克隆源码编译
git clone https://github.com/2233admin/qqcli-rs.git
cd qqcli-rs
cargo build --release
```

## 快速开始

### 1. 初始化

```bash
qq init
```

第一次运行会检测 QQ 数据库位置。

### 2. 查看会话

```bash
qq sessions
```

### 3. 查看聊天记录

```bash
# 看最近 50 条
qq history 123456789 --limit 50

# 按时间过滤
qq history 123456789 --since 2024-01-01

# 组合条件
qq history 123456789 --since 2024-01-01 --until 2024-06-30
```

### 4. 搜索消息

```bash
# 先建索引（首次搜索需要）
qq index

# 搜索
qq search "关键词"
qq search "关键词" --chat 123456789
```

### 5. 导出数据

```bash
# 导出 Markdown（带时间戳、发送者）
qq export 123456789 -o chat.md

# 导出 JSONL（兼容 qq-data-exporter）
qq export 123456789 --format jsonl -o chat.jsonl

# 打包图片
qq bundle 123456789 -o images.zip
```

## 数据库位置

qqcli 读取 QQ NT 的本地 SQLite 数据库：

| 操作系统 | 默认路径 |
|----------|----------|
| Windows | `Documents\Tencent Files\{QQ号}\nt_qq\nt_db\nt_msg.db` |
| Linux | `~/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db` |
| macOS | `~/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db` |

**指定其他路径：**
```bash
export QQCLI_DB_PATH=/path/to/nt_msg.db
qq sessions
```

## 常见问题

### Q: 提示"找不到数据库"

确保 QQ NT 运行过一次。也可以手动指定路径：
```bash
export QQCLI_DB_PATH=/path/to/nt_msg.db
```

### Q: 数据库加密了

需要先解密。使用 [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt)：

1. 下载解密工具
2. 运行 `windows_ntqq_get_key.ps1`
3. 按提示操作获取密钥
4. 重新运行 `qq init`

### Q: 搜索很慢

先建立索引：
```bash
qq index
```
建好索引后搜索会快很多。

### Q: NapCat 连接失败

确保 NapCat 已启动并监听在端口 18301。

## 技术栈

| 技术 | 用途 |
|------|------|
| [Rust](https://www.rust-lang.org) | 高性能、安全、零依赖部署 |
| [rusqlite](https://github.com/rusqlite/rusqlite) | SQLite 数据库读写 |
| [DuckDB](https://duckdb.org/) | 全文搜索索引 |
| [tokio](https://tokio.rs/) | 异步运行时 |
| [clap](https://clap.rs/) | 命令行参数解析 |

## 相关项目

- [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) — QQ NT 数据库解密工具
- [NapCat](https://github.com/NapNeko/NapCatQQ) — QQ Bot 框架（OneBot11 实现）
- [onebot](https://github.com/botuniverse/onebot) — 统一 Bot 接口标准
- [qq-data-exporter](https://github.com/mixelb/qq-data-exporter) — QQ 数据导出工具

## License

[MIT License](LICENSE)

---

**关键词：** QQ聊天记录导出 | QQ消息备份 | QQ本地数据库 | 聊天记录导出工具 | QQ记录搜索 | NapCat | OneBot11 | QQ Bot
