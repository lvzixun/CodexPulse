# Windows 对齐与双平台发布 0.1.4

日期：2026-10-07，Asia/Shanghai。基线为另一台机器完成的 macOS 提交 `2d94760`，通过 fast-forward 更新；不回退共享功能。

## 对齐范围

| 能力 | Windows | macOS |
| --- | --- | --- |
| 账户资料、五项累计、credits 与重置卡 | 共享 HTTPS 采集和总览 | 相同 |
| 当前价参考费用、平均速度、子代理过滤 | 共享日志与 SQLite 投影 | 相同 |
| 模型图表、运行优先 Sessions、半年重置与挑战 | 共享页面 | 相同 |
| 中英文、外观、两个独立刷新组 | 共享设置；即时保存 | 相同 |
| 原生宿主 | 托盘 + 可选 184 × 36 浮窗 | 菜单栏面板 |
| 登录启动 | 新增当前用户 Run 项与 OS 状态读取 | 保留 SMAppService / 旧系统 LaunchAgent |
| 脱敏诊断 | 新增 Unicode 原生剪贴板，开放诊断入口 | 保留 NSPasteboard |
| 窗口诊断 | 开启已有有界固定字段生命周期记录 | 保留 |

账户手动模式不在启动或定时发起请求；自动模式首轮之后仅在原生窗口可见时调度。Windows 浮窗也属于可见窗口。Resets 组与本地采集独立，不因关闭详情而停止。HTTPS 失败不走 CLI；没有引入通知功能。

## Windows 实现边界

- 启动项只在用户切换开关时写入，不把 OS 状态缓存为产品偏好。路径显式引用并带 `--startup`；登录触发第二实例不展开详情，普通再次打开仍展开。
- 系统 StartupApproved 只读，已禁用状态提示去系统设置确认；未知结构显示不可用。不会代替 Windows 改写审批。测试使用隔离 HKCU 子键，不修改真实登录项。
- 拒绝覆盖不属于 CodexPulse 的同名项；移动安装后只读检查不自动修复。卸载 hook 仅删除指向当前安装目录的准确命令，保留用户账本。
- 剪贴板使用真实窗口 owner 和 CF_UNICODETEXT，内存所有权成功后交给系统，所有失败路径释放资源。复制入口只接受后端白名单诊断，不接受网页提交的任意文本。
- 生命周期沿用 64 KiB 轮换及固定字段，无账户 / 路径 / 标题 / 用量 / 原始错误。诊断与启动项均按需操作，没有新增定时轮询。

## 验证与发布

实施中：原生 release 验证、CI 双平台构建、安装包校验、发布及 debug 清理完成后在此补充实测结果。未实测项目不会从历史待办中直接标记通过。

流程：`.github/workflows/release-build.yml` 在手动触发 / 版本 tag 推送时对 Windows x64 和 macOS Universal 执行检查、测试、Clippy 和构建；macOS 校验 ad-hoc 签名及 arm64 / x86_64 架构。Actions 固定到具体提交，权限只有源码读取；人工发布命令在两个构建成功后上传安装包及校验文件。

参考：[Windows Run 注册表项](https://learn.microsoft.com/windows/win32/setupapi/run-and-runonce-registry-keys)、[Windows 剪贴板所有权](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setclipboarddata)、[Tauri GitHub 构建](https://v2.tauri.app/distribute/pipelines/github/)。
