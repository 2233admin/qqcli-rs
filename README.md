# qqcli

本地 QQ 聊天记录命令行工具。不用开 QQ 就能查记录、搜消息、导数据。

![qqcli](https://raw.githubusercontent.com/2233admin/qqcli-rs/main/docs/header.png)

## 为什么写这个

我的聊天记录全在本地，但想查一条历史消息得打开 QQ、翻半天。用这个直接在终端搜，快多了。

支持：
- 看最近会话列表
- 查某人的聊天记录（按时间过滤）
- 搜索消息内容（DuckDB 全文索引）
- 导出聊天记录（Markdown / JSONL / TXT）
- 打包聊天里的图片到 ZIP

## 安装

### 下载二进制（推荐）

去 [Releases](https://github.com/2233admin/qqcli-rs/releases) 页面，根据你的系统下载对应版本。

### 源码编译

```bash
git clone https://github.com/2233admin/qqcli-rs.git
cd qqcli-rs
cargo build --release
./target/release/qq --help
```

## 前置条件

1. **QQ NT** 已运行过一次（需要本地数据库）
2. **解密数据库**（第一次需要）

解密工具：[qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt)

## 常用命令

```bash
# 初始化（首次运行）
qq init

# 最近会话
qq sessions

# 查聊天记录
qq history 1234567890 --limit 50

# 按时间过滤
qq history 1234567890 --since 2024-01-01

# 搜消息（需要先建索引）
qq index
qq search "关键词"

# 导出
qq export 1234567890 -o chat.md
qq export 1234567890 --format jsonl -o chat.jsonl

# 打包图片
qq bundle 1234567890 -o images.zip
```

更多命令见 `qq --help`。

## 数据放哪

默认读取：
```
Documents/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db
```

## 技术栈

- Rust（零依赖部署，一个二进制文件）
- rusqlite（读本地 SQLite）
- DuckDB（全文搜索）
- clap（命令行参数）

## License

MIT
