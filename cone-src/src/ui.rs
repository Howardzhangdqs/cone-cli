// UI 纯函数: 颜色 / delay_tag / speed_bar / 对齐 / 表格渲染
// 这些都是无副作用的纯函数, 便于单元测试, 严格对齐 bash 版契约
#![allow(dead_code)]

use unicode_width::UnicodeWidthStr;

// ============================== 颜色 ==============================
// 对齐 mihomo-cli.sh 的颜色常量, 在 main.rs 根据 tty 决定是否启用
pub struct Colors {
    pub reset: &'static str,
    pub bold: &'static str,
    pub green: &'static str,
    pub yellow: &'static str,
    pub red: &'static str,
    pub cyan: &'static str,
    pub dim: &'static str,
    pub blue: &'static str,
    pub magenta: &'static str,
    // 背景色 (热力砖块墙用, 256 色码跨终端更一致)
    pub on_green: &'static str,
    pub on_cyan: &'static str,
    pub on_yellow: &'static str,
    pub on_red: &'static str,
    pub on_magenta: &'static str,
    // 前景黑/亮白 (在彩色背景上保证对比度)
    pub fg_black: &'static str,
    pub fg_white: &'static str,
    // 256 色前景 (下载测速砖块条用, 与上面背景同色板但走前景码)
    pub fg_green: &'static str,
    pub fg_cyan: &'static str,
    pub fg_yellow: &'static str,
    pub fg_red: &'static str,
    pub fg_magenta: &'static str,
}

pub const ANSI: Colors = Colors {
    reset: "\u{1b}[0m",
    bold: "\u{1b}[1m",
    green: "\u{1b}[32m",
    yellow: "\u{1b}[33m",
    red: "\u{1b}[31m",
    cyan: "\u{1b}[36m",
    dim: "\u{1b}[2m",
    blue: "\u{1b}[34m",
    magenta: "\u{1b}[35m",
    // 256 色背景: 深绿22 / 深海青24 / 橄榄黄58 / 正红124 / 暗血红52
    on_green: "\u{1b}[48;5;22m",
    on_cyan: "\u{1b}[48;5;24m",
    on_yellow: "\u{1b}[48;5;58m",
    on_red: "\u{1b}[48;5;124m",
    on_magenta: "\u{1b}[48;5;52m",
    fg_black: "\u{1b}[30m",
    fg_white: "\u{1b}[97m",
    // 256 色前景 (同色板, 38;5;N)
    fg_green: "\u{1b}[38;5;22m",
    fg_cyan: "\u{1b}[38;5;24m",
    fg_yellow: "\u{1b}[38;5;58m",
    fg_red: "\u{1b}[38;5;124m",
    fg_magenta: "\u{1b}[38;5;52m",
};

pub const PLAIN: Colors = Colors {
    reset: "",
    bold: "",
    green: "",
    yellow: "",
    red: "",
    cyan: "",
    dim: "",
    blue: "",
    magenta: "",
    on_green: "",
    on_cyan: "",
    on_yellow: "",
    on_red: "",
    on_magenta: "",
    fg_black: "",
    fg_white: "",
    fg_green: "",
    fg_cyan: "",
    fg_yellow: "",
    fg_red: "",
    fg_magenta: "",
};

// 延迟失败哨兵 (对齐 bash 的 999999)
pub const FAIL_MS: i64 = 999_999;

/// 延迟标签: 严格对齐 mihomo-cli.sh 第 37-44 行
/// 输入 ms (i64), 返回带色字符串; 显示宽度恒为 6 列
/// - d >= 999999 (FAIL_MS) -> 红色 "  FAIL" (前导 2 空格, 总宽 6)
/// - d < 200   -> 绿色 "%4dms" (4 位右对齐数字 + "ms", 共 6 列)
/// - d < 500   -> 蓝色
/// - d < 1000  -> 黄色
/// - 其他      -> 红色
pub fn delay_tag(c: &Colors, d: i64) -> String {
    if d >= FAIL_MS {
        // "  FAIL" = 2 空格 + FAIL = 6 列, 红色
        format!("{}  FAIL{}", c.red, c.reset)
    } else {
        // %4d + "ms" : 数字右对齐到 4 位, 再接 "ms"
        let val = format!("{:>4}", d); // 右对齐宽度 4
        let color = if d < 200 {
            c.green
        } else if d < 500 {
            c.blue
        } else if d < 1000 {
            c.yellow
        } else {
            c.red
        };
        format!("{}{}ms{}", color, val, c.reset)
    }
}

