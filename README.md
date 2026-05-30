# qqcli

本地 QQ 聊天记录命令行工具。不用开 QQ，直接在终端里查记录、搜消息、导数据。

![qqcli](docs/header.png)

```
┌─────────────────────────────────────────────┐
│  qq sessions                               │
├─────────────────────────────────────────────┤
│  张三 (123456789)           2024-01-15     │
│  李四 (987654321)           2024-01-14     │
│  工作群 (555666777)         2024-01-14     │
└─────────────────────────────────────────────┘
```

## 功能

### 聊天记录

| 命令 | 说明 |
|------|------|
| `qq sessions` | 列出最近会话 |
| `qq history <id>` | 查看聊天记录 |
| `qq history <id> --since 2024-01-01` | 按时间过滤 |
| `qq contacts` | 好友列表 |
| `qq members <群号>` | 群成员列表 |
| `qq stats` | 统计：发了多少条消息 |

### 搜索

| 命令 | 说明 |
|------|------|
| `qq index` | 建立全文搜索索引（DuckDB） |
| `qq search "关键词"` | 搜消息 |
| `qq search "关键词" --chat <id>` | 在某个会话里搜 |

### 导出

| 命令 | 说明 |
|------|------|
| `qq export <id> -o out.md` | 导出 Markdown |
| `qq export <id> --format jsonl -o out.jsonl` | 导出 JSONL |
| `qq export <id> --format txt -o out.txt` | 导出纯文本 |
| `qq bundle <id> -o images.zip` | 把聊天里的图片打包 |

### NapCat 集成

NapCat 是 QQ Bot 框架。qqcli 可以通过 IPC 和它通信：

| 命令 | 说明 |
|------|------|
| `qq plugin ping` | 测试连接 |
| `qq plugin send private <QQ号> <消息>` | 发私聊消息 |
| `qq plugin send group <群号> <消息>` | 发群消息 |
| `qq plugin friends` | 获取好友列表 |
| `qq plugin groups` | 获取群列表 |
| `qq plugin members <群号>` | 获取群成员 |
| `qq sync` | 同步联系人到本地缓存 |

## 安装

### 下载二进制

去 [Releases 页面](https://github.com/2233admin/qqcli-rs/releases) 下载对应系统的版本：

- `qqcli-win.exe` — Windows
- `qqcli-linux` — Linux
- `qqcli-macos` — macOS

下载后重命名为 `qq` 或 `qq.exe`，放到 PATH 里。

### 编译安装

```bash
git clone https://github.com/2233admin/qqcli-rs.git
cd qqcli-rs
cargo build --release

# Linux/macOS
sudo cp target/release/qq /usr/local/bin/

# Windows - 把 target/release/qq.exe 加到 PATH
```

## 快速开始

### 1. 初始化

```bash
qq init
```

第一次运行会检测 QQ 数据库。如果数据库加密了，需要先用解密工具提取密钥。

解密工具：[qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt)

### 2. 看会话

```bash
qq sessions
```

### 3. 查聊天记录

```bash
# 最近 50 条
qq history 123456789 --limit 50

# 从某个时间开始
qq history 123456789 --since 2024-01-01

# 到某个时间为止
qq history 123456789 --until 2024-06-30

# 组合过滤
qq history 123456789 --since 2024-01-01 --until 2024-06-30
```

### 4. 搜索

```bash
# 先建索引（第一次搜索前需要）
qq index

# 搜索
qq search "关键词"
```

### 5. 导出

```bash
# 导出 Markdown（带时间戳和发送者）
qq export 123456789 -o chat.md

# 导出 JSONL（程序之间数据交换用）
qq export 123456789 --format jsonl -o chat.jsonl

# 导出纯文本
qq export 123456789 --format txt -o chat.txt
```

## 数据在哪

qqcli 读取 QQ NT 的本地数据库：

| 系统 | 默认路径 |
|------|----------|
| Windows | `Documents\Tencent Files\{QQ号}\nt_qq\nt_db\nt_msg.db` |
| Linux | `~/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db` |
| macOS | `~/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db` |

可以用环境变量指定其他路径：

```bash
export QQCLI_DB_PATH=/path/to/nt_msg.db
qq history 123456789
```

## 常见问题

### Q: 提示"找不到数据库"

A: 确保 QQ NT 运行过一次。也可以手动指定路径：
```bash
export QQCLI_DB_PATH=/path/to/nt_msg.db
```

### Q: 数据库加密了怎么办

A: 用 [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) 解密：
1. 下载解密工具
2. 运行 `windows_ntqq_get_key.ps1`
3. 按提示操作
4. 重新运行 `qq init`

### Q: 搜索很慢

A: 先建索引：
```bash
qq index
```
建好之后搜索会快很多。

### Q: NapCat 连接失败

A: 确保 NapCat 已启动并监听在正确端口（默认 18301）。

## 技术细节

- **语言**: Rust
- **数据库**: SQLite (rusqlite)
- **搜索**: DuckDB 全文索引
- **通信**: tokio + tungstenite (WebSocket), native-tcp (IPC)
- **CLI**: clap

## License

MIT

## 相关项目

- [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) — 数据库解密
- [NapCat](https://github.com/NapNeko/NapCatQQ) — QQ Bot 框架
- [onebot11-spec](https://github.com/botuniverse/onebot) — Bot 接口标准
