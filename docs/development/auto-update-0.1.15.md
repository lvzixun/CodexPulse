# 0.1.15 应用内更新

Windows x64 与 macOS Universal 使用 Tauri 官方 updater。展开详情时沿用每日自动检查和手动检查入口，通过公开 Release 的 HTTPS `latest.json` 获取稳定版本，不依赖 gh、GitHub 登录或 Codex 凭据。发现新版本后后台下载，界面显示进度；完成签名验证后才显示“重启并更新”。用户选择重启时机，安装过程无需打开下载页或手动运行安装包。

Windows 使用当前用户 NSIS 静默更新，通过末尾 /D 参数固定为当前可执行文件目录，保留安装位置与数据，安装后由安装器重新启动。macOS 使用 Universal `.app.tar.gz` 替换应用并重新启动，两个架构共用同一已签名更新包。旧版只支持下载页，需要先安装一次 0.1.15，之后才能应用内更新。

## 生命周期与性能

Rust Service 持有下载状态和已验证字节，窗口隐藏、收起或销毁后重建不会重复下载。元数据查询在阻塞工作线程；下载异步执行，进度最多每 250 毫秒推送一次，无新增轮询定时器。后台正在下载、待重启或正在安装时不发起第二份查询或下载。内存仅在有待安装更新时持有一份更新包，退出释放；重启应用后按需重新下载，不保存未校验的安装缓存。

元数据 10 秒超时，updater 二次确认 15 秒、下载 10 分钟超时；成功检查间隔 24 小时，手动至少间隔 60 秒。失败退避 15 分钟，元数据限流遵守 Retry-After。失败不会启动安装；可以重试或查看发布页。递增 revision 防止迟到的 IPC 响应覆盖新进度。Windows updater 直接退出前关闭采集器并保存待写位置；macOS 重启沿用正常退出清理。

## 发布签名

公钥内置于配置，私钥仅保存在发布机器的 `~/.tauri/codexpulse-updater.key` 和 GitHub Actions Secret `TAURI_SIGNING_PRIVATE_KEY`。后续发布必须复用此密钥，迁移机器需安全保管，不把私钥提交仓库。更新包必须通过签名校验且签名绑定的版本必须与清单一致，下载 URL 限定本仓库 HTTPS Release。

双平台 CI 生成安装包、更新包和 `.sig`，使用 `verify-update` 校验实际字节、签名中的版本，并验证篡改字节会被拒绝。只有两端通过后才生成完整的三平台清单及 SHA256SUMS，所有资产上传到草稿后一次正式发布，避免 latest 指向缺包版本。此签名用于更新验证，与 Windows Authenticode / Apple 公证不同，现有系统代码签名方式沿用。

## 验证

本机 release 模式 Rust workspace 测试、Clippy、Svelte 和 Node 测试；浏览器 IPC 测试覆盖中英文下载进度、未校验完成不可安装、安装失败恢复、安装期间禁用重复操作和原有拖动/Tibo交互。正式产物及各平台安装更新验证结果在发布后追加。不把浏览器模拟称为原生更新验收，也不把 Windows 验证称为 macOS 实机测试。

## 正式发布结果

[0.1.15 正式 Release](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.15) 已发布为 latest，非预发布。源标签提交 `b366505010e6b0c158fa9369ee80338e1a39384b`；[双平台 CI #37576575913](https://github.com/lvzixun/CodexPulse/actions/runs/37576575913) 全部通过。0.1.14 是未发布的中间标签，CI 已取消，保留源码历史。

本机 Rust release 测试 157 项通过、6 项显式网络测试忽略，Clippy 无警告；15 项前端 Node 测试、发布清单测试、Svelte 检查和中英文 IPC 交互检查通过。60 种主题 / 平台 / 语言 / 强调色组合检查通过。CI 再次运行双平台 Rust / Clippy / 前端检查，Windows 验证应用和安装器 x64 / 版本，macOS 验证 Universal arm64 + x86_64 和 ad-hoc 签名。两端实际更新资产均通过签名、绑定版本与篡改字节拒绝测试，下载后使用公钥再次复核。

| 资产 | 字节 | SHA-256 |
| --- | ---: | --- |
| Windows x64 安装 / 更新包 | 5,028,051 | `47a3065b23ccb7b28943297ef5a5c1a741c2fd485394f2567d6cd480886f9453` |
| macOS Universal DMG | 13,631,729 | `b1e18464e8db23d34387b73d11bea0c77fdc99d99a218ae3e0a022a7cec6ba02` |
| macOS Universal 更新归档 | 13,958,717 | `bd8121335c5f811c34520ed0a543171d3a59aa6c60476307fc70cc3101d6d870` |

上传到草稿的 7 个资产（以上 3 包、2 个签名、latest.json、SHA256SUMS.txt）已全部与 GitHub 服务端大小及 digest 比对，再一次正式发布。公开 HTTPS latest.json 读取成功，版本 0.1.15，Windows x64 和 Mac 两种架构的 URL 均指向该正式标签。Mac 归档内 Info.plist 版本 / 最低系统 12.0、两种 Mach-O 架构和签名资源也单独检查。

本机先用同提交的本地安装包从 0.1.13 静默升级，再用 CI 安装包执行 `/S /UPDATE /R /D=D:\files\CodexPulse` 验证当前目录安装和自动重新启动。最终进程 PID 36712，应用 `D:\files\CodexPulse\codexpulse.exe` 版本 0.1.15，SHA-256 `eea48179f7beadc41ab225359f2518c29d4266a69a47dd8ff1648210f1f4eea2`。此验证覆盖安装器及重启参数，不等同于从旧版点击一次应用内升级。macOS 本轮仅有原生 CI 和产物校验，未进行 Mac 实机点击升级。

额外的一次性 `tauri/test` MockRuntime 下载探针在本机以 `STATUS_ENTRYPOINT_NOT_FOUND` 退出，未完成网络 / 错版清单负例验证，**未计入通过**。辅助源码仅留在忽略的 work 目录，不包含在发布源码或应用内；正式不启用该测试 feature 的应用已启动并保持运行。后续可在受支持的测试运行环境继续排查此辅助程序启动问题。

升级前后各 20 秒只读进程树采样：0.1.13 / 0.1.15 的 CPU 增量为 0.250 / 0.219 秒，最大 private commit 265.6 / 244.3 MiB，working set 520.3 / 448.6 MiB，均为 7 个进程、0 个额度 CLI。界面和缓存生命周期未严格控制，作为运行状态记录，不能据此宣称更新功能降低了内存。无 target/debug 产物。
