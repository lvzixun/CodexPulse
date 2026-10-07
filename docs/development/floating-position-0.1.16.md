# 0.1.16 收起浮窗定时跳位修复

用户确认是整个小浮窗的位置间歇挪动，并非状态图标或文字刷新。检查发现 collector 调用 release_hidden，其可见窗口维护分支每 30 秒执行 correct_bounds；原先每次都按窗口 outer bounds 严格检查工作区，越界时会按锚点重新 set_size / set_position。靠边拖动、阴影边框和缩放造成的外框差异可能触发此路径。本次没有从原生坐标追踪直接复现某一次跳动，因此将原因记录为已识别的周期性重新定位路径，用户随后在本机 0.1.16 收起状态观察后确认“没有再挪动”。

修复为 Windows 先比较当前显示器 / 工作区 / 缩放与上次快照，只有真实变化才允许边界校正。正常 30 秒维护、监视器枚举顺序变化和暂时空结果均不定位。启动和主动展开 / 收起 / DPI 定位时记录新基准，避免下一次维护再次修正。主动定位也只在尺寸 / 坐标确实不同的时候调用系统 API。没有增加线程、轮询或 WebView，稳定桌面快照不重复分配数组。

测试覆盖连续 120 次稳定维护不要求校正、枚举顺序变化、工作区变化、缩放变化、显示器断开，以及空结果恢复。复用现有几何测试验证断屏回退、远程桌面小工作区和展开收起锚点。Rust 检查使用 release 模式，不创建 target/debug。

本机修复经用户确认后，按用户要求发布 [0.1.16 双平台正式 Release](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.16)，包含 Windows x64 和 macOS Universal。

## 本机交付验证

160 项 Rust release 测试通过，6 项显式联网 / 凭据测试忽略；Clippy 无警告，Svelte 检查无错误 / 警告。签名与绑定版本检查通过，安装器使用 `/S /UPDATE /R /D=D:\files\CodexPulse` 成功替换本机应用并自动重新启动。初次交付为 2026-10-07 14:02:42 的进程 PID 29692，文件版本 0.1.16，运行目录保持不变。无 target/debug 产物。修复代码提交 `9d574d4`。

用户已在本机 0.1.16 收起状态观察后明确回复“没有再挪动”，本轮通过用户实际目测确认位置跳动已消失；未将此结果描述为原生坐标自动化测试。

## 正式发布验证（2026-10-07）

标签 `v0.1.16` 指向 `e1c7669d13465ccadd073ca026594e5f819f08ff`。[双平台 CI 37579725075](https://github.com/lvzixun/CodexPulse/actions/runs/37579725075) 全部成功，包含前端检查、Node / Rust 测试、Clippy、原生构建及更新签名验证。Windows 校验 x64 PE 和应用 / 安装器版本；macOS 校验 Universal arm64 / x86_64 和 ad-hoc 签名。

下载 CI 产物后，再次验证更新签名、签名绑定 0.1.16 版本及篡改拒绝；macOS 压缩包内版本、最低系统 12.0、两个架构及签名资源均符合预期。七个上传资产的大小和 SHA-256 与 GitHub 服务端 digest 完全一致。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| Windows x64 安装器 | 5,031,405 | `8910d37c611e893f57bba2e1ca30c1cf795b22c2c434461567dd07c30b3320d3` |
| macOS Universal DMG | 13,632,198 | `2a0dc02e7d3f79efcd24748ad3f3de6ab5827f8aa82cd5ac8eed521709d5e2ab` |
| macOS 更新压缩包 | 13,958,811 | `668997dbf1730d1932f4e6bc48b3ffdffdbe2918c0b5c726fc4593f02b6de391` |

Release 为正式版、非草稿，并设置为 latest。匿名 HTTPS 获取 `releases/latest/download/latest.json` 已返回 0.1.16，包含 Windows x64、macOS Intel / Apple Silicon 三个平台键，全部指向本次 HTTPS 资产。

本机随后使用 CI 正式 Windows 安装器再次执行静默更新，退出码 0；2026-10-07 14:24:49 自动重新启动为 PID 22568，路径 `D:\files\CodexPulse\codexpulse.exe`、产品 / 文件版本均为 0.1.16。此项验证安装器替换与重启，并不等同于从旧版本点击应用内更新的端到端测试。0.1.15 可通过既有应用内更新升级，较早版本需先手动安装一次。macOS 本轮无实机升级验收；Windows 无系统代码签名，macOS 为 ad-hoc 签名、未公证，更新包使用独立签名。

保留最终 `releases/0.1.16` 产物，删除重复的 CI 下载文件；本地未生成 `target/debug`。
