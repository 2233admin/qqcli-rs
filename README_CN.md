# qqcli — Windows QQ 聊天记录搜索与导出 CLI

<p align="center">
  <img src="docs/header.gif" alt="qqcli Windows QQ 聊天记录命令行工具" width="720">
</p>

<p align="center">
  <strong>不打开 QQ、不翻聊天记录，直接搜索本机 QQ NT 聊天数据。</strong><br>
  面向 Windows 用户和 AI Agent 的 Rust 命令行工具：解密、导出和外发数据都会先获得明确的一次性授权。
</p>

<p align="center">
  <a href="https://github.com/2233admin/qqcli-rs/releases/latest"><img src="https://img.shields.io/github/v/release/2233admin/qqcli-rs?style=flat-square&logo=github&label=最新版本" alt="最新版本"></a>
  <a href="https://github.com/2233admin/qqcli-rs/actions"><img src="https://img.shields.io/github/actions/workflow/status/2233admin/qqcli-rs/CI.yml?style=flat-square&logo=github-actions&label=CI" alt="CI"></a>
  <a href="https://github.com/2233admin/qqcli-rs/releases"><img src="https://img.shields.io/github/downloads/2233admin/qqcli-rs/total?style=flat-square&logo=github&label=下载量" alt="下载量"></a>
  <img src="https://img.shields.io/badge/platform-Windows-blue?style=flat-square" alt="Windows 平台">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT 许可证"></a>
</p>

<p align="center">
  <a href="README.md"><strong>English</strong></a>
  &nbsp;·&nbsp;
  <a href="README_CN.md"><strong>简体中文</strong></a>
</p>

## 立即下载