/// 速度进度条: 严格对齐 mihomo-cli.sh 第 48-68 行
/// 输入 bytes/sec (f64), 返回带色字符串
/// - bps < 1024: 短路, 灰色 "▏" + 12 空格 + " 0 Mbps" (0 格)
/// - Mbps = round(bps * 8 / 1_000_000)
/// - 格数 x (对数, 0-12): mbps<10 时 x=mbps/10*3; 否则 x=3+(ln(mbps/10)/ln(10))*3
/// - 颜色: <10 红, [10,30) 黄, [30,60) 蓝, >=60 绿
pub fn speed_bar(c: &Colors, bps: f64) -> String {
    if bps < 1024.0 {
        // 短路: 0 格
        return format!("{}▏            {} {}0 Mbps{}", c.dim, c.reset, c.red, c.reset);
    }
    let mbps = (bps * 8.0 / 1_000_000.0).round() as i64;
    // 对数映射
    let x = if (mbps as f64) < 10.0 {
        mbps as f64 / 10.0 * 3.0
    } else {
        3.0 + ((mbps as f64) / 10.0).ln() / (10f64.ln()) * 3.0
    };
    let x = x.round() as i64;
    let x = x.clamp(0, 12);
    // 拼格子
    let bar: String = "█".repeat(x as usize);
    let pad: String = " ".repeat((12 - x) as usize);
    // 颜色
    let col = if mbps < 10 {
        c.red
    } else if mbps < 30 {
        c.yellow
    } else if mbps < 60 {
        c.blue
    } else {
        c.green
    };
    format!("{}▏{}{}{} {} Mbps", col, bar, pad, c.reset, mbps)
}

/// 剥离 ANSI 转义码 (计算显示宽度时用), 精确匹配 ESC[ ... m 序列
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // 跳过 ESC[ ... m
            i += 2;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            i += 1; // 跳过 'm'
        } else {
            // 安全地按 UTF-8 字符推进
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// 计算字符串的终端显示宽度 (剥离 ANSI 后, 中文=2, emoji 正确处理)
pub fn display_width(s: &str) -> usize {
    let stripped = strip_ansi(s);
    stripped.width()
}

/// 定宽 padding (右补空格), 按显示宽度对齐
pub fn pad_right(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

/// 左侧 padding (右对齐), 按显示宽度对齐
pub fn pad_left(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(width - w), s)
    }
}

// ============================== 表格渲染 (unicode-width 精确对齐) ==============================
/// 一个表格单元格: 内容 (可含 ANSI) + 对齐方式
#[derive(Clone)]
pub struct Cell {
    pub text: String,
    pub align: Align,
}

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Right,
}

impl Cell {
    pub fn new(text: impl Into<String>) -> Self {
        Cell { text: text.into(), align: Align::Left }
    }
    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }
}

