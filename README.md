# Cone CLI

> Mihomo (Clash.Meta) 命令行测速与节点选择工具，Rust 编写。

`cone-cli` 通过 mihomo 的 RESTful API 测试节点延迟与吞吐，并支持节点切换、订阅更新、服务控制等操作。相比 shell 脚本，提供了并发延迟测试、测速进度条、正确处理 CJK/emoji/ANSI 宽度的表格输出，以及单文件部署。

![platform](https://img.shields.io/badge/platform-linux%20x86__64%20%7C%20i686%20%7C%20arm64%20%7C%20armv7-blue)
![rust](https://img.shields.io/badge/Rust-1.75%2B-orange)

## 特性

- 12 个子命令：status / list / ping / speed / best / pick / use / update / sub / service / tun / help
- 并发延迟测试：基于 tokio 异步并发，单节点测完即输出，无需等待全部完成
- 吞吐测速：下载过程实时显示速率、已下载量与用时
- 测速报告：包含平均速度、峰值速度、warmup 耗时、TTFB、下载量、总耗时，按平均速度排序
- 表格对齐：基于 unicode-width 计算显示宽度，正确处理 CJK 字符、emoji 与 ANSI 颜色码混排
- 自动安装 mihomo：首次运行若未检测到 mihomo 二进制，自动从 GitHub 下载 compatible 版，支持镜像站回退
- 单二进制：rustls 静态链接，无 OpenSSL 依赖

## 快速开始

### 下载预编译二进制（推荐）

直接从 [Releases](../../releases) 下载对应架构的裸二进制，免去本地编译。按你的平台选：

| 文件名 | 平台 |
|--------|------|
| `cone-cli-x86_64-unknown-linux-musl` | x86_64（绝大多数 PC/服务器，静态链接，推荐） |
| `cone-cli-x86_64-unknown-linux-gnu` | x86_64（glibc 动态链接） |
| `cone-cli-i686-unknown-linux-gnu` | x86 32 位 |
| `cone-cli-aarch64-unknown-linux-musl` | ARM64（树莓派 4/5、ARM 服务器） |
| `cone-cli-aarch64-unknown-linux-gnu` | ARM64（glibc 动态链接） |
| `cone-cli-arm-unknown-linux-musleabihf` | ARM32（树莓派 2/3、嵌入式，静态链接） |
| `cone-cli-arm-unknown-linux-gnueabihf` | ARM32（glibc 动态链接） |

> `musl` 变体为完全静态链接，不依赖系统 glibc 版本，能在任意 Linux（含 Alpine、老 CentOS）运行，优先选它。

```bash
chmod +x cone-cli-<架构>
./cone-cli-<架构> status   # 首次运行会自动下载 mihomo 核心
```

### 从源码编译

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
| `cone-cli update [URL]` | 更新当前订阅（URL 可选，临时覆盖并记住）；自动重载 |
| `cone-cli sub <act> [名] [URL]` | 管理多个订阅：`add 名 URL` / `list` / `use 名` / `rm 名 [--force]` / `current` |
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
cone-cli sub add home <URL>       # 添加名为 home 的订阅（自动拉取并设为当前）
cone-cli sub add office <URL>     # 再加一个 office 订阅
cone-cli sub list                 # 列出所有订阅（✶ 标记当前）
cone-cli sub use home             # 切换当前订阅为 home（改软链 + 重载）
cone-cli update                   # 更新当前订阅（home）；update <URL> 可临时换源
cone-cli service status           # 查看 mihomo 服务状态
cone-cli tun on                   # 开启 TUN 全局透明代理
```

## 多订阅管理

`cone-cli` 支持同时维护多份独立订阅配置，每份订阅是 `~/.config/mihomo/subs/<名字>/` 下的一个独立目录（各含自己的 `config.yaml` 与 `suburl`）。顶层 `config.yaml` 始终是一个**符号链接**，指向当前活跃订阅的配置——切换订阅即改软链并重载 mihomo，无需改动 systemd service。

```
~/.config/mihomo/
├── config.yaml   → subs/<当前>/config.yaml   # 软链，service 的 -f 读这里
├── subs/
│   ├── home/      { config.yaml, suburl }
│   └── office/    { config.yaml, suburl }
└── current        # 单行文本：当前活跃订阅名
```

- **首次使用**：`sub add <名字> <URL>` 添加第一个订阅，它会被设为当前。
- **旧单订阅升级**：若检测到旧模式（顶层 `suburl` 文件且无 `subs/` 目录），首次执行任意 `sub`/`update` 命令会自动迁移为名为 `default` 的订阅，原配置无损保留。
- **`sub rm <当前>`** 默认拒绝，避免误删正在用的配置；加 `--force` 可强制删除并自动回退到任一剩余订阅。
- `cache.db` / `geoip.metadb` / 日志在顶层共享（按配置隔离、缓存共享）；TUN 设置（`tun.yaml`）顶层共享，每个订阅 `update` 时自动注入。

## 演示

`best` 命令的两阶段输出（节点名已脱敏为占位）：先并发测延迟生成**热力砖块墙**（颜色按延迟分档：绿<200ms · 青<500ms · 黄<1000ms · 红≥1000ms · 暗血红失败），再对 Top N 候选逐个下载测速，每 1MB 区间一块**速度砖块**（显示该区间 Mbps，颜色按速度分档），最后输出汇总表格并切换到最快节点。

<details>
<summary>点击展开 <code>cone-cli best</code> 完整输出演示</summary>

<pre><font color="#AAAAAA">1. 延迟筛选 Top 5 候选 (即时显示)...</font>
⠏ 52/52 测延迟中  ██████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████
<span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点01…   36ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点02…   39ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点03…   32ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点04…   29ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点05…   32ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点06…   35ms </font></span>
<span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点07…   36ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点08…  178ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点09…  181ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点10…  184ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点11…   31ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点12…  178ms </font></span>
<span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点13…  164ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点14…  239ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点15…  234ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点16…   76ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点17…  252ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点18…  272ms </font></span>
<span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点19…   75ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点20…  287ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点21…  291ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点22…  117ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点23…  140ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点24…   79ms </font></span>
<span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点25…   82ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点26…   54ms </font></span> <span style="background-color:#5F0000"><font color="#FFFFFF"> 🏳️ 节点27…  FAIL </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点28…  342ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点29…  129ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点30…  218ms </font></span>
<span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点31…   29ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点32…   64ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点33…   28ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点34…  119ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点35…   43ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点36…  333ms </font></span>
<span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点37…   82ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点38…  114ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点39…  376ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点40…  182ms </font></span> <span style="background-color:#AF0000"><font color="#FFFFFF"> 🏳️ 节点41… 1005ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点42…  174ms </font></span>
<span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点43…  243ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点44…  181ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点45…  313ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点46…  188ms </font></span> <span style="background-color:#005F00"><font color="#FFFFFF"> 🏳️ 节点47…  192ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点48…  302ms </font></span>
<span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点49…  354ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点50…  289ms </font></span> <span style="background-color:#005F87"><font color="#FFFFFF"> 🏳️ 节点51…  350ms </font></span> <span style="background-color:#5F0000"><font color="#FFFFFF"> 🏳️ 节点52…  FAIL </font></span>

<font color="#AAAAAA">2. 逐个下载测速...</font>
<font color="#AAAAAA">#1 🏳️ 节点A    </font><span style="background-color:#005F87"><font color="#FFFFFF">   70M </font></span><span style="background-color:#005F00"><font color="#FFFFFF">  144M   274M   299M   219M   571M   600M   436M   233M   231M </font></span>  <b>231 Mbps</b>  10.0 MB  0.35s
<font color="#AAAAAA">#2 🏳️ 节点B    </font><span style="background-color:#005F87"><font color="#FFFFFF">   67M </font></span><span style="background-color:#005F00"><font color="#FFFFFF">  137M   167M   611M   312M   921M   387M   224M  1570M   237M </font></span>  <b>237 Mbps</b>  10.0 MB  0.34s
<font color="#AAAAAA">#3 🏳️ 节点C    </font><span style="background-color:#005F87"><font color="#FFFFFF">   54M </font></span><span style="background-color:#005F00"><font color="#FFFFFF">  130M   248M   270M   282M   206M  3559M   325M   921M   184M </font></span>  <b>184 Mbps</b>  10.0 MB  0.43s
<font color="#AAAAAA">#4 🏳️ 节点D    </font><span style="background-color:#005F87"><font color="#FFFFFF">   75M </font></span><span style="background-color:#005F00"><font color="#FFFFFF">  147M   282M   287M   188M  4008M   355M   834M   257M   242M </font></span>  <b>242 Mbps</b>  10.0 MB  0.33s
<font color="#AAAAAA">#5 🏳️ 节点E    </font><span style="background-color:#005F87"><font color="#FFFFFF">   62M </font></span><span style="background-color:#005F00"><font color="#FFFFFF">  124M   144M   900M   271M   327M   893M   393M   933M   195M </font></span>  <b>195 Mbps</b>  10.0 MB  0.41s</pre>

<table>
<thead>
<tr><th>节点</th><th>平均速度</th><th>峰值</th><th>warmup</th><th>TTFB</th><th>下载量</th><th>耗时</th><th>延迟</th></tr>
</thead>
<tbody>
<tr><td>🏳️ 节点D</td><td><b>242 Mbps</b></td><td><span style="color:#2AA1B3">246 Mbps</span></td><td>173 ms</td><td>52 ms</td><td>10.0 MB</td><td>0.33s</td><td><span style="color:#26A269">31ms</span></td></tr>
<tr><td>🏳️ 节点B</td><td><b>237 Mbps</b></td><td><span style="color:#2AA1B3">317 Mbps</span></td><td>190 ms</td><td>52 ms</td><td>10.0 MB</td><td>0.34s</td><td><span style="color:#26A269">26ms</span></td></tr>
<tr><td>🏳️ 节点A</td><td><b>231 Mbps</b></td><td><span style="color:#2AA1B3">251 Mbps</span></td><td>189 ms</td><td>51 ms</td><td>10.0 MB</td><td>0.35s</td><td><span style="color:#26A269">31ms</span></td></tr>
<tr><td>🏳️ 节点E</td><td><b>195 Mbps</b></td><td><span style="color:#2AA1B3">401 Mbps</span></td><td>198 ms</td><td>58 ms</td><td>10.0 MB</td><td>0.41s</td><td><span style="color:#26A269">28ms</span></td></tr>
<tr><td>🏳️ 节点C</td><td><b>184 Mbps</b></td><td><span style="color:#2AA1B3">351 Mbps</span></td><td>188 ms</td><td>57 ms</td><td>10.0 MB</td><td>0.43s</td><td><span style="color:#26A269">31ms</span></td></tr>
</tbody>
</table>

<pre><font color="#A347BA"><b>最快节点:</b></font> <font color="#26A269">🏳️ 节点D</font>  （平均 242 Mbps · 峰值 246 Mbps）
<font color="#26A269">✓ 已切换</font></pre>

</details>

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
