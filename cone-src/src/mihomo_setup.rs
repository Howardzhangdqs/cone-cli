// mihomo 核心二进制的自动下载与定位
// 启动时检查同级目录是否有 mihomo, 无则从 GitHub 下载 (最新 release, compatible 变体)
// 失败时尝试镜像站, 下载过程显示进度条
#![allow(dead_code)]

use crate::ui::Colors;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// GitHub 镜像站 (主站失败时按序尝试)
const MIRRORS: &[&str] = &[
    "", // 空串 = GitHub 官方
    "https://mirror.ghproxy.com/",   // ghproxy
    "https://ghfast.top/",           // ghfast
    "https://gh-proxy.com/",         // gh-proxy
];

/// 检测当前系统对应的 mihomo 资产名 (compatible 变体)
/// amd64 -> mihomo-linux-amd64-compatible-vX.Y.Z.gz
/// arm64 -> mihomo-linux-arm64-compatible-vX.Y.Z.gz
fn asset_name(tag: &str, arch: &str) -> String {
    format!("mihomo-linux-{}-compatible-{}.gz", arch, tag)
}

/// 探测 CPU 架构 (返回 mihomo 资产命名用的 arch 字符串)
fn detect_arch() -> Result<String, String> {
    let arch = std::env::consts::ARCH;
    match arch {
        "x86_64" => Ok("amd64".to_string()),
        "aarch64" => Ok("arm64".to_string()),
        _ => Err(format!("不支持的架构: {} (仅支持 amd64/arm64)", arch)),
    }
}

/// 获取 cone-cli 二进制所在目录 (mihomo 应放在这里)
fn self_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位 cone-cli 自身: {e}"))?;
    Ok(exe.parent().unwrap_or(Path::new(".")).to_path_buf())
}

/// mihomo 二进制的预期路径 (cone-cli 同级目录)
pub fn mihomo_path() -> Result<PathBuf, String> {
    Ok(self_dir()?.join("mihomo"))
}

/// 确保 mihomo 存在: 不存在则下载, 返回最终路径
/// 已存在则直接返回; 下载失败则返回 Err
pub async fn ensure_mihomo(c: &Colors) -> Result<PathBuf, String> {
    let path = mihomo_path()?;
    if path.exists() {
        return Ok(path);
    }
    // 同级目录无 mihomo, 触发下载
    download_mihomo(c, &path).await?;
    Ok(path)
}

/// 下载并安装 mihomo 到指定路径
async fn download_mihomo(c: &Colors, dest: &Path) -> Result<(), String> {
    let arch = detect_arch()?;

    // 查询最新 release tag
    println!("{}→ 检测到尚未安装 mihomo 核心, 开始自动下载...{}", c.dim, c.reset);
    println!("{}  查询最新版本...{}", c.dim, c.reset);
    let tag = latest_tag().await?;
    let asset = asset_name(&tag, &arch);
    println!("{}  最新版本: {} · 资产: {}{}", c.dim, tag, asset, c.reset);

    // 按序尝试镜像站
    let mut last_err = String::new();
    for mirror in MIRRORS {
        let url = format!("{}https://github.com/MetaCubeX/mihomo/releases/download/{}/{}", mirror, tag, asset);
        match try_download(&url, dest).await {
            Ok(()) => {
                // 赋可执行权限
                set_executable(dest)?;
                println!("{}✓ mihomo {} 已安装到 {}{}", c.green, tag, dest.display(), c.reset);
                // 下载成功后询问是否安装 systemd service (首次引导)
                maybe_install_service(c).await;
                return Ok(());
            }
            Err(e) => {
                last_err = e;
                let which = if mirror.is_empty() { "GitHub 官方" } else { mirror };
                println!("{}  × {} 下载失败, 尝试下一个源...{}", c.yellow, which, c.reset);
            }
        }
    }
    Err(format!("所有下载源均失败。最后错误: {}。可手动从 https://github.com/MetaCubeX/mihomo/releases 下载 {} 放到 {}", last_err, asset, dest.display()))
}

/// 查询 mihomo 最新 release tag (如 "v1.19.29")
async fn latest_tag() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    // 优先用 API 查 latest (可能限流), 失败则用 redirect 解析
    let resp = client
        .get("https://api.github.com/repos/MetaCubeX/mihomo/releases/latest")
        .header("User-Agent", "cone-cli")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        if let Some(tag) = v.get("tag_name").and_then(|t| t.as_str()) {
            return Ok(tag.to_string());
        }
    }
    // 回退: 跟踪 /releases/latest 重定向拿 tag
    let resp = client
        .get("https://github.com/MetaCubeX/mihomo/releases/latest")
        .header("User-Agent", "cone-cli")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let url = resp.url().as_str();
    // url 形如 .../releases/tag/v1.19.29
    url.rsplit('/').next()
        .filter(|s| s.starts_with('v') && !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "无法解析最新版本号".to_string())
}