/// 渲染带框表格 (圆角风格), 自动按显示宽度对齐列
/// 所有列宽按「最大显示宽度」计算, emoji/中文/ANSI 均正确对齐
/// 每列内容左右各 1 空格内边距
pub fn render_table(headers: &[&str], rows: &[Vec<Cell>]) -> String {
    let ncols = headers.len();
    // 计算每列最大显示宽度 (含表头)
    let mut col_widths = vec![0usize; ncols];
    for (i, h) in headers.iter().enumerate() {
        col_widths[i] = col_widths[i].max(display_width(h));
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < ncols {
                col_widths[i] = col_widths[i].max(display_width(&cell.text));
            }
        }
    }
    let pad = 1; // 内边距

    // 生成一条横分隔线 (按列宽 + 内边距 * 2)
    let make_line = |left: &str, mid: &str, right: &str| -> String {
        let mut s = String::from(left);
        for (i, w) in col_widths.iter().enumerate() {
            s.push_str(&"─".repeat(w + pad * 2));
            if i + 1 < ncols {
                s.push_str(mid);
            }
        }
        s.push_str(right);
        s
    };
    let top = make_line("╭", "┬", "╮");
    let mid = make_line("├", "┼", "┤");
    let bottom = make_line("╰", "┴", "╯");

    let mut out = String::new();
    out.push_str(&top);
    out.push('\n');
    // 表头行 (左对齐)
    out.push_str(&render_row(headers.iter().map(|h| (h.to_string(), Align::Left)).collect::<Vec<_>>(), &col_widths, pad));
    out.push_str(&mid);
    out.push('\n');
    // 数据行
    for row in rows {
        let cells: Vec<(String, Align)> = row.iter().map(|c| (c.text.clone(), c.align)).collect();
        out.push_str(&render_row(cells, &col_widths, pad));
    }
    out.push_str(&bottom);
    out
}

/// 渲染一行 (含左右 │ 和内边距), cells = (text, align) 列表
fn render_row(cells: Vec<(String, Align)>, col_widths: &[usize], pad: usize) -> String {
    let mut s = String::from("│");
    for (i, w) in col_widths.iter().enumerate() {
        let (text, align) = cells.get(i).cloned().unwrap_or((String::new(), Align::Left));
        let inner = match align {
            Align::Left => pad_right(&text, *w),
            Align::Right => pad_left(&text, *w),
        };
        s.push_str(&" ".repeat(pad));
        s.push_str(&inner);
        s.push_str(&" ".repeat(pad));
        s.push('│');
    }
    s.push('\n');
    s
}

// ============================== 热力砖块墙 (x ping 风格) ==============================
// 每个节点渲染成一块带背景色的等宽砖, 砖内左对齐节点名 + 右对齐延迟值;
// 砖块紧贴自动折行拼接成墙, 颜色一眼分好坏。

/// 延迟 → (背景色, 前景色) 分档, 用于热力砖块
/// 256 色背景 (跨终端一致): 失败暗血红52 / 绿22 / 青海24 / 黄58 / 正红124
/// 深底上统一用亮白字保证对比度
/// - d >= FAIL_MS  → 暗血红底 + 白字 (失败)
/// - d < 200       → 深绿底 + 白字
/// - d < 500       → 深海青底 + 白字
/// - d < 1000      → 橄榄黄底 + 白字
/// - d >= 1000     → 正红底 + 白字
pub fn delay_cell_colors(c: &Colors, d: i64) -> (&'static str, &'static str) {
    if d >= FAIL_MS {
        (c.on_magenta, c.fg_white)
    } else if d < 200 {
        (c.on_green, c.fg_white)
    } else if d < 500 {
        (c.on_cyan, c.fg_white)
    } else if d < 1000 {
        (c.on_yellow, c.fg_white)
    } else {
        (c.on_red, c.fg_white)
    }
}

/// 速度 (Mbps) → 背景色分档 (用于下载测速砖块, 与 delay_cell_colors 同款色板, 越快越绿)
/// ≥100 深绿 / ≥50 深海青 / ≥20 橄榄黄 / ≥5 正红 / <5 暗血红
pub fn speed_cell_bg(c: &Colors, mbps: i64) -> &'static str {
    if mbps >= 100 {
        c.on_green
    } else if mbps >= 50 {
        c.on_cyan
    } else if mbps >= 20 {
        c.on_yellow
    } else if mbps >= 5 {
        c.on_red
    } else {
        c.on_magenta
    }
}

/// 渲染单个下载测速砖块 (固定显示宽度 = width), 显示该 1MB 区间速度
/// 砖内: [左1空格][速度 右对齐 如 "120M"][右1空格], 整块铺按速度分档的背景色
fn render_speed_brick(c: &Colors, mbps: i64, width: usize) -> String {
    let bg = speed_cell_bg(c, mbps);
    let text = format!("{}M", mbps);
    let inner_w = width.saturating_sub(2); // 去掉左右各 1 内边距
    let content = pad_left(&text, inner_w); // 右对齐到 inner_w
    format!("{}{} {} {}", bg, c.fg_white, content, c.reset)
}

