// 子命令实现
// 行为契约对齐 mihomo-cli.sh; 显示风格用 indicatif + 自研 unicode-width 表格 (无第三方 tabled)
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
pub async fn cmd_ping(client: &Client, n: usize, filter: Option<&str>) {
    let c = pick_colors();
    let mut nodes = client.get_nodes().await.unwrap_or_else(|e| die(&e));
    // 关键字过滤 (大小写不敏感包含匹配)
    if let Some(kw) = filter {
        let kw_lc = kw.to_lowercase();
        nodes.retain(|name| name.to_lowercase().contains(&kw_lc));
        if nodes.is_empty() {
            die(&format!("未找到匹配「{}」的节点", kw));
        }
        println!("{}▶ 延迟测试 (过滤: {}, 共 {} 个, 并发 {})...{}", c.dim, kw, nodes.len(), client.cfg.parallel, c.reset);
    } else {
        println!("{}▶ 延迟测试中 (并发 {}, 即时显示)...{}", c.dim, client.cfg.parallel, c.reset);
    }
    let total = nodes.len();
    let term_w = crate::ui::term_columns();
    let brick_w = 26usize;

    // spinner + 实时热力墙: 把整面墙作为多行 message, 每完成一个节点就重拼墙并 set_message,
    // indicatif 会原地清行重绘 (不向下滚动)。墙按「完成顺序」增长, 不排序。
    let spinner = ProgressBar::new(total as u64);
    spinner.set_style(
        ProgressStyle::with_template(
            "{spinner} {pos}/{len} 测延迟中  {wide_bar}\n{msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner.set_message(format!("{}(等待结果...){}", c.dim, c.reset));

    // 完成顺序缓存: 仅用于实时墙的流式增长 (最终展示前再按延迟排序)
    let mut done: Vec<(i64, String)> = Vec::with_capacity(total);
    let spinner_ref = &spinner;
    latency_stream(client, nodes, client.cfg.parallel, |r: DelayResult| {
        done.push((r.ms, r.name));
        // 每来一个新砖块, 重新拼整面墙 -> set_message 触发原地重绘
        let wall = crate::ui::render_heat_wall(c, &done, term_w, brick_w);
        spinner_ref.set_message(wall);
        spinner_ref.inc(1);
    }).await;
    // 实时墙按完成顺序增长 (流式反馈); 最终改用 finish_and_clear 清掉, 再打印按延迟排序的干净墙
    spinner.finish_and_clear();

    // 统计 (基于全量结果, 不受 N 截断影响)
    let ok = done.iter().filter(|(d, _)| *d < FAIL_MS).count();
    let fail = done.len() - ok;

    // 按延迟升序排序 (失败节点 FAIL_MS 最大自然靠后), 取前 N 展示 (与 speed/best 口径一致)
    done.sort_by_key(|(d, _)| *d);
    let show: Vec<(i64, String)> = if done.len() > n {
        done.into_iter().take(n).collect()
    } else {
        done
    };
    println!();
    println!("{}{}★ 延迟热力图{}  {}显示前 {} / {} (按延迟排序: 绿<200 · 青<500 · 黄<1000 · 红≥1000 · 暗血红失败){}",
             c.bold, c.magenta, c.reset, c.dim, show.len(), total, c.reset);
    println!("{}", crate::ui::render_heat_wall(c, &show, term_w, brick_w));
    println!("{}共{} {}{}{} 节点可达,{} {}{}{} 失败{}",
             c.dim, c.reset, c.green, ok, c.dim, c.reset, c.red, fail, c.dim, c.reset);
}

// ============================== speed / best 共享流程 ==============================
/// 延迟筛 Top N 候选 + 流式显示 (best/speed 共用)
/// filter: 可选关键字过滤 (大小写不敏感包含匹配)
async fn pick_candidates(client: &Client, n: usize, filter: Option<&str>) -> Vec<String> {
    let c = pick_colors();
    let mut nodes = client.get_nodes().await.unwrap_or_else(|e| die(&e));
    if let Some(kw) = filter {
        let kw_lc = kw.to_lowercase();
        nodes.retain(|name| name.to_lowercase().contains(&kw_lc));
        if nodes.is_empty() {
            die(&format!("未找到匹配「{}」的节点", kw));
        }
        println!("{}1. 延迟筛选 Top {} 候选 (过滤: {}, 共 {} 个)...{}", c.dim, n, kw, nodes.len(), c.reset);
    } else {
        println!("{}1. 延迟筛选 Top {} 候选 (即时显示)...{}", c.dim, n, c.reset);
    }
    let total = nodes.len();
    let term_w = crate::ui::term_columns();
    let brick_w = 26usize;
    // spinner + 实时热力墙 (同 cmd_ping): 每完成一个节点就重拼墙 set_message 原地刷新
    let spinner = ProgressBar::new(total as u64);
    spinner.set_style(
        ProgressStyle::with_template(
            "{spinner} {pos}/{len} 测延迟中  {wide_bar}\n{msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner.set_message(format!("{}(等待结果...){}", c.dim, c.reset));
    let mut cache: Vec<(i64, String)> = Vec::with_capacity(total);
    let sc = &spinner;
    latency_stream(client, nodes, client.cfg.parallel, |r: DelayResult| {
        cache.push((r.ms, r.name));
        let wall = crate::ui::render_heat_wall(c, &cache, term_w, brick_w);
        sc.set_message(wall);
        sc.inc(1);
    }).await;
    spinner.finish();
    // 排序取 Top N (候选用于后续吞吐测速, 失败节点剔除)
    let mut sorted: Vec<(i64, String)> = cache.into_iter().filter(|(d, _)| *d < FAIL_MS).collect();
    sorted.sort_by_key(|(d, _)| *d);
    sorted.into_iter().take(n).map(|(_, name)| name).collect()
}

/// 单节点吞吐测速 + 10 砖块进度墙, 返回 (详细数据, 显示用的最终延迟字符串)
/// 对齐: 切换后 sleep 500ms (审核钉死项 8), 延迟用 node_delay 单点重查 (审核建议 B)
async fn measure_one(client: &Client, name: &str, rank: usize, colors: &Colors) -> (SpeedDetail, String) {
    let c = colors;
    client.group_set(name).await.unwrap_or_else(|e| die(&e));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await; // 切换后 sleep 500ms

    // 10 砖块进度墙: target 字节均分 10 段, 每块显示该 1MB 区间的速度 (Mbps), 按速度分色
    let target = client.cfg.speed_bytes;
    let seg_bytes = (target / 10).max(1); // 每段字节数 (兜底防 0)
    let brick_w = 7usize; // 每块显示宽度 (如 "120M" 右对齐 + 内边距)
    let pb = ProgressBar::new(target);
    pb.set_style(
        ProgressStyle::with_template("{prefix} {msg}")
            .unwrap(),
    );
    pb.set_prefix(format!("{}#{}{} {}",
        c.dim, rank, c.reset, pad_right(name, 28)));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // 段状态: seg_mbps[i] = Some(mbps) 表示第 i 段已完成并记录了该段速度
    // 追踪每段起始的 (bytes, elapsed), 跨段时算出上一段真实速度 commit
    let mut seg_mbps: Vec<Option<i64>> = vec![None; 10];
    let mut last_seg: usize = 0;
    let mut seg_start_elapsed: f64 = 0.0;
    let pb_ref = &pb;
    let detail = measure_speed(client, Some(|bytes, elapsed| {
        pb_ref.set_position(bytes);
        let cur_seg = ((bytes / seg_bytes) as usize).min(9);
        // 跨段: 把 [last_seg, cur_seg) 之间的段用「该段字节数 / 该段耗时」算出 Mbps commit
        while last_seg < cur_seg {
            let seg_elapsed: f64 = elapsed - seg_start_elapsed;
            let mbps: i64 = if seg_elapsed > 0.0 {
                let seg_bits: f64 = (seg_bytes as f64) * 8.0;
                let mbps_f: f64 = seg_bits / 1_000_000.0 / seg_elapsed;
                mbps_f.round() as i64
            } else { 0 };
            seg_mbps[last_seg] = Some(mbps);
            seg_start_elapsed = elapsed;
            last_seg += 1;
        }
        // 当前瞬时速度 (整体平均, 用于尾部数字显示)
        let bps: f64 = if elapsed > 0.0 { bytes as f64 / elapsed } else { 0.0 };
        let mbps = (bps * 8.0 / 1_000_000.0).round() as i64;
        let downloaded_mb = bytes as f64 / 1_000_000.0;
        let bricks = crate::ui::render_speed_bricks(c, &seg_mbps, brick_w);
        pb_ref.set_message(format!("{}  {}{} Mbps{}  {}{:.1} MB{}  {}{:.2}s{}",
            bricks,
            c.bold, mbps, c.reset,
            c.dim, downloaded_mb, c.reset,
            c.dim, elapsed, c.reset));
    })).await;

    // 测速结束: 收尾最后一段 (从 last_seg 到实际下载位置), 用平均速度
    let final_seg = ((detail.downloaded / seg_bytes) as usize).min(10);
    while last_seg < final_seg {
        let avg_mbps = detail.avg_mbps().round() as i64;
        seg_mbps[last_seg] = Some(avg_mbps);
        last_seg += 1;
    }
    let avg_mbps = detail.avg_mbps().round() as i64;
    let dl_mb = detail.downloaded as f64 / 1_000_000.0;
    let elapsed_sec = detail.elapsed_ms as f64 / 1000.0;
    let bricks = crate::ui::render_speed_bricks(c, &seg_mbps, brick_w);
    pb.finish_with_message(format!("{}  {}{} Mbps{}  {}{:.1} MB{}  {}{:.2}s{}",
        bricks,
        c.bold, avg_mbps, c.reset,
        c.dim, dl_mb, c.reset,
        c.dim, elapsed_sec, c.reset));

    // 延迟列: node_delay 单点重查 (失败返回 "-", bash 算术展开当 0 -> 绿色低延迟)
    let delay_str = client.node_delay(name).await;
    let delay_ms: i64 = delay_str.parse().unwrap_or(0);
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

pub async fn cmd_speed(client: &Client, n: usize, filter: Option<&str>) {
    let c = pick_colors();
    let orig = client.group_now().await.unwrap_or_else(|e| die(&e));
    let cands = pick_candidates(client, n, filter).await;
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

pub async fn cmd_best(client: &Client, n: usize, filter: Option<&str>) {
    let c = pick_colors();
    // best 测完会切换到最快节点, 不需要保存/恢复原节点 (区别于 speed)
    let cands = pick_candidates(client, n, filter).await;
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
        client.group_set(want).await.unwrap_or_else(|e| die(&e));
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
            client.group_set(name).await.unwrap_or_else(|e| die(&e));
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
    client.group_set(&name).await.unwrap_or_else(|e| die(&e));
    println!("{}✓ 已切换到: {}{}", c.green, name, c.reset);
}

// ============================== update ==============================
/// 更新当前活跃订阅。读取 cfg.current_file 得到订阅名, 拉取其 suburl 并重载。
/// url 参数临时覆盖该订阅的 suburl (并持久化写回)。
pub async fn cmd_update(client: &Client, cfg: &Config, url: Option<&str>) {
    migrate_from_legacy(cfg);
    let name = current_sub(cfg).unwrap_or_else(|| {
        die("尚无订阅。请先添加: cone-cli sub add <名字> <URL>");
    });
    pull_one(client, cfg, &name, url).await;
}

/// 对指定订阅拉取一次: URL 来源 参数 > env > subs/<name>/suburl 文件。
/// 写入 subs/<name>/config.yaml, 注入 tun/external-controller, 统计节点数。
/// 若 name == 当前订阅则重载 mihomo, 否则提示 sub use 生效。
async fn pull_one(client: &Client, cfg: &Config, name: &str, url: Option<&str>) {
    let c = pick_colors();
    let dir = sub_dir(cfg, name);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| die(&format!("无法创建订阅目录: {e}")));
    let conf = sub_conf(cfg, name);
    let suburl = format!("{}/suburl", dir);

    // 解析 URL: 参数 > env > subs/<name>/suburl 文件 > 顶层 legacy suburl (兼容)
    let url = url
        .map(|s| s.to_string())
        .or_else(|| std::env::var("MIHOMO_SUB_URL").ok())
        .or_else(|| std::fs::read_to_string(&suburl).ok().map(|s| s.trim().to_string()))
        .or_else(|| std::fs::read_to_string(&cfg.suburl_file).ok().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| die(&format!("订阅 [{}] 未记录 URL。用法: cone-cli sub add {} <URL>", name, name)));

    std::fs::create_dir_all(&cfg.conf_dir).unwrap_or_else(|e| die(&format!("无法创建配置目录: {e}")));
    println!("{}正在拉取订阅 [{}]...{}", c.dim, name, c.reset);
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
        die(&format!("订阅拉取失败 (HTTP {}): {}", status.as_u16(), mask_url(&url)));
    }
    let body = resp.text().await.unwrap_or_default();

    // 简单校验: clash YAML 应含 proxies / proxy-groups / rules (对齐第 383-385 行)
    if !body.lines().any(|l| l.starts_with("proxies:")
        || l.starts_with("proxy-groups:") || l.starts_with("rules:")) {
        die("返回内容不是 clash 配置 (缺少 proxies/groups/rules)。可能 UA 不被识别或链接有误");
    }
    // 备份旧配置
    if std::path::Path::new(&conf).exists() {
        let _ = std::fs::copy(&conf, format!("{}.bak", conf));
    }
    std::fs::write(&conf, &body).unwrap_or_else(|e| die(&format!("写入配置失败: {e}")));
    std::fs::write(&suburl, &url).unwrap_or_else(|e| die(&format!("保存订阅地址失败: {e}")));

    // 注入 tun.yaml (幂等: 仅当 config 不含 ^tun: 才追加, 对齐第 393 行)
    inject_tun_at(&conf, &cfg.conf_dir);
    // 确保 external-controller 开启 (与 cfg.api 一致), 否则 cone-cli 无法连接 mihomo API
    if let Err(e) = ensure_external_controller_at(&conf, &cfg.api) {
        eprintln!("{}⚠ 注入 external-controller 失败: {e}{}", c.yellow, c.reset);
    }
    // 精确统计 proxies 段节点数 (按行正则状态机, 对齐第 397 行 awk)
    let content = std::fs::read_to_string(&conf).unwrap_or_default();
    let node_count = count_proxies(&content);
    println!("{}✓ 订阅 [{}] 已更新{}  → {} ({} 个节点)", c.green, name, c.reset, conf, node_count);
    println!("旧配置备份: {}.bak", conf);

    // 若是当前订阅则重载, 否则提示
    let cur = current_sub(cfg);
    if cur.as_deref() == Some(name) {
        ensure_symlink(cfg, name);
        reload_mihomo(cfg).await;
    } else {
        println!("{}提示: [{}] 非当前订阅, 未重载。执行 {}sub use {}{} 切换生效",
                 c.dim, name, c.green, name, c.reset);
    }
    let _ = client;
}

/// 重载 mihomo: 若进程在跑且由 systemd 管理 → systemctl restart;
/// 若在跑但非 systemd → kill 后用同级 mihomo 二进制重新后台启动 (加载新 config);
/// 若未运行 → 同样后台启动。供 cmd_update / cmd_sub 共用。
async fn reload_mihomo(cfg: &Config) {
    let c = pick_colors();
    let running = Command::new("pgrep").args(["-x", "mihomo"])
        .stdout(Stdio::null()).stderr(Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false);
    if running {
        let is_systemd = Command::new("systemctl")
            .args(["is-active", "--quiet", &cfg.svc])
            .stdout(Stdio::null()).stderr(Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false);
        if is_systemd {
            println!("{}正在重启 mihomo 服务...{}", c.dim, c.reset);
            let r = Command::new("sudo").args(["systemctl", "restart", &cfg.svc])
                .stdout(Stdio::null()).stderr(Stdio::null()).status();
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let ok = Command::new("systemctl").args(["is-active", "--quiet", &cfg.svc])
                .stdout(Stdio::null()).stderr(Stdio::null())
                .status().map(|s| s.success()).unwrap_or(false);
            match r {
                Ok(_) if ok => println!("{}✓ mihomo 已重载{}", c.green, c.reset),
                _ => eprintln!("{}✗ mihomo 重启失败，查看: journalctl -u {}{}", c.red, cfg.svc, c.reset),
            }
        } else {
            // 非 systemd: kill 旧进程, 用同级 mihomo 二进制重新后台启动
            start_mihomo_detached(cfg, true).await;
        }
    } else {
        // 未运行: 同样后台启动
        start_mihomo_detached(cfg, false).await;
    }
}

/// 用 cone-cli 同级目录的 mihomo 二进制后台启动 (nohup 风格)。
/// restart=true 时先 kill 已有的非 systemd mihomo 进程。
/// 日志重定向到 conf_dir/mihomo.log。启动后等待 API 可达并打印结果。
async fn start_mihomo_detached(cfg: &Config, restart: bool) {
    let c = pick_colors();
    let bin = match crate::mihomo_setup::mihomo_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}✗ 找不到 mihomo 二进制: {e}{}", c.red, c.reset);
            return;
        }
    };
    if restart {
        println!("{}正在重启 mihomo (非 systemd, 手动拉起)...{}", c.dim, c.reset);
        let _ = Command::new("pkill").arg("-x").arg("mihomo")
            .stdout(Stdio::null()).stderr(Stdio::null()).status();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    } else {
        println!("{}正在启动 mihomo (非 systemd)...{}", c.dim, c.reset);
    }
    let log_path = format!("{}/mihomo.log", cfg.conf_dir);
    // stdout 与 stderr 都追加写同一日志文件 (各自一个 fd)
    let open_log = || std::fs::OpenOptions::new().create(true).append(true).open(&log_path);
    let stdout_fd = match open_log() {
        Ok(f) => Stdio::from(f),
        Err(e) => {
            eprintln!("{}✗ 无法打开日志 {log_path}: {e}{}", c.red, c.reset);
            return;
        }
    };
    let stderr_fd = match open_log() {
        Ok(f) => Stdio::from(f),
        Err(e) => {
            eprintln!("{}✗ 无法打开日志 {log_path}: {e}{}", c.red, c.reset);
            return;
        }
    };
    // detached 后台进程: 父进程退出后仍存活
    let spawn = Command::new(&bin)
        .arg("-d").arg(&cfg.conf_dir)
        .arg("-f").arg(&cfg.conf)
        .stdin(Stdio::null())
        .stdout(stdout_fd)
        .stderr(stderr_fd)
        .spawn();
    match spawn {
        Ok(child) => {
            drop(child); // 不持有 handle, 让它独立存活
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let ok = Command::new("pgrep").args(["-x", "mihomo"])
                .stdout(Stdio::null()).stderr(Stdio::null())
                .status().map(|s| s.success()).unwrap_or(false);
            if ok {
                println!("{}✓ mihomo 已启动 (PID 由 pgrep 确认存活){}", c.green, c.reset);
            } else {
                eprintln!("{}✗ mihomo 启动后未存活, 查看日志: {log_path}{}", c.red, c.reset);
            }
        }
        Err(e) => eprintln!("{}✗ 启动 mihomo 失败: {e}{}", c.red, c.reset),
    }
}

// ============================== 多订阅管理 ==============================
// 每个订阅一个独立目录 subs/<name>/, 顶层 config.yaml 是符号链接指向当前订阅。
// service 文件写死 -f .../config.yaml, 通过软链切换无需改 service。

/// 订阅目录: subs/<name>/
fn sub_dir(cfg: &Config, name: &str) -> String {
    format!("{}/{}", cfg.subs_dir, name)
}

/// 订阅配置文件: subs/<name>/config.yaml
fn sub_conf(cfg: &Config, name: &str) -> String {
    format!("{}/config.yaml", sub_dir(cfg, name))
}

/// 读取当前活跃订阅名。current 文件不存在/为空时返回 None
fn current_sub(cfg: &Config) -> Option<String> {
    std::fs::read_to_string(&cfg.current_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 写入当前活跃订阅名
fn set_current_sub(cfg: &Config, name: &str) {
    std::fs::create_dir_all(&cfg.conf_dir).unwrap_or_else(|e| die(&format!("无法创建配置目录: {e}")));
    std::fs::write(&cfg.current_file, name).unwrap_or_else(|e| die(&format!("写入 current 失败: {e}")));
}

/// 列出 subs/ 下所有订阅名 (按字母序)
fn list_subs(cfg: &Config) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cfg.subs_dir) {
        for e in entries.flatten() {
            let name = e.file_name();
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && let Some(n) = name.to_str()
                && !n.starts_with('.')
            {
                names.push(n.to_string());
            }
        }
    }
    names.sort();
    names
}

/// 重建顶层 config.yaml 软链 → subs/<name>/config.yaml
/// 幂等: 先删旧 config.yaml (文件或软链), 再建软链。失败则 die
fn ensure_symlink(cfg: &Config, name: &str) {
    let target = sub_conf(cfg, name);
    let link = &cfg.conf;
    // 删除现有 config.yaml (可能是普通文件=旧模式, 或软链)
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(&target, link)
        .unwrap_or_else(|e| die(&format!("创建软链 {} → {} 失败: {e}", link, target)));
}

/// 从旧单订阅模式迁移到多订阅模式 (幂等)
/// 触发条件: 顶层 suburl 存在 且 subs/ 不存在。
/// 动作: 建 subs/default/, 拷贝顶层 config.yaml 过去, 软链指向它, current=default。
pub fn migrate_from_legacy(cfg: &Config) {
    let c = pick_colors();
    let legacy_suburl_exists = std::path::Path::new(&cfg.suburl_file).exists();
    let subs_exists = std::path::Path::new(&cfg.subs_dir).exists();
    if !legacy_suburl_exists || subs_exists {
        return; // 无需迁移
    }
    println!("{}检测到旧单订阅模式, 正在迁移到多订阅模式...{}", c.dim, c.reset);

    let name = "default";
    let dir = sub_dir(cfg, name);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| die(&format!("迁移失败: 无法创建 {dir}: {e}")));

    // suburl 复制到 subs/default/
    let _ = std::fs::copy(&cfg.suburl_file, format!("{}/suburl", dir));

    // 顶层 config.yaml (普通文件) 复制为 subs/default/config.yaml
    if std::path::Path::new(&cfg.conf).exists()
        && !std::fs::symlink_metadata(&cfg.conf).map(|m| m.file_type().is_symlink()).unwrap_or(false)
    {
        let _ = std::fs::copy(&cfg.conf, sub_conf(cfg, name));
        let _ = std::fs::remove_file(&cfg.conf);
    }
    ensure_symlink(cfg, name);
    set_current_sub(cfg, name);
    println!("{}✓ 已迁移为订阅 [{}]{} (顶层 config.yaml 现为软链; 旧 suburl 文件保留兼容)", c.green, name, c.reset);
}

