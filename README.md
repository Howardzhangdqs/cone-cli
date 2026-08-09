# Cone CLI

> Mihomo (Clash.Meta) 命令行测速与节点选择工具，Rust 编写。

`cone-cli` 通过 mihomo 的 RESTful API 测试节点延迟与吞吐，并支持节点切换、订阅更新、服务控制等操作。相比 shell 脚本，提供了并发延迟测试、测速进度条、正确处理 CJK/emoji/ANSI 宽度的表格输出，以及单文件部署。

![status 示例](https://img.shields.io/badge/platform-linux%20x86__64%20%7C%20arm64-blue)
![rust](https://img.shields.io/badge/Rust-1.75%2B-orange)

## 特性

- 11 个子命令：status / list / ping / speed / best / pick / use / update / service / tun / help
- 并发延迟测试：基于 tokio 异步并发，单节点测完即输出，无需等待全部完成
- 吞吐测速：下载过程实时显示速率、已下载量与用时
- 测速报告：包含平均速度、峰值速度、warmup 耗时、TTFB、下载量、总耗时，按平均速度排序
- 表格对齐：基于 unicode-width 计算显示宽度，正确处理 CJK 字符、emoji 与 ANSI 颜色码混排
- 自动安装 mihomo：首次运行若未检测到 mihomo 二进制，自动从 GitHub 下载 compatible 版，支持镜像站回退
- 单二进制：rustls 静态链接，无 OpenSSL 依赖

## 快速开始

### 安装

```bash
# 1. 克隆仓库
git clone https://github.com/Howardzhangdqs/cone-cli.git
cd cone-cli

# 2. 编译 (需 Rust 1.75+)
cd cone-src
cargo build --release
cp target/release/cone-cli ..
cd ..

# 3. 首次运行 —— 会自动下载 mihomo 核心
./cone-cli status
```

如果你已有 mihomo 二进制，把它放到 `cone-cli` 同级目录即可，跳过自动下载。

### mihomo 自动下载

首次运行时，如果 `cone-cli` 同级目录没有 `mihomo` 二进制，会自动：

1. 查询 [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo) 最新 release
2. 下载对应架构的 `compatible` 变体（amd64 / arm64 自动检测）
3. GitHub 失败时依次尝试镜像站（ghproxy / ghfast / gh-proxy）
4. 下载过程显示进度条，解压后赋可执行权限

### systemd 服务引导安装

mihomo 下载完成后，如果是交互式终端且系统有 systemd，会询问是否安装 `mihomo@.service`：

```
是否安装 systemd 服务?
  安装后可用 `cone-cli service on/off/restart` 管理 mihomo,
  并支持 TUN 全局透明代理 (需 cap_net_admin)。安装需要 sudo。
  安装? [Y/n]
```

输入 `Y` 或直接回车即用 sudo 安装到 `/etc/systemd/system/` 并 daemon-reload；之后即可用 `cone-cli service on` 启动。输 `n` 跳过，之后可手动 `sudo cp mihomo@.service /etc/systemd/system/`。

## 命令一览

下文示例均使用 `cone-cli` 全名（如嫌长可自行设置别名）。

| 命令 | 说明 |
|------|------|
| `cone-cli status` | 显示版本 / API / 端口 / 主选择器 / 当前节点 / 节点总数 |
| `cone-cli list` | 列出全部节点（带类型，标记当前节点） |
| `cone-cli ping [关键字] [-n N]` | 并行测延迟，以热力色块墙展示前 N（默认 15）；可加关键字过滤，如 `ping 香港 -n 10` |
| `cone-cli speed [关键字] [-n N]` | 吞吐测速（只读，测完恢复原节点），默认 5 候选；可加关键字过滤 |
| `cone-cli best [关键字] [-n N]` | 自动选最快并切换（会改变当前节点！），默认 5 候选；可加关键字过滤 |
| `cone-cli pick [ping]` | fzf 交互式选节点（`pick ping` 先测延迟排序） |
| `cone-cli use <关键字>` | 直接切换节点，支持模糊匹配 |
| `cone-cli start` | 启动 mihomo 服务（需 sudo，等同 `service on`） |
| `cone-cli stop` | 停止 mihomo 服务（需 sudo，等同 `service off`） |
| `cone-cli update [URL]` | 拉取订阅生成 config.yaml 并自动重载 |
| `cone-cli service <act>` | 控制 mihomo 服务（on/off/restart/status，需 sudo） |
| `cone-cli tun <act>` | 控制 TUN 全局透明代理（on/off/status） |
| `cone-cli help` | 显示帮助 |

### 环境变量（可选）

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `MIHOMO_API` | `http://127.0.0.1:9090` | mihomo 控制 API |
| `MIHOMO_PROXY` | `http://127.0.0.1:7890` | 代理地址（用于吞吐测速） |
| `MIHOMO_GROUP` | 自动探测 | 指定选择器组名 |
| `MIHOMO_TEST_URL` | `gstatic.com/generate_204` | 延迟测试 URL |
| `MIHOMO_SPEED_URL` | `speed.cloudflare.com/__down` | 吞吐下载 URL |
| `MIHOMO_SPEED_BYTES` | `10000000` | 吞吐下载字节数 |
| `MIHOMO_DELAY_TIMEOUT` | `5000` | 延迟超时（ms） |
| `MIHOMO_PARALLEL` | `15` | 延迟并发数 |
| `MIHOMO_CONF_DIR` | `~/.config/mihomo` | 配置目录 |
| `MIHOMO_SUB_URL` | （读取已保存文件） | 订阅地址 |
| `MIHOMO_SUB_UA` | `clash.meta` | 拉取订阅用的 UA |

## 使用示例

```bash
cone-cli                          # 显示帮助
cone-cli status                   # 看当前用哪个节点
cone-cli ping                     # 看延迟前 15（流式输出，测好即显示；结果以热力砖墙呈现，颜色按延迟分档）
cone-cli ping 30                  # 看前 30
cone-cli ping 香港                # 只看名字含「香港」的节点延迟
cone-cli ping 美国 -n 10          # 美国节点，前 10
cone-cli speed                    # 吞吐测速前 5 候选（详细报告 + 实时进度条）
cone-cli speed 日本 -n 10         # 日本节点，前 10 测速
cone-cli best                     # 一键选最快并切换
cone-cli best 3                   # 只比前 3，最快出结果
cone-cli best 香港 -n 3           # 在香港节点里选最快
cone-cli pick                     # fzf 即时选节点
cone-cli pick ping                # 边看延迟边选
cone-cli use 日本                 # 切到日本节点（模糊匹配）
cone-cli update                   # 更新订阅（首次需 cone-cli update <URL>）
cone-cli service status           # 查看 mihomo 服务状态
cone-cli tun on                   # 开启 TUN 全局透明代理
```

## 测速原理

**为什么 `best`/`speed` 要先用延迟筛 N 个候选？**

吞吐测速一次只能测一个节点（共享同一选择器，串行切换），逐个测试全部节点耗时较长。而延迟与带宽存在相关性，延迟排名靠前的节点通常也具有较高的吞吐。因此采用两阶段策略：

1. **延迟筛选**：并发测延迟，取 Top N（默认 5）作为候选
2. **吞吐精筛**：逐个切换候选节点，串行下载实测带宽（warmup → 流式下载 → 记录平均/峰值/TTFB）

**测速报告字段**：

| 字段 | 含义 |
|------|------|
| 平均速度 | 总下载量 ÷ 总耗时 |
| 峰值速度 | 100ms 采样的瞬时速率最大值 |
| warmup | warmup 请求总耗时（TCP+TLS 握手 + 首请求） |
| TTFB | 首字节时间（发起请求到第一个 chunk 到达） |
| 下载量 | 实际下载字节数 |
| 耗时 | 实测下载总耗时 |
| 延迟 | mihomo API 返回的延迟（ms） |

## 项目结构

```
.
├── cone-cli              # 编译产物（单二进制）
├── cone-src/             # Rust 源码
│   ├── src/
│   │   ├── main.rs       # 入口：clap 分发 + mihomo 自动下载
│   │   ├── api.rs        # mihomo RESTful 客户端（节点过滤/组操作/延迟）
│   │   ├── measure.rs    # 延迟并发 + 流式吞吐测速（SpeedDetail）
│   │   ├── ui.rs         # 颜色 / delay_tag / 表格渲染（unicode-width）
│   │   ├── cmds.rs       # 全部子命令实现
│   │   └── mihomo_setup.rs # mihomo 核心自动下载 + service 引导安装
│   └── Cargo.toml
└── mihomo@.service       # systemd 模板服务（TUN 需要 cap_net_admin）
```

## 技术栈

- Rust + tokio：异步并发（延迟并发，吞吐串行）
- reqwest（rustls 后端）：无 OpenSSL 依赖
- indicatif：进度条
- unicode-width：表格显示宽度计算（CJK / emoji / ANSI）

## 从 bash 版迁移

本项目最初是一个 bash 脚本，后用 Rust 重写。相比 bash 版的改进：

- 延迟测试改为异步并发（bash 版依赖 fork + 文件轮询）
- 吞吐测速支持实时进度条
- 测速报告增加峰值速度、TTFB、warmup 等字段
- 增加 mihomo 核心自动下载与 systemd 服务引导
- 表格输出基于 unicode-width 对齐，正确处理 CJK/emoji

## 许可

MIT