/// 渲染下载测速砖块条 (固定 10 块, 每块代表 target/10 字节)
/// - seg_mbps[i] = Some(mbps): 第 i 段已完成, 显示该段速度, 按速度分档背景色
/// - 其余段 (含进行中、未开始): 灰色占位 (固定宽度, 保持对齐)
///
/// 返回不带换行的单行字符串
pub fn render_speed_bricks(c: &Colors, seg_mbps: &[Option<i64>], brick_w: usize) -> String {
    let mut s = String::new();
    let inner_w = brick_w.saturating_sub(2);
    let placeholder = " ".repeat(inner_w);
    for i in 0..10usize {
        match seg_mbps.get(i).copied().flatten() {
            Some(mbps) => s.push_str(&render_speed_brick(c, mbps, brick_w)),
            None => s.push_str(&format!("{}[{}]{}", c.dim, placeholder, c.reset)),
        }
    }
    s
}

/// 按显示宽度截断字符串, 超宽时末尾用 "…" 收尾
/// (节点名超长时避免破坏砖块对齐)
fn truncate_width(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    // 逐字符累加显示宽度, 留 1 列给 "…"
    let limit = width - 1;
    let mut out = String::new();
    let mut cur = 0usize;
    for ch in s.chars() {
        let cw = ch.to_string().width();
        if cur + cw > limit {
            break;
        }
        out.push(ch);
        cur += cw;
    }
    out.push('…');
    out
}

/// 渲染单个热力砖块 (固定显示宽度 = width)
/// 砖内布局: [左1空格][节点名 左对齐/截断][1空格][延迟值 右对齐][右1空格], 整块铺背景色
/// 失败节点延迟显示 "FAIL"
/// 总显示宽度 = 1 + name_w + 1 + delay_w + 1 = width, 故 name_w = width - delay_w - 3
pub fn render_heat_brick(c: &Colors, name: &str, ms: i64, width: usize) -> String {
    let (bg, fg) = delay_cell_colors(c, ms);
    let delay_text = if ms >= FAIL_MS {
        "FAIL".to_string()
    } else {
        format!("{}ms", ms)
    };
    let delay_w = display_width(&delay_text);
    // name_w 至少留 2 列, 防止 width 太小; name_w + delay_w + 3 可能 < width, 末尾补齐
    let name_w = width.saturating_sub(delay_w + 3).max(2);
    let name_fit = truncate_width(name, name_w);
    let name_padded = pad_right(&name_fit, name_w);
    let delay_padded = pad_left(&delay_text, delay_w);
    // 末尾补齐: 若 width 极小导致 name_w 被 max(2) 抬高, 总宽可能超过 width; 反之不足则补空格
    let actual_w = 1 + name_w + 1 + delay_w + 1;
    let trailing = if actual_w < width { " ".repeat(width - actual_w) } else { String::new() };
    format!("{}{} {} {} {}{}", bg, fg, name_padded, delay_padded, trailing, c.reset)
}

/// 渲染整面热力砖块墙: results 按延迟升序排好 (失败节点因 FAIL_MS 最大自然在末尾)
/// 每行容纳 per_row = max(1, (term_width + gap) / (brick_w + gap)) 块
/// 相邻砖块之间留 1 个空格 (gap=1), 行末不留空格
pub fn render_heat_wall(c: &Colors, results: &[(i64, String)], term_width: usize, brick_w: usize) -> String {
    if results.is_empty() {
        return String::new();
    }
    let bw = brick_w.max(1);
    let gap = 1usize;
    // 每块占 bw, 块间 gap; 一行 n 块占 bw*n + gap*(n-1); 解出 n = (term + gap) / (bw + gap)
    let per_row = ((term_width + gap) / (bw + gap)).max(1);
    let mut out = String::new();
    for chunk in results.chunks(per_row) {
        for (i, (ms, name)) in chunk.iter().enumerate() {
            if i > 0 {
                out.push(' '); // 块间 1 空格
            }
            out.push_str(&render_heat_brick(c, name, *ms, bw));
        }
        out.push('\n');
    }
    out
}

