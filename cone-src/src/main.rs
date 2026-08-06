mod api;
mod cmds;
mod measure;
mod mihomo_setup;
mod ui;

use api::{Client, Config};
use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cone-cli", about = "Mihomo (Clash.Meta) 测速与节点选择工具 (Rust 版)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 显示版本/API/端口/主选择器/当前节点/节点总数
    #[command(alias = "st")]
    Status,
    /// 列出全部节点 (带类型，标记当前节点)
    #[command(alias = "ls")]
    List,
    /// 并行测全部节点延迟，按快慢显示前 N (默认 15)
    /// 可加关键字过滤，如: cone-cli ping 美国 / cone-cli ping 美国 -n 30
    #[command(alias = "latency")]
    Ping {
        /// 节点名过滤关键字 (可选，大小写不敏感包含匹配)
        filter: Option<String>,
        /// 显示前 N 名 (默认 15)
        #[arg(short = 'n', long = "num")]
        n: Option<usize>,
    },
    /// 吞吐测速 (只读，测完恢复原节点)
    /// 可加关键字过滤，如: cone-cli speed 日本 -n 10
    #[command(alias = "throughput")]
    Speed {
        /// 节点名过滤关键字 (可选)
        filter: Option<String>,
        /// 候选数 (默认 5)
        #[arg(short = 'n', long = "num")]
        n: Option<usize>,
    },
    /// 自动选最快并切换
    /// 可加关键字过滤，如: cone-cli best 香港 -n 3
    #[command(alias = "fastest")]
    Best {
        /// 节点名过滤关键字 (可选)
        filter: Option<String>,
        /// 候选数 (默认 5)
        #[arg(short = 'n', long = "num")]
        n: Option<usize>,
    },
    /// fzf 交互式选节点
    #[command(alias = "menu", alias = "fzf")]
    Pick {
        #[arg(default_value = "")]
        mode: String,
    },
    /// 直接切换节点，支持模糊匹配
    #[command(alias = "select", alias = "switch")]
    Use { name: String },
    /// 拉取订阅生成 config.yaml 并自动重载
    #[command(alias = "sub")]
    Update { url: Option<String> },
    /// 控制 mihomo 服务 (on/off/restart/status)
    #[command(alias = "svc")]
    Service {
        #[arg(default_value = "status")]
        act: String,
    },
    /// 控制 TUN (on/off/status)
    Tun {
        #[arg(default_value = "status")]
        act: String,
    },
}

fn main() {
    // 手动拦截 help: clap 内置 help 会显示自动生成版本, 我们要用自己的美化版
    let args: Vec<String> = std::env::args().skip(1).collect();
    // 无参数, 或显式 help/-h/--help -> 显示美化帮助
    if args.is_empty()
        || args.first().map(|s| s.as_str()) == Some("help")
        || args.first().map(|s| s.as_str()) == Some("-h")
        || args.first().map(|s| s.as_str()) == Some("--help")
    {
        cmds::cmd_help();
        return;
    }

    // 其余交给 clap 解析 (非法子命令由 clap 报错)
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // 非法参数: 让 clap 打印它的标准错误 (用法提示)
            e.print().ok();
            std::process::exit(e.exit_code());
        }
    };
    // 兜底: try_parse 可能因内置 help 退出, 这里 CommandFactory 用于 future-proof
    let _ = Cli::command(); // 触发一次 command 构造 (no-op, 避免 unused import)

    let cfg = Config::from_env();
    let client = Client::new(cfg.clone());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法创建 tokio 运行时");

    // 启动时确保 mihomo 核心存在 (同级目录无则自动下载)
    let colors = if console::user_attended() && std::env::var("NO_COLOR").is_err() {
        &ui::ANSI
    } else {
        &ui::PLAIN
    };
    if let Err(e) = rt.block_on(mihomo_setup::ensure_mihomo(colors)) {
        eprintln!("{}警告: {}{}", colors.yellow, e, colors.reset);
        eprintln!("{}部分命令可能无法正常工作。{}", colors.dim, colors.reset);
    }

    let cmd = cli.command.unwrap(); // 上面已确保非 None
    match cmd {
        Cmd::Status => rt.block_on(cmds::cmd_status(&client)),
        Cmd::List => rt.block_on(cmds::cmd_list(&client)),
        Cmd::Ping { filter, n } => rt.block_on(cmds::cmd_ping(&client, n.unwrap_or(15), filter.as_deref())),
        Cmd::Speed { filter, n } => rt.block_on(cmds::cmd_speed(&client, n.unwrap_or(5), filter.as_deref())),
        Cmd::Best { filter, n } => rt.block_on(cmds::cmd_best(&client, n.unwrap_or(5), filter.as_deref())),
        Cmd::Pick { mode } => {
            let do_ping = matches!(mode.as_str(), "ping" | "-p" | "--ping");
            rt.block_on(cmds::cmd_pick(&client, do_ping));
        }
        Cmd::Use { name } => rt.block_on(cmds::cmd_select(&client, &name)),
        Cmd::Update { url } => rt.block_on(cmds::cmd_update(&client, &cfg, url.as_deref())),
        Cmd::Service { act } => cmds::cmd_service(&cfg, &act),
        Cmd::Tun { act } => rt.block_on(cmds::cmd_tun(&client, &cfg, &act)),
    }
}
