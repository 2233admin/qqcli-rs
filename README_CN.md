# qqcli

<p align="center">
  <img src="docs/header.gif" alt="qqcli" width="720">
</p>

<p align="center">

  <a href="https://github.com/2233admin/qqcli-rs/actions">
    <img src="https://img.shields.io/github/actions/workflow/status/2233admin/qqcli-rs/CI.yml?style=flat-square&logo=github-actions&label=CI" alt="CI">
  </a>
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
| `qq init` | 选择账号、检查数据库状态并给出下一步 |
| `qq doctor` | 检查解密所需工具和本机配置 |
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

### Windows 安装

1. 从 [Releases](https://github.com/2233admin/qqcli-rs/releases) 下载 `qqcli-*-windows-x86_64.zip`。
2. 解压后双击 `install.cmd`。
3. 重新打开 PowerShell，运行：

```powershell
qq init
```

`qq init` 会先确认是否真的需要解密，再展示一次性授权说明：

- 只有一个账号时会自动绑定；
- 多个账号时按提示执行 `qq init --account <QQ号>`；
- QQ 使用自定义数据目录时执行 `qq init --db-path <nt_msg.db 路径>`；
- 数据库加密且依赖已就绪时，交互终端输入“同意”即可继续；工具会说明它将读取的本机资源、保存位置和不会做的事；
- Agent 必须先向用户展示 JSON 中的 `consent` 内容，得到同意后才执行 `qq init --consent-decrypt`；授权只用于这一次操作，不会被永久保存；
- 解密工具未就绪时，先运行 `qq doctor`，按下一步提示配置。

### Agent / 自动化

所有初始化状态都能以 JSON 获取：

```powershell
qq --json init
```

- 退出码 `0`：数据库可用；
- 退出码 `1`：需要选择账号、设置路径、配置依赖或获取用户解密授权；JSON 的 `next_command` 给出下一步；
- `status: "consent_required"` 时，Agent 必须把 `consent.scope` 原样告知用户；只有用户同意后，才能执行 `consent.command_after_user_agrees`；
- Agent 可通过 `QQCLI_DB_PATH` 传入数据库路径，通过 `QQCLI_DB_KEY` 临时传入密钥；后者不会写入磁盘。

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
> 先运行 `qq doctor`。如未配置工具，请从 [qq-nt-decrypt](https://github.com/MrXiaoM/qq-nt-decrypt) 获取密钥提取脚本，并准备 SQLCipher：
>
> ```powershell
> qq config set-key-script <windows_ntqq_get_key.ps1 路径>
> qq config set-sqlcipher <sqlcipher.exe 路径>
> qq init --consent-decrypt
> ```
>
> 解密密钥会使用 Windows DPAPI 保护；不会打印到终端，也不会以明文写入配置文件。

**Q: 搜索很慢？**
> 先运行 `qq index` 建立搜索索引。

**Q: 数据库在哪？**
> 默认位置：`文档\Tencent Files\{QQ号}\nt_qq\nt_db\nt_msg.db`
>
> 自定义路径：
> - 推荐一次性保存：`qq config set-db-path "D:\QQ\nt_msg.db"`
> - 单次 PowerShell：`$env:QQCLI_DB_PATH = "D:\QQ\nt_msg.db"`

**Q: Linux/macOS 能直接使用吗？**
> 当前发布包和 QQ NT 解密流程仅支持 Windows。Linux/macOS 需要自行提供兼容的本地数据库与解密工具，暂不作为受支持的安装目标。

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
