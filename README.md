# CodexPulse

**中文** · [English](README.en.md)

在桌面查看 Codex 的账户额度、工作状态、Token 用量和重置动态。支持 Windows / WSL 与 macOS 的 Codex App / CLI。

**[下载最新版本](https://github.com/lvzixun/CodexPulse/releases/latest)** · [使用与开发说明](docs/guide.md)

## 功能

- **额度**：剩余额度、恢复时间、credits 与重置卡。
- **用量**：最近 30 天总量、模型占比图表、API 等价费用估算；按模型查看运行均速、速度曲线及 Fast 模式。
- **Sessions**：运行中会话置顶，展开查看模型、用量和平均速度。
- **Tibo**：重置公告、历史记录与 28 天挑战，支持按需翻译。

## 预览

Windows 提供托盘和一行小浮窗；macOS 菜单栏显示工作状态、模型和速度，有新重置消息时标记 `↻`。点击即可展开详情。

发现新版后自动下载并验证签名，点击“重启并更新”即可安装，无需打开浏览器。

<a href="docs/screenshots/compact-zh.png"><img src="docs/screenshots/compact-zh.png" alt="Windows 小浮窗：工作状态、模型、速度与重置标记" width="184" /></a>

<a href="docs/screenshots/overview-zh.png"><img src="docs/screenshots/overview-zh.png" alt="CodexPulse 总览" width="380" /></a>

<a href="docs/screenshots/tibo-zh.png"><img src="docs/screenshots/tibo-zh.png" alt="Tibo 重置历史与 28 天挑战" width="380" /></a>

示例数据，点击查看高清原图。更多截图：[模型](docs/screenshots/models-zh.png) · [Sessions](docs/screenshots/sessions-zh.png)。

## 安装

- **Windows 10 / 11 x64**：下载 `x64-setup.exe` 安装。缺少 WebView2 时，安装器会自动联网安装。
- **macOS 12+**：下载通用 `.dmg`，拖到 Applications，支持 Apple Silicon / Intel。

Windows 安装包未签名；macOS 为 ad-hoc 签名，尚未公证。

用量账本保存在本机，不保存对话正文；费用为 API 等价估算，**不是订阅账单**。

[使用、刷新与开发](docs/guide.md) · [产品设计](docs/design/codexpulse-v1.md) · [开发状态](docs/development/status.md)
