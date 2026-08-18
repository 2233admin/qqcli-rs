# qqcli — Windows QQ chat history search and export CLI

<p align="center">
  <img src="docs/header.gif" alt="qqcli Windows QQ chat history CLI" width="720">
</p>

<p align="center">
  <strong>Search, read, export, and back up local QQ NT chat history without scrolling through QQ.</strong><br>
  A Windows-first Rust CLI for people and AI agents, with explicit consent before decryption or external disclosure.
</p>

<p align="center">
  <a href="https://github.com/2233admin/qqcli-rs/releases/latest"><img src="https://img.shields.io/github/v/release/2233admin/qqcli-rs?style=flat-square&logo=github&label=Latest%20release" alt="Latest release"></a>
  <a href="https://github.com/2233admin/qqcli-rs/actions"><img src="https://img.shields.io/github/actions/workflow/status/2233admin/qqcli-rs/CI.yml?style=flat-square&logo=github-actions&label=CI" alt="CI"></a>
  <a href="https://github.com/2233admin/qqcli-rs/releases"><img src="https://img.shields.io/github/downloads/2233admin/qqcli-rs/total?style=flat-square&logo=github&label=Downloads" alt="Downloads"></a>
  <img src="https://img.shields.io/badge/platform-Windows-blue?style=flat-square" alt="Windows platform">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT license"></a>
</p>

<p align="center">
  <a href="README.md"><strong>English</strong></a>
  &nbsp;·&nbsp;
  <a href="README_CN.md"><strong>简体中文</strong></a>
</p>

## Download

