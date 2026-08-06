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
