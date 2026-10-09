# 0.1.25 Windows 10 禁用毛玻璃

Windows 10 的 Acrylic 原生材质存在[已知的窗口拖动性能问题](https://github.com/tauri-apps/window-vibrancy#available-functions)。按用户要求，该系统固定使用不透明背景，设置中的毛玻璃开关关闭且不可开启，并提供中英文说明。

使用 `windows-version` 的 `RtlGetVersion` 结果区分 Windows 10 与 Windows 11（两者 major 均为 10，Windows 11 从 build 22000 起）。系统能力在进程内缓存，与用户开关及系统透明度 / 高对比偏好分开。Windows 10 默认关闭，启动时将旧配置中的开启状态关闭并持久化；IPC 保存和后台设置处理也强制关闭，原生材质入口再次拒绝启用。Win11 与 macOS 的开关行为保留，不使用 Blur 替代。

前端从宿主读取独立的 `glass_available` 能力，关闭状态不影响支持平台重新开启。Win10 即使收到旧的开启配置，也保持开关置灰、未选中及不透明背景；保存其他偏好时不会重新启用。

本机检查：191 项 Rust 测试通过（7 项显式联网探针忽略），新增测试覆盖系统版本边界、旧配置关闭 / 持久化 / 重读与其他设置保留；Clippy 无警告、Rust 格式检查通过。Svelte 零错误 / 警告、22 项 Node 测试、生产 UI 构建及前端格式检查通过。

真实 Svelte + 官方 Tauri IPC mocks 验证 12 组平台能力 / 中英文 / 320、380px 组合：Win10 开关不可开启、旧状态和快照更新不能开启、深浅主题使用不透明底色、保存携带关闭状态、浮窗 / 详情切换仍不透明；Win11 / macOS 关闭后仍可再次开启。此验证不冒充 Windows 10 原生拖动延迟实测。

既有中英文面板回归通过，覆盖浮窗 / 标题栏拖动入口、更新缓存、消息已读、菜单、重置提醒及隐藏刷新 / 失败 / 退出账户时的缓存行为。

已发布 [0.1.25 双平台正式版](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.25)，构建源 `3298894`，[CI 37936900183](https://github.com/lvzixun/CodexPulse/actions/runs/37936900183) 全部成功：Windows 190 项、macOS 191 项 Rust 测试，各忽略 7 项显式联网探针；Svelte、22 项 Node 测试及 Clippy 通过。Windows x64 PE 与应用 / NSIS 产品版本验证通过；macOS 双架构及严格 ad-hoc 签名验证通过。

下载后的两个平台更新包均通过签名、版本绑定与篡改拒绝复核；macOS DMG 和更新归档的版本、最低系统 12.0、arm64 / x86_64、严格签名及可执行文件一致性通过。正式发布前后全部 7 个服务端资产的大小 / SHA-256 与本地一致；最新正式 Release 和匿名公开更新清单均确认为 0.1.25。

正式安装包、签名、清单与校验记录保存在 Git 忽略的 `releases/v0.1.25`。已停止开发服务器，清理本次 2.3 GiB Rust 编译缓存、CI 下载副本、解包副本和临时 Playwright 依赖。
