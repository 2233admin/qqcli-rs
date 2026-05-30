# qqcli-rs

> QQ 本地数据 CLI 工具 — 读取聊天记录、搜索、统计、导出

[![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## 功能特性

### 核心功能

| 命令 | 说明 |
|------|------|
| `qq init` | 初始化：检测 DB 路径，确认解密状态 |
| `qq sessions` | 最近会话列表 |
| `qq history` | 查看聊天记录（支持时间范围过滤） |
| `qq search` | 搜索消息（支持 DuckDB FTS 全文索引） |
| `qq contacts` | 联系人列表（好友/群） |
| `qq members` | 群成员列表 |
| `qq stats` | 聊天统计分析 |
| `qq unread` | 未读消息会话 |
| `qq new-messages` | 自上次检查以来的新消息 |

### 数据导出

| 命令 | 说明 |
|------|------|
| `qq export` | 导出聊天记录（支持 JSONL/JSON/YAML/TXT/Markdown） |
| `qq bundle` | 打包聊天记录中的媒体文件到 ZIP |

### NapCat 集成

| 命令 | 说明 |
|------|------|
| `qq sync` | 同步联系人缓存（从 NapCat 拉取好友/群列表） |
| `qq groups` | 获取群列表（需要 NapCat 运行） |
| `qq nap` | NapCat OneBot11 操作 |
| `qq plugin` | NapCat IPC 插件模式（直接调用 wrapper API） |

### 工具

| 命令 | 说明 |
|------|------|
| `qq debug-tables` | 查看数据库表结构 |
| `qq debug-probe` | 探测消息 BLOB 原始字节 |
| `qq index` | 将消息导出到 DuckDB FTS 索引 |
| `qq completion` | 生成 Shell 补全脚本 |

## 安装

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/CurryWOE/qqcli-rs.git
cd qqcli-rs

# 编译
cargo build --release

# 可执行文件位于
./target/release/qq
```

### 前置条件

- **Rust 1.85+** ([安装指南](https://rustup.rs))
- **QQ NT** 已运行并解密本地数据库
- **NapCat** (可选，用于 IPC 集成)

## 使用示例

### 查看聊天记录

```bash
# 查看最近会话
qq sessions

# 查看与某人的聊天记录
qq history 1234567890 --limit 100

# 按时间范围过滤
qq history 1234567890 --since 2024-01-01 --until 2024-12-31
```

### 搜索消息

```bash
# 全文搜索（需要先建立索引）
qq index
qq search "关键词"

# 在特定会话中搜索
qq search "关键词" --chat 1234567890
```

### 导出数据

```bash
# 导出为 Markdown
qq export 1234567890 -o chat.md

# 导出为 JSONL（与 qq-data-exporter 兼容）
qq export 1234567890 --format jsonl -o chat.jsonl

# 打包图片到 ZIP
qq bundle 1234567890 -o images.zip
```

### NapCat IPC

```bash
# 测试 IPC 连接
qq plugin ping --port 9334

# 发送消息
qq plugin send private 123456 "Hello from qqcli!"

# 获取好友列表
qq plugin friends

# 获取群成员
qq plugin members 987654321
```

## 数据源

- **数据库**: `nt_msg.db` (QQ NT 本地 SQLite)
- **解密**: 需要 [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) 提取密钥
- **默认路径**: `Documents/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db`

## 项目结构

```
qqcli-rs/
├── src/
│   ├── main.rs          # CLI 入口
│   ├── commands.rs      # 命令实现
│   ├── db.rs            # SQLite 数据库访问
│   ├── db_index.rs      # DuckDB FTS 索引
│   ├── cache.rs         # 联系人缓存
│   ├── decrypt.rs        # 数据库解密
│   ├── schema.rs        # 数据库字段常量定义
│   ├── napcat/          # NapCat 集成
│   │   ├── mod.rs
│   │   ├── ws_client.rs # WebSocket 客户端
│   │   └── ipc_client.rs# IPC 客户端
│   └── bin/             # 调试工具
├── Cargo.toml
└── README.md
```

## 技术栈

- **Rust** — 高性能、安全、零依赖部署
- **rusqlite** — SQLite 数据库访问
- **DuckDB** — 全文搜索索引
- **tokio** — 异步运行时
- **clap** — CLI 参数解析

## 许可证

MIT License

## 致谢

- [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) — 数据库解密
- [NapCat](https://github.com/NapNeko/NapCatQQ) — QQ Bot 框架
- [OneBot11](https://github.com/botuniverse/onebot) — 统一 Bot 接口标准
