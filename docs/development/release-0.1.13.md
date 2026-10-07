# 0.1.13 双平台正式发布

用户要求重新发布 Windows / macOS，清理 GitHub 旧 Release 和预发布，并将默认主题改为跟随系统。此要求覆盖此前仅提交源码的安排。

## 代码与检查

在已合并 origin/main 的 0.1.12 功能基础上，Rust Settings::default 和前端初始 snapshot 的主题均改为 system。已有持久化主题继续使用原值，不重置用户设置。继承 Resets `.env` 代理、最新公告与历史解耦、版本 / 更新检查，以及远端标题栏拖动和标签样式修正。

前端 Svelte 无错误 / 警告，15 项 Node 测试及格式检查通过。最终标签将在 Windows / macOS 原生 CI 中执行 Rust workspace、Clippy、前端检查与生产打包；Windows 验证 x64 PE、应用与安装器版本，macOS 验证 arm64 / x86_64 及 ad-hoc 签名。不使用交叉编译的 Windows 包。

## 发布与清理计划

两个安装包与 SHA256SUMS.txt 校验后发布中文说明的正式 Release。先保证新下载可用，再删除旧正式 Release 和预发布；保留 Git 标签与源码历史。发布元数据仅备份在本地忽略目录，不上传账户、代理、日志、数据库或企业安全报备信息。

CI、资产哈希、原生结果与清理数量在完成后补充。
