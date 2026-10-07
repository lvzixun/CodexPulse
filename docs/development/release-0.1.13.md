# 0.1.13 双平台正式发布

用户要求重新发布 Windows / macOS，清理 GitHub 旧 Release 和预发布，并将默认主题改为跟随系统。此要求覆盖此前仅提交源码的安排。

## 代码与检查

在已合并 origin/main 的 0.1.12 功能基础上，Rust Settings::default 和前端初始 snapshot 的主题均改为 system。已有持久化主题继续使用原值，不重置用户设置。继承 Resets `.env` 代理、最新公告与历史解耦、版本 / 更新检查，以及远端标题栏拖动和标签样式修正。

前端 Svelte 无错误 / 警告，15 项 Node 测试及格式检查通过。最终标签已在 Windows / macOS 原生 CI 中执行 Rust workspace、Clippy、前端检查与生产打包；Windows 验证 x64 PE、应用与安装器版本，macOS 验证 arm64 / x86_64 及 ad-hoc 签名。不使用交叉编译的 Windows 包。

## 发布与清理策略

两个安装包与 SHA256SUMS.txt 校验后已发布中文说明的正式 Release。保证新下载可用后删除旧正式 Release 和预发布，保留 Git 标签与源码历史。发布元数据仅备份在本地忽略目录，不上传账户、代理、日志、数据库或企业安全报备信息。

## 最终结果

[0.1.13 正式 Release](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.13) 已发布，说明仅中文。源标签提交 `e093f98bbc806b9f235d615d5764ea8dac842e8d`；[双平台 CI #37572618775](https://github.com/lvzixun/CodexPulse/actions/runs/37572618775) 两端全部成功。Windows 应用 x64 PE、应用与安装器产品版本校验通过；macOS Universal 两种架构及 ad-hoc 签名校验通过。下载后的安装器哈希与 Windows CI 输出一致，挂载 DMG 校验版本 0.1.13、最低部署目标 12.0、arm64 / x86_64、签名及 Applications 安装入口。

| 安装包 | 大小 | SHA-256 |
| --- | ---: | --- |
| CodexPulse_0.1.13_universal.dmg | 12,269,341 B | `6a00faf26767319503fc58183113b2a8cf802697b8b14199e9bbe4468747b381` |
| CodexPulse_0.1.13_x64-setup.exe | 4,530,632 B | `da2d549058b4994e4b4af95751b0f5ab3be75ed53790d67c39cba80788e6ea8d` |

两个安装包与 SHA256SUMS.txt 的 GitHub 服务端大小、digest 均与本地文件一致，正式版为 latest。

本机 `~/Applications/CodexPulse.app` 已更新至 0.1.13；Info.plist、CodeResources、Mach-O 和图标四文件清单与 CI DMG 中的 App 一致，签名再次通过。原生面板正常加载，设置显示版本号、检查更新和跟随系统；保留已有用户设置。12:56 实际公告 / 历史 / 挑战刷新均为 connected，历史待更新标记为 false。GitHub 匿名版本 API 仍返回限流，界面保留等待状态，未绕过限流或添加凭据。

此处未将 Windows 安装向导 / GUI 或 Intel 实机运行称为本轮验收通过；Windows 原生 CI 构建及产物校验与 macOS Apple Silicon 实机验证分别记录。Windows 未签名，macOS ad-hoc 签名且未公证。

新正式版验证后，已删除此前 8 个旧 Release（v0.1.0 / 0.1.1 / 0.1.2 / 0.1.3 / 0.1.5 / 0.1.6 / 0.1.7 / 0.1.8），其中 4 个预发布；GitHub 发布列表只保留 v0.1.13。Git 标签与源码历史保留，旧发布元数据保存在本地忽略目录。不删除用户数据。

本轮构建全部使用 CI，本地 target 保持不存在；已移除重复的 CI 下载暂存目录，保留正式安装包、校验文件、验收记录和已安装 App。
