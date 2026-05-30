# qqcli

<h1 align="center">
  <img src="docs/header.gif" alt="qqcli" width="720">
</h1>

<p align="center">
本地 QQ 聊天记录命令行工具。不用开 QQ，直接在终端里查记录、搜消息、导数据。
</p>

<p align="center">
  <a href="https://github.com/2233admin/qqcli-rs/releases"><img src="https://img.shields.io/badge/downloads-v0.1.0-green" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  <a href="https://github.com/2233admin/qqcli-rs/stargazers"><img src="https://img.shields.io/badge/stars-welcome-yellow" alt="Stars"></a>
</p>

---

## 为什么写这个

想找一条半年前的消息。

打开 QQ → 翻会话 → 找群 → 滚动 → 滚动 → 滚动 → 忘了是哪年 → 关掉 → 重来。

重复三次后我决定写个工具，让这个过程变成：

```bash
qq search "关键词"
```

不用开 QQ。不用滚动。直接出结果。

---

## 功能

### 聊天记录

```bash
# 最近会话
qq sessions

# 查聊天记录
qq history 123456789 --limit 100

# 按时间过滤
qq history 123456789 --since 2024-01-01
```

### 搜索

```bash
# 建立索引（首次需要）
qq index

# 搜
qq search "关键词"
```

### 导出

```bash
# Markdown 导出
qq export 123456789 -o chat.md

# JSONL（程序用）
qq export 123456789 --format jsonl -o chat.jsonl

# 图片打包
qq bundle 123456789 -o images.zip
```

### NapCat 集成

```bash
# 发消息
qq plugin send private 123456789 "hello"

# 查好友/群
qq plugin friends
qq plugin groups
```

---

## 安装

**下载二进制：** [Releases 页面](https://github.com/2233admin/qqcli-rs/releases)

```bash
# Windows
qq.exe --help

# Linux
chmod +x qqcli && ./qqcli --help
```

**源码编译：**

```bash
git clone https://github.com/2233admin/qqcli-rs.git
cd qqcli-rs
cargo build --release
```

---

## 数据位置

读取 QQ NT 本地数据库：

| 系统 | 路径 |
|------|------|
| Windows | `Documents\Tencent Files\{QQ号}\nt_qq\nt_db\nt_msg.db` |
| Linux/macOS | `~/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db` |

**手动指定：**
```bash
export QQCLI_DB_PATH=/path/to/nt_msg.db
```

---

## 常见问题

**Q: 找不到数据库？**
A: 确保 QQ NT 运行过一次。指定路径见上方。

**Q: 数据库加密了？**
A: 用 [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) 解密后使用。

**Q: 搜索慢？**
A: 先 `qq index` 建立索引。

---

## 技术栈

[Rust](https://rust-lang.org) · [rusqlite](https://github.com/rusqlite/rusqlite) · [DuckDB](https://duckdb.org/) · [tokio](https://tokio.rs/) · [clap](https://clap.rs/)

---

## 相关项目

- [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) — 数据库解密
- [NapCat](https://github.com/NapNeko/NapCatQQ) — QQ Bot 框架
- [onebot](https://github.com/botuniverse/onebot) — Bot 接口标准

---

## License

MIT

---

## Star History

<a href="https://star-history.com/#2233admin/qqcli-rs&Date">
  <img src="https://api.star-history.com/svg?repos=2233admin/qqcli-rs&type=Date" alt="Star History" width="720">
</a>

---

*省下滚动的时间，可以用来做点别的。*
