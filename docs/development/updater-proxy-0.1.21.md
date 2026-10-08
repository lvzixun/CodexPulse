# 0.1.21 更新代理与提醒布局

更新的启动 / 手动版本检查、Tauri 清单查询及安装包下载，复用 Resets 已启用且可用的首个 Codex 来源。读取 CODEX_HOME `.env` 中的 HTTPS_PROXY（含小写）、ALL_PROXY、HTTP_PROXY 及 NO_PROXY；文件优先于进程环境，WSL 不继承宿主进程代理。每次检查 / 下载重新读取，显式空值直连，非法 / 超大配置返回通用错误。没有显式代理时保留客户端原有行为。

下载使用自定义 reqwest 路由函数，在 GitHub 跳转到资产 CDN 后重新按主机判断 NO_PROXY；ureq 检查端同样配置逐目标排除。支持带密码 HTTP CONNECT 代理与 SOCKS，代理地址 / 凭据不序列化、不写入错误和日志，也不调用会打印完整代理 URL 的 updater.proxy。公共请求不读取 auth.json / config.toml、不发送账户 Authorization、账号 ID 或 Cookie。签名、版本绑定、受信任下载地址、单飞和服务端退避保持原样；仅启动一次和手动检查，不新增定时更新。

Backend 保留现有公共来源上下文，通过 Condvar 等待首次发现，最长 5 秒；超时视为路由不可用，不趁来源未就绪直连或安排自动重试。等待运行在阻塞工作线程，停止的 WSL 来源不进入上下文。

总览在有未来计划时移除外层重置卡片的底色、边框和内边距，最近重置作为普通行，只有计划提醒保留一层卡片；离线标记只显示一次。没有未来计划时保持原有两行布局。两页计划判定、到期状态与预测区分保持不变。

## 验证与边界

本地模拟代理验证：Codex .env 优先级、每次重读、空值直连、非法 / 超大文件拒绝、HTTP 与 SOCKS URI、两种网络客户端 NO_PROXY 的精确 / 后缀 / 通配匹配；ureq 经 CONNECT 成功读取版本及保留 Retry-After；reqwest 连续跨域跳转仍经过代理，转向 NO_PROXY 本地源时直连，且代理密码不泄漏；GitHub 与资产 CDN 的 HTTPS CONNECT 分别携带代理认证，不带账户信息。用无效 auth.json 验证公共路由独立于账户认证。

新增一次性公开签名包下载探针，默认忽略。尝试本机探针发现当前启用的默认 ~/.codex 下没有 .env，未满足显式代理前提，未计作真实代理下载通过；没有修改用户配置来制造通过结果。浏览器 IPC 与模拟代理测试不等同于原生完整自动升级验收。

本机 Rust workspace 180 项通过、7 项显式联网探针默认忽略；Clippy / rustfmt、Svelte（0 错误 / 0 警告）、18 项前端测试和发布清单测试通过。生产前端构建、中英文 IPC 回归（深浅主题、窄宽度、两页倒计时、离线标记、到期状态和单层容器）通过。

## 正式发布与本机安装

[Windows x64 / macOS Universal 0.1.21 正式版](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.21) 已发布为 latest，非预发布；源码标签为 `bd909fc226cd3761de7f90ed1ccae9f7631b5898`。[CI #37717830008](https://github.com/lvzixun/CodexPulse/actions/runs/37717830008) 两端全部成功：macOS Rust 180 项、Windows 179 项通过，各 7 项显式联网探针忽略；前端、Clippy、版本 / 架构与签名门禁通过。

实际下载的双平台更新资产再次通过公钥签名、版本绑定和篡改拒绝校验。7 个资产在草稿和正式发布后分别与 GitHub 服务端大小及 SHA-256 比对一致，才切换为 latest。

本机用户和系统 Applications 中的 App 均从已验证 CI 产物安装为 0.1.21；DMG 与归档版本 0.1.21、最低 macOS 12、arm64 + x86_64、严格 ad-hoc 签名和关键文件一致。旧版通过面板退出，确认进程结束后分阶段替换；新版进程来自已替换的用户 Applications。原生设置确认 v0.1.21，总览截图确认只有一层提醒框，Tibo AX 确认同一倒计时与计划时间。

原生启动检查本次显示“暂时无法检查更新”；该机默认 Codex 目录没有 .env，本轮未验证真实代理下载或完整应用内自动升级，不把 CI / 模拟代理测试称为原生升级验收。没有绕过既有等待强制重查。公开 latest / 标签清单在本机匿名 curl 检查中连接超时；一次有界 IPv4 复查定位到 `release-assets.githubusercontent.com:443` 连接超时，系统 HTTP / HTTPS / SOCKS 代理均未启用。该网络检查未计作通过；GitHub API 的正式版本、latest 指向与 7 个资产校验单独复核。外部网页抓取清单也未成功，未作为通过证据。

清理约 2.34 GiB：本轮 Rust target、临时验证工具工程、浏览器测试临时依赖、CI 下载副本、解包 App 及已验证替换后的旧 App 备份；卸载两次验证挂载。保留正式发布资产、校验记录与预览，用户 Downloads 中原有 0.1.20 DMG、Codex 数据和应用数据库未删除。仓库 target 不存在。


| 资产 | 字节 | SHA-256 |
| --- | ---: | --- |
| CodexPulse_0.1.21_universal.app.tar.gz | 14,084,016 | `c779b22385d5189729de198784b2ff06c24b64f268633c091446ca65b3fd65cb` |
| CodexPulse_0.1.21_universal.dmg | 13,757,294 | `dc00e0790f32871e73c7829f5f70dfbc9e08390fa9fab21a3777dddc723affb0` |
| CodexPulse_0.1.21_x64-setup.exe | 5,076,464 | `c183208e2ade68754b6659da8b61975dfea917ead8948e8996223ee43723aeb9` |