| Need | Link |
|------|------|
| Windows installer ZIP | [Download the latest release](https://github.com/2233admin/qqcli-rs/releases/latest) |
| Integrity check | Verify `SHA256SUMS.txt` before extracting |
| Step-by-step guide | [Read the Windows QQ chat history guide](docs/guide/windows-qq-chat-history-search.md) |
| Agent integration | Start with [`qq --json init`](#agent-and-automation) |
| Source and issues | [GitHub repository](https://github.com/2233admin/qqcli-rs) |

## What is qqcli?

`qqcli` is a local Windows command-line tool for **QQ chat history search, QQ NT database access, chat export, and backup**. It reads the local QQ NT data directory, builds a full-text index, and lets you find messages, inspect sessions, and export a user-approved conversation as Markdown or JSONL.

The current release targets **Windows and QQ NT local data**. QQ must have run on the machine at least once. Linux and macOS are not supported installation targets yet.

Typical use cases:

- Find an old address, file name, or work message in seconds.
- Search local QQ messages from PowerShell or an AI Agent.
- Export one selected conversation for a user-approved backup or analysis.
- Diagnose a missing or encrypted QQ database without exposing keys or message content.

## Why people use it

| Capability | What it does |
|------------|--------------|
| Fast local search | Index and search QQ messages without opening QQ or scrolling through old chats. |
| QQ NT support | Works with the local QQ NT database layout, including `nt_qq\\nt_db\\nt_msg.db`. |
| Agent-ready contract | JSON output, stable exit codes, redacted diagnostics, and a `next_command` for recovery. |
| Consent by action | Decryption and external disclosure are separate actions that require explicit, one-time consent. |
| Local-first workflow | Read and search stay on the machine; export, bundle, and sync do not run silently. |
| Windows-native setup | The release ZIP includes the installer, version manifest, and required `duckdb.dll`. |

## Quick start

### Windows install

1. Download the Windows ZIP from [Releases](https://github.com/2233admin/qqcli-rs/releases/latest).
2. Verify the ZIP with the included `SHA256SUMS.txt`.
3. Extract it and run `install.cmd`.
4. Open a new PowerShell window and run:

```powershell
qq init
```

`qq init` discovers available accounts and checks the local database. If decryption is needed, it first explains the exact one-time local access and asks for consent. It does not silently decrypt data.

### Search local QQ messages

```powershell
qq sessions
qq index
qq search "meeting"
qq history <session-id> --since 2024-01-01
```

### Export a selected conversation

Export is a separate **External Disclosure** action. Ask the person for the exact conversation, destination, and format first:

```powershell
qq --json export <session-id> -o chat.md
qq export <session-id> -o chat.md --consent-external-disclosure
```

`bundle` and `sync` follow the same consent boundary. Local `sessions`, `history`, and `search` are **Read Access** and do not grant export permission.

## Agent and automation

The Agent contract is designed so an Agent can explain what is needed, pause for authorization, and recover from a failed setup without guessing.

```powershell
# Discover version and platform
qq --json version

# Discover accounts and current database state
qq --json init

# Only after the Human User approves the returned consent scope
qq --json init --consent-decrypt
```

Stable outcomes:

- Exit code `0`: the requested operation completed.
- Exit code `2`: consent is required; show the returned `consent` payload and wait.
- Exit code `3`: setup or repair is required; follow `next_command` or run `qq doctor --json`.

An Agent may provide `QQCLI_DB_PATH` and temporary `QQCLI_DB_KEY` through the environment. The key is not printed or persisted as plain text. An Agent must not invent consent, skip the consent step, or turn a read operation into an export.

## Command map

| Command | Purpose |
|---------|---------|
| `qq init` | Discover an account, inspect QQ NT data, and request consent only when decryption is needed |
| `qq doctor` | Produce a redacted diagnostic report with repair guidance |
| `qq version --json` | Report the installed version and platform for automation |
| `qq sessions` | List recent chat sessions |
| `qq history <id>` | Read chat history with timestamps |
| `qq index` | Build the full-text search index |
| `qq search "keyword"` | Search local QQ messages |
| `qq export <id>` | Export a user-approved conversation as Markdown or JSONL |
| `qq bundle <id>` | Bundle user-approved media files |
| `qq sync` | Sync only after separate external-disclosure consent |
| `qq plugin send <id> "message"` | Optional NapCat integration for sending messages |

## Safety model

qqcli separates three operations so the Agent and the Human User can see what is about to happen:

1. **Read Access** — inspect, index, and search local data.
2. **Decryption Action** — use a configured local decryption tool only after one-time explicit consent.
3. **External Disclosure** — write or sync selected data outside the local read path only after separate explicit consent.

Diagnostic reports redact user paths and never include decryption keys, message bodies, or process memory. The installer checks the release version and required runtime files. Code signing is not yet available, so verify SHA-256 before installation or upgrade.

## FAQ

**The database was not found. What should I do?**<br>
Run QQ NT once, then run `qq init`. For a custom location, use `qq config set-db-path "D:\\QQ\\nt_msg.db"` or set `QQCLI_DB_PATH` for one run.

**The database is encrypted. Does qqcli decrypt automatically?**<br>
No. Run `qq doctor`, configure the required local decryption tool and SQLCipher, review the consent scope, then run `qq init --consent-decrypt`. Saved keys are protected with Windows DPAPI and are never printed.

**Why does an Agent stop with exit code 2?**<br>
That is the safe consent pause. The Agent must show the returned scope to the Human User and run the provided command only after an explicit approval.

**Can I use this on Linux or macOS?**<br>
The released installer and QQ NT decryption workflow currently support Windows only.

## The problem it solves

> Three years of frustration. Three times searching for something I knew I'd sent before.

```text
Open QQ → scroll → scroll → wrong year → give up

qq search "keyword"     # local search, no scrolling
```

## Tech stack

Rust · rusqlite · DuckDB · tokio · clap · SQLCipher-compatible workflow

## Contributing

Contributions welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup instructions. If the tool cannot find your database, a redacted `qq doctor --json` report is the best starting point for an issue.

## License

MIT

<p align="center">
  <a href="https://star-history.com/#2233admin/qqcli-rs&Date"><img src="https://api.star-history.com/svg?repos=2233admin/qqcli-rs&type=Date" alt="qqcli star history"></a>
</p>
