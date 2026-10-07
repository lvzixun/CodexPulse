# CodexPulse

**中文** · [English](README.en.md)

Codex 的桌面状态面板。查看账户额度、会话用量和公开重置动态。

[下载 Windows / macOS 版](https://github.com/lvzixun/CodexPulse/releases) · [开发状态](docs/development/status.md)

## 功能

- **总览**：额度与重置时间、重置卡和 credits、当前工作会话；底部集中展示本机最近 30 天用量与账户累计。
- **模型 / Sessions**：模型用量占比、会话历史与行内详情、API 等价费用及计价覆盖。
- **Tibo**：重置观察与倒计时、最新公开重置、近半年重置历史和 28 天挑战；展开查看每日内容。

默认深色外观，可在设置中切换。macOS 使用菜单栏图标与原生材质面板。Windows 提供系统托盘和可选小浮窗，两平台共享账户、用量、Tibo 和设置功能。

## 界面预览

使用虚构账户和示例数据。原生材质会随系统主题和桌面背景变化。

<img src="docs/screenshots/overview-zh.svg" alt="CodexPulse 总览" width="380" />

<details>
<summary>Tibo 与模型页面</summary>

<img src="docs/screenshots/tibo-zh.svg" alt="Tibo 重置历史和挑战" width="380" />

<img src="docs/screenshots/models-zh.svg" alt="模型用量" width="380" />

</details>

## 安装

Windows 10/11 x64：从 [Releases](https://github.com/lvzixun/CodexPulse/releases) 下载 `x64-setup.exe`，安装后从开始菜单启动。需要 WebView2 Runtime；安装器可按需安装。点击浮窗或托盘左键打开同一详情；托盘右键提供设置和退出。登录启动可在设置中开启。

macOS：

1. 从 [Releases](https://github.com/lvzixun/CodexPulse/releases) 下载通用 DMG。
2. 打开 DMG，将 CodexPulse 拖到 Applications。
3. 启动应用，点击菜单栏图标打开面板。

通用包包含 Apple Silicon 和 Intel 架构，最低部署目标为 macOS 12。本轮实机验证在 Apple Silicon 完成。

macOS 包使用 **ad-hoc 签名，尚未进行 Developer ID 签名和公证**；Windows 安装包尚无 Authenticode 签名。正式 Release 指 GitHub 发布渠道，并不表示已获得平台发行证书。登录启动默认为关闭，可在设置中开启。

## 数据与统计口径

本机用量来自可读取的 Codex 日志，账户累计来自服务端，两者分别标明范围。用量账本保存在本机，个人资料仅在内存中保留。认证请求使用 Codex 的文件型凭据；程序不改写 `auth.json`，凭据不进入数据库或诊断输出，也不保存对话正文。

Windows 账户自动刷新仅在详情展开且可见时运行；小浮窗显示当前模型和平均速度，不触发账户请求。macOS 启动时执行首轮，之后仅在面板可见时按间隔运行。重新打开时按缓存期限决定是否刷新。公开重置消息和本地日志采集继续在后台运行。子代理不列入会话列表及会话数量，其 tokens 和费用仍计入总用量。

API 等价费用是参考价估算，标明未覆盖用量，**不是订阅账单**。`tok/s` 是有日志样本时的轮次平均输出速度，包含推理、工具和等待时间。缺少数据时显示 `—`。

## 开发

需要 Rust stable ≥ 1.90、Node.js 24+ 和 pnpm 11；发布检查固定使用 Rust 1.97.1。macOS 需要 Xcode Command Line Tools；Windows 需要 Visual Studio C++ 工具与 WebView2。

```sh
pnpm install --frozen-lockfile
pnpm dev
```

检查与构建：

```sh
pnpm check
node --test apps/desktop/test/*.test.ts
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm build
```

macOS 通用包：

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm build --target universal-apple-darwin
```

Cargo 和 target 标准库须使用同一 Rust 工具链。通用包位于 `target/universal-apple-darwin/release/bundle`。

## 文档

- [产品与技术设计](docs/design/codexpulse-v1.md)
- [实施状态与待验证项目](docs/development/status.md)
- [Windows 对齐与双平台发布](docs/development/windows-release-0.1.6.md)
- [开发交接](docs/development/handoff-2026-10-06.md)
- [macOS 验证记录](docs/development/macos-2026-10-06.md)
- [参考价与覆盖说明](resources/pricing/README.md)
