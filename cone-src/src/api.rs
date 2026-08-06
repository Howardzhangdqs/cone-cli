// mihomo RESTful API 客户端封装
// 对齐 mihomo-cli.sh: 配置/节点过滤/组操作/延迟查询
#![allow(dead_code)]

use serde_json::Value;
use std::time::Duration;

// ============================== 配置 (env 覆盖) ==============================
// 对齐 mihomo-cli.sh 第 7-23 行
#[derive(Clone)]
pub struct Config {
    pub api: String,           // MIHOMO_API
    pub px: String,            // MIHOMO_PROXY
    pub test_url: String,      // MIHOMO_TEST_URL
    pub speed_url: String,     // MIHOMO_SPEED_URL
    pub speed_bytes: u64,      // MIHOMO_SPEED_BYTES
    pub group: String,         // MIHOMO_GROUP (空=自动探测)
    pub delay_timeout: u64,    // MIHOMO_DELAY_TIMEOUT (ms)
    pub parallel: usize,       // MIHOMO_PARALLEL
    pub conf_dir: String,      // MIHOMO_CONF_DIR
    pub conf: String,          // $conf_dir/config.yaml
    pub suburl_file: String,   // $conf_dir/suburl
    pub sub_ua: String,        // MIHOMO_SUB_UA
    pub svc: String,           // mihomo@<user>
}