/// 探测终端列数, 失败/非 tty (管道) 时回退 80
pub fn term_columns() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

// ============================== 单元测试 ==============================
#[cfg(test)]
mod tests {
    use super::*;

    // 用 ANSI 颜色集测试 (行为契约); PLAIN 版本结构一致只是无转义码

    #[test]
    fn delay_tag_fail_is_red_with_two_leading_spaces() {
        let s = delay_tag(&ANSI, 999_999);
        // 去掉 ANSI 后应恰好是 "  FAIL"
        let plain = s.replace(ANSI.red, "").replace(ANSI.reset, "");
        assert_eq!(plain, "  FAIL");
    }

    #[test]
    fn delay_tag_normal_green_under_200() {
        // 12 -> "  12ms" (4 位右对齐 + ms)
        let s = delay_tag(&ANSI, 12);
        assert!(s.contains(ANSI.green));
        let plain = s.replace(ANSI.green, "").replace(ANSI.reset, "");
        assert_eq!(plain, "  12ms");
    }

    #[test]
    fn delay_tag_blue_200_to_500() {
        let s = delay_tag(&ANSI, 300);
        assert!(s.contains(ANSI.blue));
        let plain = s.replace(ANSI.blue, "").replace(ANSI.reset, "");
        assert_eq!(plain, " 300ms");
    }

    #[test]
    fn delay_tag_yellow_500_to_1000() {
        let s = delay_tag(&ANSI, 700);
        assert!(s.contains(ANSI.yellow));
    }

    #[test]
    fn delay_tag_red_1000_plus() {
        let s = delay_tag(&ANSI, 1500);
        assert!(s.contains(ANSI.red));
    }

    #[test]
    fn speed_bar_short_circuit_under_1024() {
        let s = speed_bar(&ANSI, 512.0);
        // 应含 12 个空格的占位 + "0 Mbps"
        let plain = strip_ansi(&s);
        assert!(plain.contains("0 Mbps"), "got: {}", plain);
        // 含 12 连续空格
        assert!(plain.contains("            "), "got: {}", plain);
    }

    #[test]
    fn speed_bar_normal_has_12_cells() {
        let s = speed_bar(&ANSI, 10_000_000.0); // 80 Mbps -> x=6 -> 6 █ + 6 空格
        let plain = strip_ansi(&s);
        assert!(plain.contains("80 Mbps"), "got: {}", plain);
        // 数 █ 的个数和紧随其后的空格(直到非空格)
        let bars = plain.chars().filter(|&c| c == '█').count();
        assert_eq!(bars, 6, "bars: {} (plain: {})", bars, plain);
        // 80Mbps: 6 █ + 6 空格 = 12 格; strip 后格式 "▏██████      {reset} 80 Mbps"
        // 即 '▏' 之后紧跟 6 █, 再 6 空格, 再 reset, 再 " 80 Mbps"
        let after = plain.split_once('▏').map(|(_, r)| r).unwrap_or("");
        let cells: String = after.chars().take_while(|&c| c == '█').collect();
        assert_eq!(cells.chars().count(), 6, "after: {}", after);
    }

    #[test]
    fn speed_bar_color_thresholds() {
        // <10 Mbps 红 ([10,30) 黄 [30,60) 蓝 >=60 绿
        // 用 Mbps=round(bps*8/1e6) 反算
        // 5 Mbps -> bps = 5*1e6/8 = 625000
        let s = speed_bar(&ANSI, 625_000.0);
        assert!(s.contains(ANSI.red), "5Mbps 应红: {}", s);
        // 20 Mbps
        let s = speed_bar(&ANSI, 2_500_000.0);
        assert!(s.contains(ANSI.yellow), "20Mbps 应黄: {}", s);
        // 40 Mbps
        let s = speed_bar(&ANSI, 5_000_000.0);
        assert!(s.contains(ANSI.blue), "40Mbps 应蓝: {}", s);
        // 100 Mbps
        let s = speed_bar(&ANSI, 12_500_000.0);
        assert!(s.contains(ANSI.green), "100Mbps 应绿: {}", s);
    }

