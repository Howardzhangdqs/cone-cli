// 延迟并发测试 + 吞吐流式测速 (数据层, 显示策略由 cmds.rs 决定)
// 对齐 mihomo-cli.sh: latency_stream / measure_speed
#![allow(dead_code)]

use crate::api::Client;
use futures_util::StreamExt;
use std::time::{Duration, Instant};

/// 单节点测速的详细结果 (用于测速后详细表格展示)
#[derive(Clone, Default)]
pub struct SpeedDetail {
    pub avg_bps: f64,        // 平均 bytes/sec
    pub peak_bps: f64,       // 瞬时峰值 bytes/sec (100ms 采样最大)
    pub warmup_ms: u64,      // warmup 请求总耗时 (ms), 0=失败
    pub ttfb_ms: u64,        // 首字节时间 (ms), 0=失败
    pub downloaded: u64,     // 下载总量 (bytes)
    pub elapsed_ms: u64,     // 实测总耗时 (ms)
}

impl SpeedDetail {
    /// 平均 Mbps
    pub fn avg_mbps(&self) -> f64 {
        self.avg_bps * 8.0 / 1_000_000.0
    }
    /// 峰值 Mbps
    pub fn peak_mbps(&self) -> f64 {
        self.peak_bps * 8.0 / 1_000_000.0
    }
}

/// 单个延迟测试结果: (ms, name); ms = 999999 表示失败
pub struct DelayResult {
    pub ms: i64,
    pub name: String,
}

/// 流式延迟测试: 所有节点并发 (并发度 = parallel), 结果按完成顺序通过 on_result 回调
/// 对齐 latency_stream: 全部 worker 并发, 完成一个回调一个
pub async fn latency_stream<F>(client: &Client, nodes: Vec<String>, parallel: usize, mut on_result: F)
where
    F: FnMut(DelayResult),
{
    let concurrency = parallel.max(1);
    let mut stream = futures_util::stream::iter(nodes.into_iter().map(|name| {
        let name_clone = name.clone();
        async move {
            let ms = client.node_delay_ms(&name).await;
            DelayResult { ms, name: name_clone }
        }
    }))
    .buffer_unordered(concurrency);

    while let Some(r) = stream.next().await {
        on_result(r);
    }
}

/// warmup URL: 把 SPEED_URL 字符串里第一处 `bytes=<数字>` 替换为 `bytes=100000`, 无则原样
/// 对齐 mihomo-cli.sh 第 125 行 sed -E 's/(bytes=)[0-9]+/\1100000/' (无 g 标志=首处)
pub fn warmup_url(speed_url: &str) -> String {
    if let Some(idx) = speed_url.find("bytes=") {
        let after = &speed_url[idx + 6..];
        let num_len = after.chars().take_while(|c| c.is_ascii_digit()).count();
        if num_len > 0 {
            let mut out = String::new();
            out.push_str(&speed_url[..idx]);
            out.push_str("bytes=100000");
            out.push_str(&after[num_len..]);
            return out;
        }
    }
    speed_url.to_string()
}

