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

双平台 CI 与产物校验待完成。Windows 无 Authenticode 签名，macOS ad-hoc 签名、未公证。新版本不自动静默安装。
