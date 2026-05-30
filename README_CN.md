# qqcli

<p align="center">
  <img src="docs/header.gif" alt="qqcli" width="720">
</p>

<p align="center">

  <a href="https://github.com/2233admin/qqcli-rs/releases">
    <img src="https://img.shields.io/github/downloads/2233admin/qqcli-rs/total?style=flat-square&logo=github&label=下载量" alt="Downloads">
  </a>
  <a href="https://crates.io/crates/qqcli">
    <img src="https://img.shields.io/crates/v/qqcli?style=flat-square&logo=rust&label=Crates.io" alt="Crates.io">
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue?style=flat-square" alt="Platform">
  <a href="LICENSE">
    <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
  </a>

</p>

<p align="center">

  <a href="README.md"><strong>English</strong></a>
  &nbsp;·&nbsp;
  <a href="README_CN.md"><strong>简体中文</strong></a>

</p>

---

## 故事

> *三年的困扰。三次翻聊天记录找一条消息。*

```
2019 │ 找一个朋友发过的地址
     │ 打开QQ → 翻 → 翻 → 翻 → 忘了是哪年的 → 放弃
     │
2020 │ 找一个群里分享过的文件
     │ 打开QQ → 翻 → 翻 → 翻 → 不记得文件名 → 放弃
     │
2021 │ 找一条重要的工作消息
     │ 打开QQ → 翻 → 翻 → 翻 → 不是这个群 → 放弃
```

第三次之后，我写了这个工具。

现在：
```bash
qq search "关键词"     # 0.3秒。不开QQ。不翻记录。
```

---

## 功能

| 命令 | 说明 |
|------|------|
| `qq sessions` | 列出最近会话 |
| `qq history <id>` | 查看聊天记录（带时间戳） |
| `qq history <id> --since 2024-01-01` | 按日期过滤 |
| `qq index` | 建立全文搜索索引 |
| `qq search "关键词"` | 搜索所有消息 |
| `qq export <id> -o chat.md` | 导出为 Markdown |
| `qq export <id> --format jsonl` | 导出为 JSONL |
| `qq bundle <id> -o images.zip` | 下载所有图片 |
| `qq plugin send <id> "消息"` | 通过 NapCat 发送消息 |

---

## 快速开始

### 下载

| 平台 | 下载方式 |
|------|----------|
| Windows | 从 [Releases](https://github.com/2233admin/qqcli-rs/releases) 下载 `qq.exe` |
| Linux/macOS | 下载 `qqcli` 二进制文件 |

### 使用

```bash
# 查看最近会话
qq sessions

# 搜索全部记录
qq index && qq search "会议"

# 导出会话
qq export 123456789 -o chat.md
```

---

## 技术栈

```
┌─────────────────────────────────────────────────────────────┐
│                         qqcli                              │
├─────────────────────────────────────────────────────────────┤
│  Rust · rusqlite · DuckDB · tokio · clap                   │
├─────────────────────────────────────────────────────────────┤
│  QQ NT 本地数据库: 文档\Tencent Files\{QQ号}\                 │
│                      nt_qq\nt_db\nt_msg.db                 │
└─────────────────────────────────────────────────────────────┘
```

---

## 常见问题

**Q: 找不到数据库？**
> 确保 QQ NT 至少运行过一次。

**Q: 数据库加密了？**
> 使用 [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) 解密。

**Q: 搜索很慢？**
> 先运行 `qq index` 建立搜索索引。

**Q: 数据库在哪？**
> 默认位置：`文档\Tencent Files\{QQ号}\nt_qq\nt_db\nt_msg.db`
>
> 自定义路径：`export QQCLI_DB_PATH=/path/to/nt_msg.db`

---

## 参与贡献

欢迎提交 PR！详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## License

MIT

---

<p align="center">

[![Star History](https://api.star-history.com/svg?repos=2233admin/qqcli-rs&type=Date)](https://star-history.com/#2233admin/qqcli-rs&Date)

</p>

<p align="center">
  <em>省下滚动的时间，可以用来做点别的。</em>
</p>