    // ---------- 热力砖块墙测试 ----------

    #[test]
    fn delay_cell_colors_thresholds() {
        // 失败 -> 暗血红底白字
        let (bg, fg) = delay_cell_colors(&ANSI, 999_999);
        assert_eq!(bg, ANSI.on_magenta);
        assert_eq!(fg, ANSI.fg_white);
        // <200 -> 深绿底白字
        let (bg, fg) = delay_cell_colors(&ANSI, 84);
        assert_eq!(bg, ANSI.on_green);
        assert_eq!(fg, ANSI.fg_white);
        // <500 -> 深海青底白字
        let (bg, fg) = delay_cell_colors(&ANSI, 300);
        assert_eq!(bg, ANSI.on_cyan);
        assert_eq!(fg, ANSI.fg_white);
        // <1000 -> 橄榄黄底白字
        let (bg, fg) = delay_cell_colors(&ANSI, 700);
        assert_eq!(bg, ANSI.on_yellow);
        assert_eq!(fg, ANSI.fg_white);
        // >=1000 -> 正红底白字
        let (bg, fg) = delay_cell_colors(&ANSI, 1500);
        assert_eq!(bg, ANSI.on_red);
        assert_eq!(fg, ANSI.fg_white);
    }

    #[test]
    fn heat_brick_plain_width_matches() {
        // PLAIN 模式: 砖块纯文本显示宽度应恰为 width
        for width in [16usize, 20, 24, 30] {
            let s = render_heat_brick(&PLAIN, "香港节点", 84, width);
            let plain = strip_ansi(&s);
            let w = display_width(&plain);
            assert_eq!(w, width, "width={}: got {} (\"{}\")", width, w, plain);
        }
    }

    #[test]
    fn heat_brick_ansi_width_matches() {
        // ANSI 模式: 去掉 ANSI 后宽度仍为 width (背景色不占显示宽度)
        let s = render_heat_brick(&ANSI, "日本东-01", 86, 22);
        let plain = strip_ansi(&s);
        assert_eq!(display_width(&plain), 22, "plain: {}", plain);
        // 含背景色码
        assert!(s.contains(ANSI.on_green), "应绿底: {}", s);
        assert!(s.contains(ANSI.fg_white), "应白字: {}", s);
    }

    #[test]
    fn heat_brick_fail_shows_fail() {
        let s = render_heat_brick(&ANSI, "德国法兰", 999_999, 22);
        let plain = strip_ansi(&s);
        assert!(plain.contains("FAIL"), "失败应显示 FAIL: {}", plain);
        assert_eq!(display_width(&plain), 22);
        assert!(s.contains(ANSI.on_magenta), "失败应暗血红底: {}", s);
    }

    #[test]
    fn heat_brick_long_name_truncated() {
        // 超长节点名应被截断到砖块宽度内, 不破坏对齐
        let long = "这是一个非常非常非常长的节点名字应该被截断";
        let s = render_heat_brick(&PLAIN, long, 84, 22);
        let plain = strip_ansi(&s);
        assert_eq!(display_width(&plain), 22, "截断后宽度应=22: \"{}\"", plain);
        assert!(plain.contains('…'), "应含省略号: {}", plain);
        assert!(plain.contains("84ms"), "应含延迟: {}", plain);
    }

    #[test]
    fn heat_wall_layout_wraps() {
        let c = &PLAIN;
        let results: Vec<(i64, String)> = vec![
            (84, "A".into()),
            (100, "B".into()),
            (242, "C".into()),
        ];
        let brick_w = 22;
        // term_width 只够 1 块 -> 3 行 (per_row = (22+1)/(22+1) = 1)
        let wall = render_heat_wall(c, &results, brick_w, brick_w);
        assert_eq!(wall.matches('\n').count(), 3, "每块独占一行应 3 换行:\n{}", wall);
        // term_width 够 3 块 -> 1 行 (3 块需 22*3+2=68 列, per_row = 69/23 = 3)
        let wall = render_heat_wall(c, &results, brick_w * 3 + 2, brick_w);
        assert_eq!(wall.matches('\n').count(), 1, "3 块一行应 1 换行:\n{}", wall);
    }

