# 0.1.17 同账号凭据更新后恢复额度刷新

用户反馈 WSL 已能使用 Codex，设置仍显示“登录已过期”。本机只读检查确认：WSL 正在运行的 Codex 与 CodexPulse 使用同一 CODEX_HOME；14:28 Codex 已更新文件，访问令牌有效至 10 月 17 日，Windows UNC 读取同样有效。产品缓存仍保存 14:22 的 credentials_expired、5 次失败及截至 14:38 的退避。诊断只输出有效性和时间，不输出凭据、个人身份、代理地址，不修改认证文件。

原因是本地探测发现凭据修订变化时，仅在账号身份变化才清缓存；同账号续期没有清除错误和认证退避，所以新文件已经有效，界面和重试仍停留在旧状态。

修复在本地探测处理阶段恢复同账号认证：有效访问令牌清除旧 credentials_expired；reauth_required 需同时确认凭据修订变化。清除认证失败计数和等待，标记 awaiting_refresh，由原调度决定是否请求。初次启动也能恢复持久化的过期错误。其他网络与服务端等待保留，普通请求开始或结束不重置认证失败计数。

不自行刷新 token、不启动 CLI / app-server、不写 Codex 认证文件。手动模式和收起 / 隐藏时不发 HTTP；Resets 与本地采集保持独立。相同的额度 / 账户统计错误只显示一次，过期提示改为“保存的访问令牌已过期，等待 Codex 更新凭据”。

回归覆盖同账号续期、重启后过期缓存恢复、自动 / 手动 / 收起策略、未变化凭据的 401 保持退避，以及续期后 429 / 403 / 网络失败仍保留等待。

本机 162 项 Rust release 测试通过，6 项需显式运行的联网测试默认忽略；Svelte 检查无错误 / 警告、15 项前端测试与发布清单测试通过，release Clippy 无警告。另显式运行真实 WSL CODEX_HOME 的 HTTPS 额度、重置卡和账户统计读取测试，全部通过，验证使用现有代理配置，不启动 CLI 或刷新认证。

本机签名安装包验证签名、版本绑定及篡改拒绝后，静默替换应用并自动重新启动为 0.1.17（2026-10-07 14:39:40，PID 27108）。实际旧 WSL 缓存从 credentials_expired / failures=5 / retry_at=1791355087 恢复为 awaiting_refresh / failures=0 / retry_at=0。收起状态下 Windows 和 WSL 的 last_attempt 均未变化，验证恢复本地状态不会触发收起时的账户 HTTP。这是原生后台与持久化状态验收，不等同于 UI 自动化或 macOS 实机验收。

## 正式发布（2026-10-07）

[0.1.17 Windows x64 / macOS Universal 正式版](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.17) 已发布，非草稿 / 非预发布，并设为 latest。标签指向修复提交 `f962989efa9781d54cf384dc51348561c5a40e3a`；[双平台 CI 37582347429](https://github.com/lvzixun/CodexPulse/actions/runs/37582347429) 全部成功，包含前端检查、测试、Clippy、原生安装包、版本 / 架构和更新签名验证。

下载实际 CI 文件后复核两端更新签名、版本绑定和篡改拒绝，检查 macOS 包内版本、最低系统 12.0、arm64 / x86_64 及签名资源。七个发布资产的大小和 SHA-256 全部与 GitHub 服务端 digest 相同。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| Windows x64 安装器 | 5,033,133 | `df408f4900d894261703e8cc5d6dcc6b33ac11bd625249bdfa8773caf7eb7c12` |
| macOS Universal DMG | 13,632,475 | `55ba31283689b125603306972a30c6e3bf589874079e5079dc838e4b02abb67b` |
| macOS 更新压缩包 | 13,960,604 | `c892b36eac3835ca0fd6a8fd4746173289506e52cce6ad71f5619c3cf4379ba8` |

匿名 HTTPS 访问 `releases/latest/download/latest.json` 返回 0.1.17，Windows x64 与 macOS 两种架构均指向本次 HTTPS 资产。0.1.15 及更新版本可在应用内更新，更早版本需先手动安装一次。

本机再用 CI 正式安装器执行 `/S /UPDATE /R /D=D:\files\CodexPulse`，退出码 0，2026-10-07 14:51:14 自动重启 PID 38512，运行路径不变，产品 / 文件版本均为 0.1.17。本轮验证安装器替换与重启，未将其描述为旧版点击应用内更新的完整端到端测试；macOS 无本轮实机升级验收。Windows 尚无系统代码签名，macOS 为 ad-hoc 签名、未公证；更新包使用独立签名。

删除重复 CI 下载文件，保留最终 releases/0.1.17 安装与更新产物，未生成 target/debug。
