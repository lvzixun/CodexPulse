# CLI 额度刷新生命周期实测

2026-10-06 16:16–16:19（Asia/Shanghai）的历史对照实测。该测试在最终切换 HTTPS 之前完成；最终决定为 HTTPS 读取额度、失败不走 CLI。此记录保留原 CLI 周期与 WSL 启动环境诊断，作为性能对照。CLI 实现只保留在显式运行的测试构建中，产品发布构建不包含这条采集路径。

## 方法

使用 release 测试直接调用产品的 `rpc::read`，每次新建进程并读取现有来源的额度。不是模拟 RPC，也不是只测 `spawn()`。计时涵盖 CLI 路径解析 / WSL 运行检查、隐藏启动、`initialize`、`initialized`、`account/read`（`refreshToken: false`）、`account/rateLimits/read`、结果归一化和 `ChildGuard` 的 `kill()` / `wait()`。

共三批，每批 Windows / WSL 各 3 次，来源按实际网络工作线程顺序执行，轮次之间间隔 3 秒。第一批记录总时间，第二批额外记录阶段时间；第三批修正 WSL shell 启动后重新测量。每次返回后确认自有 child 句柄已清理。产品仍使用原有 15 秒 RPC 全程超时；没有改为常驻进程。正常桌面应用同时运行，未暂停用户工作；不是操作系统冷启动测试，不计入 Cargo 编译时间、刷新队列等待或下一次 UI 发布延迟。

Windows 使用应用实际选择的已安装 `codex-cli 0.160.0`；WSL Ubuntu 使用 zsh / NVM 环境中已安装的 `codex-cli 0.160.1`。报告只保存来源类别、时间、成功 / 错误码和 child 清理状态，不保存账号、额度响应或认证内容；基准代码不自行读取认证文件。

## Windows 成功结果

| 批次 | 第 1 次 | 第 2 次 | 第 3 次 |
| --- | ---: | ---: | ---: |
| 总时间采样 | 3.395 s | 8.253 s | 2.613 s |
| 带阶段采样 | 3.558 s | 2.254 s | 4.369 s |
| WSL 修正后再次采样 | 2.339 s | 2.494 s | 2.374 s |

9 次全部获取到非空额度，中位数 **2.613 s**，范围 **2.254–8.253 s**。前 6 次中位数 3.477 s，Windows 启动方式未变，不把第三批较短的查询时间归因于 WSL 修正。样本少，不作为 P95 / 长期稳定性结论。

第二批阶段细分（单位 ms）：

| 阶段 | 第 1 次 | 第 2 次 | 第 3 次 |
| --- | ---: | ---: | ---: |
| 路径解析 | 0.24 | 0.25 | 0.32 |
| 创建进程 | 4.70 | 3.20 | 3.45 |
| 初始化至可调用 | 52.90 | 45.36 | 56.68 |
| `account/read` | 2079.98 | 1007.32 | 3063.65 |
| `account/rateLimits/read` | 1413.74 | 1190.76 | 1235.82 |
| 结果归一化 | 0.054 | 0.012 | 0.012 |
| 返回与清理（包含 kill / wait） | 6.64 | 7.40 | 9.53 |

主要耗时在账号和额度 RPC 的等待及处理；创建 / 初始化 / 清理的总时间约 56–70 ms。总刷新时间不等于创建进程的 CPU 成本，不能用“每分钟阻塞几秒”推算 CPU 百分比。

## WSL 修正与成功结果

最初错误地把启动环境里无法解析 `codex` 说成 WSL 没有 CLI。用户纠正后核实：Ubuntu 默认 shell 是 zsh，CLI 安装在 NVM 的 Node 24.15.0 中，zsh 的交互环境可以正常找到它。原启动方式及先前检查的 bash 环境没有加载该 PATH。

修正为通过 WSL 默认 shell 的登录 / 交互环境 `exec` 启动 CLI，指定的 Linux 用户仍是独立参数；来源 home 用位置参数传递，在 shell 配置加载之后设置 `CODEX_HOME`，不把路径拼进 shell 程序。没有重新安装 CLI 或改写用户的 shell 配置。

第三批 3 次全部获取到非空额度，完整周期分别 **3.496 / 3.129 / 7.875 s**，中位数 **3.496 s**。阶段细分（单位 ms）：

| 阶段 | 第 1 次 | 第 2 次 | 第 3 次 |
| --- | ---: | ---: | ---: |
| 确认发行版运行 | 43.04 | 42.12 | 41.83 |
| 创建 Windows WSL 进程 | 2.32 | 1.43 | 1.18 |
| shell / CLI 启动至初始化完成 | 771.13 | 858.12 | 783.11 |
| `account/read` | 1347.93 | 967.60 | 6012.45 |
| `account/rateLimits/read` | 1330.06 | 1257.97 | 1035.62 |
| 结果归一化 | 0.008 | 0.010 | 0.008 |
| 返回与清理 | 1.51 | 2.14 | 1.10 |

两来源顺序刷新同一批的完整周期为 **5.835 / 5.624 / 10.249 s**。WSL 的 shell / CLI 启动约 0.8 秒，主要波动仍在账号读取。采样后复查 Linux app-server 进程，只看到测试前已存在的两个进程（启动时间 12:54），没有留下新增 app-server。

前两批 6 次 WSL `codex_disconnected` 的 0.138–0.161 s 只是错误启动路径的失败时间，已排除于上述成功统计。WSL 成功路径已用真实 zsh / NVM 来源验证；其他 shell、多用户及发行版停启竞态仍待验证。

## 资源补充

修改测试代码之前，对现有 release 桌面应用及后代进程进行了 125.27 秒约 100 ms 间隔的 CIM 采样，观察到 2 个短时 Windows `codex.exe` 子进程。单个子进程采样峰值 `PrivatePageCount` 约 **35.45 MiB**，整个应用进程组峰值约 **280.23 MiB**。

这里是私有提交内存，不是设计预算中的私有工作集；短进程也可能落在采样间隙，因此不作为严格峰值或进程创建数量验收。最终 HTTPS 改造后的同口径采样见 [HTTPS 刷新实测](https-refresh-benchmark-2026-10-06.md)。完整 CPU / 私有工作集 / 长期漂移验收仍待完成。

## 复跑

仅显式运行的 ignored 测试会联网读取额度，普通 `cargo test` 不执行这个基准。环境变量只配置输出位置与来源路径，不传认证信息。

```powershell
New-Item -ItemType Directory -Force work | Out-Null
$env:PULSE_BENCH_REPORT = Join-Path (Get-Location) 'work/cli-lifecycle.json'
# 如需测 WSL，填写已运行且已安装 Codex CLI 的真实来源；否则不设置这两项。
$env:PULSE_BENCH_WSL_DISTRO = 'Ubuntu'
$env:PULSE_BENCH_WSL_HOME = '\\wsl.localhost\Ubuntu\home\your-user\.codex'
# 可选：PULSE_BENCH_WSL_USER 指定 Linux 用户；Windows home 默认取 CODEX_HOME / USERPROFILE。
cargo test -p codexpulse --release measure_live_refresh_lifecycle -- --ignored --nocapture
```

JSON 输出放入忽略的 `work/`。本次两批结果与阶段数据已归纳于上表；报告不包含本机账号或精确来源目录。