    #[test]
    fn heat_wall_empty_is_empty() {
        let wall = render_heat_wall(&PLAIN, &[], 80, 22);
        assert!(wall.is_empty());
    }

    #[test]
    fn heat_wall_failed_nodes_at_end() {
        // 砖墙接收的 results 应已按延迟升序排好 (调用方负责); 这里只验证渲染不区分可达/失败
        let results: Vec<(i64, String)> = vec![
            (84, "快".into()),
            (999_999, "失败".into()),
        ];
        let wall = render_heat_wall(&ANSI, &results, 22, 22); // 每行 1 块
        let plain = strip_ansi(&wall);
        assert!(plain.contains("84ms"));
        assert!(plain.contains("FAIL"));
    }

    // ---------- 下载测速砖块条测试 ----------

    #[test]
    fn speed_cell_bg_thresholds() {
        // ≥100 深绿 / ≥50 青 / ≥20 黄 / ≥5 红 / <5 暗血红
        assert_eq!(speed_cell_bg(&ANSI, 150), ANSI.on_green);
        assert_eq!(speed_cell_bg(&ANSI, 100), ANSI.on_green);
        assert_eq!(speed_cell_bg(&ANSI, 60), ANSI.on_cyan);
        assert_eq!(speed_cell_bg(&ANSI, 50), ANSI.on_cyan);
        assert_eq!(speed_cell_bg(&ANSI, 30), ANSI.on_yellow);
        assert_eq!(speed_cell_bg(&ANSI, 20), ANSI.on_yellow);
        assert_eq!(speed_cell_bg(&ANSI, 10), ANSI.on_red);
        assert_eq!(speed_cell_bg(&ANSI, 5), ANSI.on_red);
        assert_eq!(speed_cell_bg(&ANSI, 3), ANSI.on_magenta);
        assert_eq!(speed_cell_bg(&ANSI, 0), ANSI.on_magenta);
    }

    #[test]
    fn speed_bricks_shows_mbps_numbers() {
        // 已完成段应显示速度数字 (如 "120M"), 未完成段为灰色占位
        let seg: Vec<Option<i64>> = vec![Some(120), Some(45)];
        let s = render_speed_bricks(&ANSI, &seg, 7);
        let plain = strip_ansi(&s);
        assert!(plain.contains("120M"), "应显示 120M: {}", plain);
        assert!(plain.contains("45M"), "应显示 45M: {}", plain);
    }

    #[test]
    fn speed_bricks_partial_placeholder_count() {
        // 2 段完成 + 8 段占位: 占位是 [...] 形式 (含方括号)
        let seg: Vec<Option<i64>> = vec![Some(120), Some(80)];
        let s = render_speed_bricks(&ANSI, &seg, 7);
        let plain = strip_ansi(&s);
        // 8 个占位, 每个含一对 []
        let brackets = plain.matches('[').count();
        assert_eq!(brackets, 8, "应有 8 个占位 []: {}", plain);
    }

    #[test]
    fn speed_bricks_all_done_no_placeholder() {
        let seg: Vec<Option<i64>> = vec![Some(100); 10];
        let s = render_speed_bricks(&ANSI, &seg, 7);
        let plain = strip_ansi(&s);
        assert!(!plain.contains('['), "全完成应无占位: {}", plain);
        // 应有 10 个 "100M"
        assert_eq!(plain.matches("100M").count(), 10);
    }

    #[test]
    fn speed_bricks_plain_no_ansi() {
        // PLAIN 模式: 无 ANSI 码
        let s = render_speed_bricks(&PLAIN, &[Some(100), Some(50)], 7);
        assert!(!s.contains('\u{1b}'));
        let plain = strip_ansi(&s);
        assert!(plain.contains("100M"));
        assert!(plain.contains("50M"));
    }

    fn strip_ansi(s: &str) -> String {
        // 简易去 ANSI (测试用)
        let mut out = String::new();
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\u{1b}' {
                in_esc = true;
                continue;
            }
            if in_esc {
                if c == 'm' {
                    in_esc = false;
                }
                continue;
            }
            out.push(c);
        }
        out
    }
}
