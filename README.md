# CodexPulse

CodexPulse 是 Codex 的桌面状态伴侣：集中查看额度、最近工作 sessions、token 用量、API 等价费用、输出速度，以及额度重置相关消息。

Windows 首发，支持 Windows Codex App 与 WSL Codex CLI 的数据采集；后续支持 macOS。

Windows 的浮窗和托盘共用一个详情窗口，浮窗可关闭，托盘右键只显示设置、退出等简洁操作。macOS 设计为菜单栏图标及其详情面板，属于后续阶段。界面跟随系统主题，默认蓝色，设置可选蓝色、紫色、青绿、琥珀或玫红，并提供原生毛玻璃材质及不透明回退。

## 文档

- [产品、UI 与技术设计案](docs/design/codexpulse-v1.md)
- [设计文档索引](docs/design/README.md)

## 开发

需要 Windows 11、Rust stable（最低 1.90）、Visual Studio C++ 构建工具、Node.js 24 和 pnpm 11，以及 WebView2 Runtime。

```powershell
pnpm install --frozen-lockfile
pnpm dev
```

检查与构建：

```powershell
pnpm check
cargo test -p pulse-core
cargo check -p codexpulse
pnpm build
```

`pnpm build` 构建 Windows NSIS 安装包。直接在浏览器运行前端时显示未连接状态；真实用量由桌面 Rust 采集器提供。

## 仓库布局

采用单仓库、单桌面应用结构，业务核心与平台接入分离。完整目录规划与模块责任见设计案的“仓库结构”。

- `apps/desktop`：Svelte + TypeScript 前端、Tauri 原生窗口及托盘。
- `crates/pulse-core`：日志增量解析、SQLite 用量账本、模型与日期汇总、费用计价逻辑。
- `docs/design`：功能、UI、数据口径与性能设计。
- `scripts`：开发工具。
- `work`：本机实验与临时数据，Git 忽略。

当前是开发版本，尚未达到首版完整验收。已实现本地用量账本及桌面基础窗口；额度 RPC、消息服务、真实价格数据、通知、完整设置与性能验收仍在开发。具体进度见 [开发状态](docs/development/status.md)。设计中的性能数字是验收目标。
