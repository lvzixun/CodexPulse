# 0.1.32 重置计划系统通知

按用户选择，发现新的已公布重置计划时立即向系统提交一次通知，不等提前 30 分钟或每次打开窗口再提醒。

Rust 后台采集器在接受新的消息状态响应后处理，面板隐藏时同样生效；不增加网络请求或轮询。仅信任既有 Tibo 来源的常规 / 备用额度 scheduled 计划。预测、观察、取消、已过期和已执行的计划不推送。没有具体时间的有效计划显示“时间待定”，通知注明以实际执行公告为准；日期按设置时区显示，支持中英文。

计划 ID 的摘要保存在 SQLite 设置中，提交系统通知前先持久化。相同公告修改正文 / 时间、暂时消失后恢复、刷新和重启都不会再次提交。关闭开关期间发现的计划也记录为已处理，重新开启不补发。此处保证最多提交一次；系统通知权限、专注模式及操作系统投递策略仍决定是否展示，不将提交成功等同于用户已看到通知。

设置新增默认开启的“重置计划系统通知”，随其他本地设置保存。使用 [Tauri 官方通知插件](https://v2.tauri.app/plugin/notification/) 2.6.0 的 Rust 接口，不向前端开放任意通知权限。点击系统报告的通知正文操作会打开 Tibo 消息页，并返回消息列表顶部；[Windows 插件仅支持横幅仍显示时的点击回调](https://docs.rs/tauri-plugin-notification/2.6.0/tauri_plugin_notification/struct.NotificationBuilder.html#method.show)，进入通知中心后的点击不作为已验收行为。

## 验证

- macOS 本机 `cargo test --workspace --release --locked` 全量测试通过，包含新增 6 项通知测试：立即提醒、SQLite 关闭重开去重、同计划编辑及移除恢复、开关变化、缓存 / 失败响应、可信来源、过期 / 已执行和中英文时区文案。
- Clippy 全目标零警告，Rust 格式检查通过；Svelte 零错误 / 警告，22 项 Node 测试及生产前端构建通过。
- 8 组 IPC 模拟回归通过：macOS / Windows 样式、中英文、320 / 380px；验证默认开关、保存关闭状态、窄屏布局，以及收到通知导航事件后从挑战详情返回重置消息列表。
- 本机已安装 CI 正式 macOS Universal 包，原生重启确认版本 0.1.32、通知开关默认开启；真实已公布计划的持久化去重记录为 1 条，未注入测试公告或修改用户通知偏好。
- 两平台真实系统横幅展示和原生通知点击尚未验收；Windows 安装包由 CI 构建和检查，未进行 Windows 原生运行验收。

## 发布

已按用户要求通过 `gh` 发布 [v0.1.32](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.32)，并设为最新正式版。源码提交及版本标签为 `dffda40abd5f89050a293c4bde2c863885578687`。

- [双平台 CI](https://github.com/lvzixun/CodexPulse/actions/runs/38337321637) 全部通过：Windows 196 项、macOS 197 项 Rust 测试通过，两平台各 7 项显式联网 / 实机探针默认忽略；Svelte、22 项 Node 测试、Clippy 和发布构建通过。
- Windows x64 安装包版本和 PE 检查通过；macOS Universal 包包含 arm64 / x86_64，最低系统 12.0，严格 ad-hoc 签名校验通过，DMG 与更新包内可执行文件一致。
- 双平台自动更新签名、签名版本绑定和篡改拒绝通过；7 个公开服务端资产的名称、大小及 SHA-256 全部与本地一致。匿名下载的最新 `latest.json` 与本地逐字节相同，版本为 0.1.32，包含 Windows x64、macOS Intel / Apple Silicon 更新入口。
- 本机 `/Applications/CodexPulse.app` 已替换为上述正式通用包并完成原生启动及设置检查，旧版本保留在本次工作备份中。
