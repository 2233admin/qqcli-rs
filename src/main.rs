//! QQCLI — QQ 本地数据 CLI (仿 wx CLI)
//!
//! 数据源: 解密后的 nt_msg.db (SQLCipher)
//! 路径: Documents/Tencent Files/{uin}/nt_qq/nt_db/nt_msg.db

mod cache;
mod commands;
mod config;
mod db;
mod db_index;
mod decrypt;
mod napcat;
mod output;
mod schema;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use std::io;

#[derive(Parser)]
#[command(
    name = "qq",
    version,
    about = "QQ 本地数据 CLI",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 输出 JSON 格式 (默认 YAML)
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// 初始化: 检测 DB 路径, 确认解密状态
    Init {
        /// 强制重新扫描
        #[arg(long)]
        force: bool,
    },

    /// 调试: 查看表结构
    DebugTables {},

    /// 调试: 探测消息 BLOB 原始字节
    DebugProbe {},

    /// 最近会话列表
    Sessions {
        /// 会话数量
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },

    /// 查看聊天记录
    History {
        /// 会话 ID (群号或好友 QQ 号)
        chat: String,
        /// 消息数量
        #[arg(short, long, default_value_t = 50)]
        limit: usize,
        /// 分页偏移
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// 起始时间 YYYY-MM-DD
        #[arg(long)]
        since: Option<String>,
        /// 结束时间 YYYY-MM-DD
        #[arg(long)]
        until: Option<String>,
        /// 消息类型过滤
        #[arg(long)]
        msg_type: Option<String>,
    },

    /// 搜索消息
    Search {
        /// 关键词
        keyword: String,
        /// 限定会话
        #[arg(short, long)]
        chat: Option<String>,
        /// 结果数量
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        /// 起始时间
        #[arg(long)]
        since: Option<String>,
        /// 结束时间
        #[arg(long)]
        until: Option<String>,
    },

    /// 联系人 (好友/群)
    Contacts {
        /// 按名字过滤
        #[arg(short, long)]
        query: Option<String>,
        /// 显示数量
        #[arg(short, long, default_value_t = 50)]
        limit: usize,
        /// 类型过滤: friend / group / all
        #[arg(long, default_value = "all")]
        kind: String,
    },

    /// 导出聊天记录
    Export {
        /// 会话 ID
        chat: String,
        /// 起始时间
        #[arg(long)]
        since: Option<String>,
        /// 结束时间
        #[arg(long)]
        until: Option<String>,
        /// 最多导出条数
        #[arg(short, long, default_value_t = 500)]
        limit: usize,
        /// 格式: markdown / txt / json / yaml
        #[arg(short, long, default_value = "markdown")]
        format: String,
        /// 输出文件 (默认 stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// 打包聊天记录中的媒体文件到 ZIP
    Bundle {
        /// 会话 ID
        chat: String,
        /// 起始时间 YYYY-MM-DD
        #[arg(long)]
        since: Option<String>,
        /// 结束时间 YYYY-MM-DD
        #[arg(long)]
        until: Option<String>,
        /// 最大消息数
        #[arg(short, long, default_value_t = 500)]
        limit: usize,
        /// 输出 ZIP 文件路径
        #[arg(short, long, default_value = "media.zip")]
        output: String,
    },

    /// 有未读消息的会话
    Unread {
        /// 显示数量
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },

    /// 群成员
    Members {
        /// 群号
        chat: String,
    },

    /// 自上次检查以来的新消息
    NewMessages {
        /// 显示数量上限
        #[arg(short, long, default_value_t = 200)]
        limit: usize,
    },

    /// 聊天统计分析
    Stats {
        /// 会话 ID
        chat: Option<String>,
        /// 起始时间
        #[arg(long)]
        since: Option<String>,
        /// 结束时间
        #[arg(long)]
        until: Option<String>,
    },

    /// 群列表 (从 NapCat 获取，需 NapCat 运行)
    Groups {
        /// NapCat WebSocket URL
        #[arg(long, default_value = "ws://127.0.0.1:18301")]
        url: String,
        /// access_token
        #[arg(long)]
        token: Option<String>,
    },

    /// NapCat OneBot11 操作 (需 NapCat 运行于 ws://127.0.0.1:18301)
    Nap {
        /// 子命令: whoami | friends | groups | history | send
        sub: String,
        /// NapCat WebSocket URL
        #[arg(long, default_value = "ws://127.0.0.1:18301")]
        url: String,
        /// access_token
        #[arg(long)]
        token: Option<String>,
        /// 额外参数 (history: <private|group> <target_id> [count];
        ///           send: <private|group> <target_id> <message...>)
        args: Vec<String>,
    },

    /// 同步联系人缓存 (从 NapCat 拉取好友/群列表，存入 contacts.json)
    Sync {
        /// NapCat WebSocket URL
        #[arg(long, default_value = "ws://127.0.0.1:18301")]
        url: String,
        /// access_token
        #[arg(long)]
        token: Option<String>,
    },

    /// 生成 shell 补全脚本
    Completion {
        /// Shell 类型
        #[arg(value_enum)]
        shell: Shell,
    },

    /// 将消息导出到 DuckDB FTS 索引
    Index {},

    /// NapCat IPC 插件模式 (直接调用 wrapper API，低开销)
    Plugin {
        /// 子命令: ping | send | friends | groups | members | chats
        sub: String,
        /// IPC 端口 (默认 9334)
        #[arg(long, default_value_t = 9334)]
        port: u16,
        /// 额外参数
        args: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Commands::Completion { shell } = &cli.command {
        generate(*shell, &mut Cli::command(), "qq", &mut io::stdout());
        return Ok(());
    }

    match &cli.command {
        Commands::Init { force } => commands::init(*force),
        Commands::DebugTables {} => commands::debug_tables(),
        Commands::DebugProbe {} => commands::debug_probe(),
        Commands::Sessions { limit } => commands::sessions(*limit, cli.json),
        Commands::History {
            chat,
            limit,
            offset,
            since,
            until,
            msg_type,
        } => commands::history(
            chat,
            *limit,
            *offset,
            since.as_deref(),
            until.as_deref(),
            msg_type.as_deref(),
            cli.json,
        ),
        Commands::Search {
            keyword,
            chat,
            limit,
            since,
            until,
        } => commands::search(
            keyword,
            chat.as_deref(),
            *limit,
            since.as_deref(),
            until.as_deref(),
            cli.json,
        ),
        Commands::Contacts { query, limit, kind } => {
            commands::contacts(query.as_deref(), *limit, kind, cli.json)
        }
        Commands::Export {
            chat,
            since,
            until,
            limit,
            format,
            output,
        } => commands::export(
            chat,
            since.as_deref(),
            until.as_deref(),
            *limit,
            format,
            output.as_deref(),
            cli.json,
        ),
        Commands::Bundle { chat, since, until, limit, output } => commands::bundle_media(
            chat,
            since.as_deref(),
            until.as_deref(),
            *limit,
            output,
        ),
        Commands::Unread { limit } => commands::unread(*limit, cli.json),
        Commands::Members { chat } => commands::members(chat, cli.json),
        Commands::NewMessages { limit } => commands::new_messages(*limit, cli.json),
        Commands::Stats { chat, since, until } => commands::stats(
            chat.as_deref(),
            since.as_deref(),
            until.as_deref(),
            cli.json,
        ),
        Commands::Groups { url, token } => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(commands::groups(url, token.as_deref()))
        }
        Commands::Nap { sub, url, token, args } => {
            let token_ref = token.as_deref();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(napcat::run(sub, url, token_ref, &args_ref))
        }
        Commands::Sync { url, token } => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(commands::sync(url, token.as_deref()))
        }
        Commands::Completion { .. } => unreachable!(),
        Commands::Index {} => commands::index(),
        Commands::Plugin { sub, port, args } => {
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            commands::plugin(sub, *port, &args_ref)
        }
    }
}
