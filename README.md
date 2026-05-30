# qqcli

<h1 align="center">
  <img src="docs/header.gif" alt="qqcli" width="720">
</h1>

<p align="center">
本地 QQ 聊天记录命令行工具
</p>

<p align="center">
  <a href="https://github.com/2233admin/qqcli-rs/releases"><img src="https://img.shields.io/badge/downloads-v0.1.0-green" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
</p>

---

## 为什么写这个

2019年，我在找一个朋友发过的地址。

打开 QQ → 翻会话列表 → 找到他 → 滚动 → 滚动 → 滚动 → 忘了是哪年 → 关掉 → 重来。

2020年，我在找一个群里的文件。

打开 QQ → 翻会话列表 → 找群 → 滚动 → 滚动 → 滚动 → 不记得文件名 → 关掉 → 重来。

2021年，我在找一个重要的工作消息。

打开 QQ → 翻会话列表 → 找群 → 滚动 → 滚动 → 滚动 → 不是这个群 → 关掉 → 重来。

我重复了三次。

每次都是同样的流程：打开 QQ、翻、滚动、关掉、重来。

第三次之后我决定写个工具。

现在变成了：

```bash
qq search "关键词"
```

不用开 QQ。不用滚动。结果直接出来。

---

## 它能做什么

**聊天记录**
```bash
qq sessions              # 最近会话
qq history 123456789     # 查聊天记录
qq history 123456789 --since 2024-01-01
```

**搜索**（先建索引）
```bash
qq index                # 建立搜索索引
qq search "关键词"      # 搜
```

**导出**
```bash
qq export 123456789 -o chat.md        # Markdown
qq export 123456789 --format jsonl -o chat.jsonl  # JSONL
qq bundle 123456789 -o images.zip      # 图片打包
```

**NapCat 集成**
```bash
qq plugin send private 123456789 "hello"
qq plugin friends
```

---

## 安装

**下载二进制：** [Releases](https://github.com/2233admin/qqcli-rs/releases)

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

## 数据在哪

读取 QQ NT 本地数据库：

```
Windows:  Documents\Tencent Files\{QQ号}\nt_qq\nt_db\nt_msg.db
Linux/macOS: ~/Tencent Files/{QQ号}/nt_qq/nt_db/nt_msg.db
```

手动指定：
```bash
export QQCLI_DB_PATH=/path/to/nt_msg.db
```

---

## 常见问题

**Q: 找不到数据库？**
A: 确保 QQ NT 运行过一次。

**Q: 数据库加密了？**
A: 用 [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) 解密。

**Q: 搜索很慢？**
A: 先 `qq index` 建立索引。

---

## 技术栈

Rust · rusqlite · DuckDB · tokio · clap

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