/// 从单个 URL 下载 (带进度条) 并解压 gz 到 dest
async fn try_download(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .header("User-Agent", "cone-cli")
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let total = resp.content_length().unwrap_or(0);

    // 进度条
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template("{spinner} {wide_bar} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // 流式下载到内存 (mihomo 解压后约 50MB, 内存可容纳)
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取失败: {e}"))?;
        buf.extend_from_slice(&chunk);
        pb.inc(chunk.len() as u64);
    }
    pb.finish_with_message("下载完成");
    // 解压 gz
    println!("  解压中...");
    let mut gz = GzDecoder::new(&buf[..]);
    let mut bin: Vec<u8> = Vec::with_capacity(buf.len());
    gz.read_to_end(&mut bin).map_err(|e| format!("解压失败: {e}"))?;
    // 写入目标文件
    let mut f = std::fs::File::create(dest).map_err(|e| format!("写入失败: {e}"))?;
    f.write_all(&bin).map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

/// 赋可执行权限 (Unix)
fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// systemd service 模板的目标路径
const SYSTEMD_UNIT: &str = "/etc/systemd/system/mihomo@.service";

/// 检测 mihomo@.service 是否已安装到系统
fn service_installed() -> bool {
    Path::new(SYSTEMD_UNIT).exists()
}

/// 检测当前系统是否用 systemd (有 systemctl 且 PID 1 是 systemd)
fn has_systemd() -> bool {
    std::process::Command::new("systemctl")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 下载 mihomo 后引导: 询问用户是否安装 systemd service
/// 仅在交互式终端 + 有 systemd + service 未安装时询问
async fn maybe_install_service(c: &Colors) {
    // 非交互式 (管道/重定向) 或无 systemd 时跳过
    if !console::user_attended() {
        return;
    }
    if !has_systemd() {
        println!("{}  未检测到 systemd, 请手动启动 mihomo (./mihomo -d ~/.config/mihomo){}", c.dim, c.reset);
        return;
    }
    if service_installed() {
        return; // 已安装, 不重复询问
    }

    // 找到 cone-cli 同级目录下的 mihomo@.service 模板源文件
    let tmpl = match self_dir() {
        Ok(d) => d.join("mihomo@.service"),
        Err(_) => return,
    };
    if !tmpl.exists() {
        println!("{}  未找到 mihomo@.service 模板, 跳过 service 安装{}", c.dim, c.reset);
        return;
    }

    // 交互询问
    println!();
    println!("{}{}是否安装 systemd 服务?{}", c.bold, c.magenta, c.reset);
    println!("{}  安装后可用 `cone-cli service on/off/restart` 管理 mihomo,{}", c.dim, c.reset);
    println!("{}  并支持 TUN 全局透明代理 (需 cap_net_admin)。安装需要 sudo。{}", c.dim, c.reset);
    print!("{}  安装? [Y/n] {}{}", c.bold, c.reset, c.dim);
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return;
    }
    let input = input.trim().to_lowercase();
    // 默认 Yes (空回车 / y / yes 都安装)
    if input == "n" || input == "no" {
        println!("{}  已跳过。之后可用 `sudo cp mihomo@.service {}` 手动安装。{}",
                 c.dim, SYSTEMD_UNIT, c.reset);
        return;
    }

    // 执行安装: sudo cp + daemon-reload
    println!("{}  正在安装 (需要 sudo 密码)...{}", c.dim, c.reset);
    let ok = std::process::Command::new("sudo")
        .args(["cp", &tmpl.display().to_string(), SYSTEMD_UNIT])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        println!("{}  × 安装失败 (sudo cp 出错), 请手动: sudo cp {} {}{}",
                 c.red, tmpl.display(), SYSTEMD_UNIT, c.reset);
        return;
    }
    // daemon-reload
    let _ = std::process::Command::new("sudo")
        .args(["systemctl", "daemon-reload"])
        .status();
    println!("{}✓ mihomo@.service 已安装{}", c.green, c.reset);
    println!("{}  现在可以用 `cone-cli service on` 启动 mihomo 了{}", c.dim, c.reset);
}