impl Config {
    pub fn from_env() -> Self {
        let user = std::env::var("USER").unwrap_or_else(|_| {
            std::env::var("LOGNAME").unwrap_or_else(|_| "unknown".to_string())
        });
        let home = std::env::var("HOME").unwrap_or_else(|_| format!("/home/{}", user));
        let conf_dir = std::env::var("MIHOMO_CONF_DIR")
            .unwrap_or_else(|_| format!("{}/.config/mihomo", home));
        Self {
            api: std::env::var("MIHOMO_API")
                .unwrap_or_else(|_| "http://127.0.0.1:9090".to_string()),
            px: std::env::var("MIHOMO_PROXY")
                .unwrap_or_else(|_| "http://127.0.0.1:7890".to_string()),
            test_url: std::env::var("MIHOMO_TEST_URL")
                .unwrap_or_else(|_| "http://www.gstatic.com/generate_204".to_string()),
            speed_url: std::env::var("MIHOMO_SPEED_URL")
                .unwrap_or_else(|_| "https://speed.cloudflare.com/__down?bytes=10000000".to_string()),
            speed_bytes: std::env::var("MIHOMO_SPEED_BYTES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10_000_000),
            group: std::env::var("MIHOMO_GROUP").unwrap_or_default(),
            delay_timeout: std::env::var("MIHOMO_DELAY_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5000),
            parallel: std::env::var("MIHOMO_PARALLEL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(15),
            conf: format!("{}/config.yaml", conf_dir),
            suburl_file: format!("{}/suburl", conf_dir),
            sub_ua: std::env::var("MIHOMO_SUB_UA").unwrap_or_else(|_| "clash.meta".to_string()),
            svc: format!("mihomo@{}", user),
            conf_dir,
        }
    }
}

// ============================== 客户端 ==============================
pub struct Client {
    pub cfg: Config,
    http: reqwest::Client,
}

impl Client {
    pub fn new(cfg: Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("无法构造 HTTP 客户端");
        Self { cfg, http }
    }

    async fn get(&self, path: &str) -> reqwest::Result<Value> {
        let url = format!("{}{}", self.cfg.api, path);
        let resp = self.http.get(&url).send().await?;
        let json: Value = resp.json().await?;
        Ok(json)
    }

    async fn get_text(&self, path: &str) -> reqwest::Result<String> {
        let url = format!("{}{}", self.cfg.api, path);
        let resp = self.http.get(&url).send().await?;
        let text = resp.text().await?;
        Ok(text)
    }

    async fn put(&self, path: &str, body: &str) -> reqwest::Result<()> {
        let url = format!("{}{}", self.cfg.api, path);
        self.http
            .put(&url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await?;
        Ok(())
    }

    async fn patch(&self, path: &str, body: &str) -> reqwest::Result<()> {
        let url = format!("{}{}", self.cfg.api, path);
        self.http
            .patch(&url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await?;
        Ok(())
    }

    // ============================== 组操作 ==============================
    /// 主选择器: 显式 GROUP > 自动 (排除 GLOBAL 的 Selector 中 all 最长者)
    /// 对齐 mihomo-cli.sh 第 82-89 行
    pub async fn detect_group(&self) -> Result<String, String> {
        if !self.cfg.group.is_empty() {
            return Ok(self.cfg.group.clone());
        }
        let proxies = self.get("/proxies").await.map_err(|e| format!("无法连接 mihomo API: {e}"))?;
        let Some(map) = proxies.get("proxies").and_then(|v| v.as_object()) else {
            return Err("无法解析 /proxies 响应".to_string());
        };
        // 筛选 type==Selector 且 key!=GLOBAL, 按 all 长度降序
        let mut best: Option<(usize, String)> = None;
        for (key, val) in map {
            let is_selector = val.get("type").and_then(|v| v.as_str()) == Some("Selector");
            if !is_selector || key == "GLOBAL" {
                continue;
            }
            let len = val
                .get("all")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            match &best {
                None => best = Some((len, key.clone())),
                Some((bl, _)) if len > *bl => best = Some((len, key.clone())),
                _ => {}
            }
        }
        best.map(|(_, k)| k)
            .ok_or_else(|| "未找到 Selector 组".to_string())
    }

    pub async fn group_now(&self) -> Result<String, String> {
        let g = self.detect_group().await?;
        let path = format!("/proxies/{}", urlenc(&g));
        let v = self.get(&path).await.map_err(|e| e.to_string())?;
        Ok(v.get("now")
            .and_then(|x| x.as_str())
            .unwrap_or("-")
            .to_string())
    }

    pub async fn group_set(&self, node: &str) -> Result<(), String> {
        let g = self.detect_group().await?;
        let path = format!("/proxies/{}", urlenc(&g));
        // body: {"name":"<node json-escaped>"}
        let escaped = serde_json::to_string(node).unwrap_or_else(|_| format!("\"{}\"", node));
        let body = format!("{{\"name\":{}}}", escaped);
        self.put(&path, &body).await.map_err(|e| e.to_string())
    }

    // ============================== 节点列表 ==============================
    /// 非节点类型 (组/特殊) — 照抄 bash 第 102 行 (含其末尾 RejectDrop 重复 bug, 1:1 对齐)
    const EXCLUDE_TYPES: &'static [&'static str] = &[
        "Direct",
        "Reject",
        "RejectDrop",
        "Reject-Drop",
        "Pass",
        "PassRule",
        "Compatible",
        "Dns",
        "RejectDrop", // bash 既有 bug, 原样保留
    ];

    /// 所有真实节点: 返回 Vec<(name, type)>
    /// 真实节点 = 「没有 all 字段」且 「type 不在 EXCLUDE_TYPES」
    /// 对齐 mihomo-cli.sh 第 105-110 行 (has("all")|not 的反向判定是主过滤)
    pub async fn get_nodes_typed(&self) -> Result<Vec<(String, String)>, String> {
        let proxies = self.get("/proxies").await.map_err(|e| e.to_string())?;
        let Some(map) = proxies.get("proxies").and_then(|v| v.as_object()) else {
            return Err("无法解析 /proxies 响应".to_string());
        };
        let mut out = Vec::new();
        for (key, val) in map {
            // 主判定: 无 all 字段 (排除组节点)
            if val.get("all").is_some() {
                continue;
            }
            // 次判定: type 不在 EXCLUDE_TYPES
            let ty = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if Self::EXCLUDE_TYPES.contains(&ty) {
                continue;
            }
            out.push((key.clone(), ty.to_string()));
        }
        // 保持 mihomo 返回顺序 (jq to_entries 保序, serde_json 也保插入序)
        Ok(out)
    }

    pub async fn get_nodes(&self) -> Result<Vec<String>, String> {
        Ok(self
            .get_nodes_typed()
            .await?
            .into_iter()
            .map(|(n, _)| n)
            .collect())
    }

    pub async fn node_count(&self) -> Result<usize, String> {
        Ok(self.get_nodes().await?.len())
    }

    /// 单节点延迟: 返回 ms 字符串 (失败返回 "-", 对齐 bash 第 117-120 行)
    pub async fn node_delay(&self, name: &str) -> String {
        let path = format!(
            "/proxies/{}/delay?url={}&timeout={}",
            urlenc(name),
            urlenc(&self.cfg.test_url),
            self.cfg.delay_timeout
        );
        match self.get(&path).await {
            Ok(v) => v
                .get("delay")
                .and_then(|d| d.as_i64())
                .map(|d| d.to_string())
                .unwrap_or_else(|| "-".to_string()),
            Err(_) => "-".to_string(),
        }
    }

    /// 批量延迟测试用: 返回 i64, 失败 = 999999 (对齐 latency_all/latency_stream)
    pub async fn node_delay_ms(&self, name: &str) -> i64 {
        let path = format!(
            "/proxies/{}/delay?url={}&timeout={}",
            urlenc(name),
            urlenc(&self.cfg.test_url),
            self.cfg.delay_timeout
        );
        match self.get(&path).await {
            Ok(v) => v.get("delay").and_then(|d| d.as_i64()).unwrap_or(crate::ui::FAIL_MS),
            Err(_) => crate::ui::FAIL_MS,
        }
    }

    // ============================== configs (tun) ==============================
    pub async fn patch_configs(&self, body: &str) -> Result<(), String> {
        self.patch("/configs", body).await.map_err(|e| e.to_string())
    }

    pub async fn tun_enabled(&self) -> Result<bool, String> {
        let v = self.get("/configs").await.map_err(|e| e.to_string())?;
        Ok(v.get("tun")
            .and_then(|t| t.get("enable"))
            .and_then(|e| e.as_bool())
            .unwrap_or(false))
    }

    pub async fn version(&self) -> String {
        self.get("/version")
            .await
            .ok()
            .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "?".to_string())
    }

    // 暴露内部 http 给 measure.rs (测速需要自定义 client, 见 measure.rs)
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }
}

// ============================== 工具 ==============================
/// URL 编码 (对齐 bash 的 jq -rn --arg v '$v|@uri')
/// 对所有非 [A-Za-z0-9_.~-] 字符做 %HH 编码 (UTF-8 字节)
pub fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlenc_keeps_unreserved() {
        assert_eq!(urlenc("abcXYZ09-_~."), "abcXYZ09-_~.");
    }

    #[test]
    fn urlenc_encodes_special() {
        // 空格 -> %20
        assert_eq!(urlenc("a b"), "a%20b");
        // 中文 (UTF-8 三字节)
        assert_eq!(urlenc("中"), "%E4%B8%AD");
        // / -> %2F
        assert_eq!(urlenc("a/b"), "a%2Fb");
    }

    #[test]
    fn exclude_types_has_duplicate_rejectdrop_bug() {
        // bash 既有 bug: RejectDrop 重复出现两次, 1:1 原样保留
        let count = Client::EXCLUDE_TYPES.iter().filter(|&&t| t == "RejectDrop").count();
        assert_eq!(count, 2, "应保留 bash 的 RejectDrop 重复 bug");
    }
}
