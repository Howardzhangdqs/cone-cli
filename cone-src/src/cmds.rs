// 子命令实现
// 行为契约对齐 mihomo-cli.sh; 显示风格用 indicatif + tabled 美化 (用户选定)
#![allow(dead_code)]

use crate::api::{Client, Config};
use crate::measure::{latency_stream, measure_speed, DelayResult, SpeedDetail};
use crate::ui::{delay_tag, pad_right, render_table, Cell, Colors, FAIL_MS};
use indicatif::{ProgressBar, ProgressStyle};
use std::process::{Command, Stdio};
use std::io::Write;

// ============================== 辅助 ==============================
fn pick_colors() -> &'static Colors {
    if console::user_attended() && std::env::var("NO_COLOR").is_err() {
        &crate::ui::ANSI
    } else {
        &crate::ui::PLAIN
    }
}

/// 错误退出 (对齐 die: 红色错误信息到 stderr, exit 1)
pub fn die(msg: &str) -> ! {
    let c = pick_colors();
    eprintln!("{}错误: {}{}", c.red, msg, c.reset);
    std::process::exit(1);
}

// ============================== status ==============================
pub async fn cmd_status(client: &Client) {
    let c = pick_colors();
    let g = client.detect_group().await.unwrap_or_else(|e| die(&e));
    let ver = client.version().await;
    let now = client.group_now().await.unwrap_or_else(|e| die(&e));
    let total = client.node_count().await.unwrap_or(0);
    println!("{}{}╭─ mihomo {}{} ──{}", c.magenta, c.bold, c.green, ver, c.reset);
    println!("{}{}│{} {}监听{}     {}  {}({}{}){}", c.magenta, c.bold, c.reset, c.dim, c.reset,
             client.cfg.api, c.dim, client.cfg.px, c.dim, c.reset);
    println!("{}{}│{} {}选择器{}   {}", c.magenta, c.bold, c.reset, c.dim, c.reset, g);
    println!("{}{}│{} {}当前节点{} {}▶ {}{}", c.magenta, c.bold, c.reset, c.dim, c.reset, c.green, now, c.reset);
    println!("{}{}│{} {}节点总数{} {}", c.magenta, c.bold, c.reset, c.dim, c.reset, total);
    println!("{}{}╰────────────────────{}", c.magenta, c.bold, c.reset);
}

// ============================== list ==============================
pub async fn cmd_list(client: &Client) {
    let c = pick_colors();
    // 探测组以校验 mihomo 可达 (list 不显示组名, 但需确保 API 连通)
    let _ = client.detect_group().await.unwrap_or_else(|e| die(&e));
    let now = client.group_now().await.unwrap_or_else(|e| die(&e));
    let nodes = client.get_nodes_typed().await.unwrap_or_else(|e| die(&e));
    // 自建表格渲染器 (unicode-width 精确对齐, emoji/中文/ANSI 均正确)
    let rows: Vec<Vec<Cell>> = nodes
        .iter()
        .enumerate()
        .map(|(i, (name, ty))| {
            let (idx, mark) = if name == &now {
                (format!("{}{}{}", c.green, i + 1, c.reset), format!("{}◀ 当前{}", c.green, c.reset))
            } else {
                (format!("{}{}{}", c.dim, i + 1, c.reset), format!("{}─{}", c.dim, c.reset))
            };
            vec![Cell::new(idx).right(), Cell::new(name), Cell::new(ty), Cell::new(mark)]
        })
        .collect();
    println!("{}", render_table(&["#", "节点", "类型", ""], &rows));
}

