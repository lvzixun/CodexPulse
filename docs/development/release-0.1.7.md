# 0.1.7 更新检查与双平台发布

## 范围

- 仓库已公开，应用通过匿名 HTTPS 查询 GitHub 最新正式 Release，不依赖 gh。
- 设置检查更新、详情顶部新版提示；点击升级打开下载页，由用户下载安装。Windows / macOS 共用实现。
- 仅展开且可见时自动检查，成功缓存 24 小时；单飞、至少 60 秒间隔、10 秒超时、256 KiB 响应上限、失败 / 限流退避。独立于账户与消息刷新，不新增高频时钟。
- 重置标签与结果同行；Tibo 可见页面自动标记当前及新到消息已读，移除已读按钮；小浮窗原生右键仅展开、退出、隐藏。
- 精简 README 默认展示总览、小浮窗及高清 Tibo 图片，中英文对齐。

## 本机检查

- `cargo test --release --workspace --locked`：145 项通过，6 项显式联网探针忽略。
- `cargo clippy --release --workspace --all-targets --locked -- -D warnings` 通过。
- `pnpm check`：零错误、零警告；Node 15 项测试通过。
- `scripts/verify-panel.cjs`：中英文升级提示 / 手动检查 / 下载动作 / 关闭提示 / 缓存复用，以及 Tibo 可见性 / 新消息 / 写入单飞 / 失败重试、菜单顺序 / 动作 / 复用通过。
- 重置同行布局已覆盖中英文、320 / 380 宽度及长结果。浏览器 IPC 模拟不能替代原生菜单绘制或安装测试。

本次本机 Rust 检查仅使用 release profile，没有重建已清理的 debug 目录。此前清理释放 18.453 GiB 仍有效。

## 发布

[CodexPulse 0.1.7 正式 Release](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.7) 已于北京时间 2026-10-07 11:31:46 发布，`draft=false`、`prerelease=false`，为 latest；仓库 Public。应用提交及标签 `v0.1.7` 均为 `97b3b88115c142bb8e65eaa8df44e52c042e82d3`，后续 main 仅补充发布文档。

[双平台 CI #37566063502](https://github.com/lvzixun/CodexPulse/actions/runs/37566063502) 全部通过：Windows Rust 145 项、macOS Rust 147 项，分别忽略 6 项显式联网探针；两平台 Node 15 项、Svelte、Clippy、编译与打包成功。macOS 严格 ad-hoc 签名校验及 arm64 / x86_64 架构检查通过。Windows 安装器 ProductVersion / FileVersion 均为 0.1.7，GUI 子系统为 2；DMG trailer 有效。

| 正式产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `CodexPulse_0.1.7_x64-setup.exe` | 4,525,022 | `6d5fd2199ca748b85d6b73a6b729dcd5d5a61e0ce8fb5861a9b39b5a4b58e74b` |
| `CodexPulse_0.1.7_universal.dmg` | 12,252,911 | `d6cb8cf13407bc37504844d7bc51dffdb5a9be059c8e92cc23196cfa897eabf2` |
| `SHA256SUMS.txt` | 194 | `6bceddd06071dd6e3b4513083f27f0d40faf94505fdda11cb4aeea1d714dfacb` |

服务端资产大小及摘要与本地逐一匹配；发布后重新下载 SHA256SUMS，逐字节一致。产物本机保留在 `releases/v0.1.7`（Git 忽略）。使用直接引用产品 `updates.rs` 和同源 Retry-After 函数的临时命令行探针，实际通过 ureq / TLS 匿名请求公开 GitHub：发布前返回 0.1.6，11:31:59 再次请求返回最新 0.1.7，状态 current。界面新版提示使用 0.1.8 模拟数据验证，没有发布虚构版本。

Windows 无 Authenticode 签名，macOS ad-hoc 签名、未公证。新版本不自动静默安装。本轮没有重新执行 Windows 0.1.7 原生菜单绘制、安装 / 升级 / 卸载和 macOS 原生 UI 验收；Windows 安装数据保留的既有 0.1.6 检查见历史报告，不能冒充 0.1.7 实机结果。浏览器 IPC 检查、实际 HTTPS 探针及 CI 的证据范围分别如上。