| 需求 | 入口 |
|------|------|
| Windows 安装包 | [下载最新 Release](https://github.com/2233admin/qqcli-rs/releases/latest) |
| 完整性校验 | 解压前使用 `SHA256SUMS.txt` 校验 ZIP |
| 使用教程 | [阅读 Windows QQ 聊天记录搜索教程](docs/guide/windows-qq-chat-history-search.md) |
| Agent 接入 | 从 [`qq --json init`](#agent-自动化) 开始 |
| 源码和问题反馈 | [GitHub 仓库](https://github.com/2233admin/qqcli-rs) |

## qqcli 是什么？

`qqcli` 是一个面向 Windows 的本地命令行工具，用于 **QQ 聊天记录搜索、QQ NT 本地数据库访问、聊天记录导出和备份**。它读取本机 QQ NT 数据目录，建立全文索引，帮助你搜索消息、查看会话，并把用户明确选择的会话导出为 Markdown 或 JSONL。

当前发布版支持 **Windows 和 QQ NT 本地数据**。电脑上需要至少运行过一次 QQ。Linux 和 macOS 目前不是受支持的安装目标。

适合这些场景：

- 几秒内找回以前发过的地址、文件名或工作消息；
- 在 PowerShell 或 AI Agent 中搜索本地 QQ 聊天记录；
- 把指定会话导出，用于用户确认后的备份或分析；
- 诊断 QQ 数据库找不到、加密或配置失败的问题，同时不暴露密钥和消息正文。

## 核心能力

| 能力 | 说明 |
|------|------|
| 本地全文搜索 | 建立索引后搜索 QQ 消息，不需要打开 QQ 或手动翻页。 |
| QQ NT 支持 | 识别 QQ NT 本地数据库目录，包括 `nt_qq\\nt_db\\nt_msg.db`。 |
| Agent 可用 | 提供 JSON 输出、稳定退出码、脱敏诊断报告和 `next_command` 修复指引。 |
| 按动作授权 | 解密和外发是不同动作，分别需要明确的一次性授权。 |
| 本地优先 | 读取和搜索留在本机；导出、打包、同步不会静默执行。 |
| Windows 安装 | Release ZIP 包含安装器、版本清单和必需的 `duckdb.dll`。 |

## 快速开始

### Windows 安装

1. 从 [Releases](https://github.com/2233admin/qqcli-rs/releases/latest) 下载 Windows ZIP；
2. 使用其中的 `SHA256SUMS.txt` 校验 ZIP；
3. 解压后运行 `install.cmd`；
4. 重新打开 PowerShell，运行：

```powershell
qq init
```

`qq init` 会发现可用账号并检查本地数据库。如果需要解密，它会先解释本次需要访问的本机资源，再请求一次性授权，不会静默解密。

### 搜索本地 QQ 聊天记录

```powershell
qq sessions
qq index
qq search "会议"
qq history <会话ID> --since 2024-01-01
```

### 导出指定会话

导出属于独立的 **External Disclosure（外部披露）** 动作。先向用户确认准确的会话、目标路径和格式，再执行：

```powershell
qq --json export <会话ID> -o chat.md
qq export <会话ID> -o chat.md --consent-external-disclosure
```

`bundle` 和 `sync` 使用相同的授权边界。`sessions`、`history`、`search` 属于 **Read Access（读取）**，不会自动获得导出权限。

## Agent / 自动化

Agent 合约的目标是：先解释需求，再等待授权，失败时给出可执行的修复命令，不靠猜测绕过用户决定。

```powershell
# 查看版本和平台
qq --json version

# 发现账号和数据库状态
qq --json init

# 只有用户明确同意返回的授权范围后才能执行
qq --json init --consent-decrypt
```

稳定结果：

- 退出码 `0`：操作完成；
- 退出码 `2`：需要授权，展示返回的 `consent` 后等待用户同意；
- 退出码 `3`：需要配置或修复，按 `next_command` 执行，或运行 `qq doctor --json`。

Agent 可以通过环境变量临时提供 `QQCLI_DB_PATH` 和 `QQCLI_DB_KEY`。密钥不会打印，也不会以明文保存。Agent 不得伪造同意、跳过授权步骤，或把读取动作升级为导出动作。

## 命令速查

| 命令 | 用途 |
|------|------|
| `qq init` | 发现账号、检查 QQ NT 数据；仅在需要解密时请求授权 |
| `qq doctor` | 输出脱敏诊断报告和修复指引 |
| `qq version --json` | 为自动化输出版本和平台 |
| `qq sessions` | 列出最近会话 |
| `qq history <id>` | 查看带时间戳的聊天记录 |
| `qq index` | 建立全文搜索索引 |
| `qq search "关键词"` | 搜索本地 QQ 消息 |
| `qq export <id>` | 将用户确认的会话导出为 Markdown 或 JSONL |
| `qq bundle <id>` | 打包用户确认的媒体文件 |
| `qq sync` | 获得独立外部披露授权后再同步 |
| `qq plugin send <id> "消息"` | 可选的 NapCat 发消息集成 |

## 安全边界

qqcli 把操作分为三类，让 Agent 和用户清楚知道即将发生什么：

1. **Read Access（读取）**：检查、索引和搜索本地数据；
2. **Decryption Action（解密）**：配置本地解密工具后，获得一次性明确授权才能执行；
3. **External Disclosure（外部披露）**：把选中的数据写到读取路径之外或同步出去，必须再次明确授权。

诊断报告会脱敏用户路径，不包含解密密钥、消息正文或进程内存。安装器会检查 Release 版本和运行时文件。目前还没有代码签名，安装或升级前必须先校验 SHA-256。

## 常见问题

**找不到数据库怎么办？**<br>
先运行一次 QQ NT，再运行 `qq init`。如果数据库不在默认位置，可使用 `qq config set-db-path "D:\\QQ\\nt_msg.db"`，或用 `QQCLI_DB_PATH` 指定单次路径。

**数据库加密了，qqcli 会自动解密吗？**<br>
不会。先运行 `qq doctor`，配置本地解密工具和 SQLCipher，阅读授权范围并明确同意，再运行 `qq init --consent-decrypt`。保存的密钥使用 Windows DPAPI 保护，不会打印。

**为什么 Agent 停在退出码 2？**<br>
这是安全授权暂停。Agent 必须把返回的授权范围展示给用户，得到明确同意后才能执行工具提供的命令。

**Linux 或 macOS 能用吗？**<br>
当前发布包和 QQ NT 解密流程仅支持 Windows。

## 它解决的问题

> 三年的困扰，三次翻聊天记录寻找明明记得发过的内容。

```text
打开 QQ → 翻 → 翻 → 找错年份 → 放弃

qq search "关键词"     # 本地搜索，不再翻页
```

## 技术栈

Rust · rusqlite · DuckDB · tokio · clap · SQLCipher 兼容解密流程

## 参与贡献

欢迎提交 PR。开发说明见 [CONTRIBUTING.md](CONTRIBUTING.md)。如果工具找不到数据库，提交脱敏后的 `qq doctor --json` 报告会更容易定位问题。

维护者：[2233admin](https://github.com/2233admin)

## License

MIT

<p align="center">
  <a href="https://star-history.com/#2233admin/qqcli-rs&Date"><img src="https://api.star-history.com/svg?repos=2233admin/qqcli-rs&type=Date" alt="qqcli Star 历史"></a>
</p>