/// 单次流式下载测速, 返回平均 bytes/sec; 失败返回 None
/// progress_cb: 可选回调 (已下载字节, 已用秒), 由 cmds.rs 驱动 indicatif 进度条
///
/// 关键契约 (审核钉死):
/// - warmup: GET warmup_url (走代理), 丢弃, 忽略失败
/// - 实测: 显式 Accept-Encoding: identity (避免自动解压, 字节口径对齐 bash)
/// - 总超时 30s (对齐 --max-time 30)
async fn run_once<F>(
    client: &Client,
    speed_url: &str,
    target_bytes: u64,
    progress_cb: Option<&F>,
) -> Option<SpeedDetail>
where
    F: Fn(u64, f64),
{
    let proxy = reqwest::Proxy::all(&client.cfg.px).ok()?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .proxy(proxy)
        .build()
        .ok()?;
    // warmup (丢弃 body, 忽略失败), 记录耗时作为「首次连接时间」之一
    let warm = warmup_url(speed_url);
    let warm_start = Instant::now();
    let warmup_ms = match http.get(&warm).send().await {
        Ok(resp) => {
            // 消费丢弃 body
            let _ = resp.bytes().await;
            warm_start.elapsed().as_millis() as u64
        }
        Err(_) => 0, // warmup 失败不影响实测
    };
    // 实测: 流式, 记录 TTFB (send 到第一个 chunk)
    let req_start = Instant::now();
    let resp = http
        .get(speed_url)
        .header("Accept-Encoding", "identity") // 阻断项 9
        .send()
        .await
        .ok()?;
    let mut stream = resp.bytes_stream();
    let mut total: u64 = 0;
    let mut peak_bps: f64 = 0.0;
    let mut ttfb_ms: u64 = 0;
    let mut last_bytes: u64 = 0;
    let mut last_sample = req_start; // 用于 100ms 瞬时速率采样
    // start 时刻以第一个 chunk 到达为准 (之前的算 TTFB)
    let mut download_start: Option<Instant> = None;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(_) => break,
        };
        if ttfb_ms == 0 {
            ttfb_ms = req_start.elapsed().as_millis() as u64;
            download_start = Some(Instant::now());
        }
        total += chunk.len() as u64;
        let now = Instant::now();
        // 每 100ms 采样瞬时速率 (峰值)
        if now.duration_since(last_sample) >= Duration::from_millis(100) {
            let dt = now.duration_since(last_sample).as_secs_f64();
            if dt > 0.0 {
                let inst = (total - last_bytes) as f64 / dt;
                if inst > peak_bps {
                    peak_bps = inst;
                }
            }
            last_bytes = total;
            last_sample = now;
        }
        let start_ref = download_start.unwrap_or(req_start);
        let elapsed = start_ref.elapsed().as_secs_f64();
        if let Some(cb) = progress_cb {
            cb(total, elapsed);
        }
        if total >= target_bytes {
            break;
        }
        if start_ref.elapsed() >= Duration::from_secs(30) {
            break;
        }
    }
    let start_ref = download_start?;
    let elapsed = start_ref.elapsed().as_secs_f64();
    if elapsed <= 0.0 || total == 0 {
        return None;
    }
    Some(SpeedDetail {
        avg_bps: total as f64 / elapsed,
        peak_bps,
        warmup_ms,
        ttfb_ms,
        downloaded: total,
        elapsed_ms: (elapsed * 1000.0) as u64,
    })
}

/// 吞吐测速 (假设已切换到目标节点): 返回 SpeedDetail
/// 对齐 measure_speed: 重试条件 = 平均速率 < 1024 B/s 或失败 -> sleep 1 -> 重测一次
pub async fn measure_speed<F>(
    client: &Client,
    progress_cb: Option<&F>,
) -> SpeedDetail
where
    F: Fn(u64, f64),
{
    let detail = run_once(client, &client.cfg.speed_url, client.cfg.speed_bytes, progress_cb)
        .await;
    let spd_avg = detail.as_ref().map(|d| d.avg_bps).unwrap_or(0.0);
    // 重试: 平均速率 < 1024 或失败 -> sleep 1 -> 重测一次 (对齐第 128-131 行)
    if spd_avg < 1024.0 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Some(r) = run_once(client, &client.cfg.speed_url, client.cfg.speed_bytes, progress_cb).await {
            return r;
        }
    }
    detail.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_url_replaces_first_bytes() {
        let url = "https://speed.cloudflare.com/__down?bytes=10000000";
        assert_eq!(warmup_url(url), "https://speed.cloudflare.com/__down?bytes=100000");
    }

    #[test]
    fn warmup_url_no_bytes_returns_as_is() {
        let url = "https://example.com/bigfile";
        assert_eq!(warmup_url(url), url);
    }

    #[test]
    fn warmup_url_replaces_only_first_occurrence() {
        let url = "https://x?bytes=999&bytes=888";
        assert_eq!(warmup_url(url), "https://x?bytes=100000&bytes=888");
    }
}
