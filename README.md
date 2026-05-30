# qqcli

<p align="center">
  <img src="docs/header.gif" alt="qqcli" width="720">
</p>

<p align="center">

  <a href="https://github.com/2233admin/qqcli-rs/releases">
    <img src="https://img.shields.io/github/downloads/2233admin/qqcli-rs/total?style=flat-square&logo=github&label=Downloads" alt="Downloads">
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

## The Story

> *Three years of frustration. Three times searching for something I knew I'd sent before.*

```
2019 │ Looking for an address a friend sent me once
     │ Open QQ → scroll → scroll → scroll → forget which year → give up
     │
2020 │ Looking for a file shared in a group chat
     │ Open QQ → scroll → scroll → scroll → don't remember filename → give up
     │
2021 │ Looking for a work message
     │ Open QQ → scroll → scroll → scroll → wrong group → give up
```

The third time, I wrote a tool.

Now:
```bash
qq search "keyword"     # 0.3 seconds. No QQ. No scrolling.
```

---

## Features

| Command | Description |
|---------|-------------|
| `qq sessions` | List recent chat sessions |
| `qq history <id>` | View chat history with timestamps |
| `qq history <id> --since 2024-01-01` | Filter by date range |
| `qq index` | Build full-text search index |
| `qq search "keyword"` | Search across all messages |
| `qq export <id> -o chat.md` | Export as Markdown |
| `qq export <id> --format jsonl` | Export as JSONL |
| `qq bundle <id> -o images.zip` | Download all images |
| `qq plugin send <id> "message"` | Send message via NapCat |

---

## Quick Start

### Download

| Platform | Download |
|----------|----------|
| Windows | Download `qq.exe` from [Releases](https://github.com/2233admin/qqcli-rs/releases) |
| Linux/macOS | Download `qqcli` binary |

### Run

```bash
# View recent sessions
qq sessions

# Search everything
qq index && qq search "meeting"

# Export a chat
qq export 123456789 -o chat.md
```

---

## Tech Stack

```
┌─────────────────────────────────────────────────────────────┐
│                         qqcli                              │
├─────────────────────────────────────────────────────────────┤
│  Rust · rusqlite · DuckDB · tokio · clap                   │
├─────────────────────────────────────────────────────────────┤
│  QQ NT local database: Documents\Tencent Files\{QQ}\        │
│                          nt_qq\nt_db\nt_msg.db             │
└─────────────────────────────────────────────────────────────┘
```

---

## FAQ

**Q: Database not found?**
> Make sure QQ NT has been run at least once.

**Q: Database encrypted?**
> Use [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) to decrypt.

**Q: Search is slow?**
> Run `qq index` first to build the search index.

**Q: Where is the database?**
> Default location: `Documents\Tencent Files\{QQ}\nt_qq\nt_db\nt_msg.db`
>
> Custom path: `export QQCLI_DB_PATH=/path/to/nt_msg.db`

---

## Contributing

Contributions welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for setup instructions.

## License

MIT

---

<p align="center">

[![Star History](https://api.star-history.com/svg?repos=2233admin/qqcli-rs&type=Date)](https://star-history.com/#2233admin/qqcli-rs&Date)

</p>

<p align="center">
  <em>Time saved from scrolling can be used for something else.</em>
</p>