// ============================== ping ==============================
pub async fn cmd_ping(client: &Client, n: usize) {
    let c = pick_colors();
    let nodes = client.get_nodes().await.unwrap_or_else(|e| die(&e));
    let total = nodes.len();
    println!("{}▶ 延迟测试中 (并发 {}, 即时显示)...{}", c.dim, client.cfg.parallel, c.reset);

    // spinner 计数 + 流式结果: 用 pb.println 把结果行打印到 spinner 上方, 避免抢占
    let spinner = ProgressBar::new(total as u64);
    spinner.set_style(
        ProgressStyle::with_template("{spinner} {msg} {wide_bar} {pos}/{len}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.set_message("测延迟中");

    let mut cache: Vec<(i64, String)> = Vec::with_capacity(total);
    let spinner_ref = &spinner;
    latency_stream(client, nodes, client.cfg.parallel, |r: DelayResult| {
        let line = format!("  {}  {}", delay_tag(c, r.ms), r.name);
        spinner_ref.println(line); // 打到 spinner 上方, 光标由 indicatif 管理
        spinner_ref.inc(1);
        cache.push((r.ms, r.name));
    }).await;
    spinner.finish_and_clear();

    // 统计
    let ok = cache.iter().filter(|(d, _)| *d < FAIL_MS).count();
    let fail = cache.len() - ok;

    // Top N 表格
    let mut sorted: Vec<(i64, String)> = cache.into_iter().filter(|(d, _)| *d < FAIL_MS).collect();
    sorted.sort_by_key(|(d, _)| *d);
    let top: Vec<(i64, String)> = sorted.into_iter().take(n).collect();

    println!();
    println!("{}{}★ Top {} 最快{}", c.bold, c.magenta, top.len(), c.reset);
    if top.is_empty() {
        println!("{}无可达节点{}", c.dim, c.reset);
    } else {
        let rows: Vec<Vec<Cell>> = top
            .iter()
            .enumerate()
            .map(|(i, (d, name))| vec![
                Cell::new(format!("{}#{}{}", c.green, i + 1, c.reset)).right(),
                Cell::new(delay_tag(c, *d)),
                Cell::new(name.clone()),
            ])
            .collect();
        println!("{}", render_table(&["名次", "延迟", "节点"], &rows));
    }
    println!();
    println!("{}共{} {}{}{} 节点可达,{} {}{}{} 失败{}",
             c.dim, c.reset, c.green, ok, c.dim, c.reset, c.red, fail, c.dim, c.reset);
}

// ============================== speed / best 共享流程 ==============================
/// 延迟筛 Top N 候选 + 流式显示 (best/speed 共用)
async fn pick_candidates(client: &Client, n: usize) -> Vec<String> {
    let c = pick_colors();
    let nodes = client.get_nodes().await.unwrap_or_else(|e| die(&e));
    println!("{}1. 延迟筛选 Top {} 候选 (即时显示)...{}", c.dim, n, c.reset);
    let total = nodes.len();
    let spinner = ProgressBar::new(total as u64);
    spinner.set_style(
        ProgressStyle::with_template("{spinner} {msg} {wide_bar} {pos}/{len}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.set_message("测延迟中");
    let mut cache: Vec<(i64, String)> = Vec::with_capacity(total);
    let sc = &spinner;
    latency_stream(client, nodes, client.cfg.parallel, |r: DelayResult| {
        sc.println(format!("  {}  {}", delay_tag(c, r.ms), r.name));
        sc.inc(1);
        cache.push((r.ms, r.name));
    }).await;
    spinner.finish_and_clear();
    // 排序取 Top N
    let mut sorted: Vec<(i64, String)> = cache.into_iter().filter(|(d, _)| *d < FAIL_MS).collect();
    sorted.sort_by_key(|(d, _)| *d);
    sorted.into_iter().take(n).map(|(_, name)| name).collect()
}

/// 单节点吞吐测速 + indicatif 实时进度条, 返回 (详细数据, 显示用的最终延迟字符串)
/// 对齐: 切换后 sleep 500ms (审核钉死项 8), 延迟用 node_delay 单点重查 (审核建议 B)
async fn measure_one(client: &Client, name: &str, rank: usize, colors: &Colors) -> (SpeedDetail, String) {
    let c = colors;
    let _ = client.group_set(name).await.unwrap_or_else(|e| die(&e));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await; // 切换后 sleep 500ms

    // 真实进度条: 以已下载字节驱动, 显示 进度条 + 实时Mbps + 已下载MB + 用时
    let target = client.cfg.speed_bytes;
    let pb = ProgressBar::new(target);
    pb.set_style(
        ProgressStyle::with_template("{prefix} {wide_bar} {msg}")
            .unwrap()
            .progress_chars("█░ "),
    );
    pb.set_prefix(format!("{}#{}{} {}",
        c.dim, rank, c.reset, pad_right(name, 28)));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let pb_ref = &pb;
    let detail = measure_speed(client, Some(&|bytes, elapsed| {
        pb_ref.set_position(bytes);
        let bps: f64 = if elapsed > 0.0 { bytes as f64 / elapsed } else { 0.0 };
        let mbps = (bps * 8.0 / 1_000_000.0).round() as i64;
        let downloaded_mb = bytes as f64 / 1_000_000.0;
        pb_ref.set_message(format!("{}{} Mbps{}  {}{:.1} MB{}  {}{:.2}s{}",
            c.bold, mbps, c.reset,
            c.dim, downloaded_mb, c.reset,
            c.dim, elapsed, c.reset));
    })).await;

    // 延迟列: node_delay 单点重查 (失败返回 "-", bash 算术展开当 0 -> 绿色低延迟)
    let delay_str = client.node_delay(name).await;
    let delay_ms: i64 = delay_str.parse().unwrap_or(0);
    // 定格保留进度条 (显示最终平均速度 + 下载量), 而非清除
    let avg_mbps = detail.avg_mbps().round() as i64;
    let dl_mb = detail.downloaded as f64 / 1_000_000.0;
    let elapsed_sec = detail.elapsed_ms as f64 / 1000.0;
    pb.finish_with_message(format!("{}{} Mbps{}  {}{:.1} MB{}  {}{:.2}s{}",
        c.bold, avg_mbps, c.reset,
        c.dim, dl_mb, c.reset,
        c.dim, elapsed_sec, c.reset));
    (detail, delay_tag(c, delay_ms))
}

/// 格式化用时秒数为 "12s" 或 "1m03s"
fn format_elapsed(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

/// 渲染测速详细表格: 节点 | 平均速度(+速度条) | 峰值 | warmup | TTFB | 下载量 | 耗时 | 延迟
/// 接受 (name, detail, delay_tag) 列表, 按平均速度降序排列
fn render_detail_table(c: &Colors, results: &[(String, SpeedDetail, String)]) -> String {
    // 按平均速度降序
    let mut sorted: Vec<&(String, SpeedDetail, String)> = results.iter().collect();
    sorted.sort_by(|a, b| b.1.avg_bps.partial_cmp(&a.1.avg_bps).unwrap_or(std::cmp::Ordering::Equal));

    let rows: Vec<Vec<Cell>> = sorted
        .iter()
        .map(|(name, d, delay)| {
            let avg = if d.avg_mbps() >= 1.0 {
                format!("{} Mbps", d.avg_mbps().round() as i64)
            } else {
                format!("{:.1} Mbps", d.avg_mbps())
            };
            let peak = if d.peak_mbps() >= 1.0 {
                format!("{} Mbps", d.peak_mbps().round() as i64)
            } else {
                format!("{:.1} Mbps", d.peak_mbps())
            };
            vec![
                Cell::new(name.clone()),
                Cell::new(format!("{}{}{}", c.bold, avg, c.reset)).right(),
                Cell::new(format!("{}{}{}", c.cyan, peak, c.reset)).right(),
                Cell::new(if d.warmup_ms > 0 {
                    format!("{}{} ms{}", c.dim, d.warmup_ms, c.reset)
                } else {
                    format!("{}N/A{}", c.dim, c.reset)
                }).right(),
                Cell::new(if d.ttfb_ms > 0 {
                    format!("{}{} ms{}", c.dim, d.ttfb_ms, c.reset)
                } else {
                    format!("{}N/A{}", c.dim, c.reset)
                }).right(),
                Cell::new(format!("{}{:.1} MB{}", c.dim, d.downloaded as f64 / 1_000_000.0, c.reset)).right(),
                Cell::new(format!("{}{:.2}s{}", c.dim, d.elapsed_ms as f64 / 1000.0, c.reset)).right(),
                Cell::new(delay.clone()).right(),
            ]
        })
        .collect();
    render_table(
        &["节点", "平均速度", "峰值", "warmup", "TTFB", "下载量", "耗时", "延迟"],
        &rows,
    )
}

pub async fn cmd_speed(client: &Client, n: usize) {
    let c = pick_colors();
    let orig = client.group_now().await.unwrap_or_else(|e| die(&e));
    let cands = pick_candidates(client, n).await;
    if cands.is_empty() {
        die("无可用节点");
    }
    println!();
    println!("{}2. 下载测速 ({} 候选, 每点 warmup+实测)...{}", c.dim, cands.len(), c.reset);

    // 收集测速详细数据 (name, detail, delay_tag)
    let mut results: Vec<(String, SpeedDetail, String)> = Vec::new();
    for (i, name) in cands.iter().enumerate() {
        let (detail, dt) = measure_one(client, name, i + 1, c).await;
        results.push((name.clone(), detail, dt));
    }
    // 恢复原节点
    let _ = client.group_set(&orig).await;
    // 详细表格输出 (前面留一空行, 与测速过程分开)
    println!();
    println!("{}", render_detail_table(c, &results));
    println!("{}(已恢复原节点: {}){}", c.dim, orig, c.reset);
}

pub async fn cmd_best(client: &Client, n: usize) {
    let c = pick_colors();
    // best 测完会切换到最快节点, 不需要保存/恢复原节点 (区别于 speed)
    let cands = pick_candidates(client, n).await;
    if cands.is_empty() {
        die("无可用节点");
    }
    println!();
    println!("{}2. 逐个下载测速...{}", c.dim, c.reset);

    // 收集详细数据
    let mut results: Vec<(String, SpeedDetail, String)> = Vec::new();
    for (i, name) in cands.iter().enumerate() {
        let (detail, dt) = measure_one(client, name, i + 1, c).await;
        results.push((name.clone(), detail, dt));
    }
    // 选最快 (平均速度最高)
    let best = results.iter().max_by(|a, b| {
        a.1.avg_bps.partial_cmp(&b.1.avg_bps).unwrap_or(std::cmp::Ordering::Equal)
    });
    let (best_name, best_detail) = match best {
        Some((n, d, _)) if d.avg_bps > 0.0 => (n.clone(), d.clone()),
        _ => die("所有候选测速失败"),
    };
    let _ = client.group_set(&best_name).await;
    println!();
    // 详细表格
    println!("{}", render_detail_table(c, &results));
    println!();
    // 最快节点: 普通终端输出
    let avg = if best_detail.avg_mbps() >= 1.0 {
        format!("{} Mbps", best_detail.avg_mbps().round() as i64)
    } else {
        format!("{:.1} Mbps", best_detail.avg_mbps())
    };
    let peak = format!("{} Mbps", best_detail.peak_mbps().round() as i64);
    println!("{}{}最快节点:{} {}{}{}  {}（平均 {} · 峰值 {}）{}",
             c.bold, c.magenta, c.reset,
             c.green, best_name, c.reset,
             c.dim, avg, peak, c.reset);
    println!("{}✓ 已切换{}", c.green, c.reset);
}

// ============================== select (use) ==============================
pub async fn cmd_select(client: &Client, want: &str) {
    let c = pick_colors();
    let nodes = client.get_nodes().await.unwrap_or_else(|e| die(&e));
    // 精确匹配
    if nodes.iter().any(|n| n == want) {
        let _ = client.group_set(want).await.unwrap_or_else(|e| die(&e));
        println!("{}✓ 已选择: {}{}", c.green, want, c.reset);
        return;
    }
    // 模糊匹配 (大小写不敏感)
    let want_lc = want.to_lowercase();
    let m: Vec<&String> = nodes.iter().filter(|n| n.to_lowercase().contains(&want_lc)).collect();
    match m.len() {
        0 => die(&format!("未找到节点: {}", want)),
        1 => {
            let name = m[0];
            let _ = client.group_set(name).await.unwrap_or_else(|e| die(&e));
            println!("{}✓ 已选择: {}{}", c.green, name, c.reset);
        }
        _ => {
            println!("{}匹配多个节点，请更精确:{}", c.yellow, c.reset);
            for n in &m {
                eprintln!("  {}", n);
            }
            std::process::exit(1);
        }
    }
}

// ============================== pick (fzf) ==============================
pub async fn cmd_pick(client: &Client, do_ping: bool) {
    let c = pick_colors();
    // 探测 fzf 是否存在 (不引入 which crate, 直接 command -v)
    let has_fzf = Command::new("sh")
        .args(["-c", "command -v fzf >/dev/null 2>&1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !has_fzf {
        die("需要 fzf (apt install fzf / brew install fzf)");
    }
    if !console::user_attended() {
        die("pick 需要交互式终端");
    }
    let now = client.group_now().await.unwrap_or_else(|e| die(&e));
    let header = format!("当前: {}   ↑↓ 选择 · Enter 切换 · Esc 取消", now);

    let mut fzf = Command::new("fzf");
    fzf.arg("--reverse")
        .arg("--ansi")
        .arg("--prompt=节点> ")
        .arg("--header").arg(&header)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    // 准备输入列表
    let input = if do_ping {
        println!("{}延迟测试中 (用于排序)...{}", c.dim, c.reset);
        let nodes = client.get_nodes().await.unwrap_or_else(|e| die(&e));
        let mut cache: Vec<(i64, String)> = Vec::new();
        latency_stream(client, nodes, client.cfg.parallel, |r: DelayResult| {
            cache.push((r.ms, r.name));
        }).await;
        cache.sort_by_key(|(d, _)| *d);
        cache.into_iter()
            .map(|(d, name)| {
                if d >= FAIL_MS {
                    format!("    ---- |{}", name)
                } else {
                    format!("{:>7} ms|{}", d, name)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        client.get_nodes().await.unwrap_or_else(|e| die(&e))
            .into_iter().collect::<Vec<_>>()
            .join("\n")
    };

    let mut child = match fzf.spawn() {
        Ok(ch) => ch,
        Err(e) => die(&format!("无法启动 fzf: {}", e)),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => die(&format!("fzf 执行失败: {}", e)),
    };
    if !output.status.success() {
        eprintln!("{}已取消{}", c.dim, c.reset);
        std::process::exit(130);
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if line.is_empty() {
        eprintln!("{}已取消{}", c.dim, c.reset);
        std::process::exit(130);
    }
    // 解析节点名: 有 '|' 取首个 | 之后; 无则整行即名 (审核钉死项)
    let name = line.split_once('|').map(|(_, n)| n).unwrap_or(&line).to_string();
    let _ = client.group_set(&name).await.unwrap_or_else(|e| die(&e));
    println!("{}✓ 已切换到: {}{}", c.green, name, c.reset);
}

// ============================== update ==============================
pub async fn cmd_update(client: &Client, cfg: &Config, url: Option<&str>) {
    let c = pick_colors();
    // 解析订阅地址: 参数 > env > 已保存文件
    let url = url
        .map(|s| s.to_string())
        .or_else(|| std::env::var("MIHOMO_SUB_URL").ok())
        .or_else(|| std::fs::read_to_string(&cfg.suburl_file).ok().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| die("未指定订阅地址。用法: cone-cli update <URL> (首次需提供，之后会记住)"));

    std::fs::create_dir_all(&cfg.conf_dir).unwrap_or_else(|e| die(&format!("无法创建配置目录: {e}")));
    println!("{}正在拉取订阅...{}", c.dim, c.reset);
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap()
        .get(&url)
        .header("User-Agent", &cfg.sub_ua)
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => die(&format!("订阅拉取失败: {e}")),
    };
    let status = resp.status();
    if !status.is_success() {
        die(&format!("订阅拉取失败 (HTTP {}): {}", status.as_u16(), url));
    }
    let body = resp.text().await.unwrap_or_default();

    // 简单校验: clash YAML 应含 proxies / proxy-groups / rules (对齐第 383-385 行)
    if !body.lines().any(|l| l.starts_with("proxies:")
        || l.starts_with("proxy-groups:") || l.starts_with("rules:")) {
        die("返回内容不是 clash 配置 (缺少 proxies/groups/rules)。可能 UA 不被识别或链接有误");
    }
    // 备份旧配置
    if std::path::Path::new(&cfg.conf).exists() {
        let _ = std::fs::copy(&cfg.conf, format!("{}.bak", cfg.conf));
    }
    std::fs::write(&cfg.conf, &body).unwrap_or_else(|e| die(&format!("写入配置失败: {e}")));
    std::fs::write(&cfg.suburl_file, &url).unwrap_or_else(|e| die(&format!("保存订阅地址失败: {e}")));

    // 注入 tun.yaml (幂等: 仅当 config 不含 ^tun: 才追加, 对齐第 393 行)
    let tun_yaml = format!("{}/tun.yaml", cfg.conf_dir);
    if std::path::Path::new(&tun_yaml).exists()
        && !body.lines().any(|l| l.starts_with("tun:")) {
        let tun = std::fs::read_to_string(&tun_yaml).unwrap_or_default();
        let mut combined = body.clone();
        combined.push('\n');
        combined.push_str(&tun);
        std::fs::write(&cfg.conf, &combined).unwrap_or_else(|e| die(&format!("注入 tun.yaml 失败: {e}")));
    }
    // 精确统计 proxies 段节点数 (按行正则状态机, 对齐第 397 行 awk)
    let content = std::fs::read_to_string(&cfg.conf).unwrap_or_default();
    let node_count = count_proxies(&content);
    println!("{}✓ 订阅已更新{}  → {} ({} 个节点)", c.green, c.reset, cfg.conf, node_count);
    println!("旧配置备份: {}.bak", cfg.conf);

    // 重载三条分支 (对齐第 402-417 行)
    let running = Command::new("pgrep").args(["-x", "mihomo"]).status().map(|s| s.success()).unwrap_or(false);
    if running {
        let is_systemd = Command::new("systemctl")
            .args(["is-active", "--quiet", &cfg.svc])
            .status().map(|s| s.success()).unwrap_or(false);
        if is_systemd {
            println!("{}正在重启 mihomo 服务...{}", c.dim, c.reset);
            let r = Command::new("sudo").args(["systemctl", "restart", &cfg.svc]).status();
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let ok = Command::new("systemctl").args(["is-active", "--quiet", &cfg.svc])
                .status().map(|s| s.success()).unwrap_or(false);
            match r {
                Ok(_) if ok => println!("{}✓ mihomo 已重载{}", c.green, c.reset),
                _ => eprintln!("{}✗ mihomo 重启失败，查看: journalctl -u {}{}", c.red, cfg.svc, c.reset),
            }
        } else {
            println!("{}mihomo 在运行但非 systemd 管理，请手动重启{}", c.yellow, c.reset);
        }
    } else {
        println!("{}mihomo 未运行，配置已就绪 (start.sh start 启动){}", c.dim, c.reset);
    }
    // 避免 unused warning (client 在 update 里不用, 但保持签名一致)
    let _ = client;
}

/// 按 YAML 行状态机统计 proxies 段下 `- ` 条目数
/// 语义: 进入 `^proxies:` 后, 遇下一个 `^[a-z]` 顶层键退出, 期间计 `^\s+- ` 行
fn count_proxies(content: &str) -> usize {
    let mut in_proxies = false;
    let mut count = 0;
    for line in content.lines() {
        if line.starts_with("proxies:") {
            in_proxies = true;
            continue;
        }
        if in_proxies {
            // 遇到下一个顶层键 (行首小写字母 + 冒号) 退出
            if regex_starts_lowercase_key(line) {
                in_proxies = false;
                continue;
            }
            // 缩进的 `- ` 计数 (正则 ^\s+- )
            let trimmed = line.trim_start();
            if trimmed.starts_with("- ") {
                count += 1;
            }
        }
    }
    count
}

/// 检测行是否形如 `^[a-z].*:` (顶层 YAML 键, 小写字母开头)
/// 对齐 awk 的 `/^[a-z]/` 判定
fn regex_starts_lowercase_key(line: &str) -> bool {
    let mut chars = line.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => line.contains(':'),
        _ => false,
    }
}

// ============================== service ==============================
pub fn cmd_service(cfg: &Config, act: &str) {
    let c = pick_colors();
    match act {
        "on" | "start" => {
            let r = Command::new("sudo").args(["systemctl", "start", &cfg.svc]).status();
            match r {
                Ok(s) if s.success() => println!("{}✓ mihomo 已启动{}", c.green, c.reset),
                _ => die("启动失败"),
            }
        }
        "off" | "stop" => {
            let r = Command::new("sudo").args(["systemctl", "stop", &cfg.svc]).status();
            match r {
                Ok(s) if s.success() => println!("{}✓ mihomo 已停止{}", c.yellow, c.reset),
                _ => die("停止失败"),
            }
        }
        "restart" | "reload" => {
            let r = Command::new("sudo").args(["systemctl", "restart", &cfg.svc]).status();
            match r {
                Ok(s) if s.success() => println!("{}✓ mihomo 已重启{}", c.green, c.reset),
                _ => die("重启失败"),
            }
        }
        "status" => {
            let active = Command::new("systemctl").args(["is-active", "--quiet", &cfg.svc])
                .status().map(|s| s.success()).unwrap_or(false);
            if active {
                let pid = Command::new("systemctl")
                    .args(["show", "-p", "MainPID", "--value", &cfg.svc])
                    .output().ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| "?".to_string());
                println!("{}● mihomo 运行中{} (PID {})", c.green, c.reset, pid);
            } else {
                println!("{}○ mihomo 未运行{}", c.red, c.reset);
                std::process::exit(1);
            }
        }
        _ => die("用法: cone-cli service {on|off|restart|status}"),
    }
}

// ============================== tun ==============================
pub async fn cmd_tun(client: &Client, cfg: &Config, act: &str) {
    let c = pick_colors();
    match act {
        "status" => {
            let en = client.tun_enabled().await.unwrap_or(false);
            if en {
                println!("{}● TUN 已开启{} (全局透明代理)", c.green, c.reset);
            } else {
                println!("{}○ TUN 已关闭{} (仅 HTTP/SOCKS 代理)", c.dim, c.reset);
            }
        }
        "on" => {
            // 写 config: 只改 ^tun: 下一行的 enable (对齐 sed /{n;s/...}/, 非全文替换)
            let _ = flip_tun_in_config(cfg, true);
            let _ = client.patch_configs(r#"{"tun":{"enable":true}}"#).await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            // on 必须回查 (对齐第 465-472 行), 失败提示 cap_net_admin
            let en = client.tun_enabled().await.unwrap_or(false);
            if en {
                println!("{}✓ TUN 已开启{} — 所有流量走 mihomo", c.green, c.reset);
            } else {
                die("TUN 开启失败 (mihomo 是否有 cap_net_admin 权限?)");
            }
        }
        "off" => {
            let _ = flip_tun_in_config(cfg, false);
            let _ = client.patch_configs(r#"{"tun":{"enable":false}}"#).await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            // off 不回查 (对齐第 480-481 行, 保留不对称)
            println!("{}✓ TUN 已关闭{} — 仅 HTTP/SOCKS 代理", c.yellow, c.reset);
        }
        _ => die("用法: cone-cli tun {on|off|status}"),
    }
}

/// 翻转 config.yaml 里 ^tun: 段**下一行**的 enable 值
/// 对齐 bash sed -i '/^tun:/{n;s/enable: false/enable: true/}' (用 n 只改紧邻下一行)
/// 只替换紧邻下一行, 不动全文 (避免误伤其他段)
fn flip_tun_in_config(cfg: &Config, enable: bool) -> std::io::Result<()> {
    if !std::path::Path::new(&cfg.conf).exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&cfg.conf)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        out.push(line.to_string());
        if line.starts_with("tun:") && i + 1 < lines.len() {
            // 下一行: 翻转 enable 值 (只改这一行)
            let next = lines[i + 1];
            let (from, to) = if enable {
                ("enable: false", "enable: true")
            } else {
                ("enable: true", "enable: false")
            };
            let flipped = next.replacen(from, to, 1);
            out.push(flipped);
            i += 2;
        } else {
            i += 1;
        }
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    std::fs::write(&cfg.conf, result)
}

// ============================== help ==============================
pub fn cmd_help() {
    let c = pick_colors();
    println!("{}{}cone-cli{}  —  Mihomo (Clash.Meta) 测速与节点选择工具 (Rust 版){}", c.magenta, c.bold, c.reset, c.reset);
    println!("{}(建议别名 mc，下文示例用 mc 表示本命令){}\n", c.dim, c.reset);
    println!("{}【命令】{}", c.bold, c.reset);
    println!("  {}status{}              显示版本/API/端口/主选择器/当前节点/节点总数", c.green, c.reset);
    println!("  {}list{}                列出全部节点 (带类型，标记当前节点)", c.green, c.reset);
    println!();
    println!("  {}ping{} [N]            并行测全部节点延迟，按快慢显示前 N (默认 15)", c.green, c.reset);
    println!("                      只读，不改任何东西；失败节点标 FAIL");
    println!();
    println!("  {}speed{} [N]           吞吐测速 (只读，测完恢复原节点):", c.green, c.reset);
    println!("                        1. 并行测延迟 → 取前 N (默认 8)");
    println!("                        2. 逐个切换并下载实测带宽 (Mbps)，实时进度条");
    println!();
    println!("  {}best{} [N]            自动选最快并切换 (会改变当前节点!):", c.green, c.reset);
    println!("                      流程同 speed，但测完把当前节点设为带宽最高者");
    println!("                      N=候选数，默认 8；best 3 最快，best 20 更稳");
    println!();
    println!("  {}pick{} [ping]         fzf 交互式选节点:", c.green, c.reset);
    println!("                        pick        即时列出节点名，回车切换");
    println!("                        pick ping   先测延迟再按快慢排序选择");
    println!();
    println!("  {}use{} <关键字>        直接切换节点，支持模糊匹配", c.green, c.reset);
    println!("  {}update{} [URL]        拉取订阅生成 config.yaml 并自动重载", c.green, c.reset);
    println!("  {}service{} <act>       控制 mihomo 服务 {{on|off|restart|status}}", c.green, c.reset);
    println!("  {}tun{} <act>           控制 TUN {{on|off|status}}", c.green, c.reset);
    println!("  {}help{}                显示本帮助", c.green, c.reset);
    println!();
    println!("{}【环境变量】{}(可选)", c.bold, c.reset);
    println!("  MIHOMO_API / MIHOMO_PROXY / MIHOMO_GROUP / MIHOMO_TEST_URL");
    println!("  MIHOMO_SPEED_URL / MIHOMO_SPEED_BYTES / MIHOMO_DELAY_TIMEOUT");
    println!("  MIHOMO_PARALLEL / MIHOMO_CONF_DIR / MIHOMO_SUB_URL / MIHOMO_SUB_UA");
}