/// cmd_sub 命令组入口
pub async fn cmd_sub(client: &Client, cfg: &Config, action: &str, name: Option<&str>, url: Option<&str>) {
    let c = pick_colors();
    migrate_from_legacy(cfg);
    match action {
        "add" => {
            let name = name.unwrap_or_else(|| die("用法: cone-cli sub add <名字> <URL>"));
            let url = url.unwrap_or_else(|| die("用法: cone-cli sub add <名字> <URL>"));
            if std::path::Path::new(&sub_dir(cfg, name)).exists() {
                die(&format!("订阅 [{}] 已存在。删除: cone-cli sub rm {}", name, name));
            }
            std::fs::create_dir_all(sub_dir(cfg, name))
                .unwrap_or_else(|e| die(&format!("无法创建订阅目录: {e}")));
            // 先写 suburl, 让 pull_one 能读到 (pull_one 也接受 url 参数, 这里双保险)
            std::fs::write(format!("{}/suburl", sub_dir(cfg, name)), url)
                .unwrap_or_else(|e| die(&format!("写入 suburl 失败: {e}")));
            // 先设为当前, 这样 pull_one 走"当前订阅"分支, 自动建软链 + reload
            set_current_sub(cfg, name);
            pull_one(client, cfg, name, Some(url)).await;
            println!("{}✓ 已添加订阅 [{}]{} 并设为当前", c.green, name, c.reset);
        }
        "list" | "ls" => {
            let subs = list_subs(cfg);
            if subs.is_empty() {
                println!("{}尚无订阅。添加: cone-cli sub add <名字> <URL>{}", c.dim, c.reset);
                return;
            }
            let cur = current_sub(cfg);
            println!("{}{}订阅列表{} (✶ = 当前):", c.bold, c.magenta, c.reset);
            for n in &subs {
                let mark = if cur.as_deref() == Some(n.as_str()) { "✶" } else { " " };
                let nodes = std::fs::read_to_string(sub_conf(cfg, n))
                    .ok()
                    .map(|s| count_proxies(&s))
                    .unwrap_or(0);
                println!("  {} {}{}{}  ({} 个节点)", mark, c.cyan, n, c.reset, nodes);
            }
        }
        "use" => {
            let name = name.unwrap_or_else(|| die("用法: cone-cli sub use <名字>"));
            if !std::path::Path::new(&sub_dir(cfg, name)).exists() {
                die(&format!("订阅 [{}] 不存在。查看: cone-cli sub list", name));
            }
            if std::path::Path::new(&sub_conf(cfg, name)).exists() {
                set_current_sub(cfg, name);
                ensure_symlink(cfg, name);
                reload_mihomo(cfg).await;
                let nodes = std::fs::read_to_string(sub_conf(cfg, name))
                    .ok().map(|s| count_proxies(&s)).unwrap_or(0);
                println!("{}✓ 已切换到订阅 [{}]{} ({} 个节点)", c.green, name, c.reset, nodes);
            } else {
                die(&format!("订阅 [{}] 尚无配置。执行: cone-cli sub add {} <URL> 或先 update", name, name));
            }
        }
        "rm" => {
            let name = name.unwrap_or_else(|| die("用法: cone-cli sub rm <名字> [--force]"));
            let force = url == Some("--force");
            let dir = sub_dir(cfg, name);
            if !std::path::Path::new(&dir).exists() {
                die(&format!("订阅 [{}] 不存在", name));
            }
            let is_current = current_sub(cfg).as_deref() == Some(name);
            if is_current && !force {
                die(&format!("[{}] 是当前订阅, 拒绝删除。先 sub use 切换其他订阅, 或: cone-cli sub rm {} --force", name, name));
            }
            std::fs::remove_dir_all(&dir).unwrap_or_else(|e| die(&format!("删除失败: {e}")));
            if is_current {
                // 删的是当前: 回退到任一剩余订阅, 没有则清空软链
                let remaining = list_subs(cfg);
                if let Some(next) = remaining.first() {
                    set_current_sub(cfg, next);
                    ensure_symlink(cfg, next);
                    reload_mihomo(cfg).await;
                    println!("{}✓ 已删除 [{}], 回退到 [{}]{}", c.yellow, name, next, c.reset);
                } else {
                    let _ = std::fs::remove_file(&cfg.conf);
                    let _ = std::fs::remove_file(&cfg.current_file);
                    println!("{}✓ 已删除 [{}] (最后一个订阅, 顶层软链已清空){}", c.yellow, name, c.reset);
                }
            } else {
                println!("{}✓ 已删除订阅 [{}]{}", c.green, name, c.reset);
            }
        }
        "current" => {
            match current_sub(cfg) {
                Some(n) => println!("{}", n),
                None => println!("{}尚无活跃订阅{}", c.dim, c.reset),
            }
        }
        _ => die("用法: cone-cli sub {add|list|use|rm|current}"),
    }
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

/// 从 URL 剥掉 scheme, 得到 host:port
/// 例: "http://127.0.0.1:9090" → "127.0.0.1:9090"
fn strip_url_scheme(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

/// 脱敏订阅 URL: 隐藏 query (订阅 token 常在 query 里), 保留 scheme/host/path 便于排查
fn mask_url(url: &str) -> String {
    match url.split_once('?') {
        Some((before, _)) => format!("{before}?***"),
        None => url.to_string(),
    }
}

/// 确保 config.yaml 含有顶层 external-controller 行, 值与 cfg.api 一致
/// 已存在则覆盖, 不存在则在文件开头插入。幂等: 文件不存在时直接 Ok(())
/// (mihomo 的 external-controller 只接受 host:port, 不带 scheme)
fn ensure_external_controller(cfg: &Config) -> std::io::Result<()> {
    ensure_external_controller_at(&cfg.conf, &cfg.api)
}

/// 参数化版本: 对任意目标 config 路径注入 external-controller
/// (多订阅模式下, 每个订阅 config 都需要注入, 不一定是顶层 cfg.conf)
fn ensure_external_controller_at(conf: &str, api: &str) -> std::io::Result<()> {
    if !std::path::Path::new(conf).exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(conf)?;
    let addr = strip_url_scheme(api);
    let target_line = format!("external-controller: {}", addr);

    let mut found = false;
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        // 只匹配顶层键 (行首无缩进), 避免误伤缩进到其他段的同名字段
        if line.starts_with("external-controller:") {
            out.push(target_line.clone());
            found = true;
        } else {
            out.push(line.to_string());
        }
    }
    let mut result = if found {
        out.join("\n")
    } else {
        // 文件开头插入 (顶层键顺序对 mihomo 无影响)
        let mut combined = target_line;
        combined.push('\n');
        combined.push_str(&out.join("\n"));
        combined
    };
    if content.ends_with('\n') {
        result.push('\n');
    }
    std::fs::write(conf, result)
}

/// 注入 tun.yaml 到目标 config (幂等: 仅当 config 不含 ^tun: 才追加)
/// tun.yaml 顶层共享 (TUN 是全局设置, 与订阅无关), 但注入到每个订阅 config
fn inject_tun_at(conf: &str, conf_dir: &str) {
    let tun_yaml = format!("{}/tun.yaml", conf_dir);
    if !std::path::Path::new(&tun_yaml).exists() {
        return;
    }
    let content = std::fs::read_to_string(conf).unwrap_or_default();
    if content.lines().any(|l| l.starts_with("tun:")) {
        return; // 已含 tun 段, 跳过
    }
    let tun = std::fs::read_to_string(&tun_yaml).unwrap_or_default();
    let mut combined = content;
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str(&tun);
    std::fs::write(conf, &combined).unwrap_or_else(|e| die(&format!("注入 tun.yaml 失败: {e}")));
}

// ============================== help ==============================
pub fn cmd_help() {
    let c = pick_colors();
    println!("{}{}cone-cli{}  —  Mihomo (Clash.Meta) 测速与节点选择工具 (Rust 版){}", c.magenta, c.bold, c.reset, c.reset);
    println!("{}(如嫌命令长可自行设置别名){}\n", c.dim, c.reset);
    println!("{}【命令】{}", c.bold, c.reset);
    println!("  {}status{}              显示版本/API/端口/主选择器/当前节点/节点总数", c.green, c.reset);
    println!("  {}list{}                列出全部节点 (带类型，标记当前节点)", c.green, c.reset);
    println!();
    println!("  {}ping{} [关键字] [-n N]  并行测延迟，按快慢显示前 N (默认 15)", c.green, c.reset);
    println!("                      只读；失败节点标 FAIL");
    println!("                      可加关键字过滤: ping 香港 / ping 美国 -n 10");
    println!();
    println!("  {}speed{} [关键字] [-n N] 吞吐测速 (只读，测完恢复原节点):", c.green, c.reset);
    println!("                        1. 并行测延迟 → 取前 N (默认 5)");
    println!("                        2. 逐个切换并下载实测带宽 (Mbps)，实时进度条");
    println!("                      可加关键字过滤: speed 日本 -n 10");
    println!();
    println!("  {}best{} [关键字] [-n N]  自动选最快并切换 (会改变当前节点!):", c.green, c.reset);
    println!("                      流程同 speed，但测完把当前节点设为带宽最高者");
    println!("                      N=候选数，默认 5；可加关键字过滤: best 香港 -n 3");
    println!();
    println!("  {}pick{} [ping]         fzf 交互式选节点:", c.green, c.reset);
    println!("                        pick        即时列出节点名，回车切换");
    println!("                        pick ping   先测延迟再按快慢排序选择");
    println!();
    println!("  {}use{} <关键字>        直接切换节点，支持模糊匹配", c.green, c.reset);
    println!("  {}start{}               启动 mihomo 服务 (需 sudo)", c.green, c.reset);
    println!("  {}stop{}                停止 mihomo 服务 (需 sudo)", c.green, c.reset);
    println!("  {}update{} [URL]        拉取并更新当前订阅 (URL 临时覆盖并记住)", c.green, c.reset);
    println!("  {}sub{} <act> [名] [URL] 管理多个订阅:", c.green, c.reset);
    println!("                        sub add <名字> <URL>   新增订阅并设为当前");
    println!("                        sub list                列出所有订阅 (✶=当前)");
    println!("                        sub use <名字>          切换当前订阅");
    println!("                        sub rm <名字> [--force] 删除订阅");
    println!("                        sub current             显示当前订阅名");
    println!("  {}service{} <act>       控制 mihomo 服务 {{on|off|restart|status}}", c.green, c.reset);
    println!("  {}tun{} <act>           控制 TUN {{on|off|status}}", c.green, c.reset);
    println!("  {}help{}                显示本帮助", c.green, c.reset);
    println!();
    println!("{}【环境变量】{}(可选)", c.bold, c.reset);
    println!("  MIHOMO_API / MIHOMO_PROXY / MIHOMO_GROUP / MIHOMO_TEST_URL");
    println!("  MIHOMO_SPEED_URL / MIHOMO_SPEED_BYTES / MIHOMO_DELAY_TIMEOUT");
    println!("  MIHOMO_PARALLEL / MIHOMO_CONF_DIR / MIHOMO_SUB_URL / MIHOMO_SUB_UA");
}
